//! Prioritised queue of indexing requests, processed one at a time on a background task.
//! Scanning logic stays in the model and metadata services; this module orchestrates
//! dispatch, cancellation and persisted run history.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::{Db, now};
use crate::dto::{
    CompletedIndexRequestDto, IndexRequestDto, IndexRunDetailDto, IndexRunEventDto, IndexRunFileDto,
    IndexRunSummaryDto, IndexerStatusDto, PagedResultDto, index_flags,
};
use crate::entities::{IndexRun, IndexRunEvent, IndexRunFile};
use crate::error::AppResult;
use crate::services::metadata_config::MetadataConfigService;
use crate::services::models::ModelService;
use crate::services::scan::{Cancelled, FilePlan, FileResult, ProgressReporter};

const LOG_TARGET: &str = "findamodel.Indexing";
const RECENT_LIMIT: usize = 15;
const HISTORY_RETENTION_DAYS: i64 = 7;
const CANCELLED_LIVE_STATUS_VISIBILITY: Duration = Duration::from_secs(2);

/// Performs the actual indexing work for a request.
#[async_trait]
pub trait IndexWorker: Send + Sync {
    async fn sync_directories(&self) -> anyhow::Result<()>;
    async fn scan_models(
        &self,
        directory_filter: Option<&str>,
        reporter: &dyn ProgressReporter,
        cancel: &CancellationToken,
    ) -> anyhow::Result<()>;
    async fn scan_single(
        &self,
        relative_model_path: &str,
        reporter: &dyn ProgressReporter,
        cancel: &CancellationToken,
    ) -> anyhow::Result<()>;
}

/// Extension point invoked before a run starts (used by tests to hold the queue).
#[async_trait]
pub trait IndexerHooks: Send + Sync {
    async fn before_run_starts(&self) {}
}

struct NoHooks;
impl IndexerHooks for NoHooks {}

pub struct ScanWorker {
    pub metadata_config: MetadataConfigService,
    pub models: ModelService,
}

#[async_trait]
impl IndexWorker for ScanWorker {
    async fn sync_directories(&self) -> anyhow::Result<()> {
        self.metadata_config.sync_directory_configs().await
    }

    async fn scan_models(
        &self,
        directory_filter: Option<&str>,
        reporter: &dyn ProgressReporter,
        cancel: &CancellationToken,
    ) -> anyhow::Result<()> {
        self.models.scan_and_cache(None, directory_filter, Some(reporter), cancel).await.map(|_| ())
    }

    async fn scan_single(
        &self,
        relative_model_path: &str,
        reporter: &dyn ProgressReporter,
        cancel: &CancellationToken,
    ) -> anyhow::Result<()> {
        self.models.scan_and_cache_single(relative_model_path, Some(reporter), cancel).await.map(|_| ())
    }
}

#[derive(Debug, Clone)]
struct Entry {
    id: Uuid,
    run_id: Uuid,
    directory_filter: Option<String>,
    relative_model_path: Option<String>,
    flags: i32,
    requested_at: DateTime<Utc>,
    total_files: Option<i32>,
    processed_files: i32,
    cancellation_requested: bool,
}

impl Entry {
    fn to_dto(&self, status: &str) -> IndexRequestDto {
        IndexRequestDto {
            id: self.id,
            run_id: Some(self.run_id),
            directory_filter: self.directory_filter.clone(),
            relative_model_path: self.relative_model_path.clone(),
            flags: self.flags,
            requested_at: self.requested_at,
            total_files: self.total_files,
            processed_files: self.processed_files,
            status: status.to_string(),
            is_cancellation_requested: self.cancellation_requested,
        }
    }

    fn matches(&self, id: Uuid) -> bool {
        self.id == id || self.run_id == id
    }
}

#[derive(Default)]
struct QueueState {
    queue: VecDeque<Entry>,
    recent: VecDeque<CompletedIndexRequestDto>,
    current: Option<Entry>,
    last_visible_current: Option<(Entry, Instant)>,
    current_cancel: Option<CancellationToken>,
    processing: bool,
}

pub struct IndexerService {
    db: Db,
    worker: Arc<dyn IndexWorker>,
    hooks: Arc<dyn IndexerHooks>,
    state: Mutex<QueueState>,
}

fn none_if_empty(s: Option<&str>) -> Option<String> {
    s.filter(|v| !v.is_empty()).map(str::to_string)
}

