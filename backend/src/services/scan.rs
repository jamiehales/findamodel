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
use crate::entities::{DirectoryConfig, ModelRow};
use crate::geometry::{LoadedGeometry, hull, loader, support_separation};
use crate::metadata::config_reader;
use crate::metadata::inheritance::DirectoryMap;
use crate::metadata::model_metadata::{self, ComputedMetadata};
use crate::preview::{self, CURRENT_PREVIEW_GENERATION_VERSION};
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
        || stored_version.is_none_or(|v| v < minimum_version)
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
    full_path: &str,
    directory: &str,
    configs: &DirectoryMap,
    default_raft_height_mm: f32,
) -> f32 {
    raft_height_mm_for(
        &model_metadata::compute(full_path, configs.get(directory)),
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
    detected_supported: bool,
    metadata: ComputedMetadata,
    dir_config_id: Option<Uuid>,
    geometry: Option<GeometrySummary>,
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
    detected_supported: bool,
}

/// (id, directory, file_name, checksum, file_modified_at, scan_config_checksum,
/// preview_generation_version, preview_image_path)
type ExistingScanRow = (Uuid, String, String, String, DateTime<Utc>, Option<String>, Option<i32>, Option<String>);

#[derive(Debug, Clone)]
struct ExistingScanState {
    id: Uuid,
    checksum: String,
    file_modified_at: DateTime<Utc>,
    scan_config_checksum: Option<String>,
    preview_generation_version: Option<i32>,
    preview_image_path: Option<String>,
}

enum Change {
    Content(Box<FileData>),
    Geometry(GeometryRefresh),
    FileMetadataOnly,
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
}

#[derive(Clone)]
struct ScanContext {
    root: PathBuf,
    configs: Arc<DirectoryMap>,
    default_raft_height_mm: f32,
    generate_previews: bool,
    minimum_preview_version: i32,
}

impl ScanContext {
    /// A model file's metadata and raft cutoff.
    fn resolve(&self, full_path: &str, directory: &str) -> (ComputedMetadata, f32) {
        let metadata = model_metadata::compute(full_path, self.configs.get(directory));
        let raft_height_mm = raft_height_mm_for(&metadata, directory, &self.configs, self.default_raft_height_mm);
        (metadata, raft_height_mm)
    }
}

/// The with-supports preview path a model's previews should be stored under.
fn expected_preview_path(checksum: &str, raft_height_mm: f32, supported: Option<bool>) -> String {
    preview::relative_path(&preview::preview_stem(checksum, raft_height_mm, supported), true)
}

/// Stores hulls and the support detection, which share the scan config checksum.
fn apply_hull_data(
    model: &mut ModelRow,
    hulls: &hull::Hulls,
    calculated: bool,
    raft_height_mm: f32,
    detected_supported: bool,
) {
    model.detected_supported = (calculated && detected_supported).then_some(true);
    model.convex_hull = hulls.convex.clone();
    model.concave_hull = hulls.concave.clone();
    model.convex_sans_raft_hull = hulls.convex_sans_raft.clone();
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
    apply_hull_data(model, &data.hulls, data.geometry.is_some(), data.raft_height_mm, data.detected_supported);
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
}

fn apply_geometry_refresh(model: &mut ModelRow, refresh: &GeometryRefresh) {
    if refresh.include_hulls {
        apply_hull_data(
            model,
            &refresh.hulls,
            refresh.geometry_loaded,
            refresh.raft_height_mm,
            refresh.detected_supported,
        );
    }
    if !refresh.include_preview || !refresh.geometry_loaded {
        return;
    }
    model.preview_image_path = refresh.preview_image_path.clone();
    model.preview_generated_at = refresh.preview_generated_at;
    model.preview_generation_version = refresh.preview_image_path.as_ref().map(|_| CURRENT_PREVIEW_GENERATION_VERSION);
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

    /// Loads geometry, detects supports, renders previews (when requested) and computes
    /// hulls. Returns (summary, preview generated, hulls, supports detected).
    #[allow(clippy::too_many_arguments)]
    async fn process_geometry(
        &self,
        file: &Path,
        file_type: &str,
        checksum: &str,
        include_preview: bool,
        include_hulls: bool,
        raft_height_mm: f32,
        supported: Option<bool>,
    ) -> (Option<GeometrySummary>, bool, Option<hull::Hulls>, bool) {
        let path = file.to_path_buf();
        let file_type = file_type.to_string();
        let stem = preview::preview_stem(checksum, raft_height_mm, supported);
        let preview = self.preview.clone().filter(|_| include_preview);
        tokio::task::spawn_blocking(move || {
            let Some(geometry) = loader::load_model(&path, &file_type) else {
                return (None, false, None, false);
            };
            let classification = support_separation::classify(&geometry.triangles, raft_height_mm);
            let detected = classification.as_ref().is_some_and(|c| c.confident);
            let support_mask = classification.and_then(|c| c.mask_for(supported));
            let preview_generated = preview.is_some_and(|p| {
                let result = p.generate_variants(&geometry, &stem, support_mask.as_deref(), true, true);
                result.with_supports.generated || result.without_supports.generated
            });
            let hulls = include_hulls
                .then(|| hull::calculate_hulls(&geometry, hull::DEFAULT_MAX_HULL_VERTICES, raft_height_mm));
            (Some(GeometrySummary::of(&geometry)), preview_generated, hulls, detected)
        })
        .await
        .unwrap_or((None, false, None, false))
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
        let checksum = match known_checksum {
            Some(c) => c,
            None => compute_checksum(file).await?,
        };
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
            detected_supported: false,
            metadata,
            dir_config_id: dir_config.map(|d| d.id),
            geometry: None,
        };

        if is_non_geometry_type(file_type) {
            return Ok(data);
        }

        let (summary, preview_generated, hulls, detected) =
            self.process_geometry(file, file_type, &checksum, include_preview, true, raft_height_mm, supported).await;
        (data.preview_image_path, data.preview_generated_at) = self.resolve_cached_preview_metadata(
            &preview::preview_stem(&checksum, raft_height_mm, supported),
            existing_preview_path,
        );
        data.preview_generated = preview_generated;
        if let Some(summary) = summary {
            data.hulls = hulls.unwrap_or_default();
            data.hull_generated = true;
            data.detected_supported = detected;
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
        include_preview: bool,
        include_hulls: bool,
    ) -> GeometryRefresh {
        let (summary, preview_generated, hulls, detected) = self
            .process_geometry(file, file_type, checksum, include_preview, include_hulls, raft_height_mm, supported)
            .await;
        let mut refresh = GeometryRefresh {
            include_preview,
            include_hulls,
            geometry_loaded: summary.is_some(),
            preview_image_path: None,
            preview_generated_at: None,
            preview_generated: false,
            hulls: hulls.unwrap_or_default(),
            hull_generated: false,
            raft_height_mm,
            detected_supported: detected,
        };
        if summary.is_none() {
            return refresh;
        }
        if include_preview {
            (refresh.preview_image_path, refresh.preview_generated_at) =
                self.resolve_cached_preview_metadata(&preview::preview_stem(checksum, raft_height_mm, supported), None);
            refresh.preview_generated = preview_generated;
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
        };

        let Some(existing) = existing else {
            let data =
                self.compute_file_data(ctx, &file, &file_type, &directory, None, ctx.generate_previews, None).await?;
            prepared.change = Some(Change::Content(Box::new(data)));
            prepared.preparation_ms = started.elapsed().as_secs_f64() * 1000.0;
            return Ok(Some(prepared));
        };

        let (checksum, recomputed) =
            resolve_checksum(&file, file_stat.modified, Some(&existing.checksum), Some(existing.file_modified_at))
                .await?;
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

        prepared.change = if checksum == existing.checksum && !hull_mismatch && !preview_stale {
            if !recomputed {
                return Ok(None);
            }
            Some(Change::FileMetadataOnly)
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
                    ctx.generate_previews && preview_stale,
                    true,
                )
                .await,
            ))
        } else {
            info!(
                target: LOG_TARGET,
                "Preview version mismatch, regenerating preview for: {full} (stored version: {:?}, required minimum version: {}, current version: {CURRENT_PREVIEW_GENERATION_VERSION})",
                existing.preview_generation_version,
                ctx.minimum_preview_version
            );
            Some(Change::Geometry(
                self.compute_geometry_refresh(&file, &file_type, &checksum, expected_raft, supported, true, false)
                    .await,
            ))
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
            minimum_preview_version: resolve_minimum_preview_generation_version(
                app_config.minimum_preview_generation_version,
            ),
        })
    }

    async fn load_existing_states(
        &self,
    ) -> sqlx::Result<std::collections::HashMap<(String, String), ExistingScanState>> {
        let rows: Vec<ExistingScanRow> = sqlx::query_as(
            "SELECT id, directory, file_name, checksum, file_modified_at, scan_config_checksum, preview_generation_version, preview_image_path
             FROM models",
        )
        .fetch_all(&self.db)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    directory,
                    file_name,
                    checksum,
                    file_modified_at,
                    scan_config_checksum,
                    preview_generation_version,
                    preview_image_path,
                )| {
                    (
                        (directory, file_name),
                        ExistingScanState {
                            id,
                            checksum,
                            file_modified_at,
                            scan_config_checksum,
                            preview_generation_version,
                            preview_image_path,
                        },
                    )
                },
            )
            .collect())
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
                        } else {
                            "Regenerated preview from version changes".into()
                        };
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
                })
                .await;
            }
        }

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM models").fetch_one(&self.db).await?;
        info!(target: LOG_TARGET, "Scan complete: {new_count} new, {updated_count} updated, {total} total in cache.");
        Ok(new_count)
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

        let (checksum, recomputed) = resolve_checksum(
            &full_path,
            file_stat.modified,
            existing.as_ref().map(|e| e.checksum.as_str()),
            existing.as_ref().map(|e| e.file_modified_at),
        )
        .await?;
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

        if let Some(e) = &existing
            && e.checksum == checksum
            && !hull_stale
            && !preview_stale
        {
            if recomputed {
                let mut updated = e.clone();
                updated.file_modified_at = file_stat.modified;
                updated.file_size = file_stat.size;
                updated.cached_at = now();
                let mut conn = self.db.acquire().await?;
                store::save_model(&mut conn, &updated).await?;
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
                })
                .await;
            }
            return Ok(false);
        }

        let (entity, generated_preview, generated_hull) = match existing {
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
                (entity, data.preview_generated, data.hull_generated)
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
                (entity, data.preview_generated, data.hull_generated)
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
                        ctx.generate_previews && preview_stale,
                        true,
                    )
                    .await;
                apply_geometry_refresh(&mut entity, &refresh);
                entity.cached_at = now();
                entity.file_modified_at = file_stat.modified;
                entity.file_size = file_stat.size;
                (entity, refresh.preview_generated, refresh.hull_generated)
            }
            Some(mut entity) => {
                info!(target: LOG_TARGET, "Single-model index refreshing preview due to version mismatch: {full_text}");
                let refresh = self
                    .compute_geometry_refresh(
                        &full_path,
                        &file_type,
                        &entity.checksum.clone(),
                        expected_raft,
                        supported,
                        true,
                        false,
                    )
                    .await;
                apply_geometry_refresh(&mut entity, &refresh);
                entity.cached_at = now();
                entity.file_modified_at = file_stat.modified;
                entity.file_size = file_stat.size;
                (entity, refresh.preview_generated, false)
            }
        };

        {
            let mut conn = self.db.acquire().await?;
            store::save_model(&mut conn, &entity).await?;
        }

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
        let m: ModelRow =
            sqlx::query_as("SELECT * FROM models WHERE file_name = 'sample.ctb'").fetch_one(&t.db).await.unwrap();
        assert_eq!(m.file_type, "ctb");
        assert!(m.preview_image_path.is_none() && m.preview_generation_version.is_none());
        assert!(m.convex_hull.is_none() && m.concave_hull.is_none() && m.convex_sans_raft_hull.is_none());
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
        assert!(updated.convex_hull.is_some());
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
        let part: ModelRow =
            sqlx::query_as("SELECT * FROM models WHERE file_name = 'part.stl'").fetch_one(&t.db).await.unwrap();
        assert_eq!(part.directory, "nested");
        assert!(part.convex_hull.is_some());

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
        type Row = (String, String, Option<bool>, Option<bool>, Option<bool>);
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT directory, file_name, calculated_supported, detected_supported, effective_supported
             FROM models ORDER BY directory, file_name",
        )
        .fetch_all(&t.db)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                ("".into(), "plain.stl".into(), None, None, None),
                ("".into(), "supported.stl".into(), None, Some(true), Some(true)),
                ("flagged".into(), "supported.stl".into(), Some(false), Some(true), Some(false)),
            ]
        );
        assert_eq!(svc.scan_and_cache(None, None, None, &cancel).await.unwrap(), 0);
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
}
