//! Background export jobs: plate generation and printing list ZIP archives. Jobs live in
//! memory, write to a temp file, and expire an hour after finishing.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::Mutex;
use tracing::{error, warn};
use uuid::Uuid;

use crate::db::Db;
use crate::dto::{GeneratePlateRequest, PlateGenerationJobDto, PrintingListArchiveJobDto};
use crate::error::AppResult;
use crate::services::models::ModelService;
use crate::services::plate_export::{self, PlateProgress};
use crate::services::printing_lists::Caller;

const LOG_TARGET: &str = "findamodel.PrintingList";
const JOB_RETENTION: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone)]
struct Progress {
    status: &'static str,
    completed_entries: i64,
    current_entry_name: Option<String>,
    error_message: Option<String>,
    warning: Option<String>,
    skipped_models: Vec<String>,
    updated_at: Instant,
}

/// Shared progress tracking for a job.
struct Job {
    id: Uuid,
    owner_id: Option<Uuid>,
    file_name: String,
    format: String,
    path: PathBuf,
    total_entries: i64,
    progress: Mutex<Progress>,
}

impl Job {
    fn new(owner_id: Option<Uuid>, file_name: String, format: String, path: PathBuf, total_entries: i64) -> Arc<Self> {
        Arc::new(Job {
            id: Uuid::new_v4(),
            owner_id,
            file_name,
            format,
            path,
            total_entries,
            progress: Mutex::new(Progress {
                status: "queued",
                completed_entries: 0,
                current_entry_name: None,
                error_message: None,
                warning: None,
                skipped_models: Vec::new(),
                updated_at: Instant::now(),
            }),
        })
    }

    fn update(&self, f: impl FnOnce(&mut Progress)) {
        let mut p = self.progress.lock();
        f(&mut p);
        p.updated_at = Instant::now();
    }

    fn mark_running(&self) {
        self.update(|p| p.status = "running");
    }

    fn mark_completed(&self, warning: Option<String>, skipped: Vec<String>) {
        let total = self.total_entries;
        self.update(|p| {
            p.status = "completed";
            p.completed_entries = total;
            p.current_entry_name = None;
            p.warning = warning;
            p.skipped_models = skipped;
        });
    }

    fn mark_failed(&self, message: String) {
        self.update(|p| {
            p.status = "failed";
            p.error_message = Some(message);
            p.current_entry_name = None;
        });
        let _ = std::fs::remove_file(&self.path);
    }

    fn is_completed(&self) -> bool {
        self.progress.lock().status == "completed"
    }

    fn is_expired(&self) -> bool {
        let p = self.progress.lock();
        matches!(p.status, "completed" | "failed") && p.updated_at.elapsed() > JOB_RETENTION
    }

    fn progress_percent(&self, p: &Progress) -> i64 {
        let percent = if self.total_entries <= 0 {
            if p.status == "completed" { 100 } else { 0 }
        } else {
            (p.completed_entries as f64 * 100.0 / self.total_entries as f64).round() as i64
        };
        percent.clamp(0, 100)
    }

    fn plate_dto(&self) -> PlateGenerationJobDto {
        let p = self.progress.lock();
        PlateGenerationJobDto {
            job_id: self.id,
            file_name: self.file_name.clone(),
            format: self.format.clone(),
            status: p.status.to_string(),
            total_entries: self.total_entries,
            completed_entries: p.completed_entries,
            progress_percent: self.progress_percent(&p),
            current_entry_name: p.current_entry_name.clone(),
            error_message: p.error_message.clone(),
            warning: p.warning.clone(),
            skipped_models: p.skipped_models.clone(),
        }
    }

    fn archive_dto(&self) -> PrintingListArchiveJobDto {
        let p = self.progress.lock();
        PrintingListArchiveJobDto {
            job_id: self.id,
            file_name: self.file_name.clone(),
            status: p.status.to_string(),
            total_entries: self.total_entries,
            completed_entries: p.completed_entries,
            progress_percent: self.progress_percent(&p),
            current_entry_name: p.current_entry_name.clone(),
            error_message: p.error_message.clone(),
        }
    }
}

impl PlateProgress for Job {
    fn mark_current_entry(&self, entry_name: &str) {
        let name = entry_name.to_string();
        self.update(|p| p.current_entry_name = Some(name));
    }

