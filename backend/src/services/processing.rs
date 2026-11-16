//! How each model was processed (its provenance), so a problem with a model's split, exports or
//! repair can be traced back to what made them: one `model_processing` row per model and kind,
//! replaced each time that processing runs again.
//!
//! - [`SUPPORT_SPLIT`]: how the supports were split off ([`SplitMethod`]), with the detection,
//!   reader and twin versions and, for a twin split, the twins (id, scale, coverage). Written by
//!   the scan, the twin lookup and on-demand splits (viewer and exports).
//! - [`TWIN_LOOKUP`]: the twin lookup's verdict, the candidates it verified and the guard
//!   measures. Also the lookup's state: a supported model is looked up again when its content, the
//!   twin version or the catalogue's sketches change ([`crate::services::twins`]).
//! - [`REPAIR`]: the last watertight repair of the model's body: repair version, local mends, box
//!   mends, follow-up passes and time.

use serde::Serialize;
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

use crate::db::{Db, now};
use crate::entities::{ModelProcessing, ModelRow};
use crate::geometry::support_reader::CURRENT_SUPPORT_READER_VERSION;
use crate::geometry::support_separation::{CURRENT_SUPPORT_DETECTION_VERSION, Classification};
use crate::geometry::twin::CURRENT_TWIN_VERSION;
use crate::geometry::watertight::{CURRENT_REPAIR_VERSION, RepairStats};

const LOG_TARGET: &str = "findamodel.Processing";

pub const SUPPORT_SPLIT: &str = "support_split";
pub const TWIN_LOOKUP: &str = "twin_lookup";
pub const REPAIR: &str = "repair";
/// A run of the model's data update ([`crate::services::model_data`]).
pub const MODEL_UPDATE: &str = "model_update";

/// How a model's supports were split off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitMethod {
    /// Nothing split off (no supports found, or the model is not supported).
    None,
    /// Separate shells, by the classifier's rules by component.
    Shells,
    /// Supports welded into one shell, cut off by their shape ([`crate::geometry::support_separation::welded`]).
    Welded,
    /// Supports welded into one shell, cut off by the model's twin ([`crate::geometry::twin`]).
    Twin,
    /// Supports welded into one shell, cut off by the model's remeshed twin
    /// ([`crate::geometry::remesh_twin`]).
    TwinRemeshed,
}

impl SplitMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            SplitMethod::None => "none",
            SplitMethod::Shells => "shells",
            SplitMethod::Welded => "welded",
            SplitMethod::Twin => "twin",
            SplitMethod::TwinRemeshed => "twin_remeshed",
        }
    }

    /// A split by twins, exact or remeshed.
    pub fn is_twin(self) -> bool {
        matches!(self, SplitMethod::Twin | SplitMethod::TwinRemeshed)
    }

    /// The method of a classifier split (None when it found nothing).
    pub fn of(classification: Option<&Classification>) -> Self {
        match classification {
            None => SplitMethod::None,
            Some(c) if c.welded => SplitMethod::Welded,
            Some(_) => SplitMethod::Shells,
        }
    }
}

/// One twin a split used, as recorded.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TwinUse {
    pub twin_id: Uuid,
    pub scale: f64,
    pub coverage: f64,
}

/// A model's support split, for its [`SUPPORT_SPLIT`] row.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitRecord {
    pub method: SplitMethod,
    /// The twins of a twin split.
    pub twins: Vec<TwinUse>,
    /// The supports were read into an export entry at the current reader version.
    pub read: bool,
    pub duration_ms: f64,
}

impl SplitRecord {
    pub fn new(method: SplitMethod, duration_ms: f64) -> Self {
        SplitRecord { method, twins: Vec::new(), read: false, duration_ms }
    }

    pub fn row(&self, source: &str, source_checksum: &str) -> Record {
        let detail = (!self.twins.is_empty()).then(|| serde_json::json!({ "twins": self.twins }));
        Record {
            kind: SUPPORT_SPLIT,
            method: self.method.as_str().to_string(),
            source: source.to_string(),
            source_checksum: source_checksum.to_string(),
            detection_version: Some(CURRENT_SUPPORT_DETECTION_VERSION),
            reader_version: self.read.then_some(CURRENT_SUPPORT_READER_VERSION),
            twin_version: self.method.is_twin().then_some(CURRENT_TWIN_VERSION),
            repair_version: None,
            duration_ms: Some(self.duration_ms),
            detail,
        }
    }
}

