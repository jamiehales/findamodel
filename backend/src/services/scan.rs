//! Model scanning: discovers model files, computes checksums, metadata, hulls, geometry
//! bounds and previews, and keeps the model cache in sync.
//!
//! Expensive per-file preparation (hashing, mesh loading, hulls, previews) runs in
//! parallel; database writes are applied sequentially as results arrive.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::models::{ModelService, is_model_extension, is_non_geometry_type};
use crate::db::now;
use crate::entities::{DirectoryConfig, ModelHulls, ModelRow};
use crate::geometry::support_reader::{self, CURRENT_SUPPORT_READER_VERSION};
use crate::geometry::{LoadedGeometry, hull, loader, support_separation};
use crate::metadata::config_reader;
use crate::metadata::inheritance::DirectoryMap;
use crate::metadata::model_metadata::{self, ComputedMetadata};
use crate::metadata::rules;
use crate::preview::spin::{self, CURRENT_SPIN_MESH_VERSION};
use crate::preview::{self, CURRENT_PREVIEW_GENERATION_VERSION};
use crate::services::app_config::normalize_minimum_support_export_version;
use crate::services::repaired_meshes;
use crate::services::support_exports::{self, SupportExports};
use crate::services::{scan_config, store};

const LOG_TARGET: &str = "findamodel.Models";

#[derive(Debug, Clone, PartialEq)]
pub struct FilePlan {
    pub relative_path: String,
    pub file_type: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileResult {
    pub relative_path: String,
    pub file_type: String,
    /// processed | skipped | failed
    pub status: String,
    pub is_new: bool,
    pub was_updated: bool,
    pub generated_preview: bool,
    pub generated_hull: bool,
    pub message: String,
    pub duration_ms: f64,
    pub stages: StageTimings,
}

/// Time a file spent in each indexing stage, in milliseconds; stages that didn't run stay 0.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StageTimings {
    pub checksum_ms: f64,
    pub load_ms: f64,
    pub supports_ms: f64,
    pub preview_ms: f64,
    pub hull_ms: f64,
    /// Reading a pre-supported model's supports and writing its export entry.
    pub export_ms: f64,
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

/// Receives progress from a scan (persisted by the indexer).
#[async_trait]
pub trait ProgressReporter: Send + Sync {
    async fn on_scan_started(&self, total_files: usize);
    async fn on_files_discovered(&self, files: &[FilePlan]);
    async fn on_file_processed(&self, result: FileResult);
    async fn on_log(&self, level: &str, message: &str, relative_path: Option<&str>);
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("Scan cancelled")]
pub struct Cancelled;

pub fn needs_hull_regeneration(stored_checksum: Option<&str>, expected_raft_height_mm: f32) -> bool {
    stored_checksum != Some(scan_config::compute(expected_raft_height_mm).as_str())
}

/// A preview is stale when missing, older than the minimum version, or stored under a
/// different path than `expected_path` (the support split inputs changed).
pub fn needs_preview_regeneration(
    preview_image_path: Option<&str>,
    stored_version: Option<i32>,
    minimum_version: i32,
    expected_path: &str,
) -> bool {
    preview_image_path.is_none_or(|p| p.trim().is_empty() || p != expected_path)
        || preview_version_stale(stored_version, minimum_version)
}

/// A spin mesh is stale when missing, written by another spin mesh version, or stored under a
/// different path than `expected_path` (the support split inputs changed). Its own version: a
/// stale spin mesh is written again without redrawing the previews.
pub fn needs_spin_regeneration(spin_mesh_path: Option<&str>, stored_version: Option<i32>, expected_path: &str) -> bool {
    spin_mesh_path != Some(expected_path) || stored_version != Some(CURRENT_SPIN_MESH_VERSION)
}

/// The stored preview was drawn by a renderer older than the minimum version.
fn preview_version_stale(stored_version: Option<i32>, minimum_version: i32) -> bool {
    stored_version.is_none_or(|v| v < minimum_version)
}

/// Which preview files a geometry pass writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewRefresh {
    Skip,
    /// Render only the files missing from the cache.
    Missing,
    /// Render over the cached files too. Preview file names hold the checksum, raft height and
    /// supported flag but not the preview version, so a preview older than the minimum version
    /// would otherwise be reused as it is.
    Redraw,
}

impl PreviewRefresh {
    /// For a model whose preview is `stale` (see [`needs_preview_regeneration`]).
    fn for_stale(stale: bool, stored_version: Option<i32>, minimum_version: i32) -> Self {
        match (stale, preview_version_stale(stored_version, minimum_version)) {
            (false, _) => Self::Skip,
            (true, true) => Self::Redraw,
            (true, false) => Self::Missing,
        }
    }

    fn included(self) -> bool {
        self != Self::Skip
    }
}

pub fn resolve_minimum_preview_generation_version(configured: i32) -> i32 {
    configured.clamp(0, CURRENT_PREVIEW_GENERATION_VERSION)
}

fn valid_raft(v: Option<f32>) -> Option<f32> {
    v.filter(|r| r.is_finite() && *r >= 0.0)
}

pub fn resolve_raft_height_mm(directory: &str, configs: &DirectoryMap, default_raft_height_mm: f32) -> f32 {
    configs.get(directory).and_then(|d| valid_raft(d.raft_height_mm)).unwrap_or(default_raft_height_mm)
}

/// Raft cutoff for a model: its own override, then its directory's, then the default.
pub fn resolve_raft_height_mm_for_model(
    rule_path: &str,
    directory: &str,
    configs: &DirectoryMap,
    default_raft_height_mm: f32,
) -> f32 {
    raft_height_mm_for(
        &model_metadata::compute(rule_path, configs.get(directory)),
        directory,
        configs,
        default_raft_height_mm,
    )
}

fn raft_height_mm_for(
    metadata: &ComputedMetadata,
    directory: &str,
    configs: &DirectoryMap,
    default_raft_height_mm: f32,
) -> f32 {
    valid_raft(metadata.raft_height_mm)
        .unwrap_or_else(|| resolve_raft_height_mm(directory, configs, default_raft_height_mm))
}

fn finite(v: f32) -> Option<f32> {
    v.is_finite().then_some(v)
}

fn system_time_utc(t: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(t)
}

struct FileStat {
    size: i64,
    modified: DateTime<Utc>,
}

fn stat(path: &Path) -> std::io::Result<FileStat> {
    let meta = std::fs::metadata(path)?;
    Ok(FileStat { size: meta.len() as i64, modified: system_time_utc(meta.modified()?) })
}

async fn compute_checksum(path: &Path) -> anyhow::Result<String> {
    config_reader::compute_file_hash(path).await
}

/// Reuses the stored checksum while the file's modification time is unchanged.
async fn resolve_checksum(
    path: &Path,
    observed_modified: DateTime<Utc>,
    existing_checksum: Option<&str>,
    existing_modified: Option<DateTime<Utc>>,
) -> anyhow::Result<(String, bool)> {
    if let (Some(checksum), Some(modified)) = (existing_checksum.filter(|c| !c.trim().is_empty()), existing_modified)
        && modified == observed_modified
    {
        return Ok((checksum.to_string(), false));
    }
    Ok((compute_checksum(path).await?, true))
}

/// What the support classifier found in a model's geometry.
#[derive(Debug, Clone, Copy, Default)]
struct SupportDetection {
    /// Confidently a model plus supports.
    confident: bool,
    /// Any support parts, confident or not.
    found: bool,
}

impl SupportDetection {
    fn of(classification: Option<&support_separation::Classification>) -> Self {
        Self { confident: classification.is_some_and(|c| c.confident), found: classification.is_some() }
    }
}

/// Everything computed from a file's content.
#[derive(Debug, Clone)]
struct FileData {
    checksum: String,
    preview_image_path: Option<String>,
    preview_generated_at: Option<DateTime<Utc>>,
    preview_generated: bool,
    hulls: hull::Hulls,
    hull_generated: bool,
    raft_height_mm: f32,
    supports: SupportDetection,
    metadata: ComputedMetadata,
    dir_config_id: Option<Uuid>,
    geometry: Option<GeometrySummary>,
    export: ExportOutcome,
    spin_mesh_path: Option<String>,
    stages: StageTimings,
}

/// What became of a model's export entry while its geometry was processed.
#[derive(Debug, Clone, Default, PartialEq)]
enum ExportOutcome {
    /// Not asked for, or not a pre-supported model with supports to read.
    #[default]
    None,
    /// Written at the current reader version, at this relative path.
    Written(String),
    /// Read but not stored (the entry could not be written).
    Failed,
}

#[derive(Debug, Clone, Copy)]
struct GeometrySummary {
    dimension_x_mm: Option<f32>,
    dimension_y_mm: Option<f32>,
    dimension_z_mm: Option<f32>,
    sphere_centre_x: Option<f32>,
    sphere_centre_y: Option<f32>,
    sphere_centre_z: Option<f32>,
    sphere_radius: Option<f32>,
}

impl GeometrySummary {
    fn of(g: &LoadedGeometry) -> Self {
        GeometrySummary {
            dimension_x_mm: finite(g.dimension_x_mm),
            dimension_y_mm: finite(g.dimension_y_mm),
            dimension_z_mm: finite(g.dimension_z_mm),
            sphere_centre_x: finite(g.sphere_centre.x),
            sphere_centre_y: finite(g.sphere_centre.y),
            sphere_centre_z: finite(g.sphere_centre.z),
            sphere_radius: finite(g.sphere_radius),
        }
    }
}

#[derive(Debug, Clone)]
struct GeometryRefresh {
    include_preview: bool,
    include_hulls: bool,
    geometry_loaded: bool,
    preview_image_path: Option<String>,
    preview_generated_at: Option<DateTime<Utc>>,
    preview_generated: bool,
    hulls: hull::Hulls,
    hull_generated: bool,
    raft_height_mm: f32,
    supports: SupportDetection,
    export: ExportOutcome,
    include_spin: bool,
    spin_mesh_path: Option<String>,
    stages: StageTimings,
}

