//! Model lookups, per-model metadata editing and related-part discovery. Scanning lives in
//! `scan.rs`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_yaml::Value;
use uuid::Uuid;

use crate::db::Db;
use crate::dto::{ModelDto, ModelMetadataDetail, ModelMetadataEntry, RelatedModelDto, UpdateModelMetadataRequest};
use crate::entities::ModelRow;
use crate::error::{AppError, AppResult};
use crate::metadata::config_reader::{self, CONFIG_FILE_NAME};
use crate::metadata::inheritance;
use crate::metadata::{fields, model_metadata, tags, yaml};
use crate::preview::{self, PreviewService};
use crate::services::app_config::{AppConfigService, ModelsRoot};
use crate::services::metadata_config::MetadataConfigService;
use crate::services::store;

pub const MODEL_EXTENSIONS: [&str; 5] = ["stl", "obj", "lys", "lyt", "ctb"];
pub const NON_GEOMETRY_TYPES: [&str; 3] = ["lys", "lyt", "ctb"];

/// Page size for the model list when no limit is given, and the most it returns.
const DEFAULT_MODEL_LIST_LIMIT: i64 = 100;
const MAX_MODEL_LIST_LIMIT: i64 = 500;

pub fn is_non_geometry_type(file_type: &str) -> bool {
    NON_GEOMETRY_TYPES.iter().any(|t| t.eq_ignore_ascii_case(file_type))
}

pub fn is_model_extension(ext: &str) -> bool {
    MODEL_EXTENSIONS.iter().any(|t| t.eq_ignore_ascii_case(ext))
}

#[derive(Clone)]
pub struct ModelService {
    pub(crate) db: Db,
    pub(crate) models_root: ModelsRoot,
    pub(crate) preview: Option<PreviewService>,
    pub(crate) metadata_config: MetadataConfigService,
    pub(crate) app_config: AppConfigService,
    pub(crate) file_scan_threads: usize,
}

/// Preview image candidates in priority order: the requested variant, then the other
/// variant of the model's stored preview.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewPathCandidates {
    pub preferred: String,
    pub fallback: String,
}

impl PreviewPathCandidates {
    pub fn candidates(&self) -> Vec<(String, bool)> {
        vec![(self.preferred.clone(), true), (self.fallback.clone(), false)]
    }
}

fn normalize_part_grouping(value: Option<&str>) -> String {
    value.map(str::trim).unwrap_or_default().to_string()
}

impl ModelService {
    pub fn new(
        db: Db,
        models_root: ModelsRoot,
        preview: Option<PreviewService>,
        metadata_config: MetadataConfigService,
        app_config: AppConfigService,
        file_scan_threads: usize,
    ) -> Self {
        Self { db, models_root, preview, metadata_config, app_config, file_scan_threads: file_scan_threads.max(1) }
    }

    pub fn root(&self) -> Option<PathBuf> {
        self.models_root.read().clone()
    }

    pub fn full_path(&self, model: &ModelRow) -> Option<PathBuf> {
        Some(inheritance::build_full_file_path(&self.root()?, &model.directory, &model.file_name))
    }

    pub async fn get_models(&self, limit: Option<i64>) -> AppResult<Vec<ModelDto>> {
        let rows: Vec<ModelRow> = sqlx::query_as("SELECT * FROM models ORDER BY directory, file_name LIMIT ?")
            .bind(limit.filter(|l| *l >= 0).unwrap_or(DEFAULT_MODEL_LIST_LIMIT).min(MAX_MODEL_LIST_LIMIT))
            .fetch_all(&self.db)
            .await?;
        Ok(rows.iter().map(store::to_model_dto).collect())
    }

    pub async fn get_models_by_ids(&self, ids: &[Uuid]) -> AppResult<Vec<ModelDto>> {
        let mut unique = ids.to_vec();
        unique.sort();
        unique.dedup();
        let mut rows = store::get_models_by_ids(&self.db, &unique).await?;
        rows.sort_by(|a, b| a.directory.cmp(&b.directory).then_with(|| a.file_name.cmp(&b.file_name)));
        Ok(rows.iter().map(store::to_model_dto).collect())
    }

