//! Directory browsing: lists folders, model files and previewable files at a path, with
//! resolved metadata and rule information from the directory configs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::db::{Db, param_chunks, placeholders};
use crate::dto::{
    ConfigFieldsDto, ExplorerFileItemDto, ExplorerModelItemDto, ExplorerResponseDto, FolderItemDto, string_map,
};
use crate::entities::{DirectoryConfig, ModelRow};
use crate::error::{AppError, AppResult};
use crate::metadata::inheritance;
use crate::metadata::model_metadata::{self, ComputedMetadata};
use crate::metadata::rules::{self, RuleSet};
use crate::metadata::tags;
use crate::services::metadata_config::{config_fields_from_raw, config_fields_from_resolved};
use crate::services::models::is_model_extension;
use crate::services::store;

pub const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
pub const TEXT_EXTENSIONS: [&str; 2] = ["txt", "md"];

fn extension(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

pub fn is_previewable_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext) || TEXT_EXTENSIONS.contains(&ext)
}

struct Listing {
    dirs: Vec<String>,
    files: Vec<(String, u64)>,
}

fn list_dir(path: &Path) -> std::io::Result<Listing> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path)?.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            dirs.push(name);
        } else if file_type.is_file() {
            files.push((name, entry.metadata().map(|m| m.len()).unwrap_or(0)));
        }
    }
    dirs.sort_by(|a, b| tags::cmp_ignore_case(a, b));
    files.sort_by(|a, b| tags::cmp_ignore_case(&a.0, &b.0));
    Ok(Listing { dirs, files })
}

fn count_entries(path: &Path) -> (i64, i64) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return (0, 0);
    };
    let (mut dirs, mut models) = (0, 0);
    for entry in entries.flatten() {
        let Ok(t) = entry.file_type() else { continue };
        if t.is_dir() {
            dirs += 1;
        } else if t.is_file() && is_model_extension(&extension(&entry.file_name().to_string_lossy())) {
            models += 1;
        }
    }
    (dirs, models)
}

fn to_fields(m: ComputedMetadata) -> ConfigFieldsDto {
    ConfigFieldsDto {
        creator: m.creator,
        collection: m.collection,
        subcollection: m.subcollection,
        tags: Some(m.tags),
        category: m.category,
        item_type: m.item_type,
        material: m.material,
        supported: m.supported,
        raft_height_mm: m.raft_height_mm,
        model_name: m.model_name,
        part_name: m.part_name,
    }
}

fn to_plain(f: &ConfigFieldsDto) -> ComputedMetadata {
    ComputedMetadata {
        creator: f.creator.clone(),
        collection: f.collection.clone(),
        subcollection: f.subcollection.clone(),
        tags: f.tags.clone().unwrap_or_default(),
        category: f.category.clone(),
        item_type: f.item_type.clone(),
        material: f.material.clone(),
        supported: f.supported,
        raft_height_mm: f.raft_height_mm,
        model_name: f.model_name.clone(),
        part_name: f.part_name.clone(),
    }
}

fn rule_configs(rules_yaml: Option<&str>) -> Option<crate::dto::StringMap> {
    let rules = rules::deserialize_rules(rules_yaml);
    (!rules.is_empty()).then(|| {
        string_map(
            rules
                .iter()
                .map(|(field, rule)| (field.to_ascii_lowercase(), rules::rule_config_to_yaml_snippet(field, rule))),
        )
    })
}

fn local_rule_fields(raw_rules_yaml: Option<&str>) -> Option<Vec<String>> {
    let rules = rules::deserialize_rules(raw_rules_yaml);
    (!rules.is_empty()).then(|| rules.keys().map(|k| k.to_ascii_lowercase()).collect())
}

/// Effective metadata for a model file in a directory, or `None` when nothing applies.
fn model_metadata_for(
    dc: Option<&DirectoryConfig>,
    rules: &RuleSet,
    full_path: &str,
) -> (Option<ConfigFieldsDto>, Option<crate::dto::StringMap>) {
    if dc.is_none() && rules.is_empty() {
        return (None, None);
    }
    let (resolved, configs) =
        model_metadata::resolve_for_path(ComputedMetadata::from_directory(dc), rules, full_path, true);
    if resolved == ComputedMetadata::default() {
        return (None, None);
    }
    (Some(to_fields(resolved)), configs.map(string_map))
}

async fn directory_configs_for(db: &Db, paths: &[String]) -> AppResult<HashMap<String, DirectoryConfig>> {
    let mut out = HashMap::new();
    for chunk in param_chunks(paths) {
        let sql = format!("SELECT * FROM directory_configs WHERE directory_path IN ({})", placeholders(chunk.len()));
        let mut q = sqlx::query_as::<_, DirectoryConfig>(&sql);
        for p in chunk {
            q = q.bind(p);
        }
        for row in q.fetch_all(db).await? {
            out.insert(row.directory_path.clone(), row);
        }
    }
    Ok(out)
}