fn summarize_error(err: &anyhow::Error) -> String {
    let summary = err.to_string();
    let summary = if summary.trim().is_empty() { "Unknown error".to_string() } else { summary };
    summary.chars().take(180).collect()
}

fn to_summary(run: &IndexRun) -> IndexRunSummaryDto {
    IndexRunSummaryDto {
        id: run.id,
        directory_filter: run.directory_filter.clone(),
        relative_model_path: run.relative_model_path.clone(),
        flags: run.flags,
        requested_at: run.requested_at,
        started_at: run.started_at,
        completed_at: run.completed_at,
        total_files: run.total_files,
        processed_files: run.processed_files,
        status: run.status.clone(),
        outcome: run.outcome.clone(),
        error: run.error.clone(),
        is_cancellation_requested: false,
    }
}

impl IndexerService {
    pub fn new(db: Db, worker: Arc<dyn IndexWorker>) -> Arc<Self> {
        Self::with_hooks(db, worker, Arc::new(NoHooks))
    }

    pub fn with_hooks(db: Db, worker: Arc<dyn IndexWorker>, hooks: Arc<dyn IndexerHooks>) -> Arc<Self> {
        Arc::new(Self { db, worker, hooks, state: Mutex::new(QueueState::default()) })
    }

    /// Adds a request. A queued request for the same target has its flags merged and is
    /// moved to the front instead.
    pub fn enqueue(
        self: &Arc<Self>,
        directory_filter: Option<&str>,
        relative_model_path: Option<&str>,
        flags: i32,
    ) -> IndexRequestDto {
        self.enqueue_with_run(directory_filter, relative_model_path, flags, None)
    }

    fn enqueue_with_run(
        self: &Arc<Self>,
        directory_filter: Option<&str>,
        relative_model_path: Option<&str>,
        flags: i32,
        existing_run_id: Option<Uuid>,
    ) -> IndexRequestDto {
        let filter = none_if_empty(directory_filter);
        let model_path = none_if_empty(relative_model_path);
        let mut state = self.state.lock();

        if let Some(pos) =
            state.queue.iter().position(|e| e.directory_filter == filter && e.relative_model_path == model_path)
        {
            let mut existing = state.queue.remove(pos).expect("queued entry");
            existing.flags |= flags;
            let dto = existing.to_dto("queued");
            state.queue.push_front(existing);
            return dto;
        }

        let entry = Entry {
            id: Uuid::new_v4(),
            run_id: existing_run_id.unwrap_or_else(Uuid::new_v4),
            directory_filter: filter,
            relative_model_path: model_path,
            flags,
            requested_at: now(),
            total_files: None,
            processed_files: 0,
            cancellation_requested: false,
        };
        let dto = entry.to_dto("queued");
        state.queue.push_back(entry);

        if !state.processing {
            state.processing = true;
            let this = self.clone();
            tokio::spawn(async move { this.process_queue().await });
        }
        dto
    }