/// What a repair did, for its [`REPAIR`] row: cheaply counted while it ran.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairRecord {
    pub faces_in: usize,
    pub faces_out: usize,
    /// Spots of a shell crossing itself mended in place, and those left to the box mend.
    pub local_mended: usize,
    pub local_left: usize,
    /// Boxes mended in the first pass, and in the follow-up passes (over how many passes).
    pub mend_boxes: usize,
    pub follow_up_boxes: usize,
    pub follow_up_passes: usize,
    /// Parts remeshed whole, booleans run and failed.
    pub remeshed_groups: usize,
    pub booleans: usize,
    pub booleans_failed: usize,
    pub repair_ms: f64,
}

impl From<&RepairStats> for RepairRecord {
    fn from(s: &RepairStats) -> Self {
        RepairRecord {
            faces_in: s.faces_in,
            faces_out: s.faces_out,
            local_mended: s.local_mended,
            local_left: s.local_left,
            mend_boxes: s.mend_boxes,
            follow_up_boxes: s.follow_up_boxes,
            follow_up_passes: s.follow_up_passes,
            remeshed_groups: s.remeshed_groups,
            booleans: s.booleans,
            booleans_failed: s.booleans_failed,
            repair_ms: s.ms,
        }
    }
}

/// Records the repair of a model's body, when one ran (a repair loaded from the store leaves the
/// record of the repair that made it).
pub async fn record_repair(db: &Db, model: &ModelRow, stats: Option<&RepairStats>) {
    if let Some(stats) = stats {
        record_or_warn(db, model.id, &RepairRecord::from(stats).row(&model.checksum)).await;
    }
}

impl RepairRecord {
    pub fn row(&self, source_checksum: &str) -> Record {
        Record {
            kind: REPAIR,
            method: "remeshed".into(),
            source: "repair".into(),
            source_checksum: source_checksum.to_string(),
            detection_version: Some(CURRENT_SUPPORT_DETECTION_VERSION),
            reader_version: None,
            twin_version: None,
            repair_version: Some(CURRENT_REPAIR_VERSION),
            duration_ms: Some(self.repair_ms),
            detail: serde_json::to_value(self).ok(),
        }
    }
}

/// A `model_processing` row to write.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub kind: &'static str,
    pub method: String,
    pub source: String,
    pub source_checksum: String,
    pub detection_version: Option<i32>,
    pub reader_version: Option<i32>,
    pub twin_version: Option<i32>,
    pub repair_version: Option<i32>,
    pub duration_ms: Option<f64>,
    pub detail: Option<Value>,
}

/// Writes `record` as the model's row of its kind, replacing the one there.
pub async fn record(db: &Db, model_id: Uuid, record: &Record) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO model_processing (model_id, kind, method, source, source_checksum, detection_version,
            reader_version, twin_version, repair_version, duration_ms, detail, processed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(model_id, kind) DO UPDATE SET method = excluded.method, source = excluded.source,
            source_checksum = excluded.source_checksum, detection_version = excluded.detection_version,
            reader_version = excluded.reader_version, twin_version = excluded.twin_version,
            repair_version = excluded.repair_version, duration_ms = excluded.duration_ms,
            detail = excluded.detail, processed_at = excluded.processed_at",
    )
    .bind(model_id)
    .bind(record.kind)
    .bind(&record.method)
    .bind(&record.source)
    .bind(&record.source_checksum)
    .bind(record.detection_version)
    .bind(record.reader_version)
    .bind(record.twin_version)
    .bind(record.repair_version)
    .bind(record.duration_ms)
    .bind(record.detail.as_ref().map(Value::to_string))
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

/// [`record`], logging a failure instead of returning it: the processing went ahead either way.
pub async fn record_or_warn(db: &Db, model_id: Uuid, row: &Record) {
    if let Err(err) = record(db, model_id, row).await {
        warn!(target: LOG_TARGET, "Failed to record the {} of model {model_id}: {err}", row.kind);
    }
}

