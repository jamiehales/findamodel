//! Per-model metadata: evaluates a directory's resolved values and rules against a model
//! file path, then applies per-model overrides from the `model_metadata` section.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::fields;
use super::rules::{self, RuleFieldType, RuleSet};
use super::tags;
use crate::entities::DirectoryConfig;

/// Per-model overrides from findamodel.yaml `model_metadata`. All fields optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMetadataEntry {
    pub name: Option<String>,
    pub part_name: Option<String>,
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
}

impl ModelMetadataEntry {
    pub fn is_empty(&self) -> bool {
        *self == ModelMetadataEntry::default()
    }

    /// Parses an entry from a YAML mapping (case-insensitive keys). Returns `None` when no
    /// recognised value is present.
    pub fn from_yaml(entry: &serde_yaml::Mapping) -> Option<Self> {
        use super::yaml::{get_ci, scalar_to_string};
        let get_string = |key: &str| get_ci(entry, key).and_then(|(_, v)| scalar_to_string(v));
        let get_bool = |key: &str| {
            get_ci(entry, key).and_then(|(_, v)| match v {
                serde_yaml::Value::Bool(b) => Some(*b),
                other => scalar_to_string(other).and_then(|s| parse_bool(&s)),
            })
        };
        let get_float = |key: &str| {
            get_ci(entry, key).and_then(|(_, v)| scalar_to_string(v)).and_then(|s| s.trim().parse::<f32>().ok())
        };
        let get_list = |key: &str| match get_ci(entry, key) {
            Some((_, serde_yaml::Value::Sequence(items))) => {
                Some(tags::normalize(items.iter().map(|i| scalar_to_string(i).unwrap_or_default())))
            }
            _ => None,
        };

        let parsed = ModelMetadataEntry {
            name: get_string("name"),
            part_name: get_string("part_name"),
            creator: get_string("creator"),
            collection: get_string("collection"),
            subcollection: get_string("subcollection"),
            tags: get_list("tags"),
            category: get_string("category"),
            item_type: get_string("type"),
            material: get_string("material"),
            supported: get_bool("supported"),
            raft_height_mm: get_float("raft_height_mm"),
        };
        (!parsed.is_empty()).then_some(parsed)
    }

    /// YAML mapping for the `model_metadata` section; unset fields are omitted.
    pub fn to_yaml(&self) -> serde_yaml::Mapping {
        use serde_yaml::Value as Y;
        let mut m = serde_yaml::Mapping::new();
        let mut put_str = |key: &str, value: &Option<String>| {
            if let Some(v) = value {
                m.insert(Y::String(key.into()), Y::String(v.clone()));
            }
        };
        put_str("name", &self.name);
        put_str("part_name", &self.part_name);
        put_str("creator", &self.creator);
        put_str("collection", &self.collection);
        put_str("subcollection", &self.subcollection);
        if let Some(t) = &self.tags {
            m.insert(Y::String("tags".into()), Y::Sequence(tags::normalize(t).into_iter().map(Y::String).collect()));
        }
        let mut put_str = |key: &str, value: &Option<String>| {
            if let Some(v) = value {
                m.insert(Y::String(key.into()), Y::String(v.clone()));
            }
        };
        put_str("category", &self.category);
        put_str("type", &self.item_type);
        put_str("material", &self.material);
        if let Some(s) = self.supported {
            m.insert(Y::String("supported".into()), Y::Bool(s));
        }
        if let Some(r) = self.raft_height_mm {
            m.insert(Y::String("raft_height_mm".into()), super::yaml::f32_value(r));
        }
        m
    }
}

pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Serialises `file name -> entry` for storage on the directory record.
pub fn entries_to_storage_json(entries: &[(String, ModelMetadataEntry)]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let map: Map<String, Value> =
        entries.iter().map(|(k, v)| (k.clone(), serde_json::to_value(v).expect("serialising entry"))).collect();
    Some(Value::Object(map).to_string())
}