    /// Called at startup: runs left "running" by a crash or restart are reset to "queued"
    /// and re-enqueued in their original order with the same run ids.
    pub async fn requeue_interrupted_runs(self: &Arc<Self>) -> anyhow::Result<()> {
        let interrupted: Vec<IndexRun> =
            sqlx::query_as("SELECT * FROM index_runs WHERE status = 'running' ORDER BY requested_at")
                .fetch_all(&self.db)
                .await?;
        if interrupted.is_empty() {
            return Ok(());
        }

        let mut tx = self.db.begin().await?;
        for run in &interrupted {
            sqlx::query("DELETE FROM index_run_files WHERE index_run_id = ?").bind(run.id).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM index_run_events WHERE index_run_id = ?").bind(run.id).execute(&mut *tx).await?;
            sqlx::query(
                "UPDATE index_runs SET status = 'queued', outcome = NULL, error = NULL, started_at = NULL, completed_at = NULL,
                    processed_files = 0, total_files = NULL WHERE id = ?",
            )
            .bind(run.id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        for run in interrupted {
            info!(
                target: LOG_TARGET,
                "Re-queueing interrupted index run {} (flags {}, filter: {})",
                run.id,
                run.flags,
                run.directory_filter.as_deref().or(run.relative_model_path.as_deref()).unwrap_or("(all)")
            );
            self.enqueue_with_run(
                run.directory_filter.as_deref(),
                run.relative_model_path.as_deref(),
                run.flags,
                Some(run.id),
            );
        }
        Ok(())
    }

    pub fn get_status(&self) -> IndexerStatusDto {
        let state = self.state.lock();
        let visible = state.current.clone().or_else(|| {
            state
                .last_visible_current
                .as_ref()
                .filter(|(e, at)| e.cancellation_requested && at.elapsed() <= CANCELLED_LIVE_STATUS_VISIBILITY)
                .map(|(e, _)| e.clone())
        });
        IndexerStatusDto {
            is_running: visible.is_some(),
            current_request: visible.map(|e| e.to_dto("running")),
            queue: state.queue.iter().map(|e| e.to_dto("queued")).collect(),
            recent: state.recent.iter().cloned().collect(),
        }
    }

    /// Cancels a queued or running request by request id or run id.
    pub async fn cancel(&self, id: Uuid) -> anyhow::Result<bool> {
        let queued = {
            let mut state = self.state.lock();
            if let Some(pos) = state.queue.iter().position(|e| e.matches(id)) {
                state.queue.remove(pos)
            } else if state.current.as_ref().is_some_and(|c| c.matches(id)) {
                let current = state.current.as_mut().expect("current");
                current.cancellation_requested = true;
                let snapshot = current.clone();
                state.last_visible_current = Some((snapshot, Instant::now()));
                if let Some(token) = &state.current_cancel {
                    token.cancel();
                }
                return Ok(true);
            } else {
                return Ok(false);
            }
        };
        if let Some(entry) = queued {
            self.upsert_cancelled_queued_run(&entry).await?;
        }
        Ok(true)
    }

    async fn process_queue(self: Arc<Self>) {
        loop {
            let (entry, token) = {
                let mut state = self.state.lock();
                let Some(entry) = state.queue.pop_front() else {
                    state.current = None;
                    state.current_cancel = None;
                    state.processing = false;
                    return;
                };
                let token = CancellationToken::new();
                state.current = Some(entry.clone());
                state.last_visible_current = Some((entry.clone(), Instant::now()));
                state.current_cancel = Some(token.clone());
                (entry, token)
            };

            let completed = self.process_request(entry.clone(), token).await;

            let mut state = self.state.lock();
            let cancelled = state.current.as_ref().is_some_and(|c| c.cancellation_requested);
            if cancelled {
                if let Some(current) = state.current.clone() {
                    state.last_visible_current = Some((current, Instant::now()));
                }
            } else if state.last_visible_current.as_ref().is_some_and(|(e, _)| e.id == entry.id) {
                state.last_visible_current = None;
            }
            state.current = None;
            state.current_cancel = None;
            state.recent.push_front(completed);
            state.recent.truncate(RECENT_LIMIT);
        }
    }

    fn set_progress(&self, entry_id: Uuid, total: Option<i32>, processed: i32) {
        let mut state = self.state.lock();
        if let Some(current) = state.current.as_mut().filter(|c| c.id == entry_id) {
            current.total_files = total;
            current.processed_files = processed;
        }
    }

    async fn process_request(self: &Arc<Self>, entry: Entry, token: CancellationToken) -> CompletedIndexRequestDto {
        let label = match (&entry.relative_model_path, &entry.directory_filter) {
            (Some(p), _) => format!("model:{p}"),
            (None, Some(f)) => f.clone(),
            (None, None) => "(all)".to_string(),
        };
        let started_at = now();
        let reporter = RunProgressReporter::new(self.clone(), entry.id, entry.run_id);
        let mut run_created = false;

        let result: anyhow::Result<()> = async {
            self.hooks.before_run_starts().await;
            self.create_run(&entry, started_at).await?;
            run_created = true;
            self.cleanup_old_runs().await?;

            info!(target: LOG_TARGET, "Indexer: starting flags {} for {label}", entry.flags);
            reporter.on_log("info", &format!("Starting run for {label}"), None).await;
            if token.is_cancelled() {
                return Err(Cancelled.into());
            }

            if entry.flags & index_flags::DIRECTORIES != 0 {
                reporter.on_log("info", "Syncing directory configs", None).await;
                self.worker.sync_directories().await?;
                if token.is_cancelled() {
                    return Err(Cancelled.into());
                }
            }

            if entry.flags & index_flags::MODELS != 0 {
                reporter.on_log("info", "Scanning model files", None).await;
                match &entry.relative_model_path {
                    Some(p) => self.worker.scan_single(p, &reporter, &token).await?,
                    None => self.worker.scan_models(entry.directory_filter.as_deref(), &reporter, &token).await?,
                }
            }
            if token.is_cancelled() {
                return Err(Cancelled.into());
            }

            info!(target: LOG_TARGET, "Indexer: completed flags {} for {label}", entry.flags);
            reporter.on_log("info", "Run completed successfully", None).await;
            Ok(())
        }
        .await;

        let (outcome, error_message) = match &result {
            Ok(()) => ("success", None),
            Err(err) if err.is::<Cancelled>() || token.is_cancelled() => {
                info!(target: LOG_TARGET, "Indexer: cancelled flags {} for {label}", entry.flags);
                if run_created {
                    reporter.on_log("info", "Run cancelled by user", None).await;
                }
                ("cancelled", Some("Cancelled by user.".to_string()))
            }
            Err(err) => {
                let summary = summarize_error(err);
                error!(target: LOG_TARGET, "Indexer: failed processing flags {} for {label}: {err:#}", entry.flags);
                if run_created {
                    reporter.on_log("error", &format!("Run failed: {summary}"), None).await;
                }
                ("failed", Some(summary))
            }
        };

        let completed_at = now();
        let duration_ms = ((completed_at - started_at).num_microseconds().unwrap_or(0) as f64 / 1000.0).max(0.0);
        let persisted = if !run_created && outcome == "cancelled" {
            self.upsert_cancelled_queued_run(&entry).await
        } else {
            self.complete_run(entry.run_id, completed_at, duration_ms, outcome, error_message.as_deref()).await
        };
        if let Err(err) = persisted {
            error!(target: LOG_TARGET, "Failed to persist index run {}: {err:#}", entry.run_id);
        }

        CompletedIndexRequestDto {
            id: entry.id,
            run_id: Some(entry.run_id),
            directory_filter: entry.directory_filter,
            relative_model_path: entry.relative_model_path,
            flags: entry.flags,
            requested_at: entry.requested_at,
            started_at,
            completed_at,
            duration_ms,
            outcome: outcome.to_string(),
            error: error_message,
        }
    }

    async fn upsert_cancelled_queued_run(&self, entry: &Entry) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO index_runs (id, directory_filter, relative_model_path, flags, requested_at, completed_at, duration_ms,
                status, outcome, error, processed_files)
             VALUES (?, ?, ?, ?, ?, ?, 0, 'cancelled', 'cancelled', 'Cancelled before start.', 0)
             ON CONFLICT(id) DO UPDATE SET completed_at = excluded.completed_at, duration_ms = 0, status = 'cancelled',
                outcome = 'cancelled', error = 'Cancelled before start.'",
        )
        .bind(entry.run_id)
        .bind(&entry.directory_filter)
        .bind(&entry.relative_model_path)
        .bind(entry.flags)
        .bind(entry.requested_at)
        .bind(now())
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn create_run(&self, entry: &Entry, started_at: DateTime<Utc>) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO index_runs (id, directory_filter, relative_model_path, flags, requested_at, started_at, status, processed_files)
             VALUES (?, ?, ?, ?, ?, ?, 'running', 0)
             ON CONFLICT(id) DO UPDATE SET started_at = excluded.started_at, status = 'running', processed_files = 0,
                total_files = NULL, completed_at = NULL, outcome = NULL, error = NULL",
        )
        .bind(entry.run_id)
        .bind(&entry.directory_filter)
        .bind(&entry.relative_model_path)
        .bind(entry.flags)
        .bind(entry.requested_at)
        .bind(started_at)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn complete_run(
        &self,
        run_id: Uuid,
        completed_at: DateTime<Utc>,
        duration_ms: f64,
        outcome: &str,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE index_runs SET completed_at = ?, duration_ms = ?, outcome = ?, error = ?, status = ? WHERE id = ?",
        )
        .bind(completed_at)
        .bind(duration_ms)
        .bind(outcome)
        .bind(error)
        .bind(outcome)
        .bind(run_id)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn cleanup_old_runs(&self) -> anyhow::Result<()> {
        let cutoff = now() - chrono::Duration::days(HISTORY_RETENTION_DAYS);
        // Files and events cascade with their run.
        sqlx::query("DELETE FROM index_runs WHERE requested_at < ?").bind(cutoff).execute(&self.db).await?;
        Ok(())
    }

