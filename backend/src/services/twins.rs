//! The twin lookup in the app ([`crate::geometry::twin`]): a supported model whose supports are
//! welded into it (one shell, so the classifier's rules by component find nothing to split) is
//! split by its twin, the same sculpt without supports elsewhere in the catalogue. The exact twin
//! lookup comes first, then the remeshed twin lookup ([`remesh_twins`], a twin remeshed or
//! decimated, in the model's collection); the welded split by shape
//! ([`crate::geometry::support_separation::welded`]) is the fallback when no twin verifies.
//!
//! - **Sketches.** The scan writes each model's sketch (`model_twin_sketches`) from the geometry
//!   it has loaded for the hulls, so a sketch follows the scan config ([`crate::services::scan_config`]
//!   holds the twin version).
//! - **Lookup.** After a scan, [`ModelService::resolve_twins`] looks up each supported model whose
//!   scan split nothing off or split it by shape ([`pending`]): it builds an index from the
//!   sketches of the models not marked supported, probes it, verifies up to
//!   [`twin::MAX_CANDIDATES`] candidates, and when the twins pass the guards keeps their split as
//!   the model's: its export entry, previews and spin mesh, `model_twins` rows for the twins, and
//!   `support_parts_found`. Its verdict is the model's `twin_lookup` processing row. When no exact
//!   twin splits the model, the remeshed lookup runs on the stored descriptors of its collection; a
//!   twin it finds makes the verdict `twin_remeshed`, and a model is looked up again when the
//!   remeshed twin version changes.
//! - **Staleness.** A twin split holds while its twins are unchanged ([`TwinSet`]): the export entry
//!   key holds the twins' checksums, so an entry made from a twin since changed or deleted is not
//!   used, and the next lookup runs again for the model. A model the lookup found no twin for is
//!   looked up again when its content or the twin version changes, or against the sketches
//!   written since (new models without supports).
//! - **Stored probe keys.** Each supported model waiting for a twin (nothing split off, or split
//!   by shape) keeps a compacted sample of its probe keys in `model_twin_probes`, written by the
//!   scan ([`sketch_and_probes`]). A lookup probes the index with them first, from a database
//!   read and no file load; the file is loaded only when a candidate hits (or, on a lookup
//!   against every sketch, when a small part the sample cannot rule out is in the index, or when
//!   its collection has remeshed twin candidates). The keys
//!   are deleted once the model's twins split it or it stops waiting ([`drop_stale_probes`]).
//! - **Consistency.** Every split of the model (scan, viewer, previews, spin meshes, the VOXL
//!   exports and the repair) takes the twin split's mask ([`twin_mask`]): from the export entry, or
//!   made again from the recorded twins at their recorded scales when the entry is missing (a
//!   remeshed twin's by verifying it again, [`remesh_twin::split`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use serde_json::json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::{Db, now};
use crate::entities::ModelRow;
use crate::geometry::hull::DEFAULT_RAFT_HEIGHT_MM;
use crate::geometry::remesh_twin::{self, CURRENT_REMESH_TWIN_VERSION};
use crate::geometry::support_layout::SupportLayout;
use crate::geometry::support_reader::{self, CURRENT_SUPPORT_READER_VERSION};
use crate::geometry::support_separation::welded;
use crate::geometry::twin::{self, CURRENT_TWIN_VERSION, Index, Mesh, Sketch, StoredProbes, Verdict};
use crate::geometry::{LoadedGeometry, Triangle, loader, to_z_up};
use crate::metadata::inheritance;
use crate::preview::{self, spin};
use crate::services::models::{ModelService, is_non_geometry_type};
use crate::services::processing::{self, SplitMethod, SplitRecord, TwinUse};
use crate::services::remesh_twins;
use crate::services::scan::{Cancelled, ProgressReporter};
use crate::services::store;
use crate::services::support_exports::{self, SupportExports};

const LOG_TARGET: &str = "findamodel.Twins";

/// The stored sketch of loaded triangles. Blocking.
pub fn sketch_bytes(triangles: &[Triangle]) -> Vec<u8> {
    Sketch::of(&Mesh::new(triangles)).to_bytes()
}

/// The stored sketch of loaded triangles and, for a supported model waiting for a twin
/// (`waiting`), its stored probe keys ([`StoredProbes`]), from one mesh. Blocking.
pub fn sketch_and_probes(triangles: &[Triangle], waiting: bool) -> (Vec<u8>, Option<Vec<u8>>) {
    let m = Mesh::new(triangles);
    let sketch = Sketch::of(&m).to_bytes();
    (sketch, waiting.then(|| StoredProbes::of(&twin::probe_keys(&m)).to_bytes()))
}

/// Stores the probe keys of a supported model waiting for a twin, for its content `checksum`.
pub async fn store_probes(db: &Db, model_id: Uuid, checksum: &str, probes: &[u8]) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO model_twin_probes (model_id, version, model_checksum, probes, written_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(model_id) DO UPDATE SET version = excluded.version, model_checksum = excluded.model_checksum,
            probes = excluded.probes, written_at = excluded.written_at",
    )
    .bind(model_id)
    .bind(CURRENT_TWIN_VERSION)
    .bind(checksum)
    .bind(probes)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

/// Deletes a model's stored probe keys (it is split by its twins, or no longer waits for one).
pub async fn delete_probes(db: &Db, model_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM model_twin_probes WHERE model_id = ?").bind(model_id).execute(db).await?;
    Ok(())
}

/// The model's stored probe keys, when they are for its content `checksum` at this twin version.
async fn stored_probes(db: &Db, model_id: Uuid, checksum: &str) -> sqlx::Result<Option<StoredProbes>> {
    let bytes: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT probes FROM model_twin_probes WHERE model_id = ? AND model_checksum = ? AND version = ?",
    )
    .bind(model_id)
    .bind(checksum)
    .bind(CURRENT_TWIN_VERSION)
    .fetch_optional(db)
    .await?;
    Ok(bytes.and_then(|b| StoredProbes::from_bytes(&b)))
}

/// Deletes the stored probe keys of models no longer waiting for a twin: not supported, split by
/// shells or by their twins, changed since, or at another twin version.
pub async fn drop_stale_probes(db: &Db) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "DELETE FROM model_twin_probes WHERE model_id NOT IN (
            SELECT p.model_id FROM model_twin_probes p
            JOIN models m ON m.id = p.model_id AND m.checksum = p.model_checksum
            JOIN model_processing s ON s.model_id = m.id AND s.kind = 'support_split'
            WHERE p.version = ? AND m.effective_supported = 1 AND s.method IN ('none', 'welded'))",
    )
    .bind(CURRENT_TWIN_VERSION)
    .execute(db)
    .await?
    .rows_affected())
}

/// Stores a model's sketch; `sketched_at` moves only when the sketch changed, so rewriting the
/// same sketch (a rescan for another reason) does not send supported models to be looked up again.
pub async fn store_sketch(db: &Db, model_id: Uuid, sketch: &[u8]) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO model_twin_sketches (model_id, version, sketch, sketched_at) VALUES (?, ?, ?, ?)
         ON CONFLICT(model_id) DO UPDATE SET version = excluded.version, sketch = excluded.sketch,
            sketched_at = excluded.sketched_at
         WHERE model_twin_sketches.sketch IS NOT excluded.sketch OR model_twin_sketches.version != excluded.version",
    )
    .bind(model_id)
    .bind(CURRENT_TWIN_VERSION)
    .bind(sketch)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

