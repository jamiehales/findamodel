//! Keeps directory config records in sync with findamodel.yaml files and propagates
//! inherited values to descendants and to cached model metadata.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sqlx::SqliteConnection;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::{Db, now};
use crate::dto::{
    ConfigFieldsDto, ConfigValidationErrorResponse, DirectoryConfigDetailDto, UpdateDirectoryConfigRequest, string_map,
};
use crate::entities::DirectoryConfig;
use crate::error::{AppError, AppResult};
use crate::metadata::config_reader::{self, CONFIG_FILE_NAME};
use crate::metadata::inheritance::{self, DirectoryMap};
use crate::metadata::{model_metadata, rules, tags};
use crate::services::app_config::ModelsRoot;
use crate::services::store;

const LOG_TARGET: &str = "findamodel.Indexing";

#[derive(Clone)]
pub struct MetadataConfigService {
    db: Db,
    models_root: ModelsRoot,
    directory_batch_size: usize,
}

/// Tracks loaded records so only changed ones are written back.
struct TrackedConfigs {
    all: DirectoryMap,
    originals: DirectoryMap,
}

impl TrackedConfigs {
    async fn load(conn: &mut SqliteConnection) -> sqlx::Result<Self> {
        let all = store::load_directory_configs(conn).await?;
        Ok(Self { originals: all.clone(), all })
    }

    async fn flush(&mut self, conn: &mut SqliteConnection) -> sqlx::Result<()> {
        // Parents first so new children can reference them.
        let mut paths: Vec<&String> = self.all.keys().collect();
        paths.sort_by_key(|p| p.len());
        for path in paths {
            let record = &self.all[path];
            if self.originals.get(path) != Some(record) {
                store::save_directory_config(conn, record).await?;
            }
        }
        self.originals = self.all.clone();
        Ok(())
    }
}

pub fn config_fields_from_raw(d: &DirectoryConfig) -> ConfigFieldsDto {
    ConfigFieldsDto {
        creator: d.raw_creator.clone(),
        collection: d.raw_collection.clone(),
        subcollection: d.raw_subcollection.clone(),
        tags: Some(tags::from_json(d.raw_tags_json.as_deref())),
        category: d.raw_category.clone(),
        item_type: d.raw_type.clone(),
        material: d.raw_material.clone(),
        supported: d.raw_supported,
        raft_height_mm: d.raw_raft_height_mm,
        model_name: d.raw_model_name.clone(),
        part_name: d.raw_part_name.clone(),
    }
}

pub fn config_fields_from_resolved(d: Option<&DirectoryConfig>) -> ConfigFieldsDto {
    let Some(d) = d else {
        return ConfigFieldsDto { tags: Some(Vec::new()), ..Default::default() };
    };
    ConfigFieldsDto {
        creator: d.creator.clone(),
        collection: d.collection.clone(),
        subcollection: d.subcollection.clone(),
        tags: Some(tags::from_json(d.tags_json.as_deref())),
        category: d.category.clone(),
        item_type: d.item_type.clone(),
        material: d.material.clone(),
        supported: d.supported,
        raft_height_mm: d.raft_height_mm,
        model_name: d.model_name.clone(),
        part_name: d.part_name.clone(),
    }
}

/// Directories under `root` in depth-first pre-order (root first, parents before children),
/// as forward-slash relative paths.
pub fn enumerate_depth_first(root: &Path) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut stack: Vec<PathBuf> = sorted_subdirectories(root).into_iter().rev().collect();
    while let Some(current) = stack.pop() {
        if let Ok(relative) = current.strip_prefix(root) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
        stack.extend(sorted_subdirectories(&current).into_iter().rev());
    }
    out
}

fn sorted_subdirectories(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> =
        entries.filter_map(|e| e.ok()).filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
    dirs.sort();
    dirs
}