/// What a geometry pass ([`ModelService::process_geometry`]) produced.
#[derive(Debug, Default)]
struct ProcessedGeometry {
    /// None when the geometry could not be loaded.
    summary: Option<GeometrySummary>,
    preview_generated: bool,
    hulls: Option<hull::Hulls>,
    supports: SupportDetection,
    export: ExportOutcome,
    /// The spin mesh's relative path: None when not asked for, Some(None) when there is none.
    spin: Option<Option<String>>,
    stages: StageTimings,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ExistingScanState {
    id: Uuid,
    directory: String,
    file_name: String,
    checksum: String,
    file_modified_at: DateTime<Utc>,
    scan_config_checksum: Option<String>,
    preview_generation_version: Option<i32>,
    preview_image_path: Option<String>,
    detected_supported: Option<bool>,
    support_parts_found: Option<bool>,
    support_export_path: Option<String>,
    support_export_version: Option<i32>,
    spin_mesh_path: Option<String>,
    spin_mesh_version: Option<i32>,
}

enum Change {
    Content(Box<FileData>),
    Geometry(GeometryRefresh),
    /// The model's export entry is current but the row's index of it is not.
    ExportIndex(String, i32),
    FileMetadataOnly,
}

/// Whether a model is pre-supported by its metadata or a confident detection.
fn presupported(calculated: Option<bool>, detected_confident: bool) -> bool {
    calculated.or(detected_confident.then_some(true)) == Some(true)
}

/// The staleness of an indexed model's export entry, read from the entry file itself.
#[derive(Debug, Clone, PartialEq)]
enum ExportState {
    /// Not a pre-supported model with supports to read (or entries are not kept): nothing to do.
    NotWanted,
    /// Missing, damaged or below the minimum version: read the supports again.
    Stale,
    /// Current enough; the row's index should name this path and version.
    Current(String, i32),
}

/// The export entry state of an indexed model whose content and scan config are unchanged.
#[allow(clippy::too_many_arguments)]
fn export_state(
    exports: Option<&SupportExports>,
    geometry_type: bool,
    checksum: &str,
    raft_height_mm: f32,
    calculated_supported: Option<bool>,
    detected_supported: Option<bool>,
    support_parts_found: Option<bool>,
    minimum_version: i32,
) -> ExportState {
    let Some(exports) = exports else { return ExportState::NotWanted };
    if !geometry_type
        || support_parts_found != Some(true)
        || !presupported(calculated_supported, detected_supported == Some(true))
    {
        return ExportState::NotWanted;
    }
    // The scan indexes under the current scan config, so its entries are for checked detections.
    let key = support_exports::key(checksum, raft_height_mm, calculated_supported, true);
    match exports.stored_version(&key) {
        Some(version) if version >= minimum_version => {
            ExportState::Current(support_exports::relative_path(&key), version)
        }
        _ => ExportState::Stale,
    }
}

/// Whether the row's index already names the entry the file says is there.
fn export_index_matches(state: &ExportState, path: Option<&str>, version: Option<i32>) -> bool {
    match state {
        ExportState::Current(p, v) => path == Some(p.as_str()) && version == Some(*v),
        _ => true,
    }
}

struct Prepared {
    existing_id: Option<Uuid>,
    relative_path: String,
    directory: String,
    file_name: String,
    file_type: String,
    stat: FileStat,
    change: Option<Change>,
    preparation_ms: f64,
    stages: StageTimings,
}

#[derive(Clone)]
struct ScanContext {
    root: PathBuf,
    configs: Arc<DirectoryMap>,
    default_raft_height_mm: f32,
    generate_previews: bool,
    /// Spin meshes are written (previews are enabled and have a cache folder).
    generate_spin: bool,
    minimum_preview_version: i32,
    /// The oldest export entry version accepted; older entries are written again.
    minimum_export_version: i32,
    exports: Option<SupportExports>,
}

impl ScanContext {
    /// A model file's metadata and raft cutoff.
    fn resolve(&self, full_path: &str, directory: &str) -> (ComputedMetadata, f32) {
        let file_name = Path::new(full_path).file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        let metadata = model_metadata::compute(&rules::rule_path(directory, &file_name), self.configs.get(directory));
        let raft_height_mm = raft_height_mm_for(&metadata, directory, &self.configs, self.default_raft_height_mm);
        (metadata, raft_height_mm)
    }
}

/// The with-supports preview path a model's previews should be stored under.
fn expected_preview_path(checksum: &str, raft_height_mm: f32, supported: Option<bool>) -> String {
    preview::relative_path(&preview::preview_stem(checksum, raft_height_mm, supported), true)
}

/// The spin mesh path a model's spin mesh should be stored under.
fn expected_spin_path(checksum: &str, raft_height_mm: f32, supported: Option<bool>) -> String {
    spin::relative_path(&preview::preview_stem(checksum, raft_height_mm, supported))
}

/// Stores hulls and the support detection, which share the scan config checksum.
fn apply_hull_data(
    model: &mut ModelRow,
    hulls: &hull::Hulls,
    calculated: bool,
    raft_height_mm: f32,
    supports: SupportDetection,
) {
    model.detected_supported = (calculated && supports.confident).then_some(true);
    model.support_parts_found = calculated.then_some(supports.found);
    model.hulls = Some(ModelHulls {
        convex_hull: hulls.convex.clone(),
        concave_hull: hulls.concave.clone(),
        convex_sans_raft_hull: hulls.convex_sans_raft.clone(),
    });
    model.hull_generated_at = calculated.then(now);
    model.hull_generation_version = calculated.then_some(hull::CURRENT_HULL_GENERATION_VERSION);
    model.hull_raft_height_mm = calculated.then_some(raft_height_mm);
    model.scan_config_checksum = calculated.then(|| scan_config::compute(raft_height_mm));
}

fn apply_file_data(model: &mut ModelRow, data: &FileData, stat: &FileStat) {
    model.checksum = data.checksum.clone();
    model.file_size = stat.size;
    model.file_modified_at = stat.modified;
    model.cached_at = now();
    model.preview_image_path = data.preview_image_path.clone();
    model.preview_generated_at = data.preview_generated_at;
    model.preview_generation_version = data.preview_image_path.as_ref().map(|_| CURRENT_PREVIEW_GENERATION_VERSION);
    apply_hull_data(model, &data.hulls, data.geometry.is_some(), data.raft_height_mm, data.supports);
    store::apply_calculated_metadata(model, &data.metadata);
    let g = data.geometry;
    model.dimension_x_mm = g.and_then(|g| g.dimension_x_mm);
    model.dimension_y_mm = g.and_then(|g| g.dimension_y_mm);
    model.dimension_z_mm = g.and_then(|g| g.dimension_z_mm);
    model.sphere_centre_x = g.and_then(|g| g.sphere_centre_x);
    model.sphere_centre_y = g.and_then(|g| g.sphere_centre_y);
    model.sphere_centre_z = g.and_then(|g| g.sphere_centre_z);
    model.sphere_radius = g.and_then(|g| g.sphere_radius);
    model.geometry_calculated_at = g.map(|_| now());
    // New content: the index names the new entry, or none (an unsupported model stores none).
    (model.support_export_path, model.support_export_version) = export_index(&data.export);
    model.spin_mesh_path = data.spin_mesh_path.clone();
    model.spin_mesh_version = data.spin_mesh_path.as_ref().map(|_| CURRENT_SPIN_MESH_VERSION);
}

/// The row's index of an export entry the scan wrote.
fn export_index(export: &ExportOutcome) -> (Option<String>, Option<i32>) {
    match export {
        ExportOutcome::Written(path) => (Some(path.clone()), Some(CURRENT_SUPPORT_READER_VERSION)),
        _ => (None, None),
    }
}

fn apply_geometry_refresh(model: &mut ModelRow, refresh: &GeometryRefresh) {
    if refresh.include_hulls {
        apply_hull_data(model, &refresh.hulls, refresh.geometry_loaded, refresh.raft_height_mm, refresh.supports);
    }
    if refresh.include_hulls || refresh.export != ExportOutcome::None {
        (model.support_export_path, model.support_export_version) = export_index(&refresh.export);
    }
    if refresh.include_spin && refresh.geometry_loaded {
        model.spin_mesh_path = refresh.spin_mesh_path.clone();
        model.spin_mesh_version = refresh.spin_mesh_path.as_ref().map(|_| CURRENT_SPIN_MESH_VERSION);
    }
    if !refresh.include_preview || !refresh.geometry_loaded {
        return;
    }
    model.preview_image_path = refresh.preview_image_path.clone();
    model.preview_generated_at = refresh.preview_generated_at;
    model.preview_generation_version = refresh.preview_image_path.as_ref().map(|_| CURRENT_PREVIEW_GENERATION_VERSION);
}

/// Spin mesh files no model indexes are swept only once they are this old: the scan writes a
/// file before the model's row names it, and a sweep must not take a file between the two.
pub const SPIN_SWEEP_GRACE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Deletes one spin mesh file in the renders folder.
fn remove_spin_file(renders: &Path, relative: &str) -> bool {
    match std::fs::remove_file(renders.join(relative)) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            warn!(target: LOG_TARGET, "Failed to delete spin mesh {relative}: {err}");
            false
        }
    }
}

fn relative_of(root: &Path, file: &Path) -> String {
    file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/")
}

fn split_relative(relative: &str) -> (String, String) {
    match relative.rfind('/') {
        Some(i) => (relative[..i].to_string(), relative[i + 1..].to_string()),
        None => (String::new(), relative.to_string()),
    }
}

fn extension_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Recursively finds model files under `dir`, sorted by path.
pub fn discover_model_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else { continue };
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && is_model_extension(&extension_of(&path)) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

impl ModelService {
    /// Resolves cached preview files for a stem: the with-supports path when either
    /// variant exists (with the oldest variant's timestamp), else the existing stored path
    /// if its file is present.
    fn resolve_cached_preview_metadata(
        &self,
        stem: &str,
        existing: Option<&str>,
    ) -> (Option<String>, Option<DateTime<Utc>>) {
        let Some(renders) = self.preview.as_ref().and_then(|p| p.cache_dir()) else {
            return (existing.map(str::to_string), None);
        };
        let with = preview::relative_path(stem, true);
        let without = preview::relative_path(stem, false);
        let times: Vec<DateTime<Utc>> = [&with, &without]
            .iter()
            .filter_map(|p| std::fs::metadata(renders.join(p)).and_then(|m| m.modified()).ok())
            .map(system_time_utc)
            .collect();
        if let Some(oldest) = times.into_iter().min() {
            return (Some(with), Some(oldest));
        }
        if let Some(existing) = existing.filter(|e| !e.trim().is_empty())
            && let Ok(modified) = std::fs::metadata(renders.join(existing)).and_then(|m| m.modified())
        {
            return (Some(existing.to_string()), Some(system_time_utc(modified)));
        }
        (None, None)
    }

    /// The spin mesh file for a stem when one is in the renders folder (scans that don't write
    /// spin meshes still index the ones there).
    fn resolve_cached_spin(&self, stem: &str) -> Option<String> {
        let renders = self.preview.as_ref().and_then(|p| p.cache_dir())?;
        let relative = spin::relative_path(stem);
        renders.join(&relative).is_file().then_some(relative)
    }

