//! Directory browsing: lists folders, model files and previewable files at a path, with
//! resolved metadata and rule information from the directory configs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::db::{Db, param_chunks, placeholders};
use crate::dto::{
    ConfigFieldsDto, ExplorerFileItemDto, ExplorerModelItemDto, ExplorerResponseDto, FolderItemDto, RulePreviewDto,
    RulePreviewSampleDto, RulePreviewValueDto, string_map,
};
use crate::entities::{DirectoryConfig, ModelRow};
use crate::error::{AppError, AppResult};
use crate::metadata::inheritance;
use crate::metadata::model_metadata::{self, ComputedMetadata};
use crate::metadata::rules::{self, RuleSet};
use crate::metadata::{fields, tags, yaml};
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
    path: &str,
) -> (Option<ConfigFieldsDto>, Option<crate::dto::StringMap>) {
    if dc.is_none() && rules.is_empty() {
        return (None, None);
    }
    let (resolved, configs) = model_metadata::resolve_for_path(ComputedMetadata::from_directory(dc), rules, path, true);
    if resolved == ComputedMetadata::default() {
        return (None, None);
    }
    (Some(to_fields(resolved)), configs.map(string_map))
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

    // Child counts are read here too, so a slow disk (e.g. one spinning up) blocks a blocking
    // thread rather than the async runtime.
    let (listing, child_counts) = {
        let full = full.clone();
        tokio::task::spawn_blocking(move || {
            list_dir(&full).map(|listing| {
                let counts: Vec<(i64, i64)> = listing.dirs.iter().map(|n| count_entries(&full.join(n))).collect();
                (listing, counts)
            })
        })
        .await
        .map_err(anyhow::Error::from)??
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

    let child_configs = {
        let mut conn = db.acquire().await?;
        store::load_directory_configs_at(&mut conn, &child_paths).await?
    };
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
    for ((name, child_path), (subdirectory_count, model_count)) in
        listing.dirs.iter().zip(&child_paths).zip(child_counts)
    {
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
                &rules::rule_path(relative_path, name),
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
            let (resolved_metadata, rule_configs) =
                model_metadata_for(current.as_ref(), &current_rules, &rules::rule_path(relative_path, name));
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

/// Most models a rule preview runs against; a library-wide rule still answers quickly.
pub const RULE_PREVIEW_MODEL_LIMIT: i64 = 20_000;
const RULE_PREVIEW_VALUE_LIMIT: usize = 10;
const RULE_PREVIEW_MATCHED_SAMPLES: usize = 4;
const RULE_PREVIEW_UNMATCHED_SAMPLES: usize = 2;

/// Runs one field's rule over the indexed models under `dir_path` (up to
/// `RULE_PREVIEW_MODEL_LIMIT`) and summarises what it produces. Invalid rules come back as
/// `error` rather than failing the request, since the preview runs while the rule is typed.
pub async fn preview_rule(db: &Db, dir_path: &str, field: &str, rule_yaml: &str) -> AppResult<RulePreviewDto> {
    let Some(definition) = fields::try_get(field) else {
        return Err(AppError::BadRequest(format!("Unknown field: {field}")));
    };
    let key = definition.key;
    let invalid = |error: String| Ok(RulePreviewDto { error: Some(error), ..Default::default() });

    let rule_yaml = rule_yaml.trim();
    if rule_yaml.is_empty() {
        return Ok(RulePreviewDto::default());
    }
    let named = [(key.to_string(), rule_yaml.to_string())];
    if let Some((_, error)) = rules::validate_rules(named.iter().map(|(k, v)| (k, v))).into_iter().next() {
        return invalid(error);
    }
    let rule = match yaml::parse_mapping(rule_yaml) {
        Ok(mapping) => yaml::to_loose_json(&serde_yaml::Value::Mapping(mapping)),
        Err(err) => return invalid(format!("Invalid YAML: {err}")),
    };
    if let Some(error) = rules::pattern_error(&rule) {
        return invalid(format!("Invalid regex: {}.", error.trim_end_matches('.')));
    }
    let rule_set: RuleSet = [(key.to_string(), rule)].into_iter().collect();

    // Rows come back in (directory, file_name) order from the unique index; the range on
    // `directory` covers every descendant ('0' is the character after '/').
    let dir_path = dir_path.trim_matches('/');
    let mut rows: Vec<(String, String)> = if dir_path.is_empty() {
        sqlx::query_as("SELECT directory, file_name FROM models ORDER BY directory, file_name LIMIT ?")
            .bind(RULE_PREVIEW_MODEL_LIMIT + 1)
            .fetch_all(db)
            .await?
    } else {
        sqlx::query_as(
            "SELECT directory, file_name FROM models
             WHERE directory = ? OR (directory >= ? AND directory < ?)
             ORDER BY directory, file_name LIMIT ?",
        )
        .bind(dir_path)
        .bind(format!("{dir_path}/"))
        .bind(format!("{dir_path}0"))
        .bind(RULE_PREVIEW_MODEL_LIMIT + 1)
        .fetch_all(db)
        .await?
    };
    let truncated = rows.len() as i64 > RULE_PREVIEW_MODEL_LIMIT;
    rows.truncate(RULE_PREVIEW_MODEL_LIMIT as usize);

    let mut counts: HashMap<String, i64> = HashMap::new();
    let mut matched_count = 0;
    let mut matched_samples = Vec::new();
    let mut unmatched_samples = Vec::new();
    let mut last_sample_dir: [Option<&str>; 2] = [None, None];
    for (directory, file_name) in &rows {
        let path = rules::rule_path(directory, file_name);
        let (computed, _) = model_metadata::resolve_for_path(ComputedMetadata::default(), &rule_set, &path, false);
        let values = computed.rule_field_values(key);
        let matched = !values.is_empty();
        if matched {
            matched_count += 1;
        }
        for value in &values {
            *counts.entry(value.clone()).or_default() += 1;
        }

        // One sample per folder, so the samples show how the rule varies across the tree.
        let (samples, limit, last_dir) = if matched {
            (&mut matched_samples, RULE_PREVIEW_MATCHED_SAMPLES, &mut last_sample_dir[0])
        } else {
            (&mut unmatched_samples, RULE_PREVIEW_UNMATCHED_SAMPLES, &mut last_sample_dir[1])
        };
        if samples.len() < limit && *last_dir != Some(directory.as_str()) {
            *last_dir = Some(directory.as_str());
            let relative_path =
                if directory.is_empty() { file_name.clone() } else { format!("{directory}/{file_name}") };
            samples.push(RulePreviewSampleDto { relative_path, values });
        }
    }

    let mut values: Vec<RulePreviewValueDto> =
        counts.into_iter().map(|(value, count)| RulePreviewValueDto { value, count }).collect();
    values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| tags::cmp_ignore_case(&a.value, &b.value)));
    values.truncate(RULE_PREVIEW_VALUE_LIMIT);

    Ok(RulePreviewDto {
        model_count: rows.len() as i64,
        truncated,
        matched_count,
        values,
        samples: matched_samples.into_iter().chain(unmatched_samples).collect(),
        error: None,
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

    #[tokio::test]
    async fn rule_preview_summarises_values_for_models_under_the_folder() {
        let t = TestDb::new().await;
        for (name, dir) in [
            ("a.stl", "Aradia/ellis"),
            ("b.stl", "Aradia/ellis/parts"),
            ("c.stl", "Aradia/dragon"),
            ("loose.stl", "Aradia"),
            ("other.stl", "Aradia0"),
            ("x.stl", "Bulkamancer/orc"),
        ] {
            t.insert_model(&model(name, dir)).await;
        }

        let preview = preview_rule(&t.db, "Aradia", "model_name", "expression: ^/[^/]+/([^/]+)/").await.unwrap();
        assert_eq!((preview.model_count, preview.matched_count, preview.truncated), (4, 3, false));
        assert_eq!(
            preview.values,
            vec![
                RulePreviewValueDto { value: "ellis".into(), count: 2 },
                RulePreviewValueDto { value: "dragon".into(), count: 1 },
            ]
        );
        // One sample per folder, matched first.
        let samples: Vec<(&str, Vec<String>)> =
            preview.samples.iter().map(|s| (s.relative_path.as_str(), s.values.clone())).collect();
        assert_eq!(
            samples,
            vec![
                ("Aradia/dragon/c.stl", vec!["dragon".to_string()]),
                ("Aradia/ellis/a.stl", vec!["ellis".to_string()]),
                ("Aradia/ellis/parts/b.stl", vec!["ellis".to_string()]),
                ("Aradia/loose.stl", vec![]),
            ]
        );

        let all = preview_rule(&t.db, "", "supported", "expression: ellis").await.unwrap();
        assert_eq!((all.model_count, all.matched_count), (6, 2));
        assert_eq!(all.values, vec![RulePreviewValueDto { value: "true".into(), count: 2 }]);

        let tags = preview_rule(&t.db, "", "tags", "values:\n  Elf: ellis\n  Big: dragon|orc").await.unwrap();
        assert_eq!(tags.matched_count, 4);
        assert_eq!(
            tags.values.iter().map(|v| (v.value.as_str(), v.count)).collect::<Vec<_>>(),
            [("Big", 2), ("Elf", 2)]
        );
    }

    #[tokio::test]
    async fn rule_preview_reports_invalid_rules_as_errors() {
        let t = TestDb::new().await;
        t.insert_model(&model("a.stl", "A")).await;

        let empty = preview_rule(&t.db, "", "model_name", "  ").await.unwrap();
        assert_eq!(empty, RulePreviewDto::default());
        for rule in ["expression: ([^/]+", "rule: filename", "expression: [unquoted"] {
            let preview = preview_rule(&t.db, "", "model_name", rule).await.unwrap();
            assert!(preview.error.is_some(), "{rule}");
            assert_eq!(preview.model_count, 0);
        }
        assert!(matches!(preview_rule(&t.db, "", "nope", "expression: x").await, Err(AppError::BadRequest(_))));
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