impl MetadataConfigService {
    pub fn new(db: Db, models_root: ModelsRoot, directory_batch_size: usize) -> Self {
        Self { db, models_root, directory_batch_size: directory_batch_size.max(1) }
    }

    fn root(&self) -> Option<PathBuf> {
        self.models_root.read().clone()
    }

    /// Scans the whole models tree and syncs directory configs (no model indexing).
    pub async fn sync_directory_configs(&self) -> anyhow::Result<()> {
        let Some(root) = self.root() else {
            warn!(target: LOG_TARGET, "Models directory is not configured");
            return Ok(());
        };
        if !root.is_dir() {
            warn!(target: LOG_TARGET, "Models directory not accessible: {}", root.display());
            return Ok(());
        }
        let walk_root = root.clone();
        let directories = tokio::task::spawn_blocking(move || enumerate_depth_first(&walk_root)).await?;
        let count = self.sync_directory_configs_streaming(&root, &directories).await?;
        info!(target: LOG_TARGET, "Directory config sync complete: {count} directories processed.");
        Ok(())
    }

    /// Syncs records for directories given in depth-first pre-order so inheritance resolves
    /// in a single pass. Returns the number of directories processed.
    pub async fn sync_directory_configs_streaming(&self, root: &Path, directories: &[String]) -> anyhow::Result<usize> {
        let mut conn = self.db.acquire().await?;
        let mut tracked = TrackedConfigs::load(&mut conn).await?;
        let mut changed = HashSet::new();

        for (i, dir_path) in directories.iter().enumerate() {
            if self.sync_single(root, dir_path, &mut tracked.all, true).await? {
                changed.insert(dir_path.clone());
            }
            if (i + 1) % self.directory_batch_size == 0 {
                info!(target: LOG_TARGET, "Directory config sync: processed {} directories so far...", i + 1);
                tracked.flush(&mut conn).await?;
            }
        }
        tracked.flush(&mut conn).await?;

        if !changed.is_empty() {
            let affected = inheritance::expand_to_descendants(&changed, tracked.all.keys());
            recompute_model_caches(&mut conn, root, &affected, &tracked.all).await?;
        }
        Ok(directories.len())
    }