    pub async fn get_models_by_id_map(&self, ids: &[Uuid]) -> AppResult<HashMap<Uuid, ModelRow>> {
        Ok(store::get_models_by_ids(&self.db, ids).await?.into_iter().map(|m| (m.id, m)).collect())
    }

    pub async fn get_model(&self, id: Uuid) -> AppResult<Option<ModelRow>> {
        Ok(store::get_model(&self.db, id).await?)
    }

    pub async fn get_model_dto(&self, id: Uuid) -> AppResult<Option<ModelDto>> {
        Ok(self.get_model(id).await?.as_ref().map(store::to_model_dto))
    }

    pub async fn preview_path_candidates(
        &self,
        id: Uuid,
        include_supports: bool,
    ) -> AppResult<Option<PreviewPathCandidates>> {
        let stored: Option<Option<String>> = sqlx::query_scalar("SELECT preview_image_path FROM models WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.db)
            .await?;
        let Some(stored) = stored.flatten().filter(|s| !s.trim().is_empty()) else {
            return Ok(None);
        };
        let stem = preview::stem_of(&stored);
        Ok(Some(PreviewPathCandidates {
            preferred: preview::relative_path(stem, include_supports),
            fallback: preview::relative_path(stem, !include_supports),
        }))
    }

    /// Writes per-model overrides to the `model_metadata` section of the model's directory
    /// findamodel.yaml. Empty overrides remove the entry; an empty file is deleted.
    pub async fn update_model_metadata(
        &self,
        id: Uuid,
        req: &UpdateModelMetadataRequest,
    ) -> AppResult<Option<ModelDto>> {
        let Some(model) = self.get_model(id).await? else {
            return Ok(None);
        };
        let root =
            self.root().ok_or_else(|| AppError::Internal(anyhow::anyhow!("Models directory is not configured.")))?;
        let config_path = config_reader::full_dir_path(&root, &model.directory).join(CONFIG_FILE_NAME);

        let mut data = config_reader::read_config_mapping(&config_path).await;
        let section_key = yaml::get_ci(&data, "model_metadata")
            .map(|(k, _)| k.to_string())
            .unwrap_or_else(|| "model_metadata".into());
        let mut section = match data.get(Value::String(section_key.clone())) {
            Some(Value::Mapping(m)) => m.clone(),
            _ => serde_yaml::Mapping::new(),
        };

        let sanitized = ModelMetadataEntry {
            name: req.name.clone(),
            part_name: req.part_name.clone(),
            creator: req.creator.clone(),
            collection: req.collection.clone(),
            subcollection: req.subcollection.clone(),
            tags: req.tags.as_ref().map(tags::normalize),
            category: fields::validate_enum_value(req.category.as_deref()),
            item_type: fields::validate_enum_value(req.item_type.as_deref()),
            material: fields::validate_enum_value(req.material.as_deref()),
            supported: req.supported,
            raft_height_mm: req.raft_height_mm,
        };

        let file_key = Value::String(model.file_name.clone());
        if sanitized.is_empty() {
            section.remove(&file_key);
        } else {
            section.insert(file_key, Value::Mapping(sanitized.to_yaml()));
        }

        if section.is_empty() {
            data.remove(Value::String(section_key));
        } else {
            data.insert(Value::String(section_key), Value::Mapping(section));
        }
        config_reader::write_config_mapping(&config_path, &data).await?;

        self.get_model_dto(id).await
    }

    pub async fn get_model_metadata(&self, id: Uuid) -> AppResult<Option<ModelMetadataDetail>> {
        let Some(model) = self.get_model(id).await? else {
            return Ok(None);
        };
        let dir_config = store::get_directory_config(&self.db, &model.directory).await?;
        let local_values = dir_config
            .as_ref()
            .and_then(|dc| model_metadata::get_model_metadata_entry(dc, &model.file_name))
            .unwrap_or_default();

        let inherited_values = match (&dir_config, self.full_path(&model)) {
            (Some(dc), Some(full)) => {
                let inherited = model_metadata::compute_inherited(&full.to_string_lossy(), Some(dc));
                Some(ModelMetadataEntry {
                    name: inherited.model_name,
                    part_name: inherited.part_name,
                    creator: inherited.creator,
                    collection: inherited.collection,
                    subcollection: inherited.subcollection,
                    tags: Some(inherited.tags),
                    category: inherited.category,
                    item_type: inherited.item_type,
                    material: inherited.material,
                    supported: inherited.supported,
                    raft_height_mm: inherited.raft_height_mm,
                })
            }
            _ => None,
        };

        Ok(Some(ModelMetadataDetail { local_values, inherited_values }))
    }

    /// Other parts of the same model: same creator, collection, subcollection, supported
    /// flag and model name (case-insensitive).
    pub async fn get_other_parts(&self, id: Uuid) -> AppResult<Vec<RelatedModelDto>> {
        let Some(source) = self.get_model(id).await? else {
            return Ok(Vec::new());
        };
        let Some(creator) = source.calculated_creator.clone().filter(|c| !c.trim().is_empty()) else {
            return Ok(Vec::new());
        };

        let source_collection = normalize_part_grouping(source.calculated_collection.as_deref());
        let source_subcollection = normalize_part_grouping(source.calculated_subcollection.as_deref());
        let name_of = |m: &ModelRow| match m.calculated_model_name.as_deref().filter(|n| !n.trim().is_empty()) {
            Some(n) => n.to_string(),
            None => store::file_stem(&m.file_name),
        };
        let source_name = name_of(&source);

        let candidates: Vec<ModelRow> = sqlx::query_as(
            "SELECT * FROM models WHERE id != ? AND calculated_creator = ? ORDER BY directory, file_name",
        )
        .bind(id)
        .bind(&creator)
        .fetch_all(&self.db)
        .await?;

        Ok(candidates
            .iter()
            .filter(|m| {
                normalize_part_grouping(m.calculated_collection.as_deref()) == source_collection
                    && normalize_part_grouping(m.calculated_subcollection.as_deref()) == source_subcollection
                    && m.supported() == source.supported()
                    && tags::eq_ignore_case(&name_of(m), &source_name)
            })
            .map(store::to_related_dto)
            .collect())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::AppConfigDefaults;
    use crate::entities::DirectoryConfig;
    use crate::test_support::{TestDb, model};
    use parking_lot::RwLock;
    use std::sync::Arc;

    pub(crate) fn service(t: &TestDb, root: &std::path::Path) -> ModelService {
        let models_root: ModelsRoot = Arc::new(RwLock::new(Some(root.to_path_buf())));
        let metadata = MetadataConfigService::new(t.db.clone(), models_root.clone(), 1000);
        let app_config = AppConfigService::new(t.db.clone(), AppConfigDefaults::default(), None, models_root.clone());
        ModelService::new(t.db.clone(), models_root, None, metadata, app_config, 4)
    }

    #[tokio::test]
    async fn get_models_defaults_and_caps_the_page_size() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        for i in 0..MAX_MODEL_LIST_LIMIT + 10 {
            t.insert_model(&model(&format!("m{i:04}.stl"), "")).await;
        }
        let svc = service(&t, root.path());
        assert_eq!(svc.get_models(None).await.unwrap().len() as i64, DEFAULT_MODEL_LIST_LIMIT);
        assert_eq!(svc.get_models(Some(3)).await.unwrap().len(), 3);
        assert_eq!(svc.get_models(Some(-1)).await.unwrap().len() as i64, DEFAULT_MODEL_LIST_LIMIT);
        assert_eq!(svc.get_models(Some(i64::MAX)).await.unwrap().len() as i64, MAX_MODEL_LIST_LIMIT);
    }

    async fn insert_dir(t: &TestDb, dc: DirectoryConfig) {
        let mut conn = t.db.acquire().await.unwrap();
        store::save_directory_config(&mut conn, &dc).await.unwrap();
    }

    #[tokio::test]
    async fn get_model_metadata_returns_entry_for_matching_file() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let m = model("dragon.stl", "");
        t.insert_model(&m).await;
        insert_dir(
            &t,
            DirectoryConfig {
                id: Uuid::new_v4(),
                directory_path: String::new(),
                raw_model_metadata_json: Some(
                    r#"{"dragon.stl":{"Name":"Fire Dragon","PartName":"Body","Creator":"Alice","Collection":"Fantasy","Subcollection":"Creatures","Category":"miniature","Type":"whole","Material":"resin","Supported":true}}"#
                        .into(),
                ),
                updated_at: chrono::Utc::now(),
                ..Default::default()
            },
        )
        .await;

        let detail = service(&t, root.path()).get_model_metadata(m.id).await.unwrap().unwrap();
        let local = detail.local_values;
        assert_eq!(local.name.as_deref(), Some("Fire Dragon"));
        assert_eq!(local.part_name.as_deref(), Some("Body"));
        assert_eq!(local.creator.as_deref(), Some("Alice"));
        assert_eq!(local.collection.as_deref(), Some("Fantasy"));
        assert_eq!(local.subcollection.as_deref(), Some("Creatures"));
        assert_eq!(local.category.as_deref(), Some("miniature"));
        assert_eq!(local.item_type.as_deref(), Some("whole"));
        assert_eq!(local.material.as_deref(), Some("resin"));
        assert_eq!(local.supported, Some(true));
        assert!(detail.inherited_values.is_some());
    }