/// One twin of a model's split, current.
#[derive(Debug, Clone, PartialEq)]
pub struct TwinFile {
    pub id: Uuid,
    pub checksum: String,
    pub directory: String,
    pub file_name: String,
    pub file_type: String,
    /// Supported over twin.
    pub scale: f64,
    pub coverage: f64,
}

/// The twins a model's split is made from, all unchanged since (same content, not marked
/// supported, at the current twin version), with the lookup that found them for the model's
/// current content. A model has none when any of them has changed.
#[derive(Debug, Clone, PartialEq)]
pub struct TwinSet {
    pub twins: Vec<TwinFile>,
    /// Found by the remeshed twin lookup ([`remesh_twins`]).
    pub remeshed: bool,
}

impl TwinSet {
    /// What the split is made from, for its export entry's key ([`support_exports::twin_key`]).
    pub fn key_part(&self) -> String {
        key_part(self.twins.iter().map(|t| t.checksum.as_str()), self.remeshed)
    }

    pub fn uses(&self) -> Vec<TwinUse> {
        self.twins.iter().map(|t| TwinUse { twin_id: t.id, scale: t.scale, coverage: t.coverage }).collect()
    }
}

/// [`TwinSet::key_part`] of twins with these checksums (found by the remeshed lookup when
/// `remeshed`).
fn key_part<'a>(checksums: impl Iterator<Item = &'a str>, remeshed: bool) -> String {
    let mut sums: Vec<&str> = checksums.collect();
    sums.sort_unstable();
    let remesh = if remeshed { format!("r{CURRENT_REMESH_TWIN_VERSION}") } else { String::new() };
    format!("v{CURRENT_TWIN_VERSION}{remesh}:{}", sums.join(","))
}

#[derive(sqlx::FromRow)]
struct TwinRow {
    model_id: Uuid,
    twin_id: Uuid,
    twin_checksum: String,
    scale: f64,
    coverage: f64,
    directory: String,
    file_name: String,
    file_type: String,
    name: Option<String>,
    valid: bool,
    remeshed: Option<bool>,
}

/// Each model's rows, checked: a twin unchanged, not supported and at the current version, and the
/// model looked up (with this twin found) on its current content.
const TWIN_ROWS_SQL: &str = "SELECT t.model_id, t.twin_id, t.twin_checksum, t.scale, t.coverage,
        u.directory, u.file_name, u.file_type, u.calculated_model_name AS name,
        (t.version = ?1 AND u.checksum = t.twin_checksum AND u.effective_supported IS NOT 1
         AND l.method IN ('twin', 'twin_remeshed') AND l.source_checksum = m.checksum
         AND l.twin_version = ?1) AS valid,
        (l.method = 'twin_remeshed') AS remeshed
     FROM model_twins t
     JOIN models u ON u.id = t.twin_id
     JOIN models m ON m.id = t.model_id
     LEFT JOIN model_processing l ON l.model_id = t.model_id AND l.kind = 'twin_lookup'";

fn sets_of(rows: Vec<TwinRow>) -> std::collections::HashMap<Uuid, TwinSet> {
    let mut by: std::collections::HashMap<Uuid, (bool, bool, Vec<TwinFile>)> = std::collections::HashMap::new();
    for r in rows {
        let e = by.entry(r.model_id).or_insert((true, false, Vec::new()));
        e.0 &= r.valid;
        e.1 |= r.remeshed == Some(true);
        e.2.push(TwinFile {
            id: r.twin_id,
            checksum: r.twin_checksum,
            directory: r.directory,
            file_name: r.file_name,
            file_type: r.file_type,
            scale: r.scale,
            coverage: r.coverage,
        });
    }
    by.into_iter()
        .filter(|(_, (valid, _, _))| *valid)
        .map(|(id, (_, remeshed, mut twins))| {
            twins.sort_by_key(|t| t.id);
            (id, TwinSet { twins, remeshed })
        })
        .collect()
}

/// The current twins of every model with a twin split, for the scan.
pub async fn twin_sets(db: &Db) -> sqlx::Result<std::collections::HashMap<Uuid, TwinSet>> {
    let rows: Vec<TwinRow> = sqlx::query_as(TWIN_ROWS_SQL).bind(CURRENT_TWIN_VERSION).fetch_all(db).await?;
    Ok(sets_of(rows))
}

/// The model's current twins (None: no twin split, or one of its twins changed).
pub async fn twin_set(db: &Db, model_id: Uuid) -> sqlx::Result<Option<TwinSet>> {
    let rows: Vec<TwinRow> = sqlx::query_as(&format!("{TWIN_ROWS_SQL} WHERE t.model_id = ?2"))
        .bind(CURRENT_TWIN_VERSION)
        .bind(model_id)
        .fetch_all(db)
        .await?;
    Ok(sets_of(rows).remove(&model_id))
}

/// How a model was processed and the twins of its split, for `GET /api/models/{id}/processing`.
pub async fn processing_of(db: &Db, model_id: Uuid) -> sqlx::Result<crate::dto::ModelProcessingDto> {
    let processing = processing::for_model(db, model_id)
        .await?
        .into_iter()
        .map(|p| crate::dto::ProcessingEntryDto {
            kind: p.kind,
            method: p.method,
            source: p.source,
            source_checksum: p.source_checksum,
            detection_version: p.detection_version,
            reader_version: p.reader_version,
            twin_version: p.twin_version,
            repair_version: p.repair_version,
            duration_ms: p.duration_ms,
            detail: p.detail.as_deref().and_then(|d| serde_json::from_str(d).ok()),
            processed_at: p.processed_at,
        })
        .collect();
    let rows: Vec<TwinRow> = sqlx::query_as(&format!("{TWIN_ROWS_SQL} WHERE t.model_id = ?2 ORDER BY t.coverage DESC"))
        .bind(CURRENT_TWIN_VERSION)
        .bind(model_id)
        .fetch_all(db)
        .await?;
    let twins = rows
        .into_iter()
        .map(|r| crate::dto::ModelTwinDto {
            twin_id: r.twin_id,
            name: r.name.unwrap_or_else(|| store::file_stem(&r.file_name)),
            relative_path: if r.directory.is_empty() {
                r.file_name
            } else {
                format!("{}/{}", r.directory, r.file_name)
            },
            scale: r.scale,
            coverage: r.coverage,
            current: r.valid,
        })
        .collect();
    let current_versions = crate::dto::ProcessingVersionsDto {
        detection: crate::geometry::support_separation::CURRENT_SUPPORT_DETECTION_VERSION,
        reader: CURRENT_SUPPORT_READER_VERSION,
        twin: CURRENT_TWIN_VERSION,
        remesh_twin: CURRENT_REMESH_TWIN_VERSION,
        repair: crate::geometry::watertight::CURRENT_REPAIR_VERSION,
    };
    Ok(crate::dto::ModelProcessingDto { processing, twins, current_versions })
}

