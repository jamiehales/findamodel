//! The model's data update: work on one model's derived data that is due but was not done when
//! the model was scanned, run before the model's data is used. It is built to hold more steps; for
//! now it has one:
//!
//! - **Full twin search** ([`ModelService::full_twin_search`]) for a supported model waiting for a
//!   twin (no split, or split by shape, and no current twin set): every probe key of its file
//!   against the whole sketch index, then the remeshed fallback. It finds twins the post-scan
//!   re-probe from stored keys misses, such as a small part added in a later scan. A split found
//!   is kept as the post-scan lookup keeps one (export entry, previews, spin mesh, `model_twins`,
//!   processing rows, stored probe keys dropped).
//!
//! Whether a step is due is decided from the database alone, so an update with nothing to do never
//! loads the file. A step that is due loads it, unless the caller passes the geometry it loaded
//! anyway. Updates run once per model at a time: concurrent calls share one run. Each run that did
//! a step is recorded as the model's `model_update` processing row, which is what makes the step
//! not due again until its inputs change.
//!
//! Triggers: before an export of the model's split, resupported or repaired data (the VOXL
//! downloads, the split and repaired geometry, plate exports), in line so the export uses the
//! result; when the model is added to a printing list, in the background; and when its page is
//! opened (`POST /api/models/{id}/update-data`).

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use serde_json::json;
use tracing::warn;
use uuid::Uuid;

use crate::geometry::LoadedGeometry;
use crate::geometry::remesh_twin::CURRENT_REMESH_TWIN_VERSION;
use crate::geometry::twin::CURRENT_TWIN_VERSION;
use crate::services::models::ModelService;
use crate::services::processing;

const LOG_TARGET: &str = "findamodel::model_data";

/// The full twin search step.
pub const TWIN_SEARCH: &str = "twin_search";

/// One step an update ran, and how it ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRun {
    pub step: &'static str,
    /// `split` (its twins split the model), `no_twin`, or `failed` (the file could not be loaded,
    /// or the step failed).
    pub outcome: &'static str,
    pub ms: f64,
}

/// What a model's data update did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDataUpdate {
    /// The steps that ran (none when nothing was due).
    pub steps: Vec<StepRun>,
    /// The model's split changed, so its geometry and previews should be fetched again.
    pub changed: bool,
}

impl ModelService {
    /// Runs the data update steps due for model `id` ([`crate::services::model_data`]). `geometry`
    /// is the model's loaded geometry when the caller has it; it is handed back (or None when none
    /// was given), so an export that loaded the file does not load it again. Failures are logged
    /// and leave the model as it was.
    pub async fn update_model_data(
        &self,
        id: Uuid,
        geometry: Option<LoadedGeometry>,
    ) -> (ModelDataUpdate, Option<LoadedGeometry>) {
        match self.twin_search_due(id).await {
            Ok(Some(_)) => {}
            Ok(None) => return (ModelDataUpdate::default(), geometry),
            Err(err) => {
                warn!(target: LOG_TARGET, "Could not check the data update of model {id}: {err:#}");
                return (ModelDataUpdate::default(), geometry);
            }
        }
        let shared = geometry.map(Arc::new);
        let given = shared.clone();
        let update = self.data_updates.run_async(id, || self.run_update(id, given)).await;
        let geometry = shared.map(|g| Arc::try_unwrap(g).unwrap_or_else(|g| (*g).clone()));
        ((*update).clone(), geometry)
    }

    /// [`ModelService::update_model_data`] in the background: returns at once.
    pub fn update_model_data_in_background(&self, id: Uuid) {
        let models = self.clone();
        tokio::spawn(async move {
            models.update_model_data(id, None).await;
        });
    }