    /// Ensures records exist for every given directory and its ancestors (root first).
    /// Returns all directory configs keyed by path.
    pub async fn ensure_directory_configs(
        &self,
        root: &Path,
        relative_dirs: &[String],
    ) -> anyhow::Result<DirectoryMap> {
        let mut sorted: Vec<String> =
            inheritance::expand_to_all_ancestors(relative_dirs.iter().map(String::as_str)).into_iter().collect();
        sorted.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));

        let mut conn = self.db.acquire().await?;
        let mut tracked = TrackedConfigs::load(&mut conn).await?;
        let mut changed = HashSet::new();
        for dir_path in &sorted {
            if self.sync_single(root, dir_path, &mut tracked.all, true).await? {
                changed.insert(dir_path.clone());
            }
        }
        tracked.flush(&mut conn).await?;

        if !changed.is_empty() {
            let affected = inheritance::expand_to_descendants(&changed, tracked.all.keys());
            recompute_model_caches(&mut conn, root, &affected, &tracked.all).await?;
        }
        Ok(tracked.all)
    }

    /// Syncs one directory's record from its config file. Returns true if the record is new
    /// or its config file hash changed.
    async fn sync_single(
        &self,
        root: &Path,
        dir_path: &str,
        all: &mut DirectoryMap,
        always_resolve: bool,
    ) -> anyhow::Result<bool> {
        let config_path = config_reader::full_dir_path(root, dir_path).join(CONFIG_FILE_NAME);
        let exists = tokio::fs::try_exists(&config_path).await.unwrap_or(false);
        let current_hash = if exists { config_reader::compute_file_hash(&config_path).await.ok() } else { None };

        if let Some(record) = all.get(dir_path) {
            let changed = record.local_config_file_hash != current_hash;
            if changed {
                info!(
                    target: LOG_TARGET,
                    "Config file changed for directory '{dir_path}': {} -> {}",
                    record.local_config_file_hash.as_deref().unwrap_or("(none)"),
                    current_hash.as_deref().unwrap_or("(none)")
                );
                let fields = if exists { config_reader::parse_config_file(&config_path).await } else { None };
                let record = all.get_mut(dir_path).expect("record");
                inheritance::apply_raw_fields(record, fields.as_ref());
                record.local_config_file_hash = current_hash;
                record.updated_at = now();
            }
            if changed || always_resolve {
                inheritance::resolve_fields_at(dir_path, all);
            }
            return Ok(changed);
        }

        let parent_id = inheritance::parent_record(dir_path, all).map(|p| p.id);
        let mut record = DirectoryConfig {
            id: Uuid::new_v4(),
            directory_path: dir_path.to_string(),
            parent_id,
            local_config_file_hash: current_hash,
            updated_at: now(),
            ..Default::default()
        };
        let fields = if exists { config_reader::parse_config_file(&config_path).await } else { None };
        inheritance::apply_raw_fields(&mut record, fields.as_ref());
        inheritance::resolve_fields(&mut record, all);
        all.insert(dir_path.to_string(), record);
        Ok(true)
    }

    pub async fn get_directory_config_detail(&self, dir_path: &str) -> AppResult<DirectoryConfigDetailDto> {
        let mut conn = self.db.acquire().await?;
        let all = store::load_directory_configs(&mut conn).await?;
        Ok(build_detail_dto(dir_path, all.get(dir_path), &all))
    }

    /// Writes the directory's findamodel.yaml, updates its record, re-resolves descendants
    /// and recomputes cached model metadata below it.
    pub async fn update_directory_config(
        &self,
        dir_path: &str,
        req: &UpdateDirectoryConfigRequest,
    ) -> AppResult<DirectoryConfigDetailDto> {
        let root =
            self.root().ok_or_else(|| AppError::Internal(anyhow::anyhow!("Models directory is not configured.")))?;

        if let Some(field_rules) = &req.field_rules {
            let as_strings: Vec<(String, String)> =
                field_rules.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect();
            let errors = rules::validate_rules(as_strings.iter().map(|(k, v)| (k, v)));
            if !errors.is_empty() {
                let body = ConfigValidationErrorResponse { field_errors: string_map(errors) };
                return Err(AppError::UnprocessableJson(serde_json::to_value(body).expect("serialising errors")));
            }
        }

        let dir_full = config_reader::full_dir_path(&root, dir_path);
        if !dir_full.is_dir() {
            return Err(AppError::NotFound(format!("Directory not found: {dir_path}")));
        }

        config_reader::write_config_file(&root, dir_path, req).await?;
        let config_path = dir_full.join(CONFIG_FILE_NAME);
        let exists = config_path.exists();
        let new_hash = if exists { Some(config_reader::compute_file_hash(&config_path).await?) } else { None };

        let mut conn = self.db.acquire().await?;
        let mut tracked = TrackedConfigs::load(&mut conn).await?;
        if !tracked.all.contains_key(dir_path) {
            let parent_id = inheritance::parent_record(dir_path, &tracked.all).map(|p| p.id);
            tracked.all.insert(
                dir_path.to_string(),
                DirectoryConfig {
                    id: Uuid::new_v4(),
                    directory_path: dir_path.to_string(),
                    parent_id,
                    updated_at: now(),
                    ..Default::default()
                },
            );
        }

        let fields = if exists { config_reader::parse_config_file(&config_path).await } else { None };
        {
            let record = tracked.all.get_mut(dir_path).expect("record");
            inheritance::apply_raw_fields(record, fields.as_ref());
            record.local_config_file_hash = new_hash;
            record.updated_at = now();
        }
        inheritance::resolve_fields_at(dir_path, &mut tracked.all);
        inheritance::resolve_descendants(dir_path, &mut tracked.all);
        tracked.flush(&mut conn).await?;

        let affected: Vec<String> = tracked
            .all
            .keys()
            .filter(|k| dir_path.is_empty() || *k == dir_path || k.starts_with(&format!("{dir_path}/")))
            .cloned()
            .collect();
        recompute_model_caches(&mut conn, &root, &affected, &tracked.all).await?;

        Ok(build_detail_dto(dir_path, tracked.all.get(dir_path), &tracked.all))
    }
}