/// Where a model's twin split comes from: its export entry, or its twins' files.
#[derive(Clone)]
pub struct TwinSource {
    pub exports: Option<SupportExports>,
    /// The export entry's key ([`support_exports::twin_key`]).
    pub key: String,
    /// Each twin's file, type and log scale.
    pub twins: Vec<(PathBuf, String, f64)>,
    pub uses: Vec<TwinUse>,
    /// Found by the remeshed twin lookup ([`remesh_twins`]).
    pub remeshed: bool,
}

impl TwinSource {
    /// The source of `set`'s split for a model with export entry key `base_key` (its key without
    /// twins), with its twins' files under `root`.
    pub fn new(exports: Option<SupportExports>, base_key: String, set: &TwinSet, root: &Path) -> Self {
        TwinSource {
            exports,
            key: support_exports::twin_key(base_key, Some(&set.key_part())),
            twins: set
                .twins
                .iter()
                .map(|t| {
                    let path = inheritance::build_full_file_path(root, &t.directory, &t.file_name);
                    (path, t.file_type.clone(), t.scale.ln())
                })
                .collect(),
            uses: set.uses(),
            remeshed: set.remeshed,
        }
    }

    /// How the split is made.
    pub fn method(&self) -> SplitMethod {
        if self.remeshed { SplitMethod::TwinRemeshed } else { SplitMethod::Twin }
    }
}

/// The twin split's support mask for `triangles`: the one in the export entry, or (when the entry
/// is missing) the split made again from the twins at their recorded scales. None when the twins
/// cannot be loaded or no longer split the model. Blocking.
pub fn twin_mask(source: &TwinSource, triangles: &[Triangle]) -> Option<Vec<bool>> {
    if let Some(mask) = source.exports.as_ref().and_then(|e| e.read_mask(&source.key, triangles.len())) {
        return Some(mask);
    }
    let loaded: Vec<Vec<Triangle>> = source
        .twins
        .iter()
        .map(|(path, file_type, _)| loader::load_model(path, file_type).map(|g| g.triangles))
        .collect::<Option<_>>()?;
    let split = if source.remeshed {
        // A remeshed twin's split is made again by its lookup's verification.
        loaded.first().filter(|_| loaded.len() == 1).and_then(|t| remesh_twin::split(triangles, t))
    } else {
        let twins: Vec<(&[Triangle], Option<f64>)> =
            loaded.iter().zip(&source.twins).map(|(t, (_, _, s))| (t.as_slice(), Some(*s))).collect();
        twin::split_scaled(triangles, &twins)
    };
    if split.is_none() {
        warn!(target: LOG_TARGET, "The recorded twins no longer split a model; it is split without them");
    }
    split.map(|x| x.is_support)
}

/// The supports of a split read for its export entry: the pieces closed where the mask cuts a shell
/// ([`welded::closed_split`]) and the supports read from them. None when nothing is split off.
pub fn read_split(triangles: &[Triangle], mask: &[bool], raft_height_mm: f32) -> Option<SupportLayout> {
    let (body, supports) = welded::closed_split(triangles, mask);
    (!supports.is_empty())
        .then(|| support_reader::read(&to_z_up(&body), &to_z_up(&supports), raft_height_mm).classification.layout)
}

/// A supported model the twin lookup should look at.
#[derive(Debug, Clone, sqlx::FromRow)]
struct PendingRow {
    id: Uuid,
    checksum: String,
    split_method: String,
    lookup_method: Option<String>,
    lookup_checksum: Option<String>,
    lookup_version: Option<i32>,
    /// The remeshed twin version the lookup ran at (when it found no exact twin).
    remesh_version: Option<i32>,
    probed_at: Option<DateTime<Utc>>,
}

/// How a pending model is looked up: against every sketch, or only those written since it was last.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Probe {
    All,
    Since(DateTime<Utc>),
}

/// The supported models to look up, and how: those whose scan split nothing off or split by shape,
/// or whose twin split no longer holds, not looked up yet on their content at this twin version
/// (all sketches), or looked up before the newest sketch without finding a twin (the sketches
/// since). `sets` are the models' current twins.
fn pending(
    rows: Vec<PendingRow>,
    sets: &std::collections::HashMap<Uuid, TwinSet>,
    newest_sketch: Option<DateTime<Utc>>,
) -> Vec<(Uuid, Probe)> {
    rows.into_iter()
        .filter_map(|r| {
            let exact = r.lookup_method.as_deref() == Some("twin");
            // A model without an exact twin was looked up for a remeshed one too, at its version.
            let looked_up = r.lookup_checksum.as_deref() == Some(r.checksum.as_str())
                && r.lookup_version == Some(CURRENT_TWIN_VERSION)
                && (exact || r.remesh_version == Some(CURRENT_REMESH_TWIN_VERSION));
            let found = exact || r.lookup_method.as_deref() == Some("twin_remeshed");
            let twin_split = matches!(r.split_method.as_str(), "twin" | "twin_remeshed");
            let probe = if !looked_up {
                Some(Probe::All)
            } else if found || twin_split {
                // A twin split: looked up again only once it no longer holds.
                let holds = sets.contains_key(&r.id) && twin_split;
                (!holds).then_some(Probe::All)
            } else {
                match (r.probed_at, newest_sketch) {
                    (Some(at), Some(newest)) if newest > at => Some(Probe::Since(at)),
                    _ => None,
                }
            };
            probe.map(|p| (r.id, p))
        })
        .collect()
}

/// The newest sketch or remeshed twin descriptor of a model not marked supported: a lookup made
/// before it has not seen every candidate.
pub(crate) async fn newest_candidate(db: &Db) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "SELECT MAX(at) FROM (
            SELECT MAX(s.sketched_at) AS at FROM model_twin_sketches s JOIN models m ON m.id = s.model_id
            WHERE s.version = ?1 AND m.effective_supported IS NOT 1
            UNION ALL
            SELECT MAX(d.described_at) FROM model_remesh_descriptors d JOIN models m ON m.id = d.model_id
            WHERE d.version = ?2 AND m.effective_supported IS NOT 1)",
    )
    .bind(CURRENT_TWIN_VERSION)
    .bind(CURRENT_REMESH_TWIN_VERSION)
    .fetch_one(db)
    .await
}

/// How a lookup probes the index: from the model's stored probe keys first (after a scan, loading
/// the file only on a hit), or with every key of the loaded file ([`ModelService::full_twin_search`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LookupMode {
    Stored,
    Full,
}

/// What the cached index was built from: the sketches' count, newest write and a sum over their
/// models' rows and paths, so a sketch added, changed or dropped, or a model moved, rebuilds it.
type IndexStamp = (i64, Option<String>, f64);

/// A built sketch index and the models in it.
type SketchIndex = (Arc<Index>, Arc<Vec<Indexed>>);

/// The sketch index of the models not marked supported, kept in memory between lookups while the
/// sketches it was built from are unchanged (64 entries a model: about 100 MB at 100,000 models).
#[derive(Default)]
pub struct SketchIndexCache {
    built: tokio::sync::Mutex<Option<(IndexStamp, SketchIndex)>>,
    /// How many times the index was built.
    pub(crate) builds: std::sync::atomic::AtomicUsize,
}