/// Lists folders (sorted), model files (sorted) and previewable files at `relative_path`.
pub async fn get_directory_contents(db: &Db, root: &Path, relative_path: &str) -> AppResult<ExplorerResponseDto> {
    if relative_path.split('/').any(|s| s == "..") {
        return Err(AppError::NotFound(format!("Directory not found: {relative_path}")));
    }
    let full: PathBuf = if relative_path.is_empty() { root.to_path_buf() } else { root.join(relative_path) };
    if !full.is_dir() {
        return Err(AppError::NotFound(format!("Directory not found: {relative_path}")));
    }

    let listing = {
        let full = full.clone();
        tokio::task::spawn_blocking(move || list_dir(&full)).await.map_err(anyhow::Error::from)??
    };
    let child_paths: Vec<String> = listing
        .dirs
        .iter()
        .map(|n| if relative_path.is_empty() { n.clone() } else { format!("{relative_path}/{n}") })
        .collect();
    let model_files: Vec<&(String, u64)> =
        listing.files.iter().filter(|(n, _)| is_model_extension(&extension(n))).collect();
    let previewable: Vec<&(String, u64)> =
        listing.files.iter().filter(|(n, _)| is_previewable_extension(&extension(n))).collect();

    let child_configs = directory_configs_for(db, &child_paths).await?;
    let current = store::get_directory_config(db, relative_path).await?;

    let model_names: Vec<String> = model_files.iter().map(|(n, _)| n.clone()).collect();
    let mut cached: HashMap<String, ModelRow> = HashMap::new();
    for chunk in param_chunks(&model_names) {
        let sql = format!("SELECT * FROM models WHERE directory = ? AND file_name IN ({})", placeholders(chunk.len()));
        let mut q = sqlx::query_as::<_, ModelRow>(&sql).bind(relative_path);
        for n in chunk {
            q = q.bind(n);
        }
        for row in q.fetch_all(db).await? {
            cached.insert(row.file_name.clone(), row);
        }
    }

    let mut folders = Vec::with_capacity(listing.dirs.len());
    for (name, child_path) in listing.dirs.iter().zip(&child_paths) {
        let child_full = full.join(name);
        let (subdirectory_count, model_count) = count_entries(&child_full);
        let dc = child_configs.get(child_path);
        let mut resolved = match dc {
            Some(d) => config_fields_from_resolved(Some(d)),
            None => config_fields_from_resolved(current.as_ref()),
        };
        let effective_rules_yaml = match dc {
            Some(d) => d.resolved_rules_yaml.clone(),
            None => current.as_ref().and_then(|c| c.resolved_rules_yaml.clone()),
        };
        let folder_rules = rules::deserialize_rules(effective_rules_yaml.as_deref());
        if !folder_rules.is_empty() {
            let (applied, _) = model_metadata::resolve_for_path(
                to_plain(&resolved),
                &folder_rules,
                &child_full.to_string_lossy(),
                false,
            );
            resolved = to_fields(applied);
        }
        folders.push(FolderItemDto {
            name: name.clone(),
            path: child_path.clone(),
            subdirectory_count,
            model_count,
            resolved_values: resolved,
            rule_configs: rule_configs(effective_rules_yaml.as_deref()),
            local_values: Some(dc.map(config_fields_from_raw).unwrap_or_default()),
            local_rule_fields: local_rule_fields(dc.and_then(|d| d.raw_rules_yaml.as_deref())),
        });
    }

    let current_rules = rules::deserialize_rules(current.as_ref().and_then(|c| c.resolved_rules_yaml.as_deref()));
    let models = model_files
        .iter()
        .map(|(name, size)| {
            let relative = if relative_path.is_empty() { name.clone() } else { format!("{relative_path}/{name}") };
            let cm = cached.get(name);
            let full_path = inheritance::build_full_file_path(root, relative_path, name);
            let (resolved_metadata, rule_configs) =
                model_metadata_for(current.as_ref(), &current_rules, &full_path.to_string_lossy());
            ExplorerModelItemDto {
                id: cm.map(|m| m.id.to_string()),
                file_name: name.clone(),
                relative_path: relative,
                file_type: extension(name),
                file_size: Some(cm.map_or(*size as i64, |m| m.file_size)),
                has_preview: cm.is_some_and(|m| m.preview_image_path.is_some()),
                preview_url: cm.and_then(|m| {
                    m.preview_image_path.as_deref().map(|p| store::preview_url(m.id, m.preview_generation_version, p))
                }),
                resolved_metadata,
                rule_configs,
            }
        })
        .collect();

    let files = previewable
        .iter()
        .map(|(name, size)| ExplorerFileItemDto {
            file_name: name.clone(),
            relative_path: if relative_path.is_empty() { name.clone() } else { format!("{relative_path}/{name}") },
            file_type: extension(name),
            file_size: *size as i64,
        })
        .collect();

    Ok(ExplorerResponseDto {
        current_path: relative_path.to_string(),
        parent_path: inheritance::parent_path(relative_path),
        folders,
        models,
        files,
    })
}