    #[tokio::test]
    async fn update_model_metadata_writes_yaml() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("dragon.stl"), "solid dragon\nendsolid dragon\n").unwrap();
        let m = model("dragon.stl", "");
        t.insert_model(&m).await;

        let req = UpdateModelMetadataRequest {
            name: Some("Fire Dragon".into()),
            part_name: Some("Body".into()),
            creator: Some("Alice".into()),
            collection: Some("Fantasy".into()),
            subcollection: Some("Creatures".into()),
            tags: Some(vec!["32mm".into(), "metal".into()]),
            category: Some("miniature".into()),
            item_type: Some("whole".into()),
            material: Some("resin".into()),
            supported: Some(true),
            raft_height_mm: None,
        };
        assert!(service(&t, root.path()).update_model_metadata(m.id, &req).await.unwrap().is_some());

        let yaml = std::fs::read_to_string(root.path().join(CONFIG_FILE_NAME)).unwrap();
        for expected in [
            "model_metadata:",
            "dragon.stl:",
            "name: Fire Dragon",
            "part_name: Body",
            "creator: Alice",
            "collection: Fantasy",
            "subcollection: Creatures",
            "tags:",
            "- 32mm",
            "- metal",
            "category: miniature",
            "type: whole",
            "material: resin",
            "supported: true",
        ] {
            assert!(yaml.contains(expected), "missing '{expected}' in:\n{yaml}");
        }

        // Clearing every override removes the entry and the now-empty file.
        service(&t, root.path()).update_model_metadata(m.id, &UpdateModelMetadataRequest::default()).await.unwrap();
        assert!(!root.path().join(CONFIG_FILE_NAME).exists());
    }

    #[tokio::test]
    async fn preview_candidates_prefer_requested_variant() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let mut m = model("dragon.stl", "");
        m.checksum = "abc123".into();
        m.preview_image_path = Some("abc123.png".into());
        t.insert_model(&m).await;

        let candidates = service(&t, root.path()).preview_path_candidates(m.id, false).await.unwrap().unwrap();
        assert_eq!(
            candidates.candidates(),
            vec![("abc123_supportsremoved.png".to_string(), true), ("abc123.png".to_string(), false)]
        );
    }

    #[tokio::test]
    async fn other_parts_match_grouping_and_name() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let mk = |file: &str, name: &str, creator: &str| {
            let mut m = model(file, "set");
            m.calculated_creator = Some(creator.into());
            m.calculated_collection = Some("Fantasy".into());
            m.calculated_model_name = Some(name.into());
            m
        };
        let source = mk("knight_body.stl", "Knight", "Alice");
        let sibling = mk("knight_head.stl", "knight", "Alice");
        let other_name = mk("orc.stl", "Orc", "Alice");
        let other_creator = mk("knight_b.stl", "Knight", "Bob");
        for m in [&source, &sibling, &other_name, &other_creator] {
            t.insert_model(m).await;
        }
        let parts = service(&t, root.path()).get_other_parts(source.id).await.unwrap();
        assert_eq!(parts.iter().map(|p| p.id).collect::<Vec<_>>(), vec![sibling.id]);
    }
}