    fn mark_entry_completed(&self) {
        self.update(|p| p.completed_entries += 1);
    }
}

/// A finished job's output file, ready for download.
#[derive(Debug, Clone)]
pub struct CompletedJobFile {
    pub path: PathBuf,
    pub file_name: String,
    pub warning: Option<String>,
    pub skipped_models: Vec<String>,
}

#[derive(Default)]
struct JobRegistry {
    jobs: Mutex<HashMap<Uuid, Arc<Job>>>,
}

impl JobRegistry {
    fn insert(&self, job: Arc<Job>) {
        self.cleanup();
        self.jobs.lock().insert(job.id, job);
    }

    fn get(&self, id: Uuid) -> Option<Arc<Job>> {
        self.cleanup();
        self.jobs.lock().get(&id).cloned()
    }

    fn remove(&self, id: Uuid) {
        if let Some(job) = self.jobs.lock().remove(&id)
            && let Err(err) = std::fs::remove_file(&job.path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            warn!(target: LOG_TARGET, "Failed to delete temp file for job {id}: {err}");
        }
    }

    fn cleanup(&self) {
        let expired: Vec<Uuid> = self.jobs.lock().values().filter(|j| j.is_expired()).map(|j| j.id).collect();
        for id in expired {
            self.remove(id);
        }
    }

    fn completed_file(&self, job: &Job) -> Option<CompletedJobFile> {
        if !job.is_completed() || !job.path.is_file() {
            return None;
        }
        let p = job.progress.lock();
        Some(CompletedJobFile {
            path: job.path.clone(),
            file_name: job.file_name.clone(),
            warning: p.warning.clone(),
            skipped_models: p.skipped_models.clone(),
        })
    }
}

fn temp_dir(kind: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("findamodel").join(kind);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

// ── Plate generation jobs ────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PlateJobService {
    models: ModelService,
    registry: Arc<JobRegistry>,
}

impl PlateJobService {
    pub fn new(models: ModelService) -> Self {
        Self { models, registry: Arc::new(JobRegistry::default()) }
    }

    pub async fn create_job(&self, request: GeneratePlateRequest) -> AppResult<PlateGenerationJobDto> {
        let format = plate_export::normalize_format(request.format.as_deref())?;
        let path = temp_dir("plate-exports")?.join(format!("{}.{format}", Uuid::new_v4().simple()));
        let job =
            Job::new(None, plate_export::file_name(&format).to_string(), format, path, request.placements.len() as i64);
        self.registry.insert(job.clone());

        let models = self.models.clone();
        let dto = job.plate_dto();
        tokio::spawn(async move {
            job.mark_running();
            let result = async {
                let file = std::fs::File::create(&job.path)?;
                let progress: Arc<dyn PlateProgress> = job.clone();
                let (file, result) = plate_export::generate_plate(&models, &request, file, Some(progress)).await?;
                file.sync_all()?;
                AppResult::Ok(result)
            }
            .await;
            match result {
                Ok(r) => job.mark_completed(r.warning, r.skipped_models),
                Err(err) => {
                    error!(target: LOG_TARGET, "Failed to build plate export for job {}: {err}", job.id);
                    job.mark_failed(err.to_string());
                }
            }
        });
        Ok(dto)
    }

    pub fn get_job(&self, id: Uuid) -> Option<PlateGenerationJobDto> {
        self.registry.get(id).map(|j| j.plate_dto())
    }

    pub fn completed_file(&self, id: Uuid) -> Option<CompletedJobFile> {
        let job = self.registry.get(id)?;
        self.registry.completed_file(&job)
    }

    pub fn remove_job(&self, id: Uuid) {
        self.registry.remove(id);
    }
}

// ── Printing list archive jobs ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveSourceRow {
    pub model_id: Uuid,
    pub directory: String,
    pub file_name: String,
    pub quantity: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveEntry {
    pub source_path: PathBuf,
    pub archive_path: String,
}

