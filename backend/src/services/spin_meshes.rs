//! Spin meshes on request: a model whose spin mesh the scan hasn't written yet gets it built when
//! the gallery first asks for it, the same way the scan builds it ([`spin::store_for_model`]).
//!
//! Builds of one file run once at a time (concurrent hovers share one), at most
//! [`MAX_CONCURRENT_BUILDS`] run at once (sweeping the mouse over a gallery queues them instead of
//! loading dozens of large models together), and a model that could not be built is not tried
//! again for [`FAILURE_MEMO`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;
use tracing::{info, warn};
use uuid::Uuid;

use super::models::ModelService;
use crate::entities::ModelRow;
use crate::error::AppResult;
use crate::geometry::hull::DEFAULT_RAFT_HEIGHT_MM;
use crate::geometry::loader;
use crate::preview::spin::{self, CURRENT_SPIN_MESH_VERSION};
use crate::services::store;
use crate::single_flight::SingleFlight;

const LOG_TARGET: &str = "findamodel.SpinMeshes";
/// On-demand builds running at once; more wait for a slot.
pub const MAX_CONCURRENT_BUILDS: usize = 3;
/// How long a model whose spin mesh could not be built is answered "not found" without trying.
pub const FAILURE_MEMO: Duration = Duration::from_secs(60);

/// Shared state of on-demand builds.
pub struct SpinBuilds {
    /// By the file being built; the result is its relative path, None when it could not be built.
    flight: SingleFlight<String, Option<String>>,
    slots: Semaphore,
    /// Files that could not be built, and when.
    failed: parking_lot::Mutex<HashMap<String, Instant>>,
    /// Builds attempted (loads of a model), for tests.
    attempts: AtomicUsize,
}

impl Default for SpinBuilds {
    fn default() -> Self {
        SpinBuilds {
            flight: SingleFlight::default(),
            slots: Semaphore::new(MAX_CONCURRENT_BUILDS),
            failed: parking_lot::Mutex::new(HashMap::new()),
            attempts: AtomicUsize::new(0),
        }
    }
}

impl SpinBuilds {
    fn recently_failed(&self, relative: &str) -> bool {
        let mut failed = self.failed.lock();
        failed.retain(|_, at| at.elapsed() < FAILURE_MEMO);
        failed.contains_key(relative)
    }
}

impl ModelService {
    /// How many on-demand spin mesh builds have loaded a model so far.
    pub fn spin_build_attempts(&self) -> usize {
        self.spin_builds.attempts.load(Ordering::Relaxed)
    }

    /// The model's current spin mesh file (absolute), built now when it is missing. None when the
    /// model is unknown, has no previews (so no spin mesh), previews are turned off, or the mesh
    /// could not be built (recently).
    pub async fn spin_mesh_file(&self, id: Uuid) -> AppResult<Option<PathBuf>> {
        let Some(model) = self.get_model(id).await? else { return Ok(None) };
        let Some(expected) = store::expected_spin_path(&model) else { return Ok(None) };
        let Some(renders) = self.preview.as_ref().and_then(|p| p.cache_dir()).map(|d| d.to_path_buf()) else {
            return Ok(None);
        };
        let file = renders.join(&expected);
        if file.is_file() {
            self.index_spin_mesh(&model, &expected).await;
            return Ok(Some(file));
        }
        if !self.app_config.get().await?.generate_previews_enabled || self.spin_builds.recently_failed(&expected) {
            return Ok(None);
        }
        // Spawned, so the build finishes (and is stored) when the request goes away.
        let svc = self.clone();
        let built = tokio::spawn(async move {
            let builds = svc.spin_builds.clone();
            let key = expected.clone();
            builds.flight.run_async(key, || svc.build_spin_mesh(model, expected, renders)).await
        })
        .await
        .map_err(anyhow::Error::from)?;
        Ok(built.as_ref().as_ref().map(|_| file))
    }

    /// Builds and indexes one spin mesh (in a build slot). None when it could not be built.
    async fn build_spin_mesh(&self, model: ModelRow, relative: String, renders: PathBuf) -> Option<String> {
        let _slot = self.spin_builds.slots.acquire().await.ok()?;
        // Built by a call that finished while this one waited.
        if !renders.join(&relative).is_file() {
            self.spin_builds.attempts.fetch_add(1, Ordering::Relaxed);
            let started = Instant::now();
            let path = self.full_path(&model);
            let (file_type, raft_height_mm, supported) = (
                model.file_type.clone(),
                model.hull_raft_height_mm.unwrap_or(DEFAULT_RAFT_HEIGHT_MM),
                model.calculated_supported,
            );
            let (dir, target) = (renders.clone(), relative.clone());
            let stored = tokio::task::spawn_blocking(move || {
                let geometry = loader::load_model(&path?, &file_type)?;
                let mask = spin::support_mask(&geometry.triangles, raft_height_mm, supported);
                spin::store_for_model(&dir, &target, &geometry.triangles, mask.as_deref())
            })
            .await
            .ok()
            .flatten();
            if stored.is_none() {
                warn!(target: LOG_TARGET, "Could not build the spin mesh of {}/{}", model.directory, model.file_name);
                self.spin_builds.failed.lock().insert(relative, Instant::now());
                return None;
            }
            info!(
                target: LOG_TARGET,
                "Built the spin mesh of {}/{} on request in {:.0} ms",
                model.directory,
                model.file_name,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
        self.index_spin_mesh(&model, &relative).await;
        Some(relative)
    }

    /// Points the model's row at its spin mesh file, when it doesn't already, and deletes the one
    /// it named before if nothing else uses it.
    async fn index_spin_mesh(&self, model: &ModelRow, relative: &str) {
        if model.spin_mesh_path.as_deref() == Some(relative)
            && model.spin_mesh_version == Some(CURRENT_SPIN_MESH_VERSION)
        {
            return;
        }
        let updated = sqlx::query("UPDATE models SET spin_mesh_path = ?, spin_mesh_version = ? WHERE id = ?")
            .bind(relative)
            .bind(CURRENT_SPIN_MESH_VERSION)
            .bind(model.id)
            .execute(&self.db)
            .await;
        match updated {
            Ok(_) => self.release_old_spin(model.spin_mesh_path.as_deref(), Some(relative)).await,
            Err(err) => warn!(target: LOG_TARGET, "Failed to index spin mesh {relative}: {err}"),
        }
    }
}

/// Shared on-demand build state for a new [`ModelService`].
pub fn new_builds() -> Arc<SpinBuilds> {
    Arc::new(SpinBuilds::default())
}