/// Records a model's support split, unless it was split the same way at the same versions from the
/// same content before (so viewing a model does not rewrite its row each time). The row's source
/// stays the first that made it.
pub async fn record_split(db: &Db, model_id: Uuid, source: &str, source_checksum: &str, split: &SplitRecord) {
    let row = split.row(source, source_checksum);
    let current = match get(db, model_id, SUPPORT_SPLIT).await {
        Ok(current) => current,
        Err(err) => {
            warn!(target: LOG_TARGET, "Failed to read the support split of model {model_id}: {err}");
            None
        }
    };
    let same = current.is_some_and(|c| {
        c.method == row.method
            && c.source_checksum == row.source_checksum
            && c.detection_version == row.detection_version
            && c.twin_version == row.twin_version
            && (c.reader_version == row.reader_version || row.reader_version.is_none())
            && twin_ids(c.detail.as_deref().and_then(|d| serde_json::from_str::<Value>(d).ok()).as_ref())
                == twin_ids(row.detail.as_ref())
    });
    if !same {
        record_or_warn(db, model_id, &row).await;
    }
}

/// The twin ids a split record's detail names (floats read back from JSON need not round-trip).
fn twin_ids(detail: Option<&Value>) -> Vec<String> {
    detail
        .and_then(|d| d["twins"].as_array())
        .map(|t| t.iter().filter_map(|t| t["twinId"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// The model's row of `kind`.
pub async fn get(db: &Db, model_id: Uuid, kind: &str) -> sqlx::Result<Option<ModelProcessing>> {
    sqlx::query_as("SELECT * FROM model_processing WHERE model_id = ? AND kind = ?")
        .bind(model_id)
        .bind(kind)
        .fetch_optional(db)
        .await
}

/// Every row of the model, by kind.
pub async fn for_model(db: &Db, model_id: Uuid) -> sqlx::Result<Vec<ModelProcessing>> {
    sqlx::query_as("SELECT * FROM model_processing WHERE model_id = ? ORDER BY kind").bind(model_id).fetch_all(db).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDb, model};

    #[tokio::test]
    async fn a_record_replaces_the_row_of_its_kind_only() {
        let t = TestDb::new().await;
        let m = model("a.stl", "");
        t.insert_model(&m).await;
        let mut split = SplitRecord::new(SplitMethod::Welded, 12.0);
        record_split(&t.db, m.id, "scan", "abc", &split).await;
        record_or_warn(&t.db, m.id, &RepairRecord { local_mended: 3, ..Default::default() }.row("abc")).await;
        let rows = for_model(&t.db, m.id).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| (r.kind.as_str(), r.method.as_str())).collect::<Vec<_>>(),
            [("repair", "remeshed"), ("support_split", "welded")]
        );
        assert_eq!(rows[0].repair_version, Some(CURRENT_REPAIR_VERSION));
        let detail: Value = serde_json::from_str(rows[0].detail.as_deref().unwrap()).unwrap();
        assert_eq!(detail["localMended"], 3);

        split.method = SplitMethod::Twin;
        split.twins = vec![TwinUse { twin_id: Uuid::nil(), scale: 1.0, coverage: 0.99 }];
        record_split(&t.db, m.id, "twin_lookup", "abc", &split).await;
        let row = get(&t.db, m.id, SUPPORT_SPLIT).await.unwrap().unwrap();
        assert_eq!((row.method.as_str(), row.source.as_str()), ("twin", "twin_lookup"));
        assert_eq!(row.twin_version, Some(CURRENT_TWIN_VERSION));
        let detail: Value = serde_json::from_str(row.detail.as_deref().unwrap()).unwrap();
        assert_eq!(detail["twins"][0]["coverage"], 0.99);

        // The same split again (from the viewer) keeps the row as the lookup wrote it.
        record_split(&t.db, m.id, "viewer", "abc", &split).await;
        assert_eq!(get(&t.db, m.id, SUPPORT_SPLIT).await.unwrap().unwrap().source, "twin_lookup");
        assert_eq!(for_model(&t.db, m.id).await.unwrap().len(), 2);
    }
}