/// One archive entry per item instance. Quantities above one get `_N` suffixes; when
/// flattening, file names shared by different models get a `_<modelid>` suffix.
pub fn build_archive_entries(root: &Path, rows: &[ArchiveSourceRow], flatten: bool) -> Vec<ArchiveEntry> {
    let mut conflicting: Vec<String> = Vec::new();
    if flatten {
        let mut by_name: HashMap<String, Vec<Uuid>> = HashMap::new();
        for row in rows.iter().filter(|r| !r.file_name.trim().is_empty() && r.quantity > 0) {
            let ids = by_name.entry(crate::metadata::tags::fold_key(&row.file_name)).or_default();
            if !ids.contains(&row.model_id) {
                ids.push(row.model_id);
            }
        }
        conflicting = by_name.into_iter().filter(|(_, ids)| ids.len() > 1).map(|(k, _)| k).collect();
    }

    let mut entries = Vec::new();
    for row in rows {
        if row.file_name.trim().is_empty() || row.quantity <= 0 {
            continue;
        }
        let source = crate::metadata::inheritance::build_full_file_path(root, &row.directory, &row.file_name);
        let file = Path::new(&row.file_name);
        let extension = file.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let id_suffix = if flatten && conflicting.contains(&crate::metadata::tags::fold_key(&row.file_name)) {
            format!("_{}", row.model_id.simple())
        } else {
            String::new()
        };

        let zip_directory = if flatten {
            None
        } else {
            let base = if row.directory.is_empty() {
                row.file_name.clone()
            } else {
                format!("{}/{}", row.directory.trim_matches('/'), row.file_name)
            };
            base.rfind('/').map(|i| base[..i].to_string())
        };

        for instance in 1..=row.quantity {
            let instance_suffix = if row.quantity > 1 { format!("_{instance}") } else { String::new() };
            let name = format!("{stem}{id_suffix}{instance_suffix}{extension}");
            let archive_path = match &zip_directory {
                Some(d) if !d.is_empty() => format!("{d}/{name}"),
                _ => name,
            };
            entries.push(ArchiveEntry { source_path: source.clone(), archive_path });
        }
    }
    entries
}

/// `<list name>-<UTC timestamp>.zip`, with characters invalid in file names replaced.
pub fn archive_file_name(list_name: &str) -> String {
    let mut safe = String::new();
    let mut in_invalid = false;
    for c in list_name.trim().chars() {
        let invalid = c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|');
        if invalid {
            if !in_invalid {
                safe.push('-');
            }
            in_invalid = true;
        } else {
            safe.push(c);
            in_invalid = false;
        }
    }
    let safe = safe.trim_matches(|c| c == '-' || c == ' ');
    let base = if safe.is_empty() { "printing-list" } else { safe };
    format!("{base}-{}.zip", Utc::now().format("%Y%m%d-%H%M%S"))
}

#[derive(Clone)]
pub struct ArchiveJobService {
    db: Db,
    models_root: crate::services::app_config::ModelsRoot,
    registry: Arc<JobRegistry>,
}

impl ArchiveJobService {
    pub fn new(db: Db, models_root: crate::services::app_config::ModelsRoot) -> Self {
        Self { db, models_root, registry: Arc::new(JobRegistry::default()) }
    }

    /// Starts zipping the list's model files. Returns `None` when the list does not exist
    /// or the caller cannot access it.
    pub async fn create_job(
        &self,
        list_id: Uuid,
        caller: Caller,
        flatten: bool,
    ) -> AppResult<Option<PrintingListArchiveJobDto>> {
        let root =
            self.models_root.read().clone().filter(|r| r.is_dir()).ok_or_else(|| {
                crate::error::AppError::Internal(anyhow::anyhow!("Models directory is not configured."))
            })?;

        let list: Option<(String, Uuid)> = sqlx::query_as("SELECT name, owner_id FROM printing_lists WHERE id = ?")
            .bind(list_id)
            .fetch_optional(&self.db)
            .await?;
        let Some((name, _)) = list.filter(|(_, owner)| caller.is_admin || *owner == caller.user_id) else {
            return Ok(None);
        };

        let rows: Vec<(Uuid, String, String, i32)> = sqlx::query_as(
            "SELECT m.id, m.directory, m.file_name, i.quantity FROM printing_list_items i
             JOIN models m ON m.id = i.model_id
             WHERE i.printing_list_id = ? AND i.quantity > 0
             ORDER BY m.directory, m.file_name",
        )
        .bind(list_id)
        .fetch_all(&self.db)
        .await?;
        let rows: Vec<ArchiveSourceRow> = rows
            .into_iter()
            .map(|(model_id, directory, file_name, quantity)| ArchiveSourceRow {
                model_id,
                directory,
                file_name,
                quantity,
            })
            .collect();

        let entries = build_archive_entries(&root, &rows, flatten);
        let path = temp_dir("printing-list-archives")?.join(format!("{}.zip", Uuid::new_v4().simple()));
        let job = Job::new(Some(caller.user_id), archive_file_name(&name), "zip".into(), path, entries.len() as i64);
        self.registry.insert(job.clone());
        let dto = job.archive_dto();

        tokio::task::spawn_blocking(move || {
            job.mark_running();
            match write_archive(&job, &entries) {
                Ok(()) => job.mark_completed(None, Vec::new()),
                Err(err) => {
                    error!(target: LOG_TARGET, "Failed to build printing list archive for job {}: {err:#}", job.id);
                    job.mark_failed(err.to_string());
                }
            }
        });
        Ok(Some(dto))
    }

