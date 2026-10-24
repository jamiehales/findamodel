//! Registry of inheritable directory metadata fields and their accessors on
//! [`DirectoryConfig`] records.

use crate::entities::DirectoryConfig;

use super::rules::RuleFieldType;

/// A field value: strings (tags are stored as JSON text) or booleans.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Str(Option<String>),
    Bool(Option<bool>),
}

impl FieldValue {
    pub fn is_some(&self) -> bool {
        match self {
            FieldValue::Str(v) => v.is_some(),
            FieldValue::Bool(v) => v.is_some(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldDefinition {
    pub key: &'static str,
    pub rule_field_type: RuleFieldType,
}

pub const DEFINITIONS: [FieldDefinition; 10] = [
    FieldDefinition { key: "creator", rule_field_type: RuleFieldType::String },
    FieldDefinition { key: "collection", rule_field_type: RuleFieldType::String },
    FieldDefinition { key: "subcollection", rule_field_type: RuleFieldType::String },
    FieldDefinition { key: "tags", rule_field_type: RuleFieldType::String },
    FieldDefinition { key: "category", rule_field_type: RuleFieldType::Enum },
    FieldDefinition { key: "type", rule_field_type: RuleFieldType::Enum },
    FieldDefinition { key: "material", rule_field_type: RuleFieldType::Enum },
    FieldDefinition { key: "supported", rule_field_type: RuleFieldType::Bool },
    FieldDefinition { key: "model_name", rule_field_type: RuleFieldType::String },
    FieldDefinition { key: "part_name", rule_field_type: RuleFieldType::String },
];

pub fn keys() -> impl Iterator<Item = &'static str> {
    DEFINITIONS.iter().map(|d| d.key)
}

/// Case-insensitive lookup of a field definition.
pub fn try_get(key: &str) -> Option<&'static FieldDefinition> {
    DEFINITIONS.iter().find(|d| d.key.eq_ignore_ascii_case(key))
}

pub fn rule_field_type(key: &str) -> RuleFieldType {
    try_get(key).map_or(RuleFieldType::String, |d| d.rule_field_type)
}

/// Enum values are free-form; this only trims and drops empty values.
pub fn validate_enum_value(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

impl FieldDefinition {
    pub fn raw_value(&self, record: &DirectoryConfig) -> FieldValue {
        match self.key {
            "creator" => FieldValue::Str(record.raw_creator.clone()),
            "collection" => FieldValue::Str(record.raw_collection.clone()),
            "subcollection" => FieldValue::Str(record.raw_subcollection.clone()),
            "tags" => FieldValue::Str(record.raw_tags_json.clone()),
            "category" => FieldValue::Str(record.raw_category.clone()),
            "type" => FieldValue::Str(record.raw_type.clone()),
            "material" => FieldValue::Str(record.raw_material.clone()),
            "supported" => FieldValue::Bool(record.raw_supported),
            "model_name" => FieldValue::Str(record.raw_model_name.clone()),
            "part_name" => FieldValue::Str(record.raw_part_name.clone()),
            _ => unreachable!("unknown field {}", self.key),
        }
    }

    pub fn resolved_value(&self, record: &DirectoryConfig) -> FieldValue {
        match self.key {
            "creator" => FieldValue::Str(record.creator.clone()),
            "collection" => FieldValue::Str(record.collection.clone()),
            "subcollection" => FieldValue::Str(record.subcollection.clone()),
            "tags" => FieldValue::Str(record.tags_json.clone()),
            "category" => FieldValue::Str(record.category.clone()),
            "type" => FieldValue::Str(record.item_type.clone()),
            "material" => FieldValue::Str(record.material.clone()),
            "supported" => FieldValue::Bool(record.supported),
            "model_name" => FieldValue::Str(record.model_name.clone()),
            "part_name" => FieldValue::Str(record.part_name.clone()),
            _ => unreachable!("unknown field {}", self.key),
        }
    }

    pub fn set_resolved_value(&self, record: &mut DirectoryConfig, value: FieldValue) {
        let as_str = |v: FieldValue| match v {
            FieldValue::Str(s) => s,
            FieldValue::Bool(_) => None,
        };
        match self.key {
            "creator" => record.creator = as_str(value),
            "collection" => record.collection = as_str(value),
            "subcollection" => record.subcollection = as_str(value),
            "tags" => record.tags_json = as_str(value),
            "category" => record.category = as_str(value),
            "type" => record.item_type = as_str(value),
            "material" => record.material = as_str(value),
            "supported" => {
                record.supported = match value {
                    FieldValue::Bool(b) => b,
                    FieldValue::Str(_) => None,
                }
            }
            "model_name" => record.model_name = as_str(value),
            "part_name" => record.part_name = as_str(value),
            _ => unreachable!("unknown field {}", self.key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_get_is_false_for_unknown_key() {
        assert!(try_get("nonexistent").is_none());
    }

    #[test]
    fn try_get_finds_all_known_keys_case_insensitively() {
        for key in [
            "creator",
            "collection",
            "subcollection",
            "tags",
            "category",
            "type",
            "material",
            "supported",
            "model_name",
            "part_name",
        ] {
            assert!(try_get(key).is_some(), "{key}");
        }
        for key in ["CREATOR", "Creator", "CATEGORY", "Model_Name"] {
            assert!(try_get(key).is_some(), "{key}");
        }
        assert_eq!(try_get("creator").unwrap().key, "creator");
    }

    #[test]
    fn registry_contains_ten_fields() {
        assert_eq!(DEFINITIONS.len(), 10);
        let unique: std::collections::HashSet<_> = keys().collect();
        assert_eq!(unique.len(), 10);
    }

    #[test]
    fn rule_field_types() {
        for key in ["creator", "collection", "subcollection", "tags", "model_name", "part_name"] {
            assert_eq!(rule_field_type(key), RuleFieldType::String);
        }
        for key in ["category", "type", "material"] {
            assert_eq!(rule_field_type(key), RuleFieldType::Enum);
        }
        assert_eq!(rule_field_type("supported"), RuleFieldType::Bool);
        assert_eq!(rule_field_type("unknownfield"), RuleFieldType::String);
    }

    #[test]
    fn accessors_round_trip() {
        let def = try_get("creator").unwrap();
        let mut record = DirectoryConfig { raw_creator: Some("Alice".into()), ..Default::default() };
        assert_eq!(def.raw_value(&record), FieldValue::Str(Some("Alice".into())));
        def.set_resolved_value(&mut record, FieldValue::Str(Some("Bob".into())));
        assert_eq!(record.creator.as_deref(), Some("Bob"));

        let collection = try_get("collection").unwrap();
        assert!(!collection.resolved_value(&DirectoryConfig::default()).is_some());

        let supported = try_get("supported").unwrap();
        supported.set_resolved_value(&mut record, FieldValue::Bool(Some(true)));
        assert_eq!(record.supported, Some(true));
        supported.set_resolved_value(&mut record, FieldValue::Bool(None));
        assert_eq!(record.supported, None);
    }
}