/// A sketched model in the lookup's index.
struct Indexed {
    id: Uuid,
    checksum: String,
    path: PathBuf,
    file_type: String,
    sketched_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct SketchRow {
    model_id: Uuid,
    sketch: Vec<u8>,
    sketched_at: DateTime<Utc>,
    checksum: String,
    directory: String,
    file_name: String,
    file_type: String,
}

/// What a lookup run did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TwinRun {
    pub pending: usize,
    /// Looked up from stored probe keys alone (no candidate hit, no file loaded).
    pub from_stored: usize,
    /// Stored probe keys deleted for models no longer waiting.
    pub dropped_probes: u64,
    pub indexed: usize,
    pub split: usize,
    pub unsplit: usize,
    pub failed: usize,
    pub index_ms: f64,
    pub lookup_ms: f64,
}

/// One model's lookup, done on a blocking thread.
struct Looked {
    geometry: Arc<LoadedGeometry>,
    lookup: twin::Lookup,
    /// The twins of the usable split, from the index.
    twins: Vec<(Uuid, String, f64, f64)>,
    /// Its probe keys, compacted to store while it waits ([`StoredProbes`]).
    stored: Vec<u8>,
    probe_ms: f64,
    verify_ms: f64,
    /// The remeshed twin lookup, when no exact twin split the model, and its time.
    remeshed: Option<(remesh_twin::Lookup, f64)>,
}

/// Whether an indexed model may be a twin of supported model `me` (content `checksum`): not
/// itself, not a copy of it, and (when looking again) sketched since the last lookup.
fn candidate_allowed(e: &Indexed, me: Uuid, checksum: &str, probe: Probe) -> bool {
    e.id != me
        && e.checksum != checksum
        && match probe {
            Probe::All => true,
            Probe::Since(at) => e.sketched_at > at,
        }
}

/// What a supported model's stored probe keys found in the index, without its file.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StoredHit {
    /// Candidates worth verifying.
    promising: usize,
    /// Candidates the stored keys cannot rule out: models with too little of their sketch below
    /// [`twin::STORED_PROBE_BOUND`] (fewer than about 1,500 unique keys, so small parts).
    unreachable: usize,
    probe_ms: f64,
}

impl StoredHit {
    /// Whether the file must be loaded and probed in full: a candidate hit, or (on a first lookup
    /// against every sketch) a candidate the stored keys cannot rule out. A re-probe against the
    /// sketches written since loads the file only on a hit, so the waiting models are probed again
    /// for the cost of a database read; small parts added since are not found for them.
    fn needs_file(&self, probe: Probe) -> bool {
        self.promising > 0 || (probe == Probe::All && self.unreachable > 0)
    }
}

/// Probes the index with a supported model's stored keys (`me`, content `checksum`). Blocking.
fn stored_hit(
    index: &Index,
    entries: &[Indexed],
    stored: &StoredProbes,
    me: Uuid,
    checksum: &str,
    probe: Probe,
) -> StoredHit {
    let t = Instant::now();
    let unreachable = (0..entries.len() as u32)
        .filter(|&i| candidate_allowed(&entries[i as usize], me, checksum, probe) && !index.reachable_by_stored(i))
        .count();
    let promising = index
        .candidates_stored(stored)
        .into_iter()
        .filter(|c| candidate_allowed(&entries[c.model as usize], me, checksum, probe) && twin::promising(c))
        .count();
    StoredHit { promising, unreachable, probe_ms: t.elapsed().as_secs_f64() * 1000.0 }
}

/// A lookup's [`processing::TWIN_LOOKUP`] record.
fn lookup_record(verdict: &str, checksum: &str, ms: f64, detail: serde_json::Value) -> processing::Record {
    processing::Record {
        kind: processing::TWIN_LOOKUP,
        method: verdict.to_string(),
        source: "twin_lookup".into(),
        source_checksum: checksum.to_string(),
        detection_version: None,
        reader_version: None,
        twin_version: Some(CURRENT_TWIN_VERSION),
        repair_version: None,
        duration_ms: Some(ms),
        detail: Some(detail),
    }
}

fn verdict_name(v: &Verdict) -> &'static str {
    match v {
        Verdict::NoRegistration => "unregistered",
        Verdict::Uncovered(..) => "uncovered",
        Verdict::Twin { .. } => "twin",
    }
}

/// The lookup's record: its verdict as the method, and what it measured.
fn lookup_detail(
    looked: &Looked,
    entries: &[Indexed],
    remesh_candidates: &[remesh_twins::Candidate],
    probe: Probe,
) -> (String, serde_json::Value) {
    let lookup = &looked.lookup;
    let verified: Vec<serde_json::Value> = lookup
        .checked
        .iter()
        .map(|c| {
            let mut v = json!({
                "twinId": entries[c.candidate.model as usize].id,
                "verdict": verdict_name(&c.verdict),
                "hits": c.candidate.hits,
                "share": c.candidate.share(),
            });
            match &c.verdict {
                Verdict::Uncovered(coverage, gap) => {
                    v["coverage"] = json!(coverage);
                    v["largestGap"] = json!(gap);
                }
                Verdict::Twin { registration, coverage } => {
                    v["coverage"] = json!(coverage);
                    v["scale"] = json!(registration.transform.scale);
                    v["inliers"] = json!(registration.inliers);
                }
                Verdict::NoRegistration => {}
            }
            v
        })
        .collect();
    let split = lookup.split.as_ref().map(|x| {
        json!({
            "supportShare": x.shape.support_share,
            "seamLoops": x.shape.seam_loops,
            "longestSeamMm": x.shape.longest_seam_mm,
            "innerWall": x.inner,
            "cleaned": [x.cleaned.0, x.cleaned.1],
            "supportLike": x.supports,
        })
    });
    let remeshed = looked.remeshed.as_ref().map(|(l, ms)| (l, *ms));
    let method = if lookup.usable().is_some() {
        "twin"
    } else if remeshed.is_some_and(|(l, _)| l.chosen.is_some()) {
        "twin_remeshed"
    } else if lookup.split.is_some() {
        // Twins verified, but their split does not look like supports.
        "refused"
    } else if !lookup.checked.is_empty() {
        "unverified"
    } else {
        "none"
    };
    let remesh_detail = lookup
        .usable()
        .is_none()
        .then(|| remesh_twins::detail(remeshed.map(|x| x.0), remesh_candidates, remeshed.map_or(0.0, |x| x.1)));
    let detail = json!({
        "remeshVersion": CURRENT_REMESH_TWIN_VERSION,
        "remeshed": remesh_detail,
        "promising": lookup.promising,
        "verified": verified,
        "split": split,
        "guard": method,
        "probedSince": match probe { Probe::All => None, Probe::Since(at) => Some(at) },
        "probedFrom": "file",
        "probeMs": looked.probe_ms,
        "verifyMs": looked.verify_ms,
    });
    (method.to_string(), detail)
}

