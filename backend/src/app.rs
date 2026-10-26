//! Application wiring: shared state, startup sequence and the HTTP server.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::config::{RuntimeMode, Settings};
use crate::db::{self, Db};
use crate::logging::LogBuffer;
use crate::preview::{GpuState, PreviewService};
use crate::routes;
use crate::services::app_config::{AppConfigService, ModelsRoot, resolve_models_directory_path};
use crate::services::indexer::{IndexerService, ScanWorker, run_scheduler};
use crate::services::jobs::{ArchiveJobService, PlateJobService};
use crate::services::metadata_config::MetadataConfigService;
use crate::services::models::ModelService;
use crate::services::voxl_cache::VoxlCache;
use crate::services::{printing_lists, users};

const LOG_TARGET: &str = "findamodel.Startup";

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub db: Db,
    pub models_root: ModelsRoot,
    pub app_config: AppConfigService,
    pub metadata_config: MetadataConfigService,
    pub models: ModelService,
    pub preview: PreviewService,
    /// Generated model VOXLs (`/api/models/{id}/voxl`).
    pub voxl_cache: VoxlCache,
    pub indexer: Arc<IndexerService>,
    pub plate_jobs: PlateJobService,
    pub archive_jobs: ArchiveJobService,
    pub log_buffer: Arc<LogBuffer>,
}

impl AppState {
    pub fn models_root(&self) -> Option<PathBuf> {
        self.models_root.read().clone()
    }

    /// Builds services, migrates the database and runs startup tasks (admin seeding,
    /// interrupted run recovery, default printing list).
    pub async fn initialize(settings: Settings, log_buffer: Arc<LogBuffer>) -> Result<Self> {
        std::fs::create_dir_all(&settings.data_path)
            .with_context(|| format!("creating {}", settings.data_path.display()))?;
        let renders = settings.renders_path();
        std::fs::create_dir_all(&renders).with_context(|| format!("creating {}", renders.display()))?;

        let db_path = settings.database_path();
        info!(target: LOG_TARGET, "Startup DB preflight beginning for {}", db_path.display());
        db::ensure_unlocked(&db_path).await?;
        let db = db::open(&db_path).await?;

        let models_root: ModelsRoot = Arc::new(RwLock::new(None));
        let app_config = AppConfigService::new(
            db.clone(),
            settings.app_config_defaults.clone(),
            settings.models_path.clone(),
            models_root.clone(),
        );

        // The environment wins over the setup wizard's stored path.
        let configured = match &settings.models_path {
            Some(p) => Some(
                resolve_models_directory_path(p, false, &|p| p.is_dir()).map_err(|e| anyhow::anyhow!(e.to_string()))?,
            ),
            None => {
                let stored = app_config.get().await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
                stored.setup_completed.then_some(stored.models_directory_path).flatten().map(PathBuf::from)
            }
        };
        *models_root.write() = configured;

        let gpu = Arc::new(GpuState::default());
        if settings.preview_use_gpu {
            gpu.initialize();
        }
        let preview = PreviewService::new(Some(renders), settings.preview_use_gpu, gpu);

        let metadata_config =
            MetadataConfigService::new(db.clone(), models_root.clone(), settings.directory_batch_size);
        let models = ModelService::new(
            db.clone(),
            models_root.clone(),
            Some(preview.clone()),
            metadata_config.clone(),
            app_config.clone(),
            settings.file_scan_threads,
        );
        let indexer = IndexerService::new(
            db.clone(),
            Arc::new(ScanWorker { metadata_config: metadata_config.clone(), models: models.clone() }),
        );

        users::seed_admin_user(&db).await?;
        indexer.requeue_interrupted_runs().await?;
        if let Some(admin) = users::get_admin_user(&db).await? {
            printing_lists::ensure_default_list(&db, admin.id).await?;
        }

        let voxl_cache = VoxlCache::new(settings.voxl_cache_path(), settings.voxl_cache_max_bytes);

        Ok(AppState {
            settings: Arc::new(settings),
            plate_jobs: PlateJobService::new(models.clone()),
            archive_jobs: ArchiveJobService::new(db.clone(), models_root.clone()),
            db,
            models_root,
            app_config,
            metadata_config,
            models,
            preview,
            voxl_cache,
            indexer,
            log_buffer,
        })
    }
}

/// A running server: its bound address and a handle to stop it.
pub struct ServerHandle {
    pub address: SocketAddr,
    pub shutdown: CancellationToken,
    pub task: tokio::task::JoinHandle<Result<()>>,
}

/// Starts the server and background scheduler. Resolves once the listener is bound.
pub async fn start(settings: Settings, log_buffer: Arc<LogBuffer>) -> Result<ServerHandle> {
    let listen = settings.listen;
    let mode = settings.mode;
    let state = AppState::initialize(settings, log_buffer).await?;

    let shutdown = CancellationToken::new();
    tokio::spawn(run_scheduler(
        state.indexer.clone(),
        state.settings.index_schedule.clone(),
        state.settings.index_on_startup,
        shutdown.clone(),
    ));

    let listener = TcpListener::bind(listen).await.with_context(|| format!("binding {listen}"))?;
    let address = listener.local_addr()?;
    info!(target: LOG_TARGET, "Listening on http://{address} ({})", if mode == RuntimeMode::Desktop { "desktop" } else { "server" });

    let router = routes::router(state);
    let token = shutdown.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).with_graceful_shutdown(async move { token.cancelled().await }).await?;
        Ok(())
    });
    Ok(ServerHandle { address, shutdown, task })
}

/// Runs the server until `stop` completes.
pub async fn run(settings: Settings, log_buffer: Arc<LogBuffer>, stop: impl Future<Output = ()>) -> Result<()> {
    let handle = start(settings, log_buffer).await?;
    stop.await;
    handle.shutdown.cancel();
    handle.task.await?
}