    pub async fn get_history(&self, days: i64, limit: i64) -> AppResult<Vec<IndexRunSummaryDto>> {
        let cutoff = now() - chrono::Duration::days(days.max(1));
        let runs: Vec<IndexRun> =
            sqlx::query_as("SELECT * FROM index_runs WHERE requested_at >= ? ORDER BY requested_at DESC LIMIT ?")
                .bind(cutoff)
                .bind(limit.clamp(1, 1000))
                .fetch_all(&self.db)
                .await?;
        Ok(runs.iter().map(to_summary).collect())
    }

    pub async fn get_run_detail(
        &self,
        run_id: Uuid,
        files_page: i64,
        files_page_size: i64,
        files_view: &str,
        events_page: i64,
        events_page_size: i64,
    ) -> AppResult<Option<IndexRunDetailDto>> {
        let files_page = files_page.max(1);
        let files_page_size = files_page_size.clamp(25, 1000);
        let events_page = events_page.max(1);
        let events_page_size = events_page_size.clamp(25, 1000);

        let Some(run) = sqlx::query_as::<_, IndexRun>("SELECT * FROM index_runs WHERE id = ?")
            .bind(run_id)
            .fetch_optional(&self.db)
            .await?
        else {
            return Ok(None);
        };

        let view = files_view.trim().to_ascii_lowercase();
        let (filter, order) = match view.as_str() {
            "pending" => ("AND status = 'pending'", "ORDER BY relative_path"),
            "processed" => ("AND status != 'pending'", "ORDER BY processed_at DESC, relative_path"),
            _ => ("", "ORDER BY (status = 'pending'), processed_at DESC, relative_path"),
        };
        let files_total: i64 =
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM index_run_files WHERE index_run_id = ? {filter}"))
                .bind(run_id)
                .fetch_one(&self.db)
                .await?;
        let files: Vec<IndexRunFile> = sqlx::query_as(&format!(
            "SELECT * FROM index_run_files WHERE index_run_id = ? {filter} {order} LIMIT ? OFFSET ?"
        ))
        .bind(run_id)
        .bind(files_page_size)
        .bind((files_page - 1) * files_page_size)
        .fetch_all(&self.db)
        .await?;

        let events_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM index_run_events WHERE index_run_id = ?")
            .bind(run_id)
            .fetch_one(&self.db)
            .await?;
        let events: Vec<IndexRunEvent> = sqlx::query_as(
            "SELECT * FROM index_run_events WHERE index_run_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(run_id)
        .bind(events_page_size)
        .bind((events_page - 1) * events_page_size)
        .fetch_all(&self.db)
        .await?;

        Ok(Some(IndexRunDetailDto {
            run: to_summary(&run),
            files: PagedResultDto {
                items: files
                    .into_iter()
                    .map(|f| IndexRunFileDto {
                        relative_path: f.relative_path,
                        file_type: f.file_type,
                        status: f.status,
                        is_new: f.is_new,
                        was_updated: f.was_updated,
                        generated_preview: f.generated_preview,
                        generated_hull: f.generated_hull,
                        message: f.message,
                        duration_ms: f.duration_ms,
                        processed_at: f.processed_at,
                    })
                    .collect(),
                page: files_page,
                page_size: files_page_size,
                total_count: files_total,
            },
            events: PagedResultDto {
                items: events
                    .into_iter()
                    .map(|e| IndexRunEventDto {
                        created_at: e.created_at,
                        level: e.level,
                        message: e.message,
                        relative_path: e.relative_path,
                    })
                    .collect(),
                page: events_page,
                page_size: events_page_size,
                total_count: events_total,
            },
        }))
    }
}

fn normalize_relative_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_string()
}