impl ModelService {
    /// The sketch index of the models not marked supported: the cached one while its sketches are
    /// unchanged, else built again from the stored sketches.
    async fn sketch_index(&self, root: &Path) -> anyhow::Result<SketchIndex> {
        let stamp: IndexStamp = sqlx::query_as(
            "SELECT COUNT(*), MAX(s.sketched_at),
                TOTAL(m.rowid) + TOTAL(length(m.directory) + length(m.file_name)) + TOTAL(length(s.sketch))
             FROM model_twin_sketches s JOIN models m ON m.id = s.model_id
             WHERE s.version = ? AND m.effective_supported IS NOT 1",
        )
        .bind(CURRENT_TWIN_VERSION)
        .fetch_one(&self.db)
        .await?;
        let mut built = self.sketch_index.built.lock().await;
        if let Some((at, (index, entries))) = built.as_ref()
            && *at == stamp
        {
            return Ok((index.clone(), entries.clone()));
        }
        let mut index = Index::default();
        let mut entries: Vec<Indexed> = Vec::new();
        {
            let mut stream = sqlx::query_as::<_, SketchRow>(
                "SELECT s.model_id, s.sketch, s.sketched_at, m.checksum, m.directory, m.file_name, m.file_type
                 FROM model_twin_sketches s JOIN models m ON m.id = s.model_id
                 WHERE s.version = ? AND m.effective_supported IS NOT 1",
            )
            .bind(CURRENT_TWIN_VERSION)
            .fetch(&self.db);
            while let Some(row) = stream.try_next().await? {
                let Some(sketch) = Sketch::from_bytes(&row.sketch) else { continue };
                index.add(entries.len() as u32, &sketch);
                entries.push(Indexed {
                    id: row.model_id,
                    checksum: row.checksum,
                    path: inheritance::build_full_file_path(root, &row.directory, &row.file_name),
                    file_type: row.file_type,
                    sketched_at: row.sketched_at,
                });
            }
        }
        let index = Arc::new(
            tokio::task::spawn_blocking(move || {
                index.sort();
                index
            })
            .await?,
        );
        let entries = Arc::new(entries);
        self.sketch_index.builds.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *built = Some((stamp, (index.clone(), entries.clone())));
        Ok((index, entries))
    }

