//! Reading and writing findamodel.yaml directory config files: YAML parsing, file hashing
//! and rule extraction.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};
use tracing::warn;

use super::fields;
use super::model_metadata::{self, ModelMetadataEntry};
use super::tags;
use super::yaml::{self, get_ci, scalar_to_string};

const LOG_TARGET: &str = "findamodel.Config";
pub const CONFIG_FILE_NAME: &str = "findamodel.yaml";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawConfigFields {
    pub creator: Option<String>,
    pub collection: Option<String>,
    pub subcollection: Option<String>,
    pub tags: Option<Vec<String>>,
    pub category: Option<String>,
    pub item_type: Option<String>,
    pub material: Option<String>,
    pub supported: Option<bool>,
    pub raft_height_mm: Option<f32>,
    pub model_name: Option<String>,
    pub part_name: Option<String>,
    pub rules_yaml: Option<String>,
    pub model_metadata_json: Option<String>,
}

/// Request body for replacing a directory's local metadata config.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDirectoryConfigRequest {
    pub creator: Option<String>,
    pub collection: Option<String>,
    pub subcollection: Option<String>,
    pub tags: Option<Vec<String>>,
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    pub material: Option<String>,
    pub supported: Option<bool>,
    pub raft_height_mm: Option<f32>,
    pub model_name: Option<String>,
    pub part_name: Option<String>,
    /// When present, fully defines the rule set for the directory. Maps a YAML field name
    /// to the inner rule YAML. Plain values take precedence over rules.
    pub field_rules: Option<serde_json::Map<String, serde_json::Value>>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub async fn compute_file_hash(path: &Path) -> Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<String> {
        use std::io::Read;
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(hex::encode(hasher.finalize()))
    })
    .await?
}

fn non_mapping_scalar(value: &Value) -> Option<&Value> {
    match value {
        Value::Mapping(_) | Value::Sequence(_) => None,
        other => Some(other),
    }
}

