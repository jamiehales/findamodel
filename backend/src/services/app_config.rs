//! Application settings stored in the singleton `app_config` row.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::config::AppConfigDefaults;
use crate::db::{Db, now};
use crate::dto::{AppConfigDto, InitialSetupDefaultsDto, InitialSetupRequest, SetupStatusDto, UpdateAppConfigRequest};
use crate::entities::AppConfigRow;
use crate::error::{AppResult, bad_request};
use crate::preview::CURRENT_PREVIEW_GENERATION_VERSION;

pub const DATABASE_DEFAULT_RAFT_HEIGHT_MM: f32 = 2.0;

/// The models root in effect, shared by every service. Set from the environment or the
/// setup wizard.
pub type ModelsRoot = Arc<RwLock<Option<PathBuf>>>;

#[derive(Clone)]
pub struct AppConfigService {
    db: Db,
    defaults: Arc<AppConfigDefaults>,
    /// Models path configured by the environment, if any.
    configured_models_path: Option<String>,
    models_root: ModelsRoot,
}

/// Resolves a configured models directory to an absolute path. On macOS, `smb://` URLs
/// map to their mount point under /Volumes.
pub fn resolve_models_directory_path(
    configured: &str,
    require_existing: bool,
    directory_exists: &dyn Fn(&Path) -> bool,
) -> AppResult<PathBuf> {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        return Err(bad_request("Models directory path is required."));
    }

    if cfg!(target_os = "macos")
        && let Some(path) = resolve_mac_smb_path(trimmed, require_existing, directory_exists)?
    {
        return Ok(path);
    }

    let full = std::path::absolute(trimmed).map_err(|e| bad_request(format!("Invalid models directory path: {e}")))?;
    if require_existing && !directory_exists(&full) {
        return Err(bad_request(format!("Models directory does not exist: {}", full.display())));
    }
    Ok(full)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn mac_smb_candidates(url: &str) -> Option<Vec<PathBuf>> {
    let rest = url.strip_prefix("smb://").or_else(|| url.strip_prefix("SMB://"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = percent_decode(authority.rsplit('@').next().unwrap_or(authority));
    let segments: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(percent_decode).collect();

    let mut candidates = Vec::new();
    if segments.is_empty() {
        if !host.is_empty() {
            candidates.push(Path::new("/Volumes").join(&host));
        }
        return Some(candidates);
    }
    let mut share = PathBuf::from("/Volumes");
    share.extend(&segments);
    candidates.push(share);
    if !host.is_empty() {
        let mut with_host = Path::new("/Volumes").join(&host);
        with_host.extend(&segments);
        candidates.push(with_host);
    }
    Some(candidates)
}

fn resolve_mac_smb_path(
    path: &str,
    require_existing: bool,
    directory_exists: &dyn Fn(&Path) -> bool,
) -> AppResult<Option<PathBuf>> {
    let Some(candidates) = mac_smb_candidates(path).filter(|c| !c.is_empty()) else {
        return Ok(None);
    };
    if !require_existing {
        return Ok(Some(candidates[0].clone()));
    }
    if let Some(found) = candidates.into_iter().find(|c| directory_exists(c)) {
        return Ok(Some(found));
    }
    Err(bad_request(format!(
        "Models directory does not exist: {path}. On macOS, SMB URLs must point to a mounted share under /Volumes."
    )))
}

fn validate_raft_height(value: f32) -> AppResult<()> {
    if !value.is_finite() || value < 0.0 {
        return Err(bad_request("Default raft height must be a finite number greater than or equal to 0."));
    }
    Ok(())
}

fn normalize_minimum_preview_version(v: i32) -> i32 {
    v.clamp(0, CURRENT_PREVIEW_GENERATION_VERSION)
}

impl AppConfigService {
    pub fn new(
        db: Db,
        defaults: AppConfigDefaults,
        configured_models_path: Option<String>,
        models_root: ModelsRoot,
    ) -> Self {
        Self { db, defaults: Arc::new(defaults), configured_models_path, models_root }
    }

    pub fn models_root(&self) -> &ModelsRoot {
        &self.models_root
    }

    async fn ensure_config(&self) -> AppResult<AppConfigRow> {
        if let Some(row) =
            sqlx::query_as::<_, AppConfigRow>("SELECT * FROM app_config WHERE id = 1").fetch_optional(&self.db).await?
        {
            return Ok(row);
        }

        let configured = match self.configured_models_path.as_deref() {
            Some(p) => Some(resolve_models_directory_path(p, false, &|p: &Path| p.is_dir())?.display().to_string()),
            None => None,
        };
        let row = AppConfigRow {
            id: 1,
            default_raft_height_mm: self.defaults.default_raft_height_mm.unwrap_or(DATABASE_DEFAULT_RAFT_HEIGHT_MM),
            generate_previews_enabled: self.defaults.generate_previews_enabled.unwrap_or(true),
            minimum_preview_generation_version: normalize_minimum_preview_version(
                self.defaults.minimum_preview_generation_version.unwrap_or(CURRENT_PREVIEW_GENERATION_VERSION),
            ),
            setup_completed: configured.is_some(),
            models_directory_path: configured,
            updated_at: now(),
        };
        sqlx::query(
            "INSERT OR IGNORE INTO app_config (id, default_raft_height_mm, generate_previews_enabled,
                minimum_preview_generation_version, setup_completed, models_directory_path, updated_at)
             VALUES (1, ?, ?, ?, ?, ?, ?)",
        )
        .bind(row.default_raft_height_mm)
        .bind(row.generate_previews_enabled)
        .bind(row.minimum_preview_generation_version)
        .bind(row.setup_completed)
        .bind(&row.models_directory_path)
        .bind(row.updated_at)
        .execute(&self.db)
        .await?;
        Ok(sqlx::query_as("SELECT * FROM app_config WHERE id = 1").fetch_one(&self.db).await?)
    }

    fn to_dto(row: &AppConfigRow) -> AppConfigDto {
        AppConfigDto {
            default_raft_height_mm: row.default_raft_height_mm,
            generate_previews_enabled: row.generate_previews_enabled,
            minimum_preview_generation_version: normalize_minimum_preview_version(
                row.minimum_preview_generation_version,
            ),
            setup_completed: row.setup_completed,
            models_directory_path: row.models_directory_path.clone(),
        }
    }

    pub async fn get(&self) -> AppResult<AppConfigDto> {
        Ok(Self::to_dto(&self.ensure_config().await?))
    }

    pub async fn get_default_raft_height_mm(&self) -> AppResult<f32> {
        Ok(self.ensure_config().await?.default_raft_height_mm)
    }

    pub async fn update(&self, req: &UpdateAppConfigRequest) -> AppResult<AppConfigDto> {
        validate_raft_height(req.default_raft_height_mm)?;
        if req.minimum_preview_generation_version < 0
            || req.minimum_preview_generation_version > CURRENT_PREVIEW_GENERATION_VERSION
        {
            return Err(bad_request(format!(
                "Minimum preview version must be between 0 and {CURRENT_PREVIEW_GENERATION_VERSION}."
            )));
        }
        self.ensure_config().await?;
        sqlx::query(
            "UPDATE app_config SET default_raft_height_mm = ?, generate_previews_enabled = ?,
                minimum_preview_generation_version = ?, updated_at = ? WHERE id = 1",
        )
        .bind(req.default_raft_height_mm)
        .bind(req.generate_previews_enabled)
        .bind(req.minimum_preview_generation_version)
        .bind(now())
        .execute(&self.db)
        .await?;
        self.get().await
    }

    pub async fn update_default_raft_height(&self, value: f32) -> AppResult<AppConfigDto> {
        validate_raft_height(value)?;
        self.ensure_config().await?;
        sqlx::query("UPDATE app_config SET default_raft_height_mm = ?, updated_at = ? WHERE id = 1")
            .bind(value)
            .bind(now())
            .execute(&self.db)
            .await?;
        self.get().await
    }

    pub async fn complete_initial_setup(&self, req: &InitialSetupRequest) -> AppResult<AppConfigDto> {
        if req.models_directory_path.trim().is_empty() {
            return Err(bad_request("Models directory path is required."));
        }
        let full = resolve_models_directory_path(&req.models_directory_path, true, &|p: &Path| p.is_dir())?;
        validate_raft_height(req.default_raft_height_mm)?;

        self.ensure_config().await?;
        let full_text = full.display().to_string();
        sqlx::query(
            "UPDATE app_config SET setup_completed = 1, models_directory_path = ?, default_raft_height_mm = ?,
                generate_previews_enabled = ?, updated_at = ? WHERE id = 1",
        )
        .bind(&full_text)
        .bind(req.default_raft_height_mm)
        .bind(req.generate_previews_enabled)
        .bind(now())
        .execute(&self.db)
        .await?;
        *self.models_root.write() = Some(full);
        self.get().await
    }

    pub async fn get_setup_status(&self) -> AppResult<SetupStatusDto> {
        let row = self.ensure_config().await?;
        let has_models_path = row.models_directory_path.as_deref().is_some_and(|p| !p.trim().is_empty())
            || self.models_root.read().is_some();
        Ok(SetupStatusDto {
            setup_completed: row.setup_completed,
            requires_wizard: !row.setup_completed && !has_models_path,
        })
    }

    pub async fn get_initial_setup_defaults(&self) -> AppResult<InitialSetupDefaultsDto> {
        let row = self.ensure_config().await?;
        Ok(InitialSetupDefaultsDto {
            models_directory_path: self.configured_models_path.clone().or(row.models_directory_path),
            default_raft_height_mm: row.default_raft_height_mm,
            generate_previews_enabled: row.generate_previews_enabled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDb;

    fn service(t: &TestDb, configured: Option<String>) -> AppConfigService {
        AppConfigService::new(t.db.clone(), AppConfigDefaults::default(), configured, Arc::new(RwLock::new(None)))
    }

    async fn row_count(t: &TestDb) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM app_config").fetch_one(&t.db).await.unwrap()
    }

    #[tokio::test]
    async fn get_creates_default_record_once() {
        let t = TestDb::new().await;
        let sut = service(&t, None);
        let dto = sut.get().await.unwrap();
        assert_eq!(dto.default_raft_height_mm, DATABASE_DEFAULT_RAFT_HEIGHT_MM);
        assert!(dto.generate_previews_enabled);
        assert_eq!(dto.minimum_preview_generation_version, CURRENT_PREVIEW_GENERATION_VERSION);
        assert!(!dto.setup_completed);
        sut.get().await.unwrap();
        assert_eq!(row_count(&t).await, 1);
    }

    #[tokio::test]
    async fn default_raft_height_reads_stored_value_and_validates_updates() {
        let t = TestDb::new().await;
        let sut = service(&t, None);
        assert_eq!(sut.get_default_raft_height_mm().await.unwrap(), DATABASE_DEFAULT_RAFT_HEIGHT_MM);
        assert_eq!(sut.update_default_raft_height(4.0).await.unwrap().default_raft_height_mm, 4.0);
        assert_eq!(sut.get_default_raft_height_mm().await.unwrap(), 4.0);
        assert_eq!(sut.update_default_raft_height(0.0).await.unwrap().default_raft_height_mm, 0.0);
        for bad in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                matches!(sut.update_default_raft_height(bad).await, Err(crate::error::AppError::BadRequest(_))),
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn update_validates_minimum_preview_version() {
        let t = TestDb::new().await;
        let sut = service(&t, None);
        let request = |version: i32| UpdateAppConfigRequest {
            default_raft_height_mm: 3.0,
            generate_previews_enabled: true,
            minimum_preview_generation_version: version,
        };
        assert_eq!(sut.update(&request(5)).await.unwrap().minimum_preview_generation_version, 5);
        assert!(sut.update(&request(-1)).await.is_err());
        assert!(sut.update(&request(CURRENT_PREVIEW_GENERATION_VERSION + 1)).await.is_err());
    }

    #[tokio::test]
    async fn setup_wizard_is_required_without_models_path() {
        let t = TestDb::new().await;
        let status = service(&t, None).get_setup_status().await.unwrap();
        assert!(!status.setup_completed);
        assert!(status.requires_wizard);

        let configured = TestDb::new().await;
        let dir = tempfile::tempdir().unwrap();
        let status = service(&configured, Some(dir.path().display().to_string())).get_setup_status().await.unwrap();
        assert!(status.setup_completed && !status.requires_wizard);
    }

    #[tokio::test]
    async fn complete_initial_setup_persists_models_path() {
        let t = TestDb::new().await;
        let dir = tempfile::tempdir().unwrap();
        let sut = service(&t, None);
        let updated = sut
            .complete_initial_setup(&InitialSetupRequest {
                models_directory_path: dir.path().display().to_string(),
                default_raft_height_mm: 2.0,
                generate_previews_enabled: true,
            })
            .await
            .unwrap();
        assert!(updated.setup_completed);
        assert_eq!(
            updated.models_directory_path.as_deref(),
            Some(std::path::absolute(dir.path()).unwrap().to_str().unwrap())
        );
        assert_eq!(updated.minimum_preview_generation_version, CURRENT_PREVIEW_GENERATION_VERSION);
        assert_eq!(sut.models_root().read().as_deref(), Some(std::path::absolute(dir.path()).unwrap().as_path()));
    }

    #[test]
    fn resolves_local_paths_to_absolute() {
        let resolved = resolve_models_directory_path("findamodel-models", false, &|_| true).unwrap();
        assert!(resolved.is_absolute());
        assert!(resolve_models_directory_path("  ", false, &|_| true).is_err());
        assert!(resolve_models_directory_path("/missing", true, &|_| false).is_err());
    }

    #[test]
    fn maps_smb_urls_to_volumes_mounts() {
        assert_eq!(mac_smb_candidates("smb://behemoth.local").unwrap(), vec![PathBuf::from("/Volumes/behemoth.local")]);
        assert_eq!(
            mac_smb_candidates("smb://behemoth.local/share-name/folder/models").unwrap(),
            vec![
                PathBuf::from("/Volumes/share-name/folder/models"),
                PathBuf::from("/Volumes/behemoth.local/share-name/folder/models"),
            ]
        );
        assert_eq!(mac_smb_candidates("smb://host/My%20Share").unwrap()[0], PathBuf::from("/Volumes/My Share"));
        assert!(mac_smb_candidates("/local/path").is_none());

        let resolved =
            resolve_mac_smb_path("smb://behemoth.local/share-name/folder/models", false, &|_| false).unwrap();
        assert_eq!(resolved, Some(PathBuf::from("/Volumes/share-name/folder/models")));
        let err = resolve_mac_smb_path("smb://behemoth.local/share-name", true, &|_| false).unwrap_err();
        assert!(err.to_string().contains("SMB URLs"));
    }
}