/// Recomputes calculated metadata for every model in the given directories.
pub async fn recompute_model_caches(
    conn: &mut SqliteConnection,
    root: &Path,
    affected: &[String],
    all: &DirectoryMap,
) -> sqlx::Result<()> {
    if affected.is_empty() {
        return Ok(());
    }
    let models = store::models_in_directories(conn, affected).await?;
    for mut model in models {
        let full_path = inheritance::build_full_file_path(root, &model.directory, &model.file_name);
        let metadata = model_metadata::compute(&full_path.to_string_lossy(), all.get(&model.directory));
        let before = model.clone();
        store::apply_calculated_metadata(&mut model, &metadata);
        if model != before {
            store::save_model(conn, &model).await?;
        }
    }
    Ok(())
}

fn rule_snippets(rules_yaml: Option<&str>, skip: &rules::RuleSet) -> Option<Vec<(String, String)>> {
    let parsed = rules::deserialize_rules(rules_yaml);
    let snippets: Vec<(String, String)> = parsed
        .iter()
        .filter(|(field, _)| !skip.contains_key(*field))
        .map(|(field, rule)| (field.to_ascii_lowercase(), rules::rule_contents_yaml(rule)))
        .collect();
    (!snippets.is_empty()).then_some(snippets)
}

pub fn build_detail_dto(
    dir_path: &str,
    record: Option<&DirectoryConfig>,
    all: &DirectoryMap,
) -> DirectoryConfigDetailDto {
    let local_values = record.map(config_fields_from_raw).unwrap_or_default();
    let parent_path = inheritance::parent_path(dir_path);
    let parent_record = parent_path.as_ref().and_then(|p| all.get(p));
    let parent_resolved_values = parent_record.map(|p| config_fields_from_resolved(Some(p)));

    let local_rules = rules::deserialize_rules(record.and_then(|r| r.raw_rules_yaml.as_deref()));
    let (local_rule_fields, local_rule_contents) = if local_rules.is_empty() {
        (None, None)
    } else {
        let fields: Vec<String> = local_rules.keys().map(|k| k.to_ascii_lowercase()).collect();
        let contents = rule_snippets(record.and_then(|r| r.raw_rules_yaml.as_deref()), &rules::RuleSet::new());
        (Some(fields), contents.map(string_map))
    };

    let parent_resolved_rules =
        parent_record.and_then(|p| rule_snippets(p.resolved_rules_yaml.as_deref(), &local_rules)).map(string_map);

    DirectoryConfigDetailDto {
        directory_path: dir_path.to_string(),
        local_values,
        parent_resolved_values,
        parent_path,
        local_rule_fields,
        local_rule_contents,
        parent_resolved_rules,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDb, model, write_file};
    use parking_lot::RwLock;
    use std::sync::Arc;

    fn service(t: &TestDb, root: &Path) -> MetadataConfigService {
        MetadataConfigService::new(t.db.clone(), Arc::new(RwLock::new(Some(root.to_path_buf()))), 1000)
    }

    #[test]
    fn depth_first_order_lists_parents_before_children() {
        let dir = tempfile::tempdir().unwrap();
        for d in ["b/x", "a/y/z", "a/c"] {
            std::fs::create_dir_all(dir.path().join(d)).unwrap();
        }
        assert_eq!(enumerate_depth_first(dir.path()), vec!["", "a", "a/c", "a/y", "a/y/z", "b", "b/x"]);
    }

    #[tokio::test]
    async fn sync_resolves_inheritance_and_recomputes_models() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "findamodel.yaml", "creator: Alice\ntags:\n  - root\n");
        write_file(
            root.path(),
            "Fantasy/findamodel.yaml",
            "collection: Fantasy\nmodel_name:\n  source: filename\n  expression: '^(.*)\\.[^./]+$'\n",
        );
        write_file(root.path(), "Fantasy/dragon.stl", "x");
        let mut m = model("dragon.stl", "Fantasy");
        m.calculated_creator = None;
        t.insert_model(&m).await;

        let svc = service(&t, root.path());
        svc.sync_directory_configs().await.unwrap();

        let fantasy = store::get_directory_config(&t.db, "Fantasy").await.unwrap().unwrap();
        assert_eq!(fantasy.creator.as_deref(), Some("Alice"));
        assert_eq!(fantasy.collection.as_deref(), Some("Fantasy"));
        assert!(fantasy.resolved_rules_yaml.unwrap().contains("model_name"));

        let updated = store::get_model(&t.db, m.id).await.unwrap().unwrap();
        assert_eq!(updated.calculated_creator.as_deref(), Some("Alice"));
        assert_eq!(updated.calculated_model_name.as_deref(), Some("dragon"));
        assert_eq!(tags::from_json(updated.calculated_tags_json.as_deref()), vec!["root"]);
        let tag_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM model_tags WHERE model_id = ?")
            .bind(m.id)
            .fetch_one(&t.db)
            .await
            .unwrap();
        assert_eq!(tag_rows, 1);
    }

    #[tokio::test]
    async fn update_directory_config_writes_yaml_and_propagates() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("A/B")).unwrap();
        let svc = service(&t, root.path());
        svc.sync_directory_configs().await.unwrap();

        let req = UpdateDirectoryConfigRequest { creator: Some("Bob".into()), ..Default::default() };
        let detail = svc.update_directory_config("A", &req).await.unwrap();
        assert_eq!(detail.local_values.creator.as_deref(), Some("Bob"));
        assert!(root.path().join("A/findamodel.yaml").exists());

        let child = svc.get_directory_config_detail("A/B").await.unwrap();
        assert_eq!(child.parent_resolved_values.unwrap().creator.as_deref(), Some("Bob"));
        let b = store::get_directory_config(&t.db, "A/B").await.unwrap().unwrap();
        assert_eq!(b.creator.as_deref(), Some("Bob"));
    }

    #[tokio::test]
    async fn update_directory_config_rejects_invalid_rules() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        let svc = service(&t, root.path());
        let mut field_rules = serde_json::Map::new();
        field_rules.insert("creator".into(), serde_json::Value::String("rule: magic".into()));
        let req = UpdateDirectoryConfigRequest { field_rules: Some(field_rules), ..Default::default() };
        match svc.update_directory_config("", &req).await {
            Err(AppError::UnprocessableJson(body)) => {
                assert!(body["fieldErrors"]["creator"].as_str().unwrap().contains("magic"))
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn detail_lists_local_rules_and_inherited_rules() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "findamodel.yaml", "creator:\n  source: folder\n  expression: '^([^/]+)'\n");
        write_file(root.path(), "A/findamodel.yaml", "collection:\n  source: folder\n  expression: x\n");
        let svc = service(&t, root.path());
        svc.sync_directory_configs().await.unwrap();

        let detail = svc.get_directory_config_detail("A").await.unwrap();
        assert_eq!(detail.local_rule_fields, Some(vec!["collection".to_string()]));
        assert!(detail.local_rule_contents.unwrap()["collection"].as_str().unwrap().contains("source: folder"));
        assert!(detail.parent_resolved_rules.unwrap().contains_key("creator"));
    }
}