fn try_get_string(data: &Mapping, key: &str) -> Option<String> {
    let (_, value) = get_ci(data, key)?;
    let s = scalar_to_string(non_mapping_scalar(value)?)?;
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn try_get_bool(data: &Mapping, key: &str) -> Option<bool> {
    let (_, value) = get_ci(data, key)?;
    match non_mapping_scalar(value)? {
        Value::Bool(b) => Some(*b),
        other => scalar_to_string(other).and_then(|s| model_metadata::parse_bool(&s)),
    }
}

fn try_get_float(data: &Mapping, key: &str) -> Option<f32> {
    let (_, value) = get_ci(data, key)?;
    let s = scalar_to_string(non_mapping_scalar(value)?)?;
    s.trim().parse::<f32>().ok().filter(|f| f.is_finite())
}

fn try_get_string_list(data: &Mapping, key: &str) -> Option<Vec<String>> {
    match get_ci(data, key)? {
        (_, Value::Sequence(items)) => {
            Some(tags::normalize(items.iter().map(|v| scalar_to_string(v).unwrap_or_default())))
        }
        _ => None,
    }
}

/// A rule object needs at least one of the known rule keys.
pub fn looks_like_rule_object(value: &Value) -> bool {
    let Value::Mapping(m) = value else {
        return false;
    };
    !m.is_empty() && ["rule", "expression", "values", "source"].iter().any(|k| yaml::string_key(m, k).is_some())
}

fn extract_rules_yaml(data: &Mapping) -> Option<String> {
    let mut rules = Mapping::new();
    for (key, value) in data {
        let Some(key) = key.as_str() else { continue };
        let Some(field) = fields::try_get(key) else { continue };
        if looks_like_rule_object(value) {
            rules.insert(Value::String(field.key.to_string()), value.clone());
        }
    }
    (!rules.is_empty()).then(|| yaml::to_string(&Value::Mapping(rules)))
}

fn extract_model_metadata_json(data: &Mapping) -> Option<String> {
    let Some(Value::Mapping(section)) = yaml::string_key(data, "model_metadata") else {
        return None;
    };
    let mut entries: Vec<(String, ModelMetadataEntry)> = Vec::new();
    for (key, value) in section {
        let (Some(file_name), Value::Mapping(entry)) = (key.as_str(), value) else { continue };
        if let Some(parsed) = ModelMetadataEntry::from_yaml(entry) {
            entries.retain(|(k, _)| !tags::eq_ignore_case(k, file_name));
            entries.push((file_name.to_string(), parsed));
        }
    }
    model_metadata::entries_to_storage_json(&entries)
}

pub fn parse_config_text(text: &str) -> Result<RawConfigFields> {
    let data = yaml::parse_mapping(text)?;
    Ok(RawConfigFields {
        creator: try_get_string(&data, "creator"),
        collection: try_get_string(&data, "collection"),
        subcollection: try_get_string(&data, "subcollection"),
        tags: try_get_string_list(&data, "tags"),
        category: fields::validate_enum_value(try_get_string(&data, "category").as_deref()),
        item_type: fields::validate_enum_value(try_get_string(&data, "type").as_deref()),
        material: fields::validate_enum_value(try_get_string(&data, "material").as_deref()),
        supported: try_get_bool(&data, "supported"),
        raft_height_mm: try_get_float(&data, "raftHeight"),
        model_name: try_get_string(&data, "model_name"),
        part_name: try_get_string(&data, "part_name"),
        rules_yaml: extract_rules_yaml(&data),
        model_metadata_json: extract_model_metadata_json(&data),
    })
}

/// Parses a config file; invalid files are logged and treated as absent.
pub async fn parse_config_file(path: &Path) -> Option<RawConfigFields> {
    let parsed = match tokio::fs::read_to_string(path).await {
        Ok(text) => parse_config_text(&text),
        Err(err) => Err(err.into()),
    };
    match parsed {
        Ok(fields) => Some(fields),
        Err(err) => {
            warn!(target: LOG_TARGET, "Failed to parse config file '{}'; treating as empty: {err:#}", path.display());
            None
        }
    }
}

pub fn full_dir_path(root: &Path, dir_path: &str) -> PathBuf {
    if dir_path.is_empty() { root.to_path_buf() } else { root.join(dir_path) }
}

pub async fn read_config_mapping(path: &Path) -> Mapping {
    match tokio::fs::read_to_string(path).await {
        Ok(text) => yaml::parse_mapping(&text).unwrap_or_default(),
        Err(_) => Mapping::new(),
    }
}

/// Writes (or deletes, when empty) a mapping as the directory's config file.
pub async fn write_config_mapping(path: &Path, data: &Mapping) -> Result<()> {
    if data.is_empty() {
        if tokio::fs::try_exists(path).await.unwrap_or(false) {
            tokio::fs::remove_file(path).await?;
        }
        return Ok(());
    }
    tokio::fs::write(path, yaml::to_string(&Value::Mapping(data.clone()))).await?;
    Ok(())
}

/// Replaces the local metadata config for a directory. Per-model `model_metadata` entries
/// are preserved. Without `field_rules`, existing rule definitions for fields not given a
/// plain value are kept.
pub async fn write_config_file(root: &Path, dir_path: &str, req: &UpdateDirectoryConfigRequest) -> Result<()> {
    let config_path = full_dir_path(root, dir_path).join(CONFIG_FILE_NAME);
    let existing = read_config_mapping(&config_path).await;

    let mut data = Mapping::new();
    let mut put = |key: &str, value: Value| {
        data.insert(Value::String(key.to_string()), value);
    };
    if let Some(v) = &req.model_name {
        put("model_name", Value::String(v.clone()));
    }
    if let Some(v) = &req.part_name {
        put("part_name", Value::String(v.clone()));
    }
    if let Some(v) = &req.creator {
        put("creator", Value::String(v.clone()));
    }
    if let Some(v) = &req.collection {
        put("collection", Value::String(v.clone()));
    }
    if let Some(v) = &req.subcollection {
        put("subcollection", Value::String(v.clone()));
    }
    if let Some(v) = &req.tags {
        put("tags", Value::Sequence(tags::normalize(v).into_iter().map(Value::String).collect()));
    }
    if let Some(v) = &req.category {
        put("category", Value::String(v.clone()));
    }
    if let Some(v) = &req.item_type {
        put("type", Value::String(v.clone()));
    }
    if let Some(v) = &req.material {
        put("material", Value::String(v.clone()));
    }
    if let Some(v) = req.supported {
        put("supported", Value::Bool(v));
    }
    if let Some(v) = req.raft_height_mm {
        put("raftHeight", yaml::f32_value(v));
    }

    if let Some(field_rules) = &req.field_rules {
        for (field, rule) in field_rules {
            let Some(rule_yaml) = rule.as_str().filter(|r| !r.trim().is_empty()) else { continue };
            if yaml::string_key(&data, field).is_some() {
                continue;
            }
            match yaml::parse_mapping(rule_yaml) {
                Ok(mapping) => {
                    let value = Value::Mapping(mapping);
                    if looks_like_rule_object(&value) {
                        data.insert(Value::String(field.clone()), value);
                    }
                }
                Err(err) => warn!(target: LOG_TARGET, "Invalid rule YAML for field '{field}'; skipping: {err}"),
            }
        }
    } else {
        for (key, value) in &existing {
            let Some(key) = key.as_str() else { continue };
            let Some(field) = fields::try_get(key) else { continue };
            if looks_like_rule_object(value) && yaml::string_key(&data, field.key).is_none() {
                data.insert(Value::String(field.key.to_string()), value.clone());
            }
        }
    }

    if let Some((key, section)) = get_ci(&existing, "model_metadata") {
        data.insert(Value::String(key.to_string()), section.clone());
    }

    write_config_mapping(&config_path, &data).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::model_metadata::get_model_metadata_entry;

    fn parse(text: &str) -> RawConfigFields {
        parse_config_text(text).unwrap()
    }

    fn entry(fields: &RawConfigFields, file: &str) -> Option<ModelMetadataEntry> {
        let dc = crate::entities::DirectoryConfig {
            raw_model_metadata_json: fields.model_metadata_json.clone(),
            ..Default::default()
        };
        get_model_metadata_entry(&dc, file)
    }

    #[test]
    fn no_model_metadata_section_returns_none() {
        assert_eq!(parse("creator: Alice\n").model_metadata_json, None);
    }

    #[test]
    fn model_metadata_name_is_parsed() {
        let fields = parse("creator: Alice\nmodel_metadata:\n  dragon.stl:\n    name: \"Fire Dragon\"\n");
        assert_eq!(entry(&fields, "dragon.stl").unwrap().name.as_deref(), Some("Fire Dragon"));
    }

    #[test]
    fn model_metadata_part_name_is_parsed() {
        let fields = parse("model_metadata:\n  bust_torso.stl:\n    name: \"Knight Bust\"\n    part_name: \"Torso\"\n");
        let e = entry(&fields, "bust_torso.stl").unwrap();
        assert_eq!(e.name.as_deref(), Some("Knight Bust"));
        assert_eq!(e.part_name.as_deref(), Some("Torso"));
    }

    #[test]
    fn model_metadata_entry_with_no_fields_is_omitted() {
        assert_eq!(parse("model_metadata:\n  dragon.stl: {}\n").model_metadata_json, None);
    }

    #[test]
    fn plain_fields_and_rules_are_extracted() {
        let fields = parse(
            "Creator: Alice\nsupported: true\nraftHeight: 1.5\ntags:\n  - b\n  - a\ncollection:\n  source: folder\n  expression: '^([^/]+)'\n",
        );
        assert_eq!(fields.creator.as_deref(), Some("Alice"));
        assert_eq!(fields.supported, Some(true));
        assert_eq!(fields.raft_height_mm, Some(1.5));
        assert_eq!(fields.tags, Some(vec!["a".to_string(), "b".to_string()]));
        assert_eq!(fields.collection, None);
        let rules = crate::metadata::rules::deserialize_rules(fields.rules_yaml.as_deref());
        assert_eq!(rules["collection"]["source"], "folder");
    }

    #[tokio::test]
    async fn write_config_file_keeps_model_metadata_and_existing_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        tokio::fs::write(
            &path,
            "creator:\n  source: folder\n  expression: x\nmodel_metadata:\n  dragon.stl:\n    name: Fire Dragon\n",
        )
        .await
        .unwrap();

        let req = UpdateDirectoryConfigRequest { collection: Some("Fantasy".into()), ..Default::default() };
        write_config_file(dir.path(), "", &req).await.unwrap();

        let fields = parse(&tokio::fs::read_to_string(&path).await.unwrap());
        assert_eq!(fields.collection.as_deref(), Some("Fantasy"));
        assert!(fields.rules_yaml.unwrap().contains("creator"));
        assert_eq!(
            entry(&parse(&tokio::fs::read_to_string(&path).await.unwrap()), "dragon.stl").unwrap().name.as_deref(),
            Some("Fire Dragon")
        );
    }

    #[tokio::test]
    async fn write_config_file_deletes_file_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        tokio::fs::write(&path, "creator: Alice\n").await.unwrap();
        write_config_file(dir.path(), "", &UpdateDirectoryConfigRequest::default()).await.unwrap();
        assert!(!path.exists());
    }
}