/// Resolves an explorer file path inside the models root, rejecting traversal and
/// non-previewable types.
pub fn resolve_previewable_file(root: &Path, relative: &str) -> AppResult<(PathBuf, &'static str)> {
    if relative.trim().is_empty() {
        return Err(AppError::BadRequest("File path is required.".into()));
    }
    let full =
        std::path::absolute(root.join(relative)).map_err(|_| AppError::BadRequest("Invalid file path.".into()))?;
    let root_full = std::path::absolute(root).map_err(|_| AppError::BadRequest("Invalid file path.".into()))?;
    if relative.split(['/', '\\']).any(|s| s == "..") || !full.starts_with(&root_full) {
        return Err(AppError::BadRequest("Invalid file path.".into()));
    }
    let content_type = match extension(relative).as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => return Err(AppError::BadRequest("Unsupported file type.".into())),
    };
    if !full.is_file() {
        return Err(AppError::NotFound(String::new()));
    }
    Ok((full, content_type))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::metadata_config::MetadataConfigService;
    use crate::test_support::{TestDb, model, write_file};
    use parking_lot::RwLock;
    use std::sync::Arc;

    #[tokio::test]
    async fn lists_folders_models_and_files_with_metadata() {
        let t = TestDb::new().await;
        let root = tempfile::tempdir().unwrap();
        write_file(
            root.path(),
            "findamodel.yaml",
            "creator: Alice\nmodel_name:\n  source: filename\n  expression: '^(.*)\\.[^./]+$'\n",
        );
        write_file(root.path(), "Elves/archer.stl", "x");
        write_file(root.path(), "Elves/sub/x.obj", "x");
        write_file(root.path(), "dragon.stl", "x");
        write_file(root.path(), "readme.md", "hello");
        write_file(root.path(), "ignored.bin", "x");
        let svc =
            MetadataConfigService::new(t.db.clone(), Arc::new(RwLock::new(Some(root.path().to_path_buf()))), 1000);
        svc.sync_directory_configs().await.unwrap();
        let mut cached = model("dragon.stl", "");
        cached.preview_image_path = Some("p.png".into());
        t.insert_model(&cached).await;

        let response = get_directory_contents(&t.db, root.path(), "").await.unwrap();
        assert_eq!(response.parent_path, None);
        assert_eq!(response.folders.len(), 1);
        let elves = &response.folders[0];
        assert_eq!((elves.name.as_str(), elves.subdirectory_count, elves.model_count), ("Elves", 1, 1));
        assert_eq!(elves.resolved_values.creator.as_deref(), Some("Alice"));
        assert!(elves.rule_configs.as_ref().unwrap().contains_key("model_name"));

        assert_eq!(response.models.len(), 1);
        let dragon = &response.models[0];
        assert_eq!(dragon.id, Some(cached.id.to_string()));
        assert!(dragon.has_preview);
        let meta = dragon.resolved_metadata.as_ref().unwrap();
        assert_eq!(meta.model_name.as_deref(), Some("dragon"));
        assert_eq!(meta.creator.as_deref(), Some("Alice"));

        assert_eq!(response.files.iter().map(|f| f.file_name.as_str()).collect::<Vec<_>>(), vec!["readme.md"]);

        let nested = get_directory_contents(&t.db, root.path(), "Elves").await.unwrap();
        assert_eq!(nested.parent_path.as_deref(), Some(""));
        assert_eq!(nested.models[0].id, None);
        assert!(matches!(get_directory_contents(&t.db, root.path(), "missing").await, Err(AppError::NotFound(_))));
    }

    #[test]
    fn previewable_file_resolution_rejects_traversal_and_unsupported_types() {
        let root = tempfile::tempdir().unwrap();
        write_file(root.path(), "a/pic.PNG", "x");
        write_file(root.path(), "a/model.stl", "x");
        assert_eq!(resolve_previewable_file(root.path(), "a/pic.PNG").unwrap().1, "image/png");
        assert!(matches!(resolve_previewable_file(root.path(), "../secret.png"), Err(AppError::BadRequest(_))));
        assert!(matches!(resolve_previewable_file(root.path(), "a/model.stl"), Err(AppError::BadRequest(_))));
        assert!(matches!(resolve_previewable_file(root.path(), "a/missing.png"), Err(AppError::NotFound(_))));
        assert!(matches!(resolve_previewable_file(root.path(), ""), Err(AppError::BadRequest(_))));
    }
}