    fn accessible(&self, id: Uuid, caller: Caller) -> Option<Arc<Job>> {
        self.registry.get(id).filter(|j| caller.is_admin || j.owner_id == Some(caller.user_id))
    }

    pub fn get_job(&self, id: Uuid, caller: Caller) -> Option<PrintingListArchiveJobDto> {
        self.accessible(id, caller).map(|j| j.archive_dto())
    }

    pub fn completed_file(&self, id: Uuid, caller: Caller) -> Option<CompletedJobFile> {
        let job = self.accessible(id, caller)?;
        self.registry.completed_file(&job)
    }

    pub fn remove_job(&self, id: Uuid) {
        self.registry.remove(id);
    }
}

fn write_archive(job: &Job, entries: &[ArchiveEntry]) -> anyhow::Result<()> {
    let file = std::fs::File::create(&job.path)?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::with_capacity(1 << 16, file));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(1))
        .large_file(true);
    for entry in entries {
        if !entry.source_path.is_file() {
            anyhow::bail!("Model file not found: {}", entry.source_path.display());
        }
        job.mark_current_entry(&entry.archive_path);
        zip.start_file(entry.archive_path.as_str(), options)?;
        let mut source = std::io::BufReader::with_capacity(1 << 16, std::fs::File::open(&entry.source_path)?);
        std::io::copy(&mut source, &mut zip)?;
        job.mark_entry_completed();
    }
    zip.finish()?.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::models::tests::service;
    use crate::test_support::{SIMPLE_ASCII_STL, TestDb, model, write_file};

    async fn wait_for_plate(sut: &PlateJobService, id: Uuid) -> PlateGenerationJobDto {
        for _ in 0..400 {
            let job = sut.get_job(id).expect("job");
            if job.status == "completed" || job.status == "failed" {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("plate job did not finish in time");
    }

    fn placement(id: Uuid, x: f64) -> crate::dto::PlacementDto {
        crate::dto::PlacementDto { model_id: id.to_string(), instance_index: 0, x_mm: x, y_mm: 0.0, angle_rad: 0.0 }
    }

    #[tokio::test]
    async fn plate_job_reports_progress_and_produces_file() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "first.stl", SIMPLE_ASCII_STL);
        write_file(root.path(), "second.stl", SIMPLE_ASCII_STL);
        let (first, second) = (model("first.stl", ""), model("second.stl", ""));
        t.insert_model(&first).await;
        t.insert_model(&second).await;

        let sut = PlateJobService::new(service(&t, root.path()));
        let request = GeneratePlateRequest {
            placements: vec![placement(first.id, 0.0), placement(first.id, 20.0), placement(second.id, 40.0)],
            format: Some("3mf".into()),
        };
        let started = sut.create_job(request).await.unwrap();
        assert_eq!(started.status, "queued");
        assert_eq!(started.total_entries, 3);

        let done = wait_for_plate(&sut, started.job_id).await;
        assert_eq!(done.status, "completed", "{:?}", done.error_message);
        assert_eq!((done.completed_entries, done.progress_percent), (3, 100));
        assert!(done.skipped_models.is_empty());

        let file = sut.completed_file(started.job_id).unwrap();
        assert_eq!(file.file_name, "plate.3mf");
        assert!(file.path.is_file());
        sut.remove_job(started.job_id);
        assert!(!file.path.exists());
    }

    #[tokio::test]
    async fn plate_job_preserves_skipped_model_warning() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "printable.stl", SIMPLE_ASCII_STL);
        let (printable, skipped) = (model("printable.stl", ""), model("skipme.lys", ""));
        t.insert_model(&printable).await;
        t.insert_model(&skipped).await;

        let sut = PlateJobService::new(service(&t, root.path()));
        let request = GeneratePlateRequest {
            placements: vec![placement(skipped.id, 0.0), placement(printable.id, 20.0)],
            format: Some("stl".into()),
        };
        let started = sut.create_job(request).await.unwrap();
        let done = wait_for_plate(&sut, started.job_id).await;
        assert_eq!(done.status, "completed", "{:?}", done.error_message);
        assert!(done.warning.is_some());
        assert_eq!(done.skipped_models, vec!["skipme.lys"]);

        let bytes = std::fs::read(sut.completed_file(started.job_id).unwrap().path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 1);
    }

    #[tokio::test]
    async fn plate_job_fails_for_unknown_model() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let sut = PlateJobService::new(service(&t, root.path()));
        let started = sut
            .create_job(GeneratePlateRequest { placements: vec![placement(Uuid::new_v4(), 0.0)], format: None })
            .await
            .unwrap();
        let done = wait_for_plate(&sut, started.job_id).await;
        assert_eq!(done.status, "failed");
        assert!(done.error_message.unwrap().contains("Model not found"));
        assert!(sut.completed_file(started.job_id).is_none());
    }

    #[test]
    fn archive_entries_handle_quantities_folders_and_flatten_conflicts() {
        let root = Path::new("/models");
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let rows = vec![
            ArchiveSourceRow { model_id: a, directory: "x/y".into(), file_name: "knight.stl".into(), quantity: 2 },
            ArchiveSourceRow { model_id: b, directory: "z".into(), file_name: "Knight.stl".into(), quantity: 1 },
        ];
        let nested = build_archive_entries(root, &rows, false);
        assert_eq!(
            nested.iter().map(|e| e.archive_path.as_str()).collect::<Vec<_>>(),
            vec!["x/y/knight_1.stl", "x/y/knight_2.stl", "z/Knight.stl"]
        );
        assert_eq!(nested[0].source_path, PathBuf::from("/models/x/y/knight.stl"));

        let flat = build_archive_entries(root, &rows, true);
        assert_eq!(flat[0].archive_path, format!("knight_{}_1.stl", a.simple()));
        assert_eq!(flat[2].archive_path, format!("Knight_{}.stl", b.simple()));
    }

    #[test]
    fn archive_file_names_are_sanitised() {
        assert!(archive_file_name("My/List: v2").starts_with("My-List- v2-"));
        assert!(archive_file_name("  ").starts_with("printing-list-"));
        assert!(archive_file_name("x").ends_with(".zip"));
    }

    #[tokio::test]
    async fn archive_job_zips_list_files_for_owner_only() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "set/a.stl", "aaa");
        let m = model("a.stl", "set");
        t.insert_model(&m).await;
        let owner = t.add_user("owner", false).await;
        let caller = Caller { user_id: owner, is_admin: false };
        let list = crate::services::printing_lists::create_list(&t.db, owner, "My List").await.unwrap();
        crate::services::printing_lists::upsert_item(&t.db, list.id, caller, m.id, 2).await.unwrap();

        let models_root = std::sync::Arc::new(parking_lot::RwLock::new(Some(root.path().to_path_buf())));
        let sut = ArchiveJobService::new(t.db.clone(), models_root);
        let other = Caller { user_id: t.add_user("other", false).await, is_admin: false };
        assert!(sut.create_job(list.id, other, false).await.unwrap().is_none());

        let job = sut.create_job(list.id, caller, false).await.unwrap().unwrap();
        assert_eq!(job.total_entries, 2);
        let mut file = None;
        for _ in 0..200 {
            file = sut.completed_file(job.job_id, caller);
            if file.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let file = file.expect("archive completed");
        assert!(file.file_name.starts_with("My List-"));
        assert!(sut.get_job(job.job_id, other).is_none());

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&file.path).unwrap()).unwrap();
        let names: Vec<String> = (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().to_string()).collect();
        assert_eq!(names, vec!["set/a_1.stl", "set/a_2.stl"]);
    }
}