    /// Whether a supported model waits for a twin and has not had a full search since its content,
    /// the twin versions or the newest candidate changed (database reads only): supported, split
    /// by nothing or by shape (not by shells or by twins) on its current content, with no current
    /// twin set. Its content checksum when it does.
    pub(crate) async fn twin_search_due(&self, id: Uuid) -> anyhow::Result<Option<String>> {
        let checksum: Option<String> = sqlx::query_scalar(
            "SELECT m.checksum FROM models m
             JOIN model_processing s ON s.model_id = m.id AND s.kind = 'support_split'
             WHERE m.id = ? AND m.effective_supported = 1 AND s.method IN ('none', 'welded')
               AND s.source_checksum = m.checksum",
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await?;
        let Some(checksum) = checksum else { return Ok(None) };
        if twin_set(&self.db, id).await?.is_some() {
            return Ok(None);
        }
        if let Some(last) = processing::get(&self.db, id, processing::MODEL_UPDATE).await? {
            let remesh = last
                .detail
                .as_deref()
                .and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
                .and_then(|d| d["remeshVersion"].as_i64());
            let newest = newest_candidate(&self.db).await?;
            let searched = last.source_checksum == checksum
                && last.twin_version == Some(CURRENT_TWIN_VERSION)
                && remesh == Some(i64::from(CURRENT_REMESH_TWIN_VERSION))
                && newest.is_none_or(|n| n <= last.processed_at);
            if searched {
                return Ok(None);
            }
        }
        Ok(Some(checksum))
    }

    /// The full twin search of one waiting model ([`ModelService::twin_search_due`]): every probe key
    /// of its file (`geometry` when already loaded) against the whole sketch index, then the
    /// remeshed fallback, kept as the post-scan lookup keeps a split. It finds the twins a re-probe
    /// from stored keys misses (a small part added since). Some(true) when its twins split it,
    /// None when it could not be loaded.
    pub(crate) async fn full_twin_search(
        &self,
        id: Uuid,
        geometry: Option<Arc<LoadedGeometry>>,
    ) -> anyhow::Result<Option<bool>> {
        let Some(root) = self.root() else { return Ok(None) };
        let sketches = self.sketch_index(&root).await?;
        let found = self.resolve_one(id, Probe::All, &root, sketches, LookupMode::Full, geometry).await?;
        Ok(found.map(|(split, _)| split))
    }
    /// Looks up the twins of the supported models pending ([`pending`]), or of `only` that one when it
    /// is, and keeps the splits they give. Run after a scan has written the sketches and splits;
    /// cancellable between models.
    pub async fn resolve_twins(
        &self,
        only: Option<Uuid>,
        reporter: Option<&dyn ProgressReporter>,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<TwinRun> {
        let mut run = TwinRun::default();
        let Some(root) = self.root() else { return Ok(run) };
        run.dropped_probes = drop_stale_probes(&self.db).await?;
        let rows: Vec<PendingRow> = sqlx::query_as(
            "SELECT m.id, m.checksum, s.method AS split_method, l.method AS lookup_method,
                l.source_checksum AS lookup_checksum, l.twin_version AS lookup_version,
                json_extract(l.detail, '$.remeshVersion') AS remesh_version, l.processed_at AS probed_at
             FROM models m
             JOIN model_processing s ON s.model_id = m.id AND s.kind = 'support_split'
             LEFT JOIN model_processing l ON l.model_id = m.id AND l.kind = 'twin_lookup'
             WHERE m.effective_supported = 1 AND s.method IN ('none', 'welded', 'twin', 'twin_remeshed')
               AND s.source_checksum = m.checksum AND (?1 IS NULL OR m.id = ?1)",
        )
        .bind(only)
        .fetch_all(&self.db)
        .await?;
        let sets = twin_sets(&self.db).await?;
        let newest = newest_candidate(&self.db).await?;
        let todo = pending(rows, &sets, newest);
        run.pending = todo.len();
        if todo.is_empty() {
            return Ok(run);
        }

        let started = Instant::now();
        let (index, entries) = self.sketch_index(&root).await?;
        run.indexed = entries.len();
        run.index_ms = started.elapsed().as_secs_f64() * 1000.0;
        let message = format!(
            "Twin lookup: {} supported models to look up in {} sketches (index built in {:.0} ms)",
            todo.len(),
            entries.len(),
            run.index_ms
        );
        info!(target: LOG_TARGET, "{message}");
        if let Some(r) = reporter {
            r.on_log("info", &message, None).await;
        }

        let started = Instant::now();
        for (id, probe) in todo {
            if cancel.is_cancelled() {
                return Err(Cancelled.into());
            }
            match self.resolve_one(id, probe, &root, (index.clone(), entries.clone()), LookupMode::Stored, None).await {
                Ok(Some((true, _))) => run.split += 1,
                Ok(Some((false, stored))) => {
                    run.unsplit += 1;
                    run.from_stored += usize::from(stored);
                }
                Ok(None) => run.failed += 1,
                Err(err) => {
                    warn!(target: LOG_TARGET, "Twin lookup of model {id} failed: {err:#}");
                    run.failed += 1;
                }
            }
        }
        run.lookup_ms = started.elapsed().as_secs_f64() * 1000.0;
        let message = format!(
            "Twin lookup: {} split by their twins, {} without a twin, {} not loaded ({:.1} s)",
            run.split,
            run.unsplit,
            run.failed,
            run.lookup_ms / 1000.0
        );
        info!(target: LOG_TARGET, "{message}");
        if let Some(r) = reporter {
            r.on_log("info", &message, None).await;
        }
        Ok(run)
    }

    /// Looks up one model's twins and keeps what it found: Some((true, _)) when its twins split it,
    /// Some((false, from_stored)) when they do not (`from_stored`: its stored probe keys hit no
    /// candidate, so its file was not loaded), None when it could not be loaded.
    async fn resolve_one(
        &self,
        id: Uuid,
        probe: Probe,
        root: &Path,
        (index, entries): SketchIndex,
        mode: LookupMode,
        preloaded: Option<Arc<LoadedGeometry>>,
    ) -> anyhow::Result<Option<(bool, bool)>> {
        let Some(model) = self.get_model(id).await? else { return Ok(None) };
        if is_non_geometry_type(&model.file_type) {
            return Ok(None);
        }
        let previous_split = processing::get(&self.db, id, processing::SUPPORT_SPLIT).await?;
        let was_twin = previous_split.as_ref().is_some_and(|p| matches!(p.method.as_str(), "twin" | "twin_remeshed"));
        // The remeshed twin lookup's candidates (from stored descriptors), for when no exact twin is.
        let since = match probe {
            Probe::All => None,
            Probe::Since(at) => Some(at),
        };
        let remesh_candidates = Arc::new(remesh_twins::candidates(&self.db, &model, root, since).await?);
        // Probed from its stored keys first: its file is loaded only when a candidate hits, when its
        // collection has remeshed twin candidates, or when a twin split that no longer holds must be
        // split again.
        let mut stored_hits = None;
        if !was_twin
            && mode == LookupMode::Stored
            && remesh_candidates.is_empty()
            && let Some(stored) = stored_probes(&self.db, id, &model.checksum).await?
        {
            let (me, mine, ix, es) = (model.id, model.checksum.clone(), index.clone(), entries.clone());
            let hit = tokio::task::spawn_blocking(move || stored_hit(&ix, &es, &stored, me, &mine, probe)).await?;
            if !hit.needs_file(probe) {
                let detail = json!({
                    "remeshVersion": CURRENT_REMESH_TWIN_VERSION,
                    "remeshed": remesh_twins::detail(None, &remesh_candidates, 0.0),
                    "promising": 0,
                    "verified": [],
                    "split": null,
                    "guard": "none",
                    "probedSince": match probe { Probe::All => None, Probe::Since(at) => Some(at) },
                    "probedFrom": "stored",
                    "storedHits": 0,
                    "unreachable": hit.unreachable,
                    "probeMs": hit.probe_ms,
                    "verifyMs": 0.0,
                });
                processing::record(&self.db, id, &lookup_record("none", &model.checksum, hit.probe_ms, detail)).await?;
                return Ok(Some((false, true)));
            }
            stored_hits = Some(hit.promising);
        }
        let path = inheritance::build_full_file_path(root, &model.directory, &model.file_name);
        let (me, my_checksum, file_type) = (model.id, model.checksum.clone(), model.file_type.clone());
        let loads = self.twin_lookup_loads.clone();
        let looked = {
            let entries = entries.clone();
            let remesh_candidates = remesh_candidates.clone();
            tokio::task::spawn_blocking(move || -> Option<Looked> {
                let geometry = match preloaded {
                    Some(g) => g,
                    None => {
                        let g = loader::load_model(&path, &file_type)?;
                        loads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        Arc::new(g)
                    }
                };
                let t = Instant::now();
                let s = Mesh::new(&geometry.triangles);
                let probes = twin::probe_keys(&s);
                let candidates: Vec<twin::Candidate> = index
                    .candidates_sorted(&probes)
                    .into_iter()
                    .filter(|c| candidate_allowed(&entries[c.model as usize], me, &my_checksum, probe))
                    .collect();
                let stored = StoredProbes::of(&probes).to_bytes();
                let probe_ms = t.elapsed().as_secs_f64() * 1000.0;
                let t = Instant::now();
                let lookup = twin::lookup(&s, &probes, &candidates, |c| {
                    let e = &entries[c.model as usize];
                    loader::load_model(&e.path, &e.file_type).map(|g| g.triangles)
                });
                let twins = lookup
                    .usable()
                    .map(|x| {
                        x.twins
                            .iter()
                            .map(|(i, reg, coverage)| {
                                let e = &entries[lookup.checked[*i].candidate.model as usize];
                                (e.id, e.checksum.clone(), reg.transform.scale, *coverage)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let verify_ms = t.elapsed().as_secs_f64() * 1000.0;
                // No exact twin: the remeshed twin lookup.
                let remeshed = (lookup.usable().is_none() && !remesh_candidates.is_empty()).then(|| {
                    let t = Instant::now();
                    let found = remesh_twins::look_up(s, &remesh_candidates);
                    (found, t.elapsed().as_secs_f64() * 1000.0)
                });
                let twins = match remeshed.as_ref().and_then(|(l, _)| remesh_twins::chosen(l, &remesh_candidates)) {
                    Some((c, scale, coverage)) => vec![(c.id, c.checksum.clone(), scale, coverage)],
                    None => twins,
                };
                Some(Looked { geometry, lookup, twins, stored, probe_ms, verify_ms, remeshed })
            })
            .await?
        };
        let Some(looked) = looked else { return Ok(None) };
        let (verdict, mut detail) = lookup_detail(&looked, &entries, &remesh_candidates, probe);
        detail["storedHits"] = json!(stored_hits);
        detail["full"] = json!(mode == LookupMode::Full);
        let found = matches!(verdict.as_str(), "twin" | "twin_remeshed");
        // Its probe keys kept while it waits for a twin; dropped once its twins split it.
        if found {
            delete_probes(&self.db, id).await?;
        } else {
            store_probes(&self.db, id, &model.checksum, &looked.stored).await?;
        }

        // The model's twins: replaced by those found, or none.
        let mut tx = self.db.begin().await?;
        sqlx::query("DELETE FROM model_twins WHERE model_id = ?").bind(id).execute(&mut *tx).await?;
        for (twin_id, checksum, scale, coverage) in &looked.twins {
            sqlx::query(
                "INSERT INTO model_twins (model_id, twin_id, twin_checksum, scale, coverage, version)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(twin_id)
            .bind(checksum)
            .bind(scale)
            .bind(coverage)
            .bind(CURRENT_TWIN_VERSION)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        let ms = looked.probe_ms + looked.verify_ms + looked.remeshed.as_ref().map_or(0.0, |x| x.1);
        let lookup_row = lookup_record(&verdict, &model.checksum, ms, detail);
        processing::record(&self.db, id, &lookup_row).await?;

        if found {
            let remeshed = verdict == "twin_remeshed";
            let mask = if remeshed {
                looked.remeshed.as_ref().and_then(|(l, _)| l.split.as_ref()).map(|x| x.is_support.clone())
            } else {
                looked.lookup.usable().map(|x| x.is_support.clone())
            }
            .unwrap_or_default();
            let uses: Vec<TwinUse> = looked
                .twins
                .iter()
                .map(|(t, _, scale, coverage)| TwinUse { twin_id: *t, scale: *scale, coverage: *coverage })
                .collect();
            let set_key = key_part(looked.twins.iter().map(|t| t.1.as_str()), remeshed);
            let method = if remeshed { SplitMethod::TwinRemeshed } else { SplitMethod::Twin };
            let geometry = Arc::unwrap_or_clone(looked.geometry);
            self.keep_split(&model, geometry, Some(mask), method, uses, Some(set_key)).await?;
        } else if was_twin {
            // The twin split no longer holds and no twin was found: split as the scan would without one.
            let raft_height_mm = model.hull_raft_height_mm.unwrap_or(DEFAULT_RAFT_HEIGHT_MM);
            let supported = model.calculated_supported;
            let geometry = Arc::unwrap_or_clone(looked.geometry);
            let (geometry, classification) = tokio::task::spawn_blocking(move || {
                let c = spin::checked_classification(&geometry.triangles, raft_height_mm);
                (geometry, c)
            })
            .await?;
            let method = SplitMethod::of(classification.as_ref());
            let mask = classification.and_then(|c| c.mask_for(supported));
            self.keep_split(&model, geometry, mask, method, Vec::new(), None).await?;
        }
        Ok(Some((found, false)))
    }

    /// Makes `mask` the model's split everywhere the scan keeps one: its export entry (supports
    /// read from the closed pieces), previews and spin mesh redrawn, `support_parts_found`, and its
    /// support split record. `twins` is the key part of a twin split's twins.
    async fn keep_split(
        &self,
        model: &ModelRow,
        geometry: LoadedGeometry,
        mask: Option<Vec<bool>>,
        method: SplitMethod,
        uses: Vec<TwinUse>,
        twins: Option<String>,
    ) -> anyhow::Result<()> {
        let raft_height_mm = model.hull_raft_height_mm.unwrap_or(DEFAULT_RAFT_HEIGHT_MM);
        let supported = model.calculated_supported;
        let key = support_exports::twin_key(
            support_exports::key(&model.checksum, raft_height_mm, supported, true),
            twins.as_deref(),
        );
        let exports = self.support_exports.clone();
        let previews_on = self.app_config.get().await.map(|c| c.generate_previews_enabled).unwrap_or(false);
        let preview = self.preview.clone().filter(|_| previews_on);
        let stem = preview::preview_stem(&model.checksum, raft_height_mm, supported);
        let spin_target =
            preview.as_ref().and_then(|p| p.cache_dir()).map(Path::to_path_buf).zip(store::expected_spin_path(model));
        let started = Instant::now();
        let (stored, found, spin_written) = tokio::task::spawn_blocking(move || {
            let layout = mask.as_deref().and_then(|m| read_split(&geometry.triangles, m, raft_height_mm));
            let found = layout.is_some();
            let stored = exports.as_ref().and_then(|e| e.store(&key, mask.as_deref().zip(layout.as_ref())));
            if let Some(p) = &preview {
                p.generate_variants(&geometry, &stem, mask.as_deref().filter(|_| found), true, true, true);
            }
            let spin_written = spin_target.map(|(dir, relative)| {
                // Its name holds no mask, so the file there is drawn again.
                let _ = std::fs::remove_file(dir.join(&relative));
                spin::store_for_model(&dir, &relative, &geometry.triangles, mask.as_deref().filter(|_| found))
            });
            (stored, found, spin_written)
        })
        .await?;
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        if let Some(path) = stored {
            let previous =
                store::set_support_export_index(&self.db, model.id, &path, CURRENT_SUPPORT_READER_VERSION).await?;
            if let Some(previous) = previous.filter(|p| *p != path) {
                self.release_old_export(Some(&previous), Some(&path)).await;
            }
        }
        if let Some(written) = spin_written {
            sqlx::query("UPDATE models SET spin_mesh_path = ?, spin_mesh_version = ? WHERE id = ?")
                .bind(&written)
                .bind(written.as_ref().map(|_| spin::CURRENT_SPIN_MESH_VERSION))
                .bind(model.id)
                .execute(&self.db)
                .await?;
        }
        self.set_support_parts_found(model.id, found).await?;
        let method = if found { method } else { SplitMethod::None };
        let split = SplitRecord { method, twins: if found { uses } else { Vec::new() }, read: found, duration_ms: ms };
        processing::record_split(&self.db, model.id, "twin_lookup", &model.checksum, &split).await;
        Ok(())
    }

    /// The source of the model's twin split, when it has one that holds: for every split of the
    /// model (viewer, previews, spin meshes, exports) to take the twin split's mask.
    pub async fn twin_source(&self, model: &ModelRow, raft_height_mm: f32, checked: bool) -> Option<TwinSource> {
        if model.supported() != Some(true) {
            return None;
        }
        let root = self.root()?;
        let set = match twin_set(&self.db, model.id).await {
            Ok(set) => set?,
            Err(err) => {
                warn!(target: LOG_TARGET, "Failed to read the twins of model {}: {err}", model.id);
                return None;
            }
        };
        let base = support_exports::key(&model.checksum, raft_height_mm, model.calculated_supported, checked);
        Some(TwinSource::new(self.support_exports.clone(), base, &set, &root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(split: &str, lookup: Option<(&str, &str, i32)>, probed: Option<i64>) -> PendingRow {
        PendingRow {
            id: Uuid::new_v4(),
            checksum: "abc".into(),
            split_method: split.into(),
            lookup_method: lookup.map(|l| l.0.into()),
            lookup_checksum: lookup.map(|l| l.1.into()),
            lookup_version: lookup.map(|l| l.2),
            remesh_version: lookup.map(|_| CURRENT_REMESH_TWIN_VERSION),
            probed_at: probed.map(|s| DateTime::from_timestamp(s, 0).unwrap()),
        }
    }

    #[test]
    fn pending_models_are_looked_up_once_until_something_changes() {
        let v = CURRENT_TWIN_VERSION;
        let newest = Some(DateTime::from_timestamp(100, 0).unwrap());
        let never = row("welded", None, None);
        let other_content = row("none", Some(("none", "old", v)), Some(50));
        let other_version = row("none", Some(("none", "abc", v - 1)), Some(200));
        let current = row("welded", Some(("none", "abc", v)), Some(200));
        let since = row("none", Some(("refused", "abc", v)), Some(50));
        let holding = row("twin", Some(("twin", "abc", v)), Some(50));
        let broken = row("twin", Some(("twin", "abc", v)), Some(50));
        let fell_back = row("welded", Some(("twin", "abc", v)), Some(50));
        let mut sets = std::collections::HashMap::new();
        for r in [&holding, &fell_back] {
            sets.insert(r.id, TwinSet { twins: Vec::new(), remeshed: false });
        }
        let ids =
            [&never, &other_content, &other_version, &current, &since, &holding, &broken, &fell_back].map(|r| r.id);
        let rows = vec![never, other_content, other_version, current, since, holding, broken, fell_back];
        let got = pending(rows, &sets, newest);
        let probe_of = |id: Uuid| got.iter().find(|(i, _)| *i == id).map(|(_, p)| *p);
        assert_eq!(probe_of(ids[0]), Some(Probe::All));
        assert_eq!(probe_of(ids[1]), Some(Probe::All));
        assert_eq!(probe_of(ids[2]), Some(Probe::All));
        assert_eq!(probe_of(ids[3]), None, "looked up after the newest sketch");
        assert_eq!(probe_of(ids[4]), Some(Probe::Since(DateTime::from_timestamp(50, 0).unwrap())));
        assert_eq!(probe_of(ids[5]), None, "its twin split holds");
        assert_eq!(probe_of(ids[6]), Some(Probe::All), "a twin changed");
        assert_eq!(probe_of(ids[7]), Some(Probe::All), "the scan no longer split it by its twins");
    }

    #[tokio::test]
    async fn a_re_probe_finds_a_new_twin_from_stored_keys_and_loads_the_file_only_on_the_hit() {
        use crate::geometry::twin::Similarity;
        use crate::geometry::twin::tests::{blob, rot, supported, to_tris};
        use crate::test_support::{TestDb, model, write_file};
        use std::sync::atomic::Ordering;

        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let svc = crate::services::models::tests::service(&t, root.path());
        let cancel = tokio_util::sync::CancellationToken::new();
        let loads = || svc.twin_lookup_loads.load(Ordering::Relaxed);

        // A supported model with welded supports, waiting for a twin: its keys stored by the scan.
        let u = blob(80, 7);
        let x = Similarity { r: rot(0.7, 1.1), scale: 0.75, t: [5.0, 20.0, -3.0] };
        let s = to_tris(&supported(&u, &x).0);
        // Files are Z up; the loader turns them Y up, with the spikes below the ball.
        let save = |t: &[Triangle]| crate::geometry::saver::save_stl(&crate::geometry::to_z_up(t), None);
        write_file(root.path(), "Mini/ball_supported.stl", save(&s));
        let mut waiting = model("ball_supported.stl", "Mini");
        waiting.calculated_supported = Some(true);
        t.insert_model(&waiting).await;
        processing::record_split(
            &t.db,
            waiting.id,
            "scan",
            &waiting.checksum,
            &SplitRecord::new(SplitMethod::Welded, 0.0),
        )
        .await;
        let (_, probes) = sketch_and_probes(&s, true);
        store_probes(&t.db, waiting.id, &waiting.checksum, &probes.unwrap()).await.unwrap();

        // An unrelated model without supports: the first lookup is done from the stored keys alone.
        let add = |name: &'static str, mesh: Vec<Triangle>| {
            let (t, root, save) = (&t, root.path(), &save);
            async move {
                write_file(root, &format!("Bare/{name}"), save(&mesh));
                let mut m = model(name, "Bare");
                m.calculated_supported = Some(false);
                t.insert_model(&m).await;
                store_sketch(&t.db, m.id, &sketch_bytes(&mesh)).await.unwrap();
                m.id
            }
        };
        add("other.stl", to_tris(&blob(80, 8))).await;
        let run = svc.resolve_twins(None, None, &cancel).await.unwrap();
        assert_eq!((run.pending, run.from_stored, run.unsplit), (1, 1, 1), "{run:?}");
        assert_eq!(loads(), 0, "no candidate hit: the supported file was not loaded");

        // Nothing new: nothing pending, nothing loaded.
        let run = svc.resolve_twins(None, None, &cancel).await.unwrap();
        assert_eq!(run.pending, 0, "{run:?}");

        // Its twin appears. The re-probe against the new sketch finds it from the stored keys, and
        // only then loads the supported file, once, to verify and split it.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let twin_id = add("ball.stl", to_tris(&u)).await;
        let run = svc.resolve_twins(None, None, &cancel).await.unwrap();
        let lookup = processing::get(&t.db, waiting.id, processing::TWIN_LOOKUP).await.unwrap().unwrap();
        assert_eq!((run.pending, run.from_stored, run.split), (1, 0, 1), "{run:?} {lookup:?}");
        assert_eq!(loads(), 1, "loaded once, on the hit");
        let lookup = processing::get(&t.db, waiting.id, processing::TWIN_LOOKUP).await.unwrap().unwrap();
        assert_eq!(lookup.method, "twin");
        let detail: serde_json::Value = serde_json::from_str(lookup.detail.as_deref().unwrap()).unwrap();
        assert_eq!(detail["storedHits"], 1, "{detail}");
        assert!(detail["probedSince"].is_string(), "{detail}");
        let set = twin_set(&t.db, waiting.id).await.unwrap().expect("split by its twin");
        assert_eq!(set.twins.iter().map(|t| t.id).collect::<Vec<_>>(), [twin_id]);
        // Resolved: its keys are deleted.
        assert!(stored_probes(&t.db, waiting.id, &waiting.checksum).await.unwrap().is_none());
    }

    #[test]
    fn models_without_an_exact_twin_are_looked_up_again_at_a_new_remeshed_version() {
        let v = CURRENT_TWIN_VERSION;
        let newest = Some(DateTime::from_timestamp(100, 0).unwrap());
        let mut old_remesh = row("welded", Some(("none", "abc", v)), Some(200));
        old_remesh.remesh_version = Some(CURRENT_REMESH_TWIN_VERSION - 1);
        let mut exact = row("twin", Some(("twin", "abc", v)), Some(200));
        exact.remesh_version = None;
        let remeshed = row("twin_remeshed", Some(("twin_remeshed", "abc", v)), Some(50));
        let broken_remeshed = row("twin_remeshed", Some(("twin_remeshed", "abc", v)), Some(50));
        let mut sets = std::collections::HashMap::new();
        for r in [&exact, &remeshed] {
            sets.insert(r.id, TwinSet { twins: Vec::new(), remeshed: r.split_method == "twin_remeshed" });
        }
        let ids = [&old_remesh, &exact, &remeshed, &broken_remeshed].map(|r| r.id);
        let got = pending(vec![old_remesh, exact, remeshed, broken_remeshed], &sets, newest);
        let probe_of = |id: Uuid| got.iter().find(|(i, _)| *i == id).map(|(_, p)| *p);
        assert_eq!(probe_of(ids[0]), Some(Probe::All), "looked up at an older remeshed version");
        assert_eq!(probe_of(ids[1]), None, "an exact twin needs no remeshed lookup");
        assert_eq!(probe_of(ids[2]), None, "its remeshed twin split holds");
        assert_eq!(probe_of(ids[3]), Some(Probe::All), "its remeshed twin changed");
    }

    #[test]
    fn a_twin_sets_key_names_its_twins_in_any_order() {
        let twin = |c: &str| TwinFile {
            id: Uuid::new_v4(),
            checksum: c.into(),
            directory: String::new(),
            file_name: String::new(),
            file_type: "stl".into(),
            scale: 1.0,
            coverage: 1.0,
        };
        let a = TwinSet { twins: vec![twin("b"), twin("a")], remeshed: false };
        let b = TwinSet { twins: vec![twin("a"), twin("b")], remeshed: false };
        assert_eq!(a.key_part(), b.key_part());
        assert_ne!(a.key_part(), TwinSet { twins: vec![twin("a")], remeshed: false }.key_part());
        // A remeshed twin's split is another entry than an exact one's.
        assert_ne!(a.key_part(), TwinSet { remeshed: true, ..a.clone() }.key_part());
    }
}