    /// One run of the steps due, recorded.
    async fn run_update(&self, id: Uuid, geometry: Option<Arc<LoadedGeometry>>) -> ModelDataUpdate {
        let mut update = ModelDataUpdate::default();
        // Checked again: a run that finished just before this one began may have done it.
        let checksum = match self.twin_search_due(id).await {
            Ok(Some(checksum)) => checksum,
            Ok(None) => return update,
            Err(err) => {
                warn!(target: LOG_TARGET, "Could not check the data update of model {id}: {err:#}");
                return update;
            }
        };
        let started = Instant::now();
        let outcome = match self.full_twin_search(id, geometry).await {
            Ok(Some(true)) => "split",
            Ok(Some(false)) => "no_twin",
            Ok(None) => "failed",
            Err(err) => {
                warn!(target: LOG_TARGET, "The full twin search of model {id} failed: {err:#}");
                "failed"
            }
        };
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        update.changed = outcome == "split";
        update.steps.push(StepRun { step: TWIN_SEARCH, outcome, ms });
        let row = processing::Record {
            kind: processing::MODEL_UPDATE,
            method: outcome.to_string(),
            source: "update_data".into(),
            source_checksum: checksum,
            detection_version: None,
            reader_version: None,
            twin_version: Some(CURRENT_TWIN_VERSION),
            repair_version: None,
            duration_ms: Some(ms),
            detail: Some(json!({ "remeshVersion": CURRENT_REMESH_TWIN_VERSION, "steps": update.steps })),
        };
        processing::record_or_warn(&self.db, id, &row).await;
        update
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::geometry::Triangle;
    use crate::geometry::twin::Similarity;
    use crate::geometry::twin::tests::{blob, rot, supported, to_tris};
    use crate::services::processing::{SplitMethod, SplitRecord};
    use crate::services::twins;
    use crate::test_support::{TestDb, model, write_file};

    /// Files are Z up; the loader turns them Y up, with the spikes below the ball.
    fn save(t: &[Triangle]) -> Vec<u8> {
        crate::geometry::saver::save_stl(&crate::geometry::to_z_up(t), None)
    }

    struct Scene {
        t: TestDb,
        root: tempfile::TempDir,
        svc: ModelService,
    }

    impl Scene {
        async fn new() -> Self {
            let t = TestDb::new().await;
            let root = tempfile::tempdir().unwrap();
            let svc = crate::services::models::tests::service(&t, root.path());
            Scene { t, root, svc }
        }

        fn loads(&self) -> usize {
            self.svc.twin_lookup_loads.load(Ordering::Relaxed)
        }

        /// A supported model with welded supports, as the scan leaves one: split `method`, and (when
        /// it waits for a twin) its probe keys stored.
        async fn supported(&self, name: &str, mesh: &[Triangle], method: SplitMethod) -> Uuid {
            write_file(self.root.path(), &format!("Mini/{name}"), save(mesh));
            let mut m = model(name, "Mini");
            m.calculated_supported = Some(true);
            self.t.insert_model(&m).await;
            processing::record_split(&self.t.db, m.id, "scan", &m.checksum, &SplitRecord::new(method, 0.0)).await;
            if matches!(method, SplitMethod::None | SplitMethod::Welded) {
                let (_, probes) = twins::sketch_and_probes(mesh, true);
                twins::store_probes(&self.t.db, m.id, &m.checksum, &probes.unwrap()).await.unwrap();
            }
            m.id
        }

        /// A model without supports, sketched as the scan sketches it.
        async fn bare(&self, name: &str, mesh: &[Triangle]) -> Uuid {
            write_file(self.root.path(), &format!("Bare/{name}"), save(mesh));
            let mut m = model(name, "Bare");
            m.calculated_supported = Some(false);
            self.t.insert_model(&m).await;
            twins::store_sketch(&self.t.db, m.id, &twins::sketch_bytes(mesh)).await.unwrap();
            m.id
        }
    }

    /// A small sculpt (about 1,000 triangles: too few keys for the stored probe keys to find it).
    fn small() -> Vec<[[f64; 3]; 3]> {
        blob(16, 11)
    }

    fn welded_small() -> Vec<Triangle> {
        let x = Similarity { r: rot(0.4, 0.9), scale: 0.5, t: [3.0, 12.0, -2.0] };
        to_tris(&supported(&small(), &x).0)
    }

    #[tokio::test]
    async fn a_small_twin_added_later_is_found_by_the_full_search() {
        let s = Scene::new().await;
        let cancel = tokio_util::sync::CancellationToken::new();
        let id = s.supported("small_supported.stl", &welded_small(), SplitMethod::Welded).await;
        s.bare("other.stl", &to_tris(&blob(80, 8))).await;
        let run = s.svc.resolve_twins(None, None, &cancel).await.unwrap();
        assert_eq!((run.pending, run.from_stored), (1, 1), "{run:?}");

        // Its twin arrives in a later scan. The re-probe from its stored keys cannot reach a part
        // this small, so the post-scan lookup leaves it unsplit without loading it.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let twin_id = s.bare("small.stl", &to_tris(&small())).await;
        let run = s.svc.resolve_twins(None, None, &cancel).await.unwrap();
        assert_eq!((run.pending, run.from_stored, run.split), (1, 1, 0), "{run:?}");
        assert_eq!(s.loads(), 0);

        // The full search loads it once, finds the twin and keeps its split.
        let (update, geometry) = s.svc.update_model_data(id, None).await;
        assert!(geometry.is_none());
        assert_eq!(update.steps.len(), 1, "{update:?}");
        assert_eq!(update.steps[0].outcome, "split", "{update:?}");
        assert!(update.changed);
        assert_eq!(s.loads(), 1);
        let set = twins::twin_set(&s.t.db, id).await.unwrap().expect("split by its twin");
        assert_eq!(set.twins.iter().map(|t| t.id).collect::<Vec<_>>(), [twin_id]);
        let split = processing::get(&s.t.db, id, processing::SUPPORT_SPLIT).await.unwrap().unwrap();
        assert_eq!(split.method, "twin");
        let lookup = processing::get(&s.t.db, id, processing::TWIN_LOOKUP).await.unwrap().unwrap();
        assert_eq!(lookup.method, "twin");
        let row = processing::get(&s.t.db, id, processing::MODEL_UPDATE).await.unwrap().unwrap();
        assert_eq!((row.method.as_str(), row.source.as_str()), ("split", "update_data"));
        let probes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM model_twin_probes WHERE model_id = ?")
            .bind(id)
            .fetch_one(&s.t.db)
            .await
            .unwrap();
        assert_eq!(probes, 0, "its stored probe keys are dropped");

        // Now split by a current twin: not searched again, and nothing loaded.
        let (update, _) = s.svc.update_model_data(id, None).await;
        assert_eq!(update, ModelDataUpdate::default());
        assert_eq!(s.loads(), 1);
    }

    #[tokio::test]
    async fn nothing_is_loaded_when_no_step_is_due() {
        let s = Scene::new().await;
        s.bare("other.stl", &to_tris(&blob(80, 8))).await;
        let welded = welded_small();
        let shells = s.supported("shells.stl", &welded, SplitMethod::Shells).await;
        let bare = s.bare("plain.stl", &to_tris(&blob(20, 3))).await;
        let waiting = s.supported("waiting.stl", &welded, SplitMethod::Welded).await;
        for id in [shells, bare] {
            let (update, _) = s.svc.update_model_data(id, None).await;
            assert_eq!(update, ModelDataUpdate::default());
        }
        assert_eq!(s.loads(), 0, "split by shells, or not supported: no step");

        // A waiting model is searched once; again only when something changes.
        let (update, _) = s.svc.update_model_data(waiting, None).await;
        assert_eq!(update.steps[0].outcome, "no_twin", "{update:?}");
        assert_eq!(s.loads(), 1);
        let (update, _) = s.svc.update_model_data(waiting, None).await;
        assert_eq!(update, ModelDataUpdate::default());
        assert_eq!(s.loads(), 1, "searched since its last search's candidates");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        s.bare("another.stl", &to_tris(&blob(40, 9))).await;
        let (update, _) = s.svc.update_model_data(waiting, None).await;
        assert_eq!(update.steps.len(), 1, "a new candidate: searched again");
        assert_eq!(s.loads(), 2);
    }

    #[tokio::test]
    async fn the_geometry_given_is_used_and_handed_back() {
        let s = Scene::new().await;
        s.bare("small.stl", &to_tris(&small())).await;
        let welded = welded_small();
        let id = s.supported("small_supported.stl", &welded, SplitMethod::Welded).await;
        let geometry = crate::geometry::loader::load_model(&s.root.path().join("Mini/small_supported.stl"), "stl");
        let n = geometry.as_ref().unwrap().triangles.len();
        let (update, back) = s.svc.update_model_data(id, geometry).await;
        let lookup = processing::get(&s.t.db, id, processing::TWIN_LOOKUP).await.unwrap();
        assert_eq!(update.steps[0].outcome, "split", "{update:?} {lookup:?}");
        assert_eq!(back.map(|g| g.triangles.len()), Some(n));
        assert_eq!(s.loads(), 0, "the file was not loaded again");
    }

    #[tokio::test]
    async fn concurrent_updates_share_one_run() {
        let s = Scene::new().await;
        s.bare("other.stl", &to_tris(&blob(80, 8))).await;
        let id = s.supported("waiting.stl", &welded_small(), SplitMethod::Welded).await;
        let (a, b) = tokio::join!(s.svc.update_model_data(id, None), s.svc.update_model_data(id, None));
        assert_eq!(a.0, b.0, "both get the one run's result");
        assert_eq!(a.0.steps.len(), 1);
        assert_eq!((s.svc.data_updates.runs(), s.svc.data_updates.shared()), (1, 1));
        assert_eq!(s.loads(), 1);
    }
}