/// Looks up a file's entry (case-insensitive file name) in the stored JSON. Property names
/// are matched case-insensitively.
pub fn get_model_metadata_entry(dir_config: &DirectoryConfig, file_name: &str) -> Option<ModelMetadataEntry> {
    let json = dir_config.raw_model_metadata_json.as_deref()?;
    let Value::Object(map) = serde_json::from_str::<Value>(json).ok()? else {
        return None;
    };
    let (_, raw) = map.iter().find(|(k, _)| tags::eq_ignore_case(k, file_name))?;
    let Value::Object(props) = raw else {
        return None;
    };
    let normalized: Map<String, Value> = props
        .iter()
        .map(|(k, v)| {
            let canonical = match k.to_ascii_lowercase().as_str() {
                "name" => "name",
                "partname" | "part_name" => "partName",
                "creator" => "creator",
                "collection" => "collection",
                "subcollection" => "subcollection",
                "tags" => "tags",
                "category" => "category",
                "type" => "type",
                "material" => "material",
                "supported" => "supported",
                "raftheightmm" | "raft_height_mm" => "raftHeightMm",
                _ => "",
            };
            (canonical.to_string(), v.clone())
        })
        .filter(|(k, _)| !k.is_empty())
        .collect();
    serde_json::from_value(Value::Object(normalized)).ok()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComputedMetadata {
    pub creator: Option<String>,
    pub collection: Option<String>,
    pub subcollection: Option<String>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub item_type: Option<String>,
    pub material: Option<String>,
    pub supported: Option<bool>,
    pub raft_height_mm: Option<f32>,
    pub model_name: Option<String>,
    pub part_name: Option<String>,
}

impl ComputedMetadata {
    /// The directory's resolved plain values (rules not yet applied).
    pub fn from_directory(dc: Option<&DirectoryConfig>) -> Self {
        let Some(dc) = dc else {
            return Self::default();
        };
        ComputedMetadata {
            creator: dc.creator.clone(),
            collection: dc.collection.clone(),
            subcollection: dc.subcollection.clone(),
            tags: tags::from_json(dc.tags_json.as_deref()),
            category: dc.category.clone(),
            item_type: dc.item_type.clone(),
            material: dc.material.clone(),
            supported: dc.supported,
            raft_height_mm: dc.raft_height_mm,
            model_name: dc.model_name.clone(),
            part_name: dc.part_name.clone(),
        }
    }

    fn string_field_mut(&mut self, key: &str) -> Option<&mut Option<String>> {
        match key {
            "creator" => Some(&mut self.creator),
            "collection" => Some(&mut self.collection),
            "subcollection" => Some(&mut self.subcollection),
            "category" => Some(&mut self.category),
            "type" => Some(&mut self.item_type),
            "material" => Some(&mut self.material),
            "model_name" => Some(&mut self.model_name),
            "part_name" => Some(&mut self.part_name),
            _ => None,
        }
    }
}

/// Evaluates resolved rules against `full_file_path` and merges them over the plain values.
/// Returns the merged metadata and, when requested, `field -> YAML snippet` for every
/// rule-derived field.
pub fn resolve_for_path(
    plain: ComputedMetadata,
    resolved_rules: &RuleSet,
    full_file_path: &str,
    collect_rule_configs: bool,
) -> (ComputedMetadata, Option<Vec<(String, String)>>) {
    if resolved_rules.is_empty() {
        return (plain, None);
    }

    let mut resolved = plain;
    let mut rule_configs: Vec<(String, String)> = Vec::new();
    let record_rule = |field: &str, key: &str, rule: &Value, configs: &mut Vec<(String, String)>| {
        if collect_rule_configs {
            configs.retain(|(k, _)| k != key);
            configs.push((key.to_string(), rules::rule_config_to_yaml_snippet(field, rule)));
        }
    };

    for (field, rule) in resolved_rules {
        let normalized = fields::try_get(field).map_or_else(|| field.to_ascii_lowercase(), |d| d.key.to_string());
        let field_type = fields::rule_field_type(&normalized);
        let value = rules::evaluate(full_file_path, rule, field_type);

        if normalized == "tags" {
            let options: RuleSet = match rule {
                Value::Object(m) => m
                    .iter()
                    .filter(|(k, _)| !k.eq_ignore_ascii_case("rule"))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                _ => RuleSet::new(),
            };
            let matched = rules::parse_matching_value_keys(full_file_path, &options);
            if !matched.is_empty() {
                record_rule(field, &normalized, rule, &mut rule_configs);
                resolved.tags = tags::merge([resolved.tags.as_slice(), matched.as_slice()]);
                continue;
            }
            let Some(value) = value else { continue };
            record_rule(field, &normalized, rule, &mut rule_configs);
            resolved.tags = tags::merge([resolved.tags.as_slice(), std::slice::from_ref(&value)]);
            continue;
        }

        let Some(value) = value else { continue };
        record_rule(field, &normalized, rule, &mut rule_configs);

        match field_type {
            RuleFieldType::Bool => {
                if normalized == "supported" {
                    resolved.supported = Some(value.eq_ignore_ascii_case("true"));
                }
            }
            RuleFieldType::Enum => {
                if let Some(slot) = resolved.string_field_mut(&normalized) {
                    *slot = fields::validate_enum_value(Some(&value));
                }
            }
            RuleFieldType::String => {
                if let Some(slot) = resolved.string_field_mut(&normalized) {
                    *slot = Some(value);
                }
            }
        }
    }

    resolved.tags = tags::normalize(&resolved.tags);
    let configs = (collect_rule_configs && !rule_configs.is_empty()).then_some(rule_configs);
    (resolved, configs)
}

/// Folder-resolved metadata a model inherits, without per-model overrides.
pub fn compute_inherited(full_file_path: &str, dir_config: Option<&DirectoryConfig>) -> ComputedMetadata {
    let Some(dc) = dir_config else {
        return ComputedMetadata::default();
    };
    let plain = ComputedMetadata::from_directory(Some(dc));
    let resolved_rules = rules::deserialize_rules(dc.resolved_rules_yaml.as_deref());
    resolve_for_path(plain, &resolved_rules, full_file_path, false).0
}

/// Full per-model metadata: inherited values plus `model_metadata` overrides.
pub fn compute(full_file_path: &str, dir_config: Option<&DirectoryConfig>) -> ComputedMetadata {
    let mut computed = compute_inherited(full_file_path, dir_config);
    let Some(dc) = dir_config else {
        return computed;
    };
    let file_name = full_file_path.rsplit(['/', '\\']).next().unwrap_or(full_file_path);
    let Some(entry) = get_model_metadata_entry(dc, file_name) else {
        return computed;
    };

    if entry.name.is_some() {
        computed.model_name = entry.name;
    }
    if entry.part_name.is_some() {
        computed.part_name = entry.part_name;
    }
    if entry.creator.is_some() {
        computed.creator = entry.creator;
    }
    if entry.collection.is_some() {
        computed.collection = entry.collection;
    }
    if entry.subcollection.is_some() {
        computed.subcollection = entry.subcollection;
    }
    if let Some(t) = &entry.tags {
        computed.tags = tags::merge([computed.tags.as_slice(), t.as_slice()]);
    }
    if entry.category.is_some() {
        computed.category = entry.category;
    }
    if entry.item_type.is_some() {
        computed.item_type = entry.item_type;
    }
    if entry.material.is_some() {
        computed.material = entry.material;
    }
    if entry.supported.is_some() {
        computed.supported = entry.supported;
    }
    if entry.raft_height_mm.is_some() {
        computed.raft_height_mm = entry.raft_height_mm;
    }
    computed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(f: impl FnOnce(&mut DirectoryConfig)) -> DirectoryConfig {
        let mut dc = DirectoryConfig::default();
        f(&mut dc);
        dc
    }

    #[test]
    fn null_dir_config_returns_empty_metadata() {
        assert_eq!(compute("/models/dragon.stl", None), ComputedMetadata::default());
    }

    #[test]
    fn plain_resolved_values_are_returned_directly() {
        let dc = dir(|d| {
            d.creator = Some("Alice".into());
            d.collection = Some("Fantasy".into());
            d.subcollection = Some("Elves".into());
            d.category = Some("miniature".into());
            d.item_type = Some("tabletop".into());
            d.material = Some("resin".into());
            d.supported = Some(true);
            d.model_name = Some("Elf Warrior".into());
        });
        let r = compute("/models/elf_warrior.stl", Some(&dc));
        assert_eq!(r.creator.as_deref(), Some("Alice"));
        assert_eq!(r.collection.as_deref(), Some("Fantasy"));
        assert_eq!(r.subcollection.as_deref(), Some("Elves"));
        assert_eq!(r.category.as_deref(), Some("miniature"));
        assert_eq!(r.item_type.as_deref(), Some("tabletop"));
        assert_eq!(r.material.as_deref(), Some("resin"));
        assert_eq!(r.supported, Some(true));
        assert_eq!(r.model_name.as_deref(), Some("Elf Warrior"));
    }

    #[test]
    fn default_regex_rule_overrides_plain_value() {
        let dc = dir(|d| {
            d.model_name = Some("Static Name".into());
            d.resolved_rules_yaml =
                Some("model_name:\n  source: filename\n  expression: \"^(.*)\\\\.[^./]+$\"\n".into());
        });
        assert_eq!(compute("/models/fantasy/dragon.stl", Some(&dc)).model_name.as_deref(), Some("dragon"));
    }

    #[test]
    fn regex_rule_extracts_group() {
        let dc = dir(|d| {
            d.resolved_rules_yaml =
                Some("creator:\n  rule: regex\n  source: folder\n  expression: \"^([^/]+)\"\n".into())
        });
        assert_eq!(compute("Fantasy/Elves/warrior.stl", Some(&dc)).creator.as_deref(), Some("Fantasy"));
    }

    #[test]
    fn bool_rule_parses_to_bool_and_missing_expression_is_none() {
        let dc =
            dir(|d| d.resolved_rules_yaml = Some("supported:\n  rule: regex\n  expression: \"supported\"\n".into()));
        assert_eq!(compute("/models/supported_model.stl", Some(&dc)).supported, Some(true));
        assert_eq!(compute("/models/plain_model.stl", Some(&dc)).supported, Some(false));

        let no_expr = dir(|d| d.resolved_rules_yaml = Some("supported:\n  rule: regex\n  source: filename\n".into()));
        assert_eq!(compute("/models/yes_model.stl", Some(&no_expr)).supported, None);
    }

    #[test]
    fn unknown_rule_leaves_field_empty() {
        let dc = dir(|d| d.resolved_rules_yaml = Some("creator:\n  rule: nonexistent\n".into()));
        assert_eq!(compute("/models/model.stl", Some(&dc)).creator, None);
    }

    #[test]
    fn rules_and_plain_values_mix_across_fields() {
        let dc = dir(|d| {
            d.creator = Some("Alice".into());
            d.collection = Some("Fantasy".into());
            d.resolved_rules_yaml =
                Some("model_name:\n  source: filename\n  expression: \"^(.*)\\\\.[^./]+$\"\n".into());
        });
        let r = compute("/models/dragon.stl", Some(&dc));
        assert_eq!(r.creator.as_deref(), Some("Alice"));
        assert_eq!(r.collection.as_deref(), Some("Fantasy"));
        assert_eq!(r.model_name.as_deref(), Some("dragon"));
    }

    #[test]
    fn model_metadata_overrides_and_lookup() {
        let dc = dir(|d| {
            d.model_name = Some("Generic Name".into());
            d.raw_model_metadata_json = Some(r#"{"dragon.stl":{"Name":"Fire Dragon","PartName":null}}"#.into());
        });
        assert_eq!(compute("/models/dragon.stl", Some(&dc)).model_name.as_deref(), Some("Fire Dragon"));

        let with_part = dir(|d| {
            d.raw_model_metadata_json = Some(r#"{"bust_torso.stl":{"Name":"Knight Bust","PartName":"Torso"}}"#.into())
        });
        let r = compute("/models/bust_torso.stl", Some(&with_part));
        assert_eq!(r.model_name.as_deref(), Some("Knight Bust"));
        assert_eq!(r.part_name.as_deref(), Some("Torso"));

        let upper = dir(|d| {
            d.raw_model_metadata_json = Some(r#"{"Dragon.STL":{"Name":"Fire Dragon","PartName":null}}"#.into())
        });
        assert_eq!(compute("/models/dragon.stl", Some(&upper)).model_name.as_deref(), Some("Fire Dragon"));

        let other = dir(|d| {
            d.model_name = Some("Generic Name".into());
            d.raw_model_metadata_json = Some(r#"{"other.stl":{"Name":"Other","PartName":null}}"#.into());
        });
        let r = compute("/models/dragon.stl", Some(&other));
        assert_eq!(r.model_name.as_deref(), Some("Generic Name"));
        assert_eq!(r.part_name, None);
    }

    #[test]
    fn tags_are_additive_across_folder_rules_and_model_entries() {
        let dc = dir(|d| {
            d.tags_json = Some(r#"["32mm","small","monster"]"#.into());
            d.raw_model_metadata_json = Some(r#"{"dragon.stl":{"Tags":["metal"]}}"#.into());
        });
        let r = compute("/models/dragon.stl", Some(&dc));
        assert_eq!(r.tags, vec!["32mm", "metal", "monster", "small"]);

        let rule = dir(|d| {
            d.tags_json = Some(r#"["fantasy"]"#.into());
            d.resolved_rules_yaml =
                Some("tags:\n  rule: regex\n  source: filename\n  expression: \"supported\"\n".into());
        });
        assert!(compute("/models/elf_supported.stl", Some(&rule)).tags.contains(&"supported".to_string()));
        let plain = compute("/models/elf_plain.stl", Some(&rule));
        assert_eq!(plain.tags, vec!["fantasy"]);

        let values = dir(|d| {
            d.tags_json = Some(r#"["fantasy"]"#.into());
            d.resolved_rules_yaml = Some(
                "tags:\n  rule: regex\n  source: filename\n  values:\n    Supported: 'supported'\n    Miniature: 'mini'\n    Bust: 'bust'\n".into(),
            );
        });
        let r = compute("/models/elf_mini_supported.stl", Some(&values));
        assert_eq!(r.tags, vec!["fantasy", "Miniature", "Supported"]);
    }

    #[test]
    fn get_model_metadata_entry_handles_missing_data() {
        let dc = dir(|d| {
            d.raw_model_metadata_json = Some(r#"{"dragon.stl":{"Name":"Fire Dragon","PartName":"Body"}}"#.into())
        });
        let entry = get_model_metadata_entry(&dc, "dragon.stl").unwrap();
        assert_eq!(entry.name.as_deref(), Some("Fire Dragon"));
        assert_eq!(entry.part_name.as_deref(), Some("Body"));
        assert!(get_model_metadata_entry(&DirectoryConfig::default(), "dragon.stl").is_none());
        assert!(get_model_metadata_entry(&dc, "other.stl").is_none());
    }

    #[test]
    fn model_level_raft_override_is_read_case_insensitively() {
        let dc = dir(|d| d.raw_model_metadata_json = Some(r#"{"dragon.stl":{"raftHeightMm":7.5}}"#.into()));
        assert_eq!(compute("/m/dragon.stl", Some(&dc)).raft_height_mm, Some(7.5));
    }
}