    /// Loads geometry, detects supports, renders previews (when requested), computes hulls,
    /// (when `include_export`) reads a pre-supported model's supports into its export entry and
    /// (when `include_spin`) writes the spin mesh of the supports-removed body.
    #[allow(clippy::too_many_arguments)]
    async fn process_geometry(
        &self,
        file: &Path,
        file_type: &str,
        checksum: &str,
        preview_refresh: PreviewRefresh,
        include_hulls: bool,
        include_export: bool,
        include_spin: bool,
        raft_height_mm: f32,
        supported: Option<bool>,
    ) -> ProcessedGeometry {
        let path = file.to_path_buf();
        let file_type = file_type.to_string();
        let stem = preview::preview_stem(checksum, raft_height_mm, supported);
        let preview = self.preview.clone().filter(|_| preview_refresh.included());
        let redraw = preview_refresh == PreviewRefresh::Redraw;
        let exports = self.support_exports.clone().filter(|_| include_export && supported != Some(false));
        // Indexed under the current scan config, the detection counts as checked.
        let export_key = support_exports::key(checksum, raft_height_mm, supported, true);
        let spin_target = include_spin
            .then(|| self.preview.as_ref().and_then(|p| p.cache_dir()).map(Path::to_path_buf))
            .flatten()
            .map(|dir| (dir, spin::relative_path(&stem)));
        tokio::task::spawn_blocking(move || {
            let mut stages = StageTimings::default();
            let started = Instant::now();
            let geometry = loader::load_model(&path, &file_type);
            stages.load_ms = elapsed_ms(started);
            let Some(geometry) = geometry else {
                return ProcessedGeometry { stages, ..Default::default() };
            };
            let started = Instant::now();
            let classification = spin::checked_classification(&geometry.triangles, raft_height_mm);
            let detected = SupportDetection::of(classification.as_ref());
            let support_mask = classification.and_then(|c| c.mask_for(supported));
            stages.supports_ms = elapsed_ms(started);
            let preview_generated = preview.is_some_and(|p| {
                let started = Instant::now();
                let result = p.generate_variants(&geometry, &stem, support_mask.as_deref(), true, true, redraw);
                stages.preview_ms = elapsed_ms(started);
                result.with_supports.generated || result.without_supports.generated
            });
            // The spin mesh: the body the supports-removed preview draws. Timed with the previews.
            let spin = spin_target.map(|(dir, relative)| {
                let started = Instant::now();
                let stored = spin::store_for_model(&dir, &relative, &geometry.triangles, support_mask.as_deref());
                stages.preview_ms += elapsed_ms(started);
                stored
            });
            let hulls = include_hulls.then(|| {
                let started = Instant::now();
                let hulls = hull::calculate_hulls(&geometry, hull::DEFAULT_MAX_HULL_VERTICES, raft_height_mm);
                stages.hull_ms = elapsed_ms(started);
                hulls
            });
            // A pre-supported model with supports found: its export entry, as an export of it
            // would read it (split by the detection's mask, supports read from the split).
            let mut export = ExportOutcome::None;
            if let Some(exports) = exports.filter(|_| detected.found && presupported(supported, detected.confident)) {
                let started = Instant::now();
                let read = support_mask.and_then(|mask| {
                    let (body, supports) = support_separation::split(&geometry.triangles, &mask);
                    (!supports.is_empty()).then(|| {
                        let body = crate::geometry::to_z_up(&body);
                        let supports = crate::geometry::to_z_up(&supports);
                        let layout = support_reader::read(&body, &supports, raft_height_mm).classification.layout;
                        (mask, layout)
                    })
                });
                export = match exports.store(&export_key, read.as_ref().map(|(m, l)| (m.as_slice(), l))) {
                    Some(path) => ExportOutcome::Written(path),
                    None => ExportOutcome::Failed,
                };
                stages.export_ms = elapsed_ms(started);
            }
            ProcessedGeometry {
                summary: Some(GeometrySummary::of(&geometry)),
                preview_generated,
                hulls,
                supports: detected,
                export,
                spin,
                stages,
            }
        })
        .await
        .unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments)]
    async fn compute_file_data(
        &self,
        ctx: &ScanContext,
        file: &Path,
        file_type: &str,
        directory: &str,
        known_checksum: Option<String>,
        include_preview: bool,
        existing_preview_path: Option<&str>,
    ) -> anyhow::Result<FileData> {
        let started = Instant::now();
        let checksum = match known_checksum {
            Some(c) => c,
            None => compute_checksum(file).await?,
        };
        let checksum_ms = elapsed_ms(started);
        let dir_config: Option<&DirectoryConfig> = ctx.configs.get(directory);
        let (metadata, raft_height_mm) = ctx.resolve(&file.to_string_lossy(), directory);
        let supported = metadata.supported;

        let mut data = FileData {
            checksum: checksum.clone(),
            preview_image_path: None,
            preview_generated_at: None,
            preview_generated: false,
            hulls: hull::Hulls::default(),
            hull_generated: false,
            raft_height_mm,
            supports: SupportDetection::default(),
            metadata,
            dir_config_id: dir_config.map(|d| d.id),
            geometry: None,
            export: ExportOutcome::None,
            spin_mesh_path: None,
            stages: StageTimings { checksum_ms, ..Default::default() },
        };

        if is_non_geometry_type(file_type) {
            return Ok(data);
        }

        let processed = self
            .process_geometry(
                file,
                file_type,
                &checksum,
                if include_preview { PreviewRefresh::Missing } else { PreviewRefresh::Skip },
                true,
                true,
                include_preview && ctx.generate_spin,
                raft_height_mm,
                supported,
            )
            .await;
        data.export = processed.export;
        data.stages = StageTimings { checksum_ms, ..processed.stages };
        let stem = preview::preview_stem(&checksum, raft_height_mm, supported);
        (data.preview_image_path, data.preview_generated_at) =
            self.resolve_cached_preview_metadata(&stem, existing_preview_path);
        data.spin_mesh_path = match processed.spin {
            Some(written) => written,
            None => self.resolve_cached_spin(&stem),
        };
        data.preview_generated = processed.preview_generated;
        if let Some(summary) = processed.summary {
            data.hulls = processed.hulls.unwrap_or_default();
            data.hull_generated = true;
            data.supports = processed.supports;
            data.geometry = Some(summary);
        }
        Ok(data)
    }

    #[allow(clippy::too_many_arguments)]
    async fn compute_geometry_refresh(
        &self,
        file: &Path,
        file_type: &str,
        checksum: &str,
        raft_height_mm: f32,
        supported: Option<bool>,
        preview_refresh: PreviewRefresh,
        include_hulls: bool,
        include_export: bool,
        include_spin: bool,
    ) -> GeometryRefresh {
        let include_preview = preview_refresh.included();
        let processed = self
            .process_geometry(
                file,
                file_type,
                checksum,
                preview_refresh,
                include_hulls,
                include_export,
                include_spin,
                raft_height_mm,
                supported,
            )
            .await;
        let mut refresh = GeometryRefresh {
            include_preview,
            include_hulls,
            geometry_loaded: processed.summary.is_some(),
            preview_image_path: None,
            preview_generated_at: None,
            preview_generated: false,
            hulls: processed.hulls.unwrap_or_default(),
            hull_generated: false,
            raft_height_mm,
            supports: processed.supports,
            export: processed.export,
            include_spin,
            spin_mesh_path: processed.spin.flatten(),
            stages: processed.stages,
        };
        if processed.summary.is_none() {
            return refresh;
        }
        if include_preview {
            (refresh.preview_image_path, refresh.preview_generated_at) =
                self.resolve_cached_preview_metadata(&preview::preview_stem(checksum, raft_height_mm, supported), None);
            refresh.preview_generated = processed.preview_generated;
        }
        refresh.hull_generated = include_hulls;
        refresh
    }

    async fn prepare(
        &self,
        ctx: &ScanContext,
        file: PathBuf,
        existing: Option<ExistingScanState>,
    ) -> anyhow::Result<Option<Prepared>> {
        let started = Instant::now();
        let relative_path = relative_of(&ctx.root, &file);
        let (directory, file_name) = split_relative(&relative_path);
        let file_type = extension_of(&file);
        let file_stat = stat(&file)?;
        let full = file.to_string_lossy().into_owned();

        let mut prepared = Prepared {
            existing_id: existing.as_ref().map(|e| e.id),
            relative_path,
            directory: directory.clone(),
            file_name,
            file_type: file_type.clone(),
            stat: FileStat { size: file_stat.size, modified: file_stat.modified },
            change: None,
            preparation_ms: 0.0,
            stages: StageTimings::default(),
        };

        let Some(existing) = existing else {
            let data =
                self.compute_file_data(ctx, &file, &file_type, &directory, None, ctx.generate_previews, None).await?;
            prepared.stages = data.stages;
            prepared.change = Some(Change::Content(Box::new(data)));
            prepared.preparation_ms = started.elapsed().as_secs_f64() * 1000.0;
            return Ok(Some(prepared));
        };

        let checksum_started = Instant::now();
        let (checksum, recomputed) =
            resolve_checksum(&file, file_stat.modified, Some(&existing.checksum), Some(existing.file_modified_at))
                .await?;
        let checksum_ms = elapsed_ms(checksum_started);
        let (metadata, expected_raft) = ctx.resolve(&full, &directory);
        let supported = metadata.supported;
        let geometry_type = !is_non_geometry_type(&file_type);
        let hull_mismatch =
            geometry_type && needs_hull_regeneration(existing.scan_config_checksum.as_deref(), expected_raft);
        let preview_stale = ctx.generate_previews
            && geometry_type
            && needs_preview_regeneration(
                existing.preview_image_path.as_deref(),
                existing.preview_generation_version,
                ctx.minimum_preview_version,
                &expected_preview_path(&checksum, expected_raft, supported),
            );
        let spin_stale = ctx.generate_spin
            && geometry_type
            && needs_spin_regeneration(
                existing.spin_mesh_path.as_deref(),
                existing.spin_mesh_version,
                &expected_spin_path(&checksum, expected_raft, supported),
            );

        // The export entry, from its file, when nothing else rewrites it.
        let export = if checksum == existing.checksum && !hull_mismatch {
            export_state(
                ctx.exports.as_ref(),
                geometry_type,
                &checksum,
                expected_raft,
                supported,
                existing.detected_supported,
                existing.support_parts_found,
                ctx.minimum_export_version,
            )
        } else {
            ExportState::NotWanted
        };
        let export_stale = export == ExportState::Stale;

        prepared.change = if checksum == existing.checksum
            && !hull_mismatch
            && !preview_stale
            && !export_stale
            && !spin_stale
        {
            if let ExportState::Current(path, version) = &export
                && !export_index_matches(
                    &export,
                    existing.support_export_path.as_deref(),
                    existing.support_export_version,
                )
            {
                // The file is the authority: the row's index follows it.
                Some(Change::ExportIndex(path.clone(), *version))
            } else if !recomputed {
                return Ok(None);
            } else {
                Some(Change::FileMetadataOnly)
            }
        } else if checksum != existing.checksum {
            let data = self
                .compute_file_data(
                    ctx,
                    &file,
                    &file_type,
                    &directory,
                    Some(checksum),
                    ctx.generate_previews,
                    existing.preview_image_path.as_deref(),
                )
                .await?;
            Some(Change::Content(Box::new(data)))
        } else if hull_mismatch {
            info!(
                target: LOG_TARGET,
                "Hull config mismatch detected, regenerating hulls for: {full} (stored checksum: {:?}, expected checksum: {})",
                existing.scan_config_checksum,
                scan_config::compute(expected_raft)
            );
            Some(Change::Geometry(
                self.compute_geometry_refresh(
                    &file,
                    &file_type,
                    &checksum,
                    expected_raft,
                    supported,
                    PreviewRefresh::for_stale(
                        preview_stale,
                        existing.preview_generation_version,
                        ctx.minimum_preview_version,
                    ),
                    true,
                    true,
                    spin_stale,
                )
                .await,
            ))
        } else {
            if preview_stale {
                info!(
                    target: LOG_TARGET,
                    "Preview version mismatch, regenerating preview for: {full} (stored version: {:?}, required minimum version: {}, current version: {CURRENT_PREVIEW_GENERATION_VERSION})",
                    existing.preview_generation_version,
                    ctx.minimum_preview_version
                );
            }
            if export_stale {
                info!(
                    target: LOG_TARGET,
                    "Support export missing or older than version {}, reading supports for: {full}",
                    ctx.minimum_export_version
                );
            }
            if spin_stale {
                info!(
                    target: LOG_TARGET,
                    "Spin mesh missing or not at version {CURRENT_SPIN_MESH_VERSION}, writing it for: {full}"
                );
            }
            Some(Change::Geometry(
                self.compute_geometry_refresh(
                    &file,
                    &file_type,
                    &checksum,
                    expected_raft,
                    supported,
                    PreviewRefresh::for_stale(
                        preview_stale,
                        existing.preview_generation_version,
                        ctx.minimum_preview_version,
                    ),
                    false,
                    export_stale,
                    spin_stale,
                )
                .await,
            ))
        };

        prepared.stages = match &prepared.change {
            Some(Change::Content(data)) => {
                StageTimings { checksum_ms: checksum_ms + data.stages.checksum_ms, ..data.stages }
            }
            Some(Change::Geometry(refresh)) => StageTimings { checksum_ms, ..refresh.stages },
            _ => StageTimings { checksum_ms, ..Default::default() },
        };
        prepared.preparation_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok(Some(prepared))
    }

    async fn scan_context(&self, root: &Path, relative_dirs: &[String]) -> anyhow::Result<ScanContext> {
        let configs = self.metadata_config.ensure_directory_configs(root, relative_dirs).await?;
        let app_config = self.app_config.get().await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(ScanContext {
            root: root.to_path_buf(),
            configs: Arc::new(configs),
            default_raft_height_mm: app_config.default_raft_height_mm,
            generate_previews: app_config.generate_previews_enabled,
            generate_spin: app_config.generate_previews_enabled
                && self.preview.as_ref().and_then(|p| p.cache_dir()).is_some(),
            minimum_preview_version: resolve_minimum_preview_generation_version(
                app_config.minimum_preview_generation_version,
            ),
            minimum_export_version: normalize_minimum_support_export_version(app_config.minimum_support_export_version),
            exports: self.support_exports.clone(),
        })
    }

    async fn load_existing_states(
        &self,
    ) -> sqlx::Result<std::collections::HashMap<(String, String), ExistingScanState>> {
        let rows: Vec<ExistingScanState> = sqlx::query_as(
            "SELECT id, directory, file_name, checksum, file_modified_at, scan_config_checksum, preview_generation_version,
                preview_image_path, detected_supported, support_parts_found, support_export_path, support_export_version,
                spin_mesh_path, spin_mesh_version
             FROM models",
        )
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(|row| ((row.directory.clone(), row.file_name.clone()), row)).collect())
    }

    /// Scans all model files (optionally below `directory_filter`) and updates the cache.
    /// Returns the number of new models indexed.
    pub async fn scan_and_cache(
        &self,
        limit: Option<usize>,
        directory_filter: Option<&str>,
        reporter: Option<&dyn ProgressReporter>,
        cancel: &CancellationToken,
    ) -> anyhow::Result<usize> {
        let Some(root) = self.root() else {
            warn!(target: LOG_TARGET, "Models directory is not configured");
            return Ok(0);
        };
        if !root.is_dir() {
            warn!(target: LOG_TARGET, "Models directory not accessible: {}", root.display());
            return Ok(0);
        }
        let search_root = match directory_filter {
            Some(f) => root.join(f),
            None => root.clone(),
        };
        if !search_root.is_dir() {
            warn!(target: LOG_TARGET, "Filter directory not found: {}", search_root.display());
            return Ok(0);
        }

        let walk = search_root.clone();
        let files = tokio::task::spawn_blocking(move || discover_model_files(&walk)).await?;
        let relative_paths: Vec<String> = files.iter().map(|f| relative_of(&root, f)).collect();

        if let Some(r) = reporter {
            r.on_scan_started(files.len()).await;
            let plans: Vec<FilePlan> = files
                .iter()
                .zip(&relative_paths)
                .map(|(f, rel)| FilePlan { relative_path: rel.clone(), file_type: extension_of(f) })
                .collect();
            r.on_files_discovered(&plans).await;
        }
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }

        let mut relative_dirs: Vec<String> = relative_paths.iter().map(|r| split_relative(r).0).collect();
        relative_dirs.sort();
        relative_dirs.dedup();

        let ctx = self.scan_context(&root, &relative_dirs).await?;
        let mut existing = self.load_existing_states().await?;

        info!(target: LOG_TARGET, "Scanning {} model files with up to {} concurrent workers.", files.len(), self.file_scan_threads);

        let inputs: Vec<(PathBuf, Option<ExistingScanState>)> = files
            .iter()
            .zip(&relative_paths)
            .map(|(f, rel)| {
                let (dir, name) = split_relative(rel);
                (f.clone(), existing.remove(&(dir, name)))
            })
            .collect();

        let mut stream = futures::stream::iter(inputs.into_iter().map(|(file, state)| {
            let ctx = ctx.clone();
            let cancel = cancel.clone();
            async move {
                if cancel.is_cancelled() {
                    return (file, Err(anyhow::Error::new(Cancelled)));
                }
                let result = self.prepare(&ctx, file.clone(), state).await;
                (file, result)
            }
        }))
        .buffer_unordered(self.file_scan_threads);

        let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut new_count = 0usize;
        let mut updated_count = 0usize;

        while let Some((file, result)) = stream.next().await {
            if cancel.is_cancelled() {
                return Err(Cancelled.into());
            }
            let prepared = match result {
                Ok(Some(p)) => p,
                Ok(None) => continue,
                Err(err) => {
                    if err.is::<Cancelled>() {
                        return Err(err);
                    }
                    let relative = relative_of(&root, &file);
                    warn!(target: LOG_TARGET, "Failed to scan {relative}: {err:#}");
                    processed.insert(relative.clone());
                    if let Some(r) = reporter {
                        r.on_file_processed(FileResult {
                            relative_path: relative,
                            file_type: extension_of(&file),
                            status: "failed".into(),
                            is_new: false,
                            was_updated: false,
                            generated_preview: false,
                            generated_hull: false,
                            message: format!("Unable to compute file data: {err}"),
                            duration_ms: 0.0,
                            stages: StageTimings::default(),
                        })
                        .await;
                    }
                    continue;
                }
            };
            let timer = Instant::now();
            processed.insert(prepared.relative_path.clone());

            if let Some(existing_id) = prepared.existing_id {
                let Some(mut entity) = store::get_model(&self.db, existing_id).await? else { continue };
                let previous_export = entity.support_export_path.clone();
                let previous_spin = entity.spin_mesh_path.clone();
                let (mut generated_preview, mut generated_hull, mut was_updated) = (false, false, false);
                let mut message = "No cache changes required".to_string();
                let mut status = "skipped";

                match &prepared.change {
                    Some(Change::Content(data)) => {
                        info!(target: LOG_TARGET, "Model content changed, updating: {}", prepared.relative_path);
                        entity.file_type = prepared.file_type.clone();
                        entity.directory_config_id = data.dir_config_id;
                        apply_file_data(&mut entity, data, &prepared.stat);
                        generated_preview = data.preview_generated;
                        generated_hull = data.hull_generated;
                        was_updated = true;
                        status = "processed";
                        message = "Content changed; rebuilt cached metadata and geometry".into();
                    }
                    Some(Change::Geometry(refresh)) => {
                        apply_geometry_refresh(&mut entity, refresh);
                        entity.cached_at = now();
                        entity.file_modified_at = prepared.stat.modified;
                        entity.file_size = prepared.stat.size;
                        generated_preview = refresh.preview_generated;
                        generated_hull = refresh.hull_generated;
                        was_updated = true;
                        status = "processed";
                        message = if refresh.include_hulls {
                            "Regenerated hulls/preview from config or version changes".into()
                        } else if refresh.include_preview {
                            "Regenerated preview from version changes".into()
                        } else if refresh.include_spin && refresh.export == ExportOutcome::None {
                            "Wrote the spin mesh".into()
                        } else {
                            "Rebuilt the support export entry".into()
                        };
                    }
                    Some(Change::ExportIndex(path, version)) => {
                        entity.support_export_path = Some(path.clone());
                        entity.support_export_version = Some(*version);
                        entity.cached_at = now();
                        entity.file_modified_at = prepared.stat.modified;
                        entity.file_size = prepared.stat.size;
                        was_updated = true;
                        status = "processed";
                        message = "Indexed the support export entry found on disk".into();
                    }
                    Some(Change::FileMetadataOnly) => {
                        entity.cached_at = now();
                        entity.file_modified_at = prepared.stat.modified;
                        entity.file_size = prepared.stat.size;
                        was_updated = true;
                        status = "processed";
                        message = "Updated file timestamp after checksum verification".into();
                    }
                    None => {}
                }

                if was_updated {
                    let mut conn = self.db.acquire().await?;
                    store::save_model(&mut conn, &entity).await?;
                    drop(conn);
                    self.release_old_export(previous_export.as_deref(), entity.support_export_path.as_deref()).await;
                    self.release_old_spin(previous_spin.as_deref(), entity.spin_mesh_path.as_deref()).await;
                }
                if let Some(r) = reporter {
                    r.on_file_processed(FileResult {
                        relative_path: prepared.relative_path.clone(),
                        file_type: prepared.file_type.clone(),
                        status: status.into(),
                        is_new: false,
                        was_updated,
                        generated_preview,
                        generated_hull,
                        message,
                        duration_ms: prepared.preparation_ms + timer.elapsed().as_secs_f64() * 1000.0,
                        stages: prepared.stages,
                    })
                    .await;
                }
                updated_count += 1;
                continue;
            }

            if limit.is_some_and(|l| new_count >= l) {
                if let Some(r) = reporter {
                    r.on_file_processed(FileResult {
                        relative_path: prepared.relative_path.clone(),
                        file_type: prepared.file_type.clone(),
                        status: "skipped".into(),
                        is_new: true,
                        was_updated: false,
                        generated_preview: false,
                        generated_hull: false,
                        message: "Skipped due to scan limit".into(),
                        duration_ms: timer.elapsed().as_secs_f64() * 1000.0,
                        stages: prepared.stages,
                    })
                    .await;
                }
                continue;
            }

            let Some(Change::Content(data)) = &prepared.change else { continue };
            info!(target: LOG_TARGET, "Discovered {} model: {}", prepared.file_type.to_uppercase(), prepared.relative_path);
            let mut entity = ModelRow {
                id: Uuid::new_v4(),
                file_name: prepared.file_name.clone(),
                directory: prepared.directory.clone(),
                file_type: prepared.file_type.clone(),
                directory_config_id: data.dir_config_id,
                ..Default::default()
            };
            apply_file_data(&mut entity, data, &prepared.stat);
            {
                let mut conn = self.db.acquire().await?;
                store::save_model(&mut conn, &entity).await?;
            }
            if let Some(r) = reporter {
                r.on_file_processed(FileResult {
                    relative_path: prepared.relative_path.clone(),
                    file_type: prepared.file_type.clone(),
                    status: "processed".into(),
                    is_new: true,
                    was_updated: false,
                    generated_preview: data.preview_image_path.is_some(),
                    generated_hull: data.hulls != hull::Hulls::default(),
                    message: "Indexed new model file".into(),
                    duration_ms: prepared.preparation_ms + timer.elapsed().as_secs_f64() * 1000.0,
                    stages: prepared.stages,
                })
                .await;
            }
            new_count += 1;
        }

        if let Some(r) = reporter {
            for (file, relative) in files.iter().zip(&relative_paths) {
                if processed.contains(relative) {
                    continue;
                }
                if cancel.is_cancelled() {
                    return Err(Cancelled.into());
                }
                r.on_file_processed(FileResult {
                    relative_path: relative.clone(),
                    file_type: extension_of(file),
                    status: "skipped".into(),
                    is_new: false,
                    was_updated: false,
                    generated_preview: false,
                    generated_hull: false,
                    message: "Already up-to-date".into(),
                    duration_ms: 0.0,
                    stages: StageTimings::default(),
                })
                .await;
            }
        }

        // A full scan has brought every row's index up to date: entry files no row names are left
        // over from changed or removed models (or from exports of a detection since rechecked).
        if directory_filter.is_none()
            && limit.is_none()
            && let Some(exports) = &self.support_exports
        {
            let removed = exports.sweep(&self.db, support_exports::SWEEP_GRACE).await;
            if removed > 0 {
                info!(target: LOG_TARGET, "Removed {removed} support export files no model uses.");
            }
        }
        // Repaired meshes of changed or removed models, or of an older repair version.
        if directory_filter.is_none()
            && limit.is_none()
            && let Some(repaired) = &self.repaired_meshes
        {
            let removed = repaired.sweep(&self.db, repaired_meshes::SWEEP_GRACE).await;
            if removed > 0 {
                info!(target: LOG_TARGET, "Removed {removed} repaired mesh files no model uses.");
            }
        }
        // Spin meshes of changed or removed models, or of an older spin mesh version.
        if directory_filter.is_none() && limit.is_none() {
            let removed = self.sweep_spin_meshes(SPIN_SWEEP_GRACE).await;
            if removed > 0 {
                info!(target: LOG_TARGET, "Removed {removed} spin mesh files no model uses.");
            }
        }

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM models").fetch_one(&self.db).await?;
        info!(target: LOG_TARGET, "Scan complete: {new_count} new, {updated_count} updated, {total} total in cache.");
        Ok(new_count)
    }

    /// Deletes the export entry a model's row named before it was saved, when it names another
    /// one now and no other model (with the same content) still uses it.
    async fn release_old_export(&self, before: Option<&str>, now: Option<&str>) {
        if let (Some(before), Some(exports)) = (before, &self.support_exports)
            && Some(before) != now
        {
            exports.release(&self.db, before).await;
        }
    }

    /// Deletes the spin mesh a model's row named before it was saved, when it names another one
    /// now and no other model (with the same content) still uses it.
    pub(crate) async fn release_old_spin(&self, before: Option<&str>, now: Option<&str>) {
        let Some(renders) = self.preview.as_ref().and_then(|p| p.cache_dir()) else { return };
        let Some(before) = before.filter(|b| Some(*b) != now) else { return };
        let used: sqlx::Result<bool> =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM models WHERE spin_mesh_path = ?)")
                .bind(before)
                .fetch_one(&self.db)
                .await;
        match used {
            Ok(false) => {
                remove_spin_file(renders, before);
            }
            Ok(true) => {}
            Err(err) => warn!(target: LOG_TARGET, "Failed to check who uses spin mesh {before}: {err}"),
        }
    }

    /// Deletes the spin mesh files in the renders folder that no model indexes, of those last
    /// written before `grace` ago (a scan writes a file before its row names it). Returns how many
    /// were deleted. Run at the end of a full scan only, when every row's index is up to date.
    pub async fn sweep_spin_meshes(&self, grace: std::time::Duration) -> usize {
        let Some(renders) = self.preview.as_ref().and_then(|p| p.cache_dir()).map(Path::to_path_buf) else {
            return 0;
        };
        let indexed: std::collections::HashSet<String> = match sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT spin_mesh_path FROM models WHERE spin_mesh_path IS NOT NULL",
        )
        .fetch_all(&self.db)
        .await
        {
            Ok(paths) => paths.into_iter().collect(),
            Err(err) => {
                warn!(target: LOG_TARGET, "Failed to list indexed spin meshes: {err}");
                return 0;
            }
        };
        tokio::task::spawn_blocking(move || {
            let cutoff = SystemTime::now().checked_sub(grace).unwrap_or(SystemTime::UNIX_EPOCH);
            let Ok(entries) = std::fs::read_dir(&renders) else { return 0 };
            let mut removed = 0;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let old = entry.metadata().and_then(|m| m.modified()).is_ok_and(|m| m < cutoff);
                if spin::is_spin_file(&name) && old && !indexed.contains(&name) && remove_spin_file(&renders, &name) {
                    removed += 1;
                }
            }
            removed
        })
        .await
        .unwrap_or(0)
    }

    /// Indexes a single model file. Returns false when nothing needed updating.
    pub async fn scan_and_cache_single(
        &self,
        relative_model_path: &str,
        reporter: Option<&dyn ProgressReporter>,
        cancel: &CancellationToken,
    ) -> anyhow::Result<bool> {
        let Some(root) = self.root() else {
            warn!(target: LOG_TARGET, "Models directory is not configured");
            return Ok(false);
        };
        if !root.is_dir() {
            warn!(target: LOG_TARGET, "Models directory not accessible: {}", root.display());
            return Ok(false);
        }
        if relative_model_path.trim().is_empty() {
            warn!(target: LOG_TARGET, "Single-model indexing called with empty relative path");
            return Ok(false);
        }

        let normalized = relative_model_path.replace('\\', "/").trim_start_matches('/').to_string();
        let full_path = std::path::absolute(root.join(&normalized))?;
        let root_full = std::path::absolute(&root)?;
        if normalized.split('/').any(|s| s == "..") || !full_path.starts_with(&root_full) {
            warn!(target: LOG_TARGET, "Rejected model path outside models root: {relative_model_path}");
            return Ok(false);
        }
        if !full_path.is_file() {
            warn!(target: LOG_TARGET, "Model file not found: {relative_model_path}");
            return Ok(false);
        }
        let file_type = extension_of(&full_path);
        if !is_model_extension(&file_type) {
            warn!(target: LOG_TARGET, "Unsupported model extension for single-model indexing: {}", full_path.display());
            return Ok(false);
        }

        let (directory, file_name) = split_relative(&normalized);
        let file_stat = stat(&full_path)?;
        let timer = Instant::now();

        if let Some(r) = reporter {
            r.on_scan_started(1).await;
            r.on_files_discovered(&[FilePlan { relative_path: normalized.clone(), file_type: file_type.clone() }])
                .await;
        }

        let ctx = self.scan_context(&root, std::slice::from_ref(&directory)).await?;
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        let full_text = full_path.to_string_lossy().into_owned();
        let (metadata, expected_raft) = ctx.resolve(&full_text, &directory);
        let supported = metadata.supported;

        let existing: Option<ModelRow> = sqlx::query_as("SELECT * FROM models WHERE directory = ? AND file_name = ?")
            .bind(&directory)
            .bind(&file_name)
            .fetch_optional(&self.db)
            .await?;
        let is_new = existing.is_none();

        let checksum_started = Instant::now();
        let (checksum, recomputed) = resolve_checksum(
            &full_path,
            file_stat.modified,
            existing.as_ref().map(|e| e.checksum.as_str()),
            existing.as_ref().map(|e| e.file_modified_at),
        )
        .await?;
        let checksum_ms = elapsed_ms(checksum_started);
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }

        let geometry_type = !is_non_geometry_type(&file_type);
        let preview_stale = existing.as_ref().is_some_and(|e| {
            ctx.generate_previews
                && geometry_type
                && needs_preview_regeneration(
                    e.preview_image_path.as_deref(),
                    e.preview_generation_version,
                    ctx.minimum_preview_version,
                    &expected_preview_path(&checksum, expected_raft, supported),
                )
        });
        let hull_stale = existing.as_ref().is_some_and(|e| {
            geometry_type && needs_hull_regeneration(e.scan_config_checksum.as_deref(), expected_raft)
        });
        let spin_stale = existing.as_ref().is_some_and(|e| {
            ctx.generate_spin
                && geometry_type
                && needs_spin_regeneration(
                    e.spin_mesh_path.as_deref(),
                    e.spin_mesh_version,
                    &expected_spin_path(&checksum, expected_raft, supported),
                )
        });

        let export = match &existing {
            Some(e) if e.checksum == checksum && !hull_stale => export_state(
                ctx.exports.as_ref(),
                geometry_type,
                &checksum,
                expected_raft,
                supported,
                e.detected_supported,
                e.support_parts_found,
                ctx.minimum_export_version,
            ),
            _ => ExportState::NotWanted,
        };
        let export_stale = export == ExportState::Stale;

        if let Some(e) = &existing
            && e.checksum == checksum
            && !hull_stale
            && !preview_stale
            && !export_stale
            && !spin_stale
        {
            let index_matches =
                export_index_matches(&export, e.support_export_path.as_deref(), e.support_export_version);
            if recomputed || !index_matches {
                let mut updated = e.clone();
                updated.file_modified_at = file_stat.modified;
                updated.file_size = file_stat.size;
                updated.cached_at = now();
                if let ExportState::Current(path, version) = &export {
                    // The file is the authority: the row's index follows it.
                    updated.support_export_path = Some(path.clone());
                    updated.support_export_version = Some(*version);
                }
                let mut conn = self.db.acquire().await?;
                store::save_model(&mut conn, &updated).await?;
                drop(conn);
                self.release_old_export(e.support_export_path.as_deref(), updated.support_export_path.as_deref()).await;
            }
            debug!(target: LOG_TARGET, "Single-model scan skipped changes for {full_text}; checksum/hulls/preview are up-to-date.");
            if let Some(r) = reporter {
                r.on_file_processed(FileResult {
                    relative_path: normalized,
                    file_type,
                    status: "skipped".into(),
                    is_new: false,
                    was_updated: false,
                    generated_preview: false,
                    generated_hull: false,
                    message: "Already up-to-date".into(),
                    duration_ms: timer.elapsed().as_secs_f64() * 1000.0,
                    stages: StageTimings { checksum_ms, ..Default::default() },
                })
                .await;
            }
            return Ok(false);
        }

        let previous_export = existing.as_ref().and_then(|e| e.support_export_path.clone());
        let previous_spin = existing.as_ref().and_then(|e| e.spin_mesh_path.clone());
        let (entity, generated_preview, generated_hull, stages) = match existing {
            None => {
                info!(target: LOG_TARGET, "Indexing single model file: {full_text}");
                let data = self
                    .compute_file_data(
                        &ctx,
                        &full_path,
                        &file_type,
                        &directory,
                        Some(checksum),
                        ctx.generate_previews,
                        None,
                    )
                    .await?;
                let mut entity = ModelRow {
                    id: Uuid::new_v4(),
                    file_name: file_name.clone(),
                    directory: directory.clone(),
                    file_type: file_type.clone(),
                    directory_config_id: data.dir_config_id,
                    ..Default::default()
                };
                apply_file_data(&mut entity, &data, &file_stat);
                if let Some(r) = reporter {
                    r.on_log("info", "Discovered new model", Some(&normalized)).await;
                }
                (entity, data.preview_generated, data.hull_generated, data.stages)
            }
            Some(mut entity) if entity.checksum != checksum => {
                info!(target: LOG_TARGET, "Model content changed, updating single file: {full_text}");
                let data = self
                    .compute_file_data(
                        &ctx,
                        &full_path,
                        &file_type,
                        &directory,
                        Some(checksum),
                        ctx.generate_previews,
                        entity.preview_image_path.clone().as_deref(),
                    )
                    .await?;
                entity.file_type = file_type.clone();
                entity.directory_config_id = data.dir_config_id;
                apply_file_data(&mut entity, &data, &file_stat);
                (entity, data.preview_generated, data.hull_generated, data.stages)
            }
            Some(mut entity) if hull_stale => {
                info!(
                    target: LOG_TARGET,
                    "Single-model index refreshing hulls due to config mismatch: {full_text} (stored checksum: {:?}, expected checksum: {})",
                    entity.scan_config_checksum,
                    scan_config::compute(expected_raft)
                );
                entity.file_type = file_type.clone();
                if let Some(dc) = ctx.configs.get(&directory) {
                    entity.directory_config_id = Some(dc.id);
                }
                let refresh = self
                    .compute_geometry_refresh(
                        &full_path,
                        &file_type,
                        &entity.checksum.clone(),
                        expected_raft,
                        supported,
                        PreviewRefresh::for_stale(
                            preview_stale,
                            entity.preview_generation_version,
                            ctx.minimum_preview_version,
                        ),
                        true,
                        true,
                        spin_stale,
                    )
                    .await;
                apply_geometry_refresh(&mut entity, &refresh);
                entity.cached_at = now();
                entity.file_modified_at = file_stat.modified;
                entity.file_size = file_stat.size;
                (entity, refresh.preview_generated, refresh.hull_generated, refresh.stages)
            }
            Some(mut entity) => {
                info!(target: LOG_TARGET, "Single-model index refreshing preview, support export or spin mesh: {full_text}");
                let refresh = self
                    .compute_geometry_refresh(
                        &full_path,
                        &file_type,
                        &entity.checksum.clone(),
                        expected_raft,
                        supported,
                        PreviewRefresh::for_stale(
                            preview_stale,
                            entity.preview_generation_version,
                            ctx.minimum_preview_version,
                        ),
                        false,
                        export_stale,
                        spin_stale,
                    )
                    .await;
                apply_geometry_refresh(&mut entity, &refresh);
                entity.cached_at = now();
                entity.file_modified_at = file_stat.modified;
                entity.file_size = file_stat.size;
                (entity, refresh.preview_generated, false, refresh.stages)
            }
        };

        {
            let mut conn = self.db.acquire().await?;
            store::save_model(&mut conn, &entity).await?;
        }
        self.release_old_export(previous_export.as_deref(), entity.support_export_path.as_deref()).await;
        self.release_old_spin(previous_spin.as_deref(), entity.spin_mesh_path.as_deref()).await;

        if let Some(r) = reporter {
            r.on_file_processed(FileResult {
                relative_path: normalized,
                file_type,
                status: "processed".into(),
                is_new,
                was_updated: true,
                generated_preview,
                generated_hull,
                message: "Indexed single model".into(),
                duration_ms: timer.elapsed().as_secs_f64() * 1000.0,
                stages: StageTimings { checksum_ms: checksum_ms + stages.checksum_ms, ..stages },
            })
            .await;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::models::tests::service;
    use crate::test_support::{SIMPLE_ASCII_STL, TestDb, model, write_file};

    #[test]
    fn needs_preview_regeneration_uses_minimum_version() {
        for (path, stored, minimum, expected) in [
            (None, None, 0, true),
            (Some("preview.png"), None, 0, true),
            (Some("preview.png"), Some(5), 5, false),
            (Some("preview.png"), Some(4), 5, true),
            (Some("preview.png"), Some(6), 5, false),
            (Some("other.png"), Some(6), 5, true),
        ] {
            assert_eq!(
                needs_preview_regeneration(path, stored, minimum, "preview.png"),
                expected,
                "{path:?} {stored:?} {minimum}"
            );
        }
    }

    #[test]
    fn model_level_raft_override_changes_expected_checksum() {
        let mut configs = DirectoryMap::new();
        configs.insert(
            String::new(),
            DirectoryConfig {
                raw_model_metadata_json: Some(r#"{"dragon.stl":{"raftHeightMm":7.5}}"#.into()),
                raft_height_mm: Some(2.0),
                ..Default::default()
            },
        );
        let expected = resolve_raft_height_mm_for_model("/models/dragon.stl", "", &configs, 2.0);
        assert_eq!(expected, 7.5);
        assert!(needs_hull_regeneration(Some(&scan_config::compute(2.0)), expected));
        assert!(!needs_hull_regeneration(Some(&scan_config::compute(7.5)), expected));
    }

    async fn disable_previews(t: &TestDb) {
        sqlx::query("INSERT OR REPLACE INTO app_config (id, generate_previews_enabled, minimum_preview_generation_version, updated_at) VALUES (1, 0, ?, ?)")
            .bind(CURRENT_PREVIEW_GENERATION_VERSION)
            .bind(now())
            .execute(&t.db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn single_scan_indexes_non_geometry_file_without_geometry_artifacts() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "sample.ctb", [0x43, 0x54, 0x42, 0x00, 0x01, 0x02]);
        let svc = service(&t, root.path());
        let cancel = CancellationToken::new();

        assert!(svc.scan_and_cache_single("sample.ctb", None, &cancel).await.unwrap());
        let id: Uuid =
            sqlx::query_scalar("SELECT id FROM models WHERE file_name = 'sample.ctb'").fetch_one(&t.db).await.unwrap();
        let m = store::get_model(&t.db, id).await.unwrap().unwrap();
        assert_eq!(m.file_type, "ctb");
        assert!(m.preview_image_path.is_none() && m.preview_generation_version.is_none());
        assert_eq!(m.hulls, Some(ModelHulls::default()));
        assert!(m.scan_config_checksum.is_none() && m.geometry_calculated_at.is_none());

        assert!(!svc.scan_and_cache_single("sample.ctb", None, &cancel).await.unwrap());
    }

    #[tokio::test]
    async fn single_scan_preserves_existing_preview_when_previews_disabled() {
        let t = TestDb::new().await;
        disable_previews(&t).await;
        let root = tempfile::tempdir().unwrap();
        let path = write_file(root.path(), "dragon.stl", SIMPLE_ASCII_STL);
        let checksum = crate::metadata::config_reader::sha256_hex(&std::fs::read(&path).unwrap());

        let mut m = model("dragon.stl", "");
        m.checksum = "oldchecksum".into();
        m.preview_image_path = Some("oldchecksum.png".into());
        m.preview_generation_version = Some(CURRENT_PREVIEW_GENERATION_VERSION);
        m.scan_config_checksum = Some(scan_config::compute(hull::DEFAULT_RAFT_HEIGHT_MM));
        m.file_modified_at = stat(&path).unwrap().modified - chrono::Duration::minutes(1);
        t.insert_model(&m).await;

        assert!(
            service(&t, root.path())
                .scan_and_cache_single("dragon.stl", None, &CancellationToken::new())
                .await
                .unwrap()
        );
        let updated = store::get_model(&t.db, m.id).await.unwrap().unwrap();
        assert_eq!(updated.preview_image_path.as_deref(), Some("oldchecksum.png"));
        assert_eq!(updated.preview_generation_version, Some(CURRENT_PREVIEW_GENERATION_VERSION));
        assert_eq!(updated.checksum, checksum);
        assert!(updated.hulls.is_some_and(|h| h.convex_hull.is_some()));
        assert!(updated.dimension_x_mm.is_some());
    }

    #[tokio::test]
    async fn single_scan_skips_preview_refresh_when_version_meets_minimum() {
        let t = TestDb::new().await;
        let accepted = CURRENT_PREVIEW_GENERATION_VERSION - 1;
        sqlx::query("INSERT INTO app_config (id, generate_previews_enabled, minimum_preview_generation_version, updated_at) VALUES (1, 1, ?, ?)")
            .bind(accepted)
            .bind(now())
            .execute(&t.db)
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = write_file(root.path(), "dragon.stl", SIMPLE_ASCII_STL);
        let checksum = crate::metadata::config_reader::sha256_hex(&std::fs::read(&path).unwrap());

        let mut m = model("dragon.stl", "");
        m.checksum = checksum.clone();
        m.preview_image_path = Some(expected_preview_path(&checksum, hull::DEFAULT_RAFT_HEIGHT_MM, None));
        m.preview_generation_version = Some(accepted);
        m.scan_config_checksum = Some(scan_config::compute(hull::DEFAULT_RAFT_HEIGHT_MM));
        m.file_modified_at = stat(&path).unwrap().modified;
        t.insert_model(&m).await;

        assert!(
            !service(&t, root.path())
                .scan_and_cache_single("dragon.stl", None, &CancellationToken::new())
                .await
                .unwrap()
        );
        let unchanged = store::get_model(&t.db, m.id).await.unwrap().unwrap();
        assert_eq!(unchanged.preview_generation_version, Some(accepted));
    }

    /// A model service rendering previews into `cache` on the CPU.
    fn service_with_previews(t: &TestDb, root: &Path, cache: &Path) -> ModelService {
        use crate::config::AppConfigDefaults;
        use crate::preview::{GpuState, PreviewService};
        use crate::services::app_config::AppConfigService;
        use crate::services::metadata_config::MetadataConfigService;
        use parking_lot::RwLock;
        let models_root = Arc::new(RwLock::new(Some(root.to_path_buf())));
        let metadata = MetadataConfigService::new(t.db.clone(), models_root.clone(), 1000);
        let app_config = AppConfigService::new(t.db.clone(), AppConfigDefaults::default(), None, models_root.clone());
        let preview = PreviewService::new(Some(cache.to_path_buf()), false, Arc::new(GpuState::default()));
        ModelService::new(
            t.db.clone(),
            models_root,
            Some(preview),
            metadata,
            app_config,
            4,
            Some(crate::services::models::tests::test_exports(root)),
        )
    }

    /// An indexed model whose preview files are in the cache (as `old` placeholders) and whose
    /// stored preview version is `version`; previews required at the current version.
    async fn model_with_cached_preview(
        t: &TestDb,
        root: &Path,
        cache: &Path,
        version: i32,
    ) -> (ModelRow, Vec<PathBuf>) {
        sqlx::query("INSERT OR REPLACE INTO app_config (id, generate_previews_enabled, minimum_preview_generation_version, updated_at) VALUES (1, 1, ?, ?)")
            .bind(CURRENT_PREVIEW_GENERATION_VERSION)
            .bind(now())
            .execute(&t.db)
            .await
            .unwrap();
        let path = write_file(root, "dragon.stl", SIMPLE_ASCII_STL);
        let checksum = crate::metadata::config_reader::sha256_hex(&std::fs::read(&path).unwrap());
        let stem = preview::preview_stem(&checksum, hull::DEFAULT_RAFT_HEIGHT_MM, None);
        let files: Vec<PathBuf> = [true, false].map(|with| cache.join(preview::relative_path(&stem, with))).into();
        for f in &files {
            std::fs::write(f, b"old").unwrap();
        }
        let mut m = model("dragon.stl", "");
        m.checksum = checksum.clone();
        m.preview_image_path = Some(expected_preview_path(&checksum, hull::DEFAULT_RAFT_HEIGHT_MM, None));
        m.preview_generation_version = Some(version);
        m.scan_config_checksum = Some(scan_config::compute(hull::DEFAULT_RAFT_HEIGHT_MM));
        m.file_modified_at = stat(&path).unwrap().modified;
        t.insert_model(&m).await;
        (m, files)
    }

    #[tokio::test]
    async fn stale_preview_version_redraws_cached_preview_files() {
        let t = TestDb::new().await;
        let (root, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (m, files) =
            model_with_cached_preview(&t, root.path(), cache.path(), CURRENT_PREVIEW_GENERATION_VERSION - 1).await;

        let svc = service_with_previews(&t, root.path(), cache.path());
        assert!(svc.scan_and_cache_single("dragon.stl", None, &CancellationToken::new()).await.unwrap());
        for f in &files {
            assert_ne!(std::fs::read(f).unwrap(), b"old", "{} redrawn", f.display());
        }
        let updated = store::get_model(&t.db, m.id).await.unwrap().unwrap();
        assert_eq!(updated.preview_generation_version, Some(CURRENT_PREVIEW_GENERATION_VERSION));
    }

    #[tokio::test]
    async fn current_preview_files_are_reused_when_only_hulls_are_refreshed() {
        let t = TestDb::new().await;
        let (root, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (mut m, files) =
            model_with_cached_preview(&t, root.path(), cache.path(), CURRENT_PREVIEW_GENERATION_VERSION).await;
        // A stale scan config (a support detection version bump) refreshes the hulls only.
        m.scan_config_checksum = Some("stale".into());
        let mut conn = t.db.acquire().await.unwrap();
        store::save_model(&mut conn, &m).await.unwrap();
        drop(conn);

        let svc = service_with_previews(&t, root.path(), cache.path());
        assert!(svc.scan_and_cache_single("dragon.stl", None, &CancellationToken::new()).await.unwrap());
        for f in &files {
            assert_eq!(std::fs::read(f).unwrap(), b"old", "{} reused", f.display());
        }
        let updated = store::get_model(&t.db, m.id).await.unwrap().unwrap();
        assert_eq!(updated.scan_config_checksum, Some(scan_config::compute(hull::DEFAULT_RAFT_HEIGHT_MM)));
    }

    #[test]
    fn needs_spin_regeneration_checks_path_and_version() {
        let expected = expected_spin_path("abc", 0.5, None);
        assert!(needs_spin_regeneration(None, None, &expected));
        assert!(needs_spin_regeneration(Some(&expected), None, &expected));
        assert!(needs_spin_regeneration(Some(&expected), Some(CURRENT_SPIN_MESH_VERSION - 1), &expected));
        assert!(needs_spin_regeneration(
            Some(&expected_spin_path("abc", 1.0, None)),
            Some(CURRENT_SPIN_MESH_VERSION),
            &expected
        ));
        assert!(!needs_spin_regeneration(Some(&expected), Some(CURRENT_SPIN_MESH_VERSION), &expected));
    }

    fn expected_spin_for(m: &ModelRow) -> String {
        expected_spin_path(&m.checksum, hull::DEFAULT_RAFT_HEIGHT_MM, None)
    }

    #[tokio::test]
    async fn a_missing_spin_mesh_is_written_without_redrawing_previews() {
        let t = TestDb::new().await;
        let (root, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        // Current previews, no spin mesh: an install from before spin meshes.
        let (m, files) =
            model_with_cached_preview(&t, root.path(), cache.path(), CURRENT_PREVIEW_GENERATION_VERSION).await;

        let svc = service_with_previews(&t, root.path(), cache.path());
        let reporter = RecordingReporter::default();
        svc.scan_and_cache(None, None, Some(&reporter), &CancellationToken::new()).await.unwrap();

        for f in &files {
            assert_eq!(std::fs::read(f).unwrap(), b"old", "{} not redrawn", f.display());
        }
        let updated = store::get_model(&t.db, m.id).await.unwrap().unwrap();
        let expected = expected_spin_for(&m);
        assert_eq!(updated.spin_mesh_path.as_deref(), Some(expected.as_str()));
        assert_eq!(updated.spin_mesh_version, Some(CURRENT_SPIN_MESH_VERSION));
        assert_eq!(updated.preview_generation_version, Some(CURRENT_PREVIEW_GENERATION_VERSION));
        let raw = spin::decompress(&std::fs::read(cache.path().join(&expected)).unwrap()).unwrap();
        assert_eq!(spin::decode(&raw).unwrap().indices.len(), 3, "the one-triangle model");
        let results = reporter.results.lock().clone();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "Wrote the spin mesh");

        // Up to date now: the next scan leaves the model alone.
        let again = RecordingReporter::default();
        svc.scan_and_cache(None, None, Some(&again), &CancellationToken::new()).await.unwrap();
        assert_eq!(again.results.lock()[0].message, "Already up-to-date");
    }

    #[tokio::test]
    async fn a_single_model_scan_writes_a_missing_spin_mesh() {
        let t = TestDb::new().await;
        let (root, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (m, files) =
            model_with_cached_preview(&t, root.path(), cache.path(), CURRENT_PREVIEW_GENERATION_VERSION).await;

        let svc = service_with_previews(&t, root.path(), cache.path());
        assert!(svc.scan_and_cache_single("dragon.stl", None, &CancellationToken::new()).await.unwrap());
        for f in &files {
            assert_eq!(std::fs::read(f).unwrap(), b"old", "{} not redrawn", f.display());
        }
        let updated = store::get_model(&t.db, m.id).await.unwrap().unwrap();
        assert_eq!(updated.spin_mesh_path, Some(expected_spin_for(&m)));
        assert!(cache.path().join(expected_spin_for(&m)).is_file());
        assert!(!svc.scan_and_cache_single("dragon.stl", None, &CancellationToken::new()).await.unwrap());
    }

    #[tokio::test]
    async fn an_older_spin_mesh_is_replaced_and_its_file_deleted() {
        let t = TestDb::new().await;
        let (root, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (mut m, _) =
            model_with_cached_preview(&t, root.path(), cache.path(), CURRENT_PREVIEW_GENERATION_VERSION).await;
        let old = format!("{}_spin_v0.bin", preview::preview_stem(&m.checksum, hull::DEFAULT_RAFT_HEIGHT_MM, None));
        std::fs::write(cache.path().join(&old), b"old").unwrap();
        m.spin_mesh_path = Some(old.clone());
        m.spin_mesh_version = Some(CURRENT_SPIN_MESH_VERSION - 1);
        t.insert_model(&m).await;

        let svc = service_with_previews(&t, root.path(), cache.path());
        assert!(svc.scan_and_cache_single("dragon.stl", None, &CancellationToken::new()).await.unwrap());
        let updated = store::get_model(&t.db, m.id).await.unwrap().unwrap();
        assert_eq!(updated.spin_mesh_path, Some(expected_spin_for(&m)));
        assert_eq!(updated.spin_mesh_version, Some(CURRENT_SPIN_MESH_VERSION));
        assert!(!cache.path().join(&old).exists(), "the old version's file is deleted");
    }

    #[tokio::test]
    async fn current_spin_meshes_are_kept() {
        let t = TestDb::new().await;
        let (root, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (mut m, _) =
            model_with_cached_preview(&t, root.path(), cache.path(), CURRENT_PREVIEW_GENERATION_VERSION).await;
        let current = expected_spin_for(&m);
        std::fs::write(cache.path().join(&current), b"current").unwrap();
        m.spin_mesh_path = Some(current.clone());
        m.spin_mesh_version = Some(CURRENT_SPIN_MESH_VERSION);
        t.insert_model(&m).await;

        let svc = service_with_previews(&t, root.path(), cache.path());
        assert!(!svc.scan_and_cache_single("dragon.stl", None, &CancellationToken::new()).await.unwrap());
        assert_eq!(std::fs::read(cache.path().join(&current)).unwrap(), b"current");

        // A preview redraw reuses the spin mesh file already there.
        sqlx::query("UPDATE models SET spin_mesh_path = NULL, spin_mesh_version = NULL").execute(&t.db).await.unwrap();
        assert!(svc.scan_and_cache_single("dragon.stl", None, &CancellationToken::new()).await.unwrap());
        assert_eq!(std::fs::read(cache.path().join(&current)).unwrap(), b"current");
        let updated = store::get_model(&t.db, m.id).await.unwrap().unwrap();
        assert_eq!(updated.spin_mesh_path, Some(current));
    }

    #[tokio::test]
    async fn no_spin_meshes_are_written_with_previews_disabled() {
        let t = TestDb::new().await;
        disable_previews(&t).await;
        let (root, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write_file(root.path(), "dragon.stl", SIMPLE_ASCII_STL);
        let svc = service_with_previews(&t, root.path(), cache.path());
        svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
        let paths: Vec<Option<String>> =
            sqlx::query_scalar("SELECT spin_mesh_path FROM models").fetch_all(&t.db).await.unwrap();
        assert_eq!(paths, vec![None]);
        assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn new_models_get_a_spin_mesh() {
        let t = TestDb::new().await;
        let (root, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write_file(root.path(), "dragon.stl", SIMPLE_ASCII_STL);
        let svc = service_with_previews(&t, root.path(), cache.path());
        svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
        let (path, version): (Option<String>, Option<i32>) =
            sqlx::query_as("SELECT spin_mesh_path, spin_mesh_version FROM models").fetch_one(&t.db).await.unwrap();
        assert!(cache.path().join(path.unwrap()).is_file());
        assert_eq!(version, Some(CURRENT_SPIN_MESH_VERSION));
    }

    #[tokio::test]
    async fn the_sweep_deletes_old_spin_files_no_model_uses() {
        let t = TestDb::new().await;
        let (root, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut m = model("dragon.stl", "");
        m.spin_mesh_path = Some("used_spin_v1.bin".into());
        m.spin_mesh_version = Some(CURRENT_SPIN_MESH_VERSION);
        t.insert_model(&m).await;
        for name in ["used_spin_v1.bin", "orphan_spin_v1.bin", "orphan.png"] {
            std::fs::write(cache.path().join(name), b"x").unwrap();
        }
        let svc = service_with_previews(&t, root.path(), cache.path());

        // Files younger than the grace period stay (a scan may be about to index them).
        assert_eq!(svc.sweep_spin_meshes(SPIN_SWEEP_GRACE).await, 0);
        assert_eq!(svc.sweep_spin_meshes(std::time::Duration::ZERO).await, 1);
        assert!(cache.path().join("used_spin_v1.bin").exists());
        assert!(!cache.path().join("orphan_spin_v1.bin").exists());
        assert!(cache.path().join("orphan.png").exists(), "previews are not swept");
    }

    #[tokio::test]
    async fn full_scan_indexes_files_in_parallel_and_is_incremental() {
        let t = TestDb::new().await;
        disable_previews(&t).await;
        let root = tempfile::tempdir().unwrap();
        for (name, byte) in [("alpha.ctb", 1u8), ("beta.ctb", 2), ("gamma.ctb", 3)] {
            write_file(root.path(), name, [0x43, 0x54, 0x42, byte]);
        }
        write_file(root.path(), "nested/part.stl", SIMPLE_ASCII_STL);
        let svc = service(&t, root.path());
        let cancel = CancellationToken::new();

        assert_eq!(svc.scan_and_cache(None, None, None, &cancel).await.unwrap(), 4);
        let names: Vec<String> =
            sqlx::query_scalar("SELECT file_name FROM models ORDER BY file_name").fetch_all(&t.db).await.unwrap();
        assert_eq!(names, vec!["alpha.ctb", "beta.ctb", "gamma.ctb", "part.stl"]);
        let id: Uuid =
            sqlx::query_scalar("SELECT id FROM models WHERE file_name = 'part.stl'").fetch_one(&t.db).await.unwrap();
        let part = store::get_model(&t.db, id).await.unwrap().unwrap();
        assert_eq!(part.directory, "nested");
        assert!(part.hulls.is_some_and(|h| h.convex_hull.is_some()));

        assert_eq!(svc.scan_and_cache(None, None, None, &cancel).await.unwrap(), 0);
    }

    /// Writes Y-up triangles as a Z-up binary STL, the way slicers export.
    fn write_stl(root: &Path, name: &str, triangles: &[crate::geometry::Triangle]) {
        use crate::geometry::{Triangle, swap_up_axis};
        let z_up: Vec<Triangle> = triangles
            .iter()
            .map(|t| Triangle::new(swap_up_axis(t.v0), swap_up_axis(t.v1), swap_up_axis(t.v2), swap_up_axis(t.normal)))
            .collect();
        write_file(root, name, crate::geometry::saver::save_stl(&z_up, None));
    }

    #[tokio::test]
    async fn scan_stores_support_detection_and_metadata_wins() {
        use crate::geometry::support_separation::tests::{body, posts};
        let t = TestDb::new().await;
        disable_previews(&t).await;
        let root = tempfile::tempdir().unwrap();
        let mut supported = body();
        supported.extend(posts(50));
        write_stl(root.path(), "supported.stl", &supported);
        write_stl(root.path(), "plain.stl", &body());
        write_file(root.path(), "flagged/supported.stl", std::fs::read(root.path().join("supported.stl")).unwrap());
        write_file(root.path(), "flagged/findamodel.yaml", "supported: false\n");
        let svc = service(&t, root.path());
        let cancel = CancellationToken::new();

        assert_eq!(svc.scan_and_cache(None, None, None, &cancel).await.unwrap(), 3);
        type Row = (String, String, Option<bool>, Option<bool>, Option<bool>, Option<bool>);
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT directory, file_name, calculated_supported, detected_supported, effective_supported,
                support_parts_found
             FROM models ORDER BY directory, file_name",
        )
        .fetch_all(&t.db)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                ("".into(), "plain.stl".into(), None, None, None, Some(false)),
                ("".into(), "supported.stl".into(), None, Some(true), Some(true), Some(true)),
                ("flagged".into(), "supported.stl".into(), Some(false), Some(true), Some(false), Some(true)),
            ]
        );
        assert_eq!(svc.scan_and_cache(None, None, None, &cancel).await.unwrap(), 0);
    }

    type ExportIndexRow = (String, Option<String>, Option<i32>);

    async fn export_index_rows(t: &TestDb) -> Vec<ExportIndexRow> {
        sqlx::query_as(
            "SELECT directory || '/' || file_name, support_export_path, support_export_version FROM models
             ORDER BY directory, file_name",
        )
        .fetch_all(&t.db)
        .await
        .unwrap()
    }

    /// A supported model and a plain one, scanned once.
    async fn scanned_supported_and_plain(t: &TestDb, root: &Path) -> (ModelService, RecordingReporter) {
        use crate::geometry::support_separation::tests::{body, posts};
        disable_previews(t).await;
        let mut supported = body();
        supported.extend(posts(50));
        write_stl(root, "supported.stl", &supported);
        write_stl(root, "plain.stl", &body());
        let svc = service(t, root);
        let reporter = RecordingReporter::default();
        svc.scan_and_cache(None, None, Some(&reporter), &CancellationToken::new()).await.unwrap();
        (svc, reporter)
    }

    fn supported_entry_key(t_root: &Path) -> String {
        let checksum = sha256_hex(&std::fs::read(t_root.join("supported.stl")).unwrap());
        support_exports::key(&checksum, hull::DEFAULT_RAFT_HEIGHT_MM, None, true)
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(bytes))
    }

    #[tokio::test]
    async fn the_scan_writes_export_entries_for_presupported_models_and_times_them() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let (svc, reporter) = scanned_supported_and_plain(&t, root.path()).await;
        let key = supported_entry_key(root.path());
        let path = support_exports::relative_path(&key);
        assert_eq!(
            export_index_rows(&t).await,
            vec![
                ("/plain.stl".into(), None, None),
                ("/supported.stl".into(), Some(path.clone()), Some(CURRENT_SUPPORT_READER_VERSION)),
            ]
        );
        let exports = svc.support_exports().unwrap();
        let triangles = loader::load_model(&root.path().join("supported.stl"), "stl").unwrap().triangles;
        let (body, _) = exports.read(&key, &triangles).expect("a current entry").expect("with a split");
        assert!(body.len() < triangles.len(), "the supports are split off");
        let stage =
            |name: &str| reporter.results.lock().iter().find(|r| r.relative_path == name).unwrap().stages.export_ms;
        assert!(stage("supported.stl") > 0.0, "the entry's generation is timed");
        assert_eq!(stage("plain.stl"), 0.0, "a model without supports has no entry");

        // Nothing to do on a rescan: the entry is current.
        let modified = std::fs::metadata(exports.dir().join(&path)).unwrap().modified().unwrap();
        let again = RecordingReporter::default();
        svc.scan_and_cache(None, None, Some(&again), &CancellationToken::new()).await.unwrap();
        assert!(again.results.lock().iter().all(|r| r.stages.export_ms == 0.0));
        assert_eq!(std::fs::metadata(exports.dir().join(&path)).unwrap().modified().unwrap(), modified);
    }

    /// Rewrites the entry for `key` as if an older reader had written it.
    fn age_entry(exports: &SupportExports, key: &str, version: i32) {
        let path = exports.dir().join(support_exports::relative_path(key));
        let (header, export) = support_exports::codec::decode(&std::fs::read(&path).unwrap()).unwrap();
        let old = support_exports::Header { reader_version: version, ..header };
        let bytes = support_exports::codec::encode(&old, export.as_ref().map(|(m, l)| (m.as_slice(), l)));
        std::fs::write(&path, bytes).unwrap();
    }

    #[tokio::test]
    async fn the_scan_rewrites_entries_below_the_minimum_version_only() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let (svc, _) = scanned_supported_and_plain(&t, root.path()).await;
        let exports = svc.support_exports().unwrap().clone();
        let key = supported_entry_key(root.path());
        let old = CURRENT_SUPPORT_READER_VERSION - 1;
        age_entry(&exports, &key, old);

        // At or above the minimum (0 here), an old entry stays for the scan; exports refresh it.
        svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
        assert_eq!(exports.stored_version(&key), Some(old));
        assert_eq!(export_index_rows(&t).await[1].2, Some(old), "the row's index follows the file");

        sqlx::query("UPDATE app_config SET minimum_support_export_version = ?")
            .bind(CURRENT_SUPPORT_READER_VERSION)
            .execute(&t.db)
            .await
            .unwrap();
        let reporter = RecordingReporter::default();
        svc.scan_and_cache(None, None, Some(&reporter), &CancellationToken::new()).await.unwrap();
        assert_eq!(exports.stored_version(&key), Some(CURRENT_SUPPORT_READER_VERSION));
        assert_eq!(export_index_rows(&t).await[1].2, Some(CURRENT_SUPPORT_READER_VERSION));
        let results = reporter.results.lock();
        let rebuilt = results.iter().find(|r| r.relative_path == "supported.stl").unwrap();
        assert!(rebuilt.stages.export_ms > 0.0);
        assert_eq!(rebuilt.stages.hull_ms, 0.0, "only the entry is rebuilt");
    }

    fn entry_files(exports: &SupportExports) -> Vec<String> {
        let mut out = Vec::new();
        for dir in std::fs::read_dir(exports.dir()).unwrap().flatten() {
            for file in std::fs::read_dir(dir.path()).unwrap().flatten() {
                out.push(format!("{}/{}", dir.file_name().to_string_lossy(), file.file_name().to_string_lossy()));
            }
        }
        out.sort();
        out
    }

    #[tokio::test]
    async fn a_changed_entry_key_deletes_the_old_entry_unless_another_model_uses_it() {
        use crate::geometry::support_separation::tests::{body, posts};
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let (svc, _) = scanned_supported_and_plain(&t, root.path()).await;
        let exports = svc.support_exports().unwrap().clone();
        let first = support_exports::relative_path(&supported_entry_key(root.path()));
        assert_eq!(entry_files(&exports), vec![first.clone()]);

        // New content: the new entry replaces the old one.
        let mut changed = body();
        changed.extend(posts(60));
        write_stl(root.path(), "supported.stl", &changed);
        svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
        let second = support_exports::relative_path(&supported_entry_key(root.path()));
        assert_ne!(second, first);
        assert_eq!(entry_files(&exports), vec![second.clone()]);

        // A copy with the same content shares the entry: changing the original keeps it.
        std::fs::copy(root.path().join("supported.stl"), root.path().join("copy.stl")).unwrap();
        svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
        assert_eq!(entry_files(&exports), vec![second.clone()]);
        let mut again = body();
        again.extend(posts(70));
        write_stl(root.path(), "supported.stl", &again);
        svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
        let third = support_exports::relative_path(&supported_entry_key(root.path()));
        let mut want = vec![second, third];
        want.sort();
        assert_eq!(entry_files(&exports), want, "the copy still uses the second entry");

        // Supported metadata changes the key too.
        write_file(root.path(), "findamodel.yaml", "supported: true\n");
        svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
        let rows: Vec<Option<String>> = sqlx::query_scalar(
            "SELECT support_export_path FROM models WHERE support_export_path IS NOT NULL ORDER BY file_name",
        )
        .fetch_all(&t.db)
        .await
        .unwrap();
        let mut indexed: Vec<String> = rows.into_iter().flatten().collect();
        indexed.sort();
        indexed.dedup();
        assert_eq!(entry_files(&exports), indexed, "only the entries rows name are left");
    }

    #[tokio::test]
    async fn only_a_full_scan_sweeps_entries_no_model_uses() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "sub/part.stl", SIMPLE_ASCII_STL);
        let (svc, _) = scanned_supported_and_plain(&t, root.path()).await;
        let exports = svc.support_exports().unwrap().clone();
        let used = entry_files(&exports);
        let orphan = exports.dir().join("zz/orphan.export");
        std::fs::create_dir_all(orphan.parent().unwrap()).unwrap();
        std::fs::write(&orphan, b"left over").unwrap();
        let old = SystemTime::now() - support_exports::SWEEP_GRACE * 2;
        std::fs::File::options().write(true).open(&orphan).unwrap().set_modified(old).unwrap();

        svc.scan_and_cache(None, Some("sub"), None, &CancellationToken::new()).await.unwrap();
        assert!(orphan.exists(), "a partial scan does not sweep");
        svc.scan_and_cache_single("plain.stl", None, &CancellationToken::new()).await.unwrap();
        assert!(orphan.exists(), "nor does a single-model scan");
        svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
        assert!(!orphan.exists());
        assert_eq!(entry_files(&exports), used);
    }

    #[tokio::test]
    async fn the_entry_file_wins_when_the_row_disagrees() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let (svc, _) = scanned_supported_and_plain(&t, root.path()).await;
        let exports = svc.support_exports().unwrap().clone();
        let key = supported_entry_key(root.path());
        let file = exports.dir().join(support_exports::relative_path(&key));

        // The row says nothing is stored, but the file is current: only the index is fixed.
        sqlx::query("UPDATE models SET support_export_path = NULL, support_export_version = NULL")
            .execute(&t.db)
            .await
            .unwrap();
        let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
        svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), modified, "not rewritten");
        assert_eq!(export_index_rows(&t).await[1].2, Some(CURRENT_SUPPORT_READER_VERSION));

        // The row says it is current, but the file is gone or damaged: it is written again.
        for damage in [None, Some(&b"FMEX\x01broken"[..])] {
            match damage {
                None => std::fs::remove_file(&file).unwrap(),
                Some(bytes) => std::fs::write(&file, bytes).unwrap(),
            }
            svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap();
            assert_eq!(exports.stored_version(&key), Some(CURRENT_SUPPORT_READER_VERSION), "{damage:?}");
        }
    }

    #[tokio::test]
    async fn a_single_model_scan_writes_its_entry() {
        use crate::geometry::support_separation::tests::{body, posts};
        let t = TestDb::new().await;
        disable_previews(&t).await;
        let root = tempfile::tempdir().unwrap();
        let mut supported = body();
        supported.extend(posts(50));
        write_stl(root.path(), "supported.stl", &supported);
        let svc = service(&t, root.path());
        assert!(svc.scan_and_cache_single("supported.stl", None, &CancellationToken::new()).await.unwrap());
        let key = supported_entry_key(root.path());
        assert_eq!(svc.support_exports().unwrap().stored_version(&key), Some(CURRENT_SUPPORT_READER_VERSION));
        assert_eq!(export_index_rows(&t).await[0].2, Some(CURRENT_SUPPORT_READER_VERSION));
    }

    #[tokio::test]
    async fn scan_finds_no_support_parts_in_model_parts_that_do_not_rebuild_as_supports() {
        use crate::geometry::support_separation::tests::{body, straps};
        let t = TestDb::new().await;
        disable_previews(&t).await;
        let root = tempfile::tempdir().unwrap();
        let mut strapped = body();
        strapped.extend(straps());
        write_stl(root.path(), "kit/strapped.stl", &strapped);
        write_file(root.path(), "kit/findamodel.yaml", "supported: true\n");
        let svc = service(&t, root.path());

        assert_eq!(svc.scan_and_cache(None, None, None, &CancellationToken::new()).await.unwrap(), 1);
        let row: (Option<bool>, Option<bool>, Option<bool>) =
            sqlx::query_as("SELECT calculated_supported, detected_supported, support_parts_found FROM models")
                .fetch_one(&t.db)
                .await
                .unwrap();
        assert_eq!(row, (Some(true), None, Some(false)));
    }

    #[tokio::test]
    async fn scan_rejects_paths_outside_root() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        assert!(
            !service(&t, root.path())
                .scan_and_cache_single("../etc/passwd", None, &CancellationToken::new())
                .await
                .unwrap()
        );
    }

    #[derive(Default)]
    struct RecordingReporter {
        results: parking_lot::Mutex<Vec<FileResult>>,
    }

    #[async_trait]
    impl ProgressReporter for RecordingReporter {
        async fn on_scan_started(&self, _: usize) {}
        async fn on_files_discovered(&self, _: &[FilePlan]) {}
        async fn on_file_processed(&self, result: FileResult) {
            self.results.lock().push(result);
        }
        async fn on_log(&self, _: &str, _: &str, _: Option<&str>) {}
    }

    #[tokio::test]
    async fn scans_report_time_per_stage() {
        let t = TestDb::new().await;
        disable_previews(&t).await;
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "part.stl", SIMPLE_ASCII_STL);
        let svc = service(&t, root.path());
        let reporter = RecordingReporter::default();
        svc.scan_and_cache(None, None, Some(&reporter), &CancellationToken::new()).await.unwrap();

        let results = reporter.results.lock();
        let stages = results.iter().find(|r| r.relative_path == "part.stl").unwrap().stages;
        for (stage, ms) in [
            ("checksum", stages.checksum_ms),
            ("load", stages.load_ms),
            ("supports", stages.supports_ms),
            ("hull", stages.hull_ms),
        ] {
            assert!(ms > 0.0, "{stage} was not timed: {stages:?}");
        }
        assert_eq!(stages.preview_ms, 0.0, "previews are disabled");
    }
}