/// Persists run progress (files and events) and mirrors counts into the live status.
struct RunProgressReporter {
    indexer: Arc<IndexerService>,
    entry_id: Uuid,
    run_id: Uuid,
    progress: Mutex<(Option<i32>, i32)>,
    completed_paths: Mutex<HashSet<String>>,
}

impl RunProgressReporter {
    fn new(indexer: Arc<IndexerService>, entry_id: Uuid, run_id: Uuid) -> Self {
        Self { indexer, entry_id, run_id, progress: Mutex::new((None, 0)), completed_paths: Mutex::new(HashSet::new()) }
    }

    fn db(&self) -> &Db {
        &self.indexer.db
    }
}

#[async_trait]
impl ProgressReporter for RunProgressReporter {
    async fn on_scan_started(&self, total_files: usize) {
        let total = total_files as i32;
        *self.progress.lock() = (Some(total), 0);
        self.indexer.set_progress(self.entry_id, Some(total), 0);
        if let Err(err) = sqlx::query("UPDATE index_runs SET total_files = ?, processed_files = 0 WHERE id = ?")
            .bind(total)
            .bind(self.run_id)
            .execute(self.db())
            .await
        {
            error!(target: LOG_TARGET, "Failed to record scan start: {err}");
        }
    }

    async fn on_files_discovered(&self, files: &[FilePlan]) {
        if files.is_empty() {
            return;
        }
        let result: sqlx::Result<()> = async {
            let mut tx = self.db().begin().await?;
            for f in files {
                sqlx::query(
                    "INSERT OR IGNORE INTO index_run_files (id, index_run_id, relative_path, file_type, status) VALUES (?, ?, ?, ?, 'pending')",
                )
                .bind(Uuid::new_v4())
                .bind(self.run_id)
                .bind(normalize_relative_path(&f.relative_path))
                .bind(&f.file_type)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await
        }
        .await;
        if let Err(err) = result {
            error!(target: LOG_TARGET, "Failed to record discovered files: {err}");
        }
    }

    async fn on_file_processed(&self, r: FileResult) {
        let path = normalize_relative_path(&r.relative_path);
        let (total, processed) = {
            let first = self.completed_paths.lock().insert(path.clone());
            let mut progress = self.progress.lock();
            if first {
                progress.1 += 1;
            }
            *progress
        };
        self.indexer.set_progress(self.entry_id, total, processed);

        let result: sqlx::Result<()> = async {
            let mut tx = self.db().begin().await?;
            sqlx::query(
                "INSERT INTO index_run_files (id, index_run_id, relative_path, file_type, status, is_new, was_updated,
                    generated_preview, generated_hull, message, duration_ms, processed_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(index_run_id, relative_path) DO UPDATE SET status = excluded.status, is_new = excluded.is_new,
                    was_updated = excluded.was_updated, generated_preview = excluded.generated_preview,
                    generated_hull = excluded.generated_hull, message = excluded.message, duration_ms = excluded.duration_ms,
                    processed_at = excluded.processed_at",
            )
            .bind(Uuid::new_v4())
            .bind(self.run_id)
            .bind(&path)
            .bind(&r.file_type)
            .bind(&r.status)
            .bind(r.is_new)
            .bind(r.was_updated)
            .bind(r.generated_preview)
            .bind(r.generated_hull)
            .bind(&r.message)
            .bind(r.duration_ms)
            .bind(now())
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE index_runs SET total_files = ?, processed_files = ? WHERE id = ?")
                .bind(total)
                .bind(processed)
                .bind(self.run_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await
        }
        .await;
        if let Err(err) = result {
            error!(target: LOG_TARGET, "Failed to record processed file {path}: {err}");
        }
    }

    async fn on_log(&self, level: &str, message: &str, relative_path: Option<&str>) {
        let level = if level.trim().is_empty() { "info" } else { level };
        if let Err(err) = sqlx::query(
            "INSERT INTO index_run_events (id, index_run_id, created_at, level, message, relative_path) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(self.run_id)
        .bind(now())
        .bind(level)
        .bind(message)
        .bind(relative_path.map(normalize_relative_path))
        .execute(self.db())
        .await
        {
            error!(target: LOG_TARGET, "Failed to record index run event: {err}");
        }
    }
}

/// Enqueues a directory sync at startup (when enabled) and a full scan on a cron schedule.
pub async fn run_scheduler(
    indexer: Arc<IndexerService>,
    schedule: String,
    index_on_startup: bool,
    shutdown: CancellationToken,
) {
    if index_on_startup {
        indexer.enqueue(None, None, index_flags::DIRECTORIES);
    }

    let cron = match croner::Cron::new(&schedule).parse() {
        Ok(c) => c,
        Err(err) => {
            error!(target: LOG_TARGET, "Invalid cron expression '{schedule}': {err}. Indexer will not run again.");
            return;
        }
    };

    loop {
        let Ok(next) = cron.find_next_occurrence(&Utc::now(), false) else {
            info!(target: LOG_TARGET, "Cron expression '{schedule}' has no future occurrences. Scheduler stopped.");
            return;
        };
        info!(target: LOG_TARGET, "Next scan scheduled at {next} UTC");
        let wait = (next - Utc::now()).to_std().unwrap_or_default();
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = shutdown.cancelled() => return,
        }
        info!(target: LOG_TARGET, "Enqueueing scheduled full scan");
        indexer.enqueue(None, None, index_flags::DIRECTORIES | index_flags::MODELS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDb;

    struct NoopWorker;

    #[async_trait]
    impl IndexWorker for NoopWorker {
        async fn sync_directories(&self) -> anyhow::Result<()> {
            Ok(())
        }
        async fn scan_models(
            &self,
            _: Option<&str>,
            _: &dyn ProgressReporter,
            _: &CancellationToken,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn scan_single(&self, _: &str, _: &dyn ProgressReporter, _: &CancellationToken) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Holds every run start until released.
    struct Gate {
        open: tokio::sync::watch::Sender<bool>,
    }

    impl Default for Gate {
        fn default() -> Self {
            Self { open: tokio::sync::watch::channel(false).0 }
        }
    }

    impl Gate {
        fn release(&self) {
            self.open.send_replace(true);
        }
    }

    #[async_trait]
    impl IndexerHooks for Gate {
        async fn before_run_starts(&self) {
            let mut rx = self.open.subscribe();
            let _ = rx.wait_for(|open| *open).await;
        }
    }

    async fn wait_until(condition: impl Fn() -> bool, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(condition(), "condition was not met within timeout");
    }

    fn indexer(t: &TestDb, gate: Arc<Gate>) -> Arc<IndexerService> {
        IndexerService::with_hooks(t.db.clone(), Arc::new(NoopWorker), gate)
    }

    async fn run(t: &TestDb, id: Uuid) -> Option<IndexRun> {
        sqlx::query_as("SELECT * FROM index_runs WHERE id = ?").bind(id).fetch_optional(&t.db).await.unwrap()
    }

    #[tokio::test]
    async fn processes_in_fifo_order_and_lists_next_up_first() {
        let t = TestDb::new().await;
        let gate = Arc::new(Gate::default());
        let sut = indexer(&t, gate.clone());

        let first = sut.enqueue(Some("a"), None, 0);
        let second = sut.enqueue(Some("b"), None, 0);
        let third = sut.enqueue(Some("c"), None, 0);
        wait_until(|| sut.get_status().current_request.is_some(), Duration::from_secs(2)).await;

        let status = sut.get_status();
        assert_eq!(status.current_request.unwrap().id, first.id);
        assert_eq!(status.queue.iter().map(|q| q.id).collect::<Vec<_>>(), vec![second.id, third.id]);

        gate.release();
        wait_until(|| !sut.get_status().is_running, Duration::from_secs(5)).await;
        let second_run = run(&t, second.run_id.unwrap()).await.unwrap();
        let third_run = run(&t, third.run_id.unwrap()).await.unwrap();
        assert!(second_run.started_at.unwrap() <= third_run.started_at.unwrap());
        assert_eq!(second_run.status, "success");
    }

    #[tokio::test]
    async fn enqueue_merges_flags_and_moves_duplicate_to_front() {
        let t = TestDb::new().await;
        let gate = Arc::new(Gate::default());
        let sut = indexer(&t, gate.clone());
        sut.enqueue(Some("running"), None, 0);
        wait_until(|| sut.get_status().current_request.is_some(), Duration::from_secs(2)).await;
        let a = sut.enqueue(Some("a"), None, index_flags::DIRECTORIES);
        sut.enqueue(Some("b"), None, 0);
        let merged = sut.enqueue(Some("a"), None, index_flags::MODELS);
        assert_eq!(merged.id, a.id);
        let status = sut.get_status();
        assert_eq!(status.queue[0].id, a.id);
        assert_eq!(status.queue[0].flags, index_flags::DIRECTORIES | index_flags::MODELS);
        gate.release();
    }

    #[tokio::test]
    async fn requeue_resets_running_runs_to_queued_with_same_ids() {
        let t = TestDb::new().await;
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for (id, filter, minutes) in [(a, "cats", 5), (b, "dogs", 3)] {
            sqlx::query(
                "INSERT INTO index_runs (id, directory_filter, flags, requested_at, started_at, status, processed_files, total_files)
                 VALUES (?, ?, 2, ?, ?, 'running', 3, 10)",
            )
            .bind(id)
            .bind(filter)
            .bind(now() - chrono::Duration::minutes(minutes))
            .bind(now() - chrono::Duration::minutes(minutes - 1))
            .execute(&t.db)
            .await
            .unwrap();
        }

        let gate = Arc::new(Gate::default());
        let sut = indexer(&t, gate.clone());
        sut.requeue_interrupted_runs().await.unwrap();
        wait_until(|| sut.get_status().current_request.is_some(), Duration::from_secs(2)).await;

        let run_a = run(&t, a).await.unwrap();
        assert_eq!(run_a.status, "queued");
        assert!(run_a.outcome.is_none() && run_a.error.is_none() && run_a.started_at.is_none());
        assert_eq!(run_a.processed_files, 0);
        assert_eq!(run(&t, b).await.unwrap().status, "queued");

        let status = sut.get_status();
        assert_eq!(status.current_request.unwrap().run_id, Some(a));
        assert_eq!(status.queue.len(), 1);
        assert_eq!(status.queue[0].run_id, Some(b));
        gate.release();
    }

    #[tokio::test]
    async fn cancel_removes_queued_request_and_marks_run_cancelled() {
        let t = TestDb::new().await;
        let gate = Arc::new(Gate::default());
        let sut = indexer(&t, gate.clone());
        let first = sut.enqueue(Some("alpha"), None, 0);
        let second = sut.enqueue(Some("beta"), None, 0);
        wait_until(|| sut.get_status().current_request.is_some(), Duration::from_secs(2)).await;

        assert!(sut.cancel(second.run_id.unwrap()).await.unwrap());
        let status = sut.get_status();
        assert!(!status.queue.iter().any(|q| q.run_id == second.run_id));
        let cancelled = run(&t, second.run_id.unwrap()).await.unwrap();
        assert_eq!((cancelled.status.as_str(), cancelled.outcome.as_deref()), ("cancelled", Some("cancelled")));

        gate.release();
        wait_until(|| !sut.get_status().is_running, Duration::from_secs(5)).await;
        assert_eq!(sut.get_status().recent.first().and_then(|r| r.run_id), first.run_id);
        assert!(!sut.cancel(Uuid::new_v4()).await.unwrap());
    }

    #[tokio::test]
    async fn cancelling_running_request_shows_in_live_status_and_persists_outcome() {
        let t = TestDb::new().await;
        let gate = Arc::new(Gate::default());
        let sut = indexer(&t, gate.clone());
        let request = sut.enqueue(Some("running"), None, 0);
        wait_until(|| sut.get_status().current_request.is_some(), Duration::from_secs(2)).await;

        assert!(sut.cancel(request.run_id.unwrap()).await.unwrap());
        let live = sut.get_status().current_request.unwrap();
        assert_eq!(live.run_id, request.run_id);
        assert!(live.is_cancellation_requested);

        gate.release();
        wait_until(|| !sut.get_status().recent.is_empty(), Duration::from_secs(5)).await;
        let recent = sut.get_status().recent[0].clone();
        assert_eq!((recent.run_id, recent.outcome.as_str()), (request.run_id, "cancelled"));
        let persisted = run(&t, request.run_id.unwrap()).await.unwrap();
        assert_eq!((persisted.status.as_str(), persisted.outcome.as_deref()), ("cancelled", Some("cancelled")));
    }

    #[tokio::test]
    async fn run_detail_pages_files_and_events() {
        let t = TestDb::new().await;
        let sut = IndexerService::new(t.db.clone(), Arc::new(NoopWorker));
        let request = sut.enqueue(None, None, index_flags::DIRECTORIES);
        wait_until(|| !sut.get_status().recent.is_empty(), Duration::from_secs(5)).await;

        let history = sut.get_history(7, 250).await.unwrap();
        assert_eq!(history.len(), 1);
        let detail = sut.get_run_detail(request.run_id.unwrap(), 1, 200, "all", 1, 200).await.unwrap().unwrap();
        assert_eq!(detail.run.status, "success");
        assert!(detail.events.items.iter().any(|e| e.message == "Run completed successfully"));
        assert_eq!(detail.files.page_size, 200);
        assert!(sut.get_run_detail(Uuid::new_v4(), 1, 10, "all", 1, 10).await.unwrap().is_none());
    }
}
