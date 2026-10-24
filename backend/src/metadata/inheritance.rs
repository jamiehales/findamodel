//! Resolves inherited metadata fields across the directory config tree. Functions operate
//! on in-memory maps keyed by directory path; callers persist the changes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::Utc;

use super::config_reader::RawConfigFields;
use super::fields::{self, FieldValue};
use super::rules::{self, RuleSet};
use super::tags;
use crate::entities::DirectoryConfig;

pub type DirectoryMap = HashMap<String, DirectoryConfig>;

pub fn parent_path(dir_path: &str) -> Option<String> {
    if dir_path.is_empty() {
        return None;
    }
    Some(dir_path.rfind('/').map_or(String::new(), |i| dir_path[..i].to_string()))
}

pub fn parent_record<'a>(dir_path: &str, all: &'a DirectoryMap) -> Option<&'a DirectoryConfig> {
    all.get(&parent_path(dir_path)?)
}

pub fn build_full_file_path(root: &Path, directory: &str, file_name: &str) -> PathBuf {
    if directory.is_empty() { root.join(file_name) } else { root.join(directory).join(file_name) }
}

/// Expands paths to include every ancestor; the root ("") is always included.
pub fn expand_to_all_ancestors<'a>(paths: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
    let mut result = HashSet::from([String::new()]);
    for path in paths {
        result.insert(path.to_string());
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        for i in 1..parts.len() {
            result.insert(parts[..i].join("/"));
        }
    }
    result
}

/// Expands changed directories to include all of their descendants among `all_known`.
pub fn expand_to_descendants<'a>(
    changed: &HashSet<String>,
    all_known: impl IntoIterator<Item = &'a String>,
) -> Vec<String> {
    let mut result: HashSet<String> = changed.clone();
    for dir in all_known {
        if changed.iter().any(|root| root.is_empty() || dir.starts_with(&format!("{root}/"))) {
            result.insert(dir.clone());
        }
    }
    result.into_iter().collect()
}

pub fn apply_raw_fields(record: &mut DirectoryConfig, fields: Option<&RawConfigFields>) {
    let f = fields.cloned().unwrap_or_default();
    record.raw_creator = f.creator;
    record.raw_collection = f.collection;
    record.raw_subcollection = f.subcollection;
    record.raw_tags_json = tags::to_json_or_null(f.tags);
    record.raw_category = f.category;
    record.raw_type = f.item_type;
    record.raw_material = f.material;
    record.raw_supported = f.supported;
    record.raw_raft_height_mm = f.raft_height_mm;
    record.raw_model_name = f.model_name;
    record.raw_part_name = f.part_name;
    record.raw_rules_yaml = f.rules_yaml;
    record.raw_model_metadata_json = f.model_metadata_json;
}

/// Re-resolves every descendant of `parent` (ancestor-first, so each child sees its
/// parent already resolved).
pub fn resolve_descendants(parent: &str, all: &mut DirectoryMap) {
    let prefix = if parent.is_empty() { String::new() } else { format!("{parent}/") };
    let mut descendants: Vec<String> = all
        .keys()
        .filter(|p| if parent.is_empty() { !p.is_empty() } else { p.starts_with(&prefix) })
        .cloned()
        .collect();
    descendants.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));

    for path in descendants {
        resolve_fields_at(&path, all);
        if let Some(r) = all.get_mut(&path) {
            r.updated_at = Utc::now();
        }
    }
}

/// Resolves the record stored at `path` in place.
pub fn resolve_fields_at(path: &str, all: &mut DirectoryMap) {
    let Some(mut record) = all.get(path).cloned() else { return };
    resolve_fields(&mut record, all);
    all.insert(path.to_string(), record);
}

/// Resolves all metadata fields by walking up the ancestor chain using raw values only.
/// The record's own raw values win; then the closest ancestor's raw value or rule. Rules
/// land in `resolved_rules_yaml`; plain values in the resolved columns. Tags are additive
/// across the whole chain, and raft height inherits from the closest ancestor that sets it.
pub fn resolve_fields(record: &mut DirectoryConfig, all: &DirectoryMap) {
    let mut resolved: HashMap<&'static str, FieldValue> = HashMap::new();
    let mut resolved_rules = RuleSet::new();
    let mut claimed: HashSet<&'static str> = HashSet::new();

    for field in &fields::DEFINITIONS {
        let raw = field.raw_value(record);
        if raw.is_some() {
            claimed.insert(field.key);
        }
        resolved.insert(field.key, raw);
    }

    for (key, rule) in rules::deserialize_rules(record.raw_rules_yaml.as_deref()) {
        let Some(field) = fields::try_get(&key) else { continue };
        if !claimed.insert(field.key) {
            continue;
        }
        resolved_rules.insert(field.key.to_string(), rule);
    }

    let mut current = parent_record(&record.directory_path, all);
    while let Some(ancestor) = current {
        if claimed.len() >= fields::DEFINITIONS.len() {
            break;
        }
        let parent_rules = rules::deserialize_rules(ancestor.raw_rules_yaml.as_deref());
        for field in &fields::DEFINITIONS {
            if claimed.contains(field.key) {
                continue;
            }
            let raw = field.raw_value(ancestor);
            if raw.is_some() {
                resolved.insert(field.key, raw);
                claimed.insert(field.key);
                continue;
            }
            if let Some(rule) = parent_rules.get(field.key) {
                resolved_rules.insert(field.key.to_string(), rule.clone());
                claimed.insert(field.key);
            }
        }
        current = parent_record(&ancestor.directory_path, all);
    }

    for field in &fields::DEFINITIONS {
        let value = resolved.remove(field.key).unwrap_or(FieldValue::Str(None));
        field.set_resolved_value(record, value);
    }

    // Tags merge root-first across the whole ancestor chain.
    let mut chain: Vec<Option<String>> = vec![record.raw_tags_json.clone()];
    let mut walker = parent_record(&record.directory_path, all);
    while let Some(a) = walker {
        chain.push(a.raw_tags_json.clone());
        walker = parent_record(&a.directory_path, all);
    }
    chain.reverse();
    let merged: Vec<String> = chain.iter().flat_map(|t| tags::from_json(t.as_deref())).collect();
    record.tags_json = tags::to_json_or_null(Some(merged));

    record.raft_height_mm = record.raw_raft_height_mm.or_else(|| {
        let mut parent = parent_record(&record.directory_path, all);
        while let Some(p) = parent {
            if p.raw_raft_height_mm.is_some() {
                return p.raw_raft_height_mm;
            }
            parent = parent_record(&p.directory_path, all);
        }
        None
    });

    record.resolved_rules_yaml = (!resolved_rules.is_empty()).then(|| rules::serialize_rules(&resolved_rules));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dc(path: &str, f: impl FnOnce(&mut DirectoryConfig)) -> DirectoryConfig {
        let mut d = DirectoryConfig { directory_path: path.to_string(), ..Default::default() };
        f(&mut d);
        d
    }

    fn map(records: Vec<DirectoryConfig>) -> DirectoryMap {
        records.into_iter().map(|r| (r.directory_path.clone(), r)).collect()
    }

    #[test]
    fn parent_path_handles_root_and_nesting() {
        assert_eq!(parent_path(""), None);
        assert_eq!(parent_path("Fantasy").as_deref(), Some(""));
        assert_eq!(parent_path("Fantasy/Elves").as_deref(), Some("Fantasy"));
        assert_eq!(parent_path("Fantasy/Elves/Warriors").as_deref(), Some("Fantasy/Elves"));
    }

    #[test]
    fn parent_record_lookup() {
        let all = map(vec![dc("", |_| {}), dc("Fantasy", |_| {})]);
        assert!(parent_record("", &all).is_none());
        assert!(parent_record("Fantasy", &DirectoryMap::new()).is_none());
        assert_eq!(parent_record("Fantasy", &all).unwrap().directory_path, "");
        assert_eq!(parent_record("Fantasy/Elves", &all).unwrap().directory_path, "Fantasy");
    }

    #[test]
    fn expand_to_all_ancestors_cases() {
        let empty = expand_to_all_ancestors([]);
        assert_eq!(empty.len(), 1);
        assert!(empty.contains(""));

        let deep = expand_to_all_ancestors(["Fantasy/Elves/Warriors"]);
        assert_eq!(deep.len(), 4);
        for p in ["", "Fantasy", "Fantasy/Elves", "Fantasy/Elves/Warriors"] {
            assert!(deep.contains(p));
        }
        assert_eq!(expand_to_all_ancestors(["Fantasy/A", "Fantasy/B"]).len(), 4);
        assert_eq!(expand_to_all_ancestors(["Fantasy"]).len(), 2);
    }

    #[test]
    fn expand_to_descendants_cases() {
        let all: Vec<String> = ["", "A", "A/B", "B"].iter().map(|s| s.to_string()).collect();
        let result = expand_to_descendants(&HashSet::from([String::new()]), &all);
        for p in ["", "A", "A/B", "B"] {
            assert!(result.contains(&p.to_string()));
        }

        let all: Vec<String> =
            ["", "Fantasy", "Fantasy/Elves", "Fantasy/Dwarves", "SciFi"].iter().map(|s| s.to_string()).collect();
        let result = expand_to_descendants(&HashSet::from(["Fantasy".to_string()]), &all);
        assert!(result.contains(&"Fantasy/Elves".to_string()) && result.contains(&"Fantasy/Dwarves".to_string()));
        assert!(!result.contains(&String::new()) && !result.contains(&"SciFi".to_string()));

        let all: Vec<String> = ["", "A", "AB", "B"].iter().map(|s| s.to_string()).collect();
        let result = expand_to_descendants(&HashSet::from(["A".to_string()]), &all);
        assert!(result.contains(&"A".to_string()));
        assert!(!result.contains(&"AB".to_string()) && !result.contains(&"B".to_string()));
    }

    #[test]
    fn apply_raw_fields_sets_and_clears() {
        let mut record = dc("", |d| {
            d.raw_creator = Some("someone".into());
            d.raw_category = Some("cat".into());
        });
        apply_raw_fields(&mut record, None);
        assert_eq!(record.raw_creator, None);
        assert_eq!(record.raw_category, None);
        assert_eq!(record.raw_rules_yaml, None);

        let fields = RawConfigFields {
            creator: Some("Alice".into()),
            collection: Some("Fantasy".into()),
            subcollection: Some("Elves".into()),
            tags: Some(vec!["32mm".into(), "small".into()]),
            category: Some("miniature".into()),
            item_type: Some("tabletop".into()),
            material: Some("resin".into()),
            supported: Some(true),
            raft_height_mm: Some(1.5),
            model_name: Some("Elf Warrior".into()),
            part_name: None,
            rules_yaml: Some("creator:\n  rule: regex\n  source: folder\n  expression: '^([^/]+)'".into()),
            model_metadata_json: None,
        };
        apply_raw_fields(&mut record, Some(&fields));
        assert_eq!(record.raw_creator.as_deref(), Some("Alice"));
        assert_eq!(record.raw_tags_json.as_deref(), Some("[\"32mm\",\"small\"]"));
        assert_eq!(record.raw_supported, Some(true));
        assert_eq!(record.raw_raft_height_mm, Some(1.5));
        assert_eq!(
            record.raw_rules_yaml.as_deref(),
            Some("creator:\n  rule: regex\n  source: folder\n  expression: '^([^/]+)'")
        );
    }

    #[test]
    fn tags_merge_across_ancestor_chain() {
        let mut all = map(vec![
            dc("", |d| d.raw_tags_json = Some("[\"32mm\",\"small\"]".into())),
            dc("Fantasy", |d| d.raw_tags_json = Some("[\"monster\"]".into())),
            dc("Fantasy/Elites", |d| d.raw_tags_json = Some("[\"metal\"]".into())),
        ]);
        for p in ["", "Fantasy", "Fantasy/Elites"] {
            resolve_fields_at(p, &mut all);
        }
        assert_eq!(all["Fantasy/Elites"].tags_json.as_deref(), Some("[\"32mm\",\"metal\",\"monster\",\"small\"]"));
    }

    #[test]
    fn resolve_fields_inheritance_rules() {
        let mut all = map(vec![dc("", |d| {
            d.raw_creator = Some("Alice".into());
            d.raw_collection = Some("Fantasy".into());
        })]);
        resolve_fields_at("", &mut all);
        assert_eq!(all[""].creator.as_deref(), Some("Alice"));
        assert_eq!(all[""].collection.as_deref(), Some("Fantasy"));

        let mut all = map(vec![dc("", |d| d.raw_creator = Some("Alice".into())), dc("Fantasy", |_| {})]);
        resolve_fields_at("", &mut all);
        resolve_fields_at("Fantasy", &mut all);
        assert_eq!(all["Fantasy"].creator.as_deref(), Some("Alice"));

        let mut all = map(vec![
            dc("", |d| d.raw_creator = Some("Bob".into())),
            dc("Fantasy", |d| d.raw_creator = Some("Alice".into())),
        ]);
        resolve_fields_at("", &mut all);
        resolve_fields_at("Fantasy", &mut all);
        assert_eq!(all["Fantasy"].creator.as_deref(), Some("Alice"));
    }

    #[test]
    fn raft_height_inheritance() {
        let mut all = map(vec![dc("", |d| d.raw_raft_height_mm = Some(3.0)), dc("Fantasy", |_| {})]);
        resolve_fields_at("", &mut all);
        resolve_fields_at("Fantasy", &mut all);
        assert_eq!(all["Fantasy"].raft_height_mm, Some(3.0));

        let mut all = map(vec![
            dc("", |d| d.raw_raft_height_mm = Some(3.0)),
            dc("Fantasy", |d| d.raw_raft_height_mm = Some(1.0)),
        ]);
        resolve_fields_at("", &mut all);
        resolve_fields_at("Fantasy", &mut all);
        assert_eq!(all["Fantasy"].raft_height_mm, Some(1.0));
    }

    #[test]
    fn rule_resolution() {
        let rule = "creator:\n  rule: regex\n  source: folder\n  expression: '^([^/]+)'\n";

        let mut all = map(vec![dc("", |_| {})]);
        resolve_fields_at("", &mut all);
        assert_eq!(all[""].resolved_rules_yaml, None);

        let mut all = map(vec![dc("", |d| d.raw_rules_yaml = Some(rule.into()))]);
        resolve_fields_at("", &mut all);
        assert!(all[""].resolved_rules_yaml.as_deref().unwrap().contains("creator"));

        let mut all = map(vec![dc("", |d| d.raw_rules_yaml = Some(rule.into())), dc("Fantasy", |_| {})]);
        resolve_fields_at("", &mut all);
        resolve_fields_at("Fantasy", &mut all);
        assert!(all["Fantasy"].resolved_rules_yaml.as_deref().unwrap().contains("creator"));

        let mut all = map(vec![
            dc("", |d| d.raw_rules_yaml = Some(rule.into())),
            dc("Fantasy", |d| d.raw_creator = Some("Alice".into())),
        ]);
        resolve_fields_at("", &mut all);
        resolve_fields_at("Fantasy", &mut all);
        assert_eq!(all["Fantasy"].creator.as_deref(), Some("Alice"));
        assert!(all["Fantasy"].resolved_rules_yaml.as_deref().is_none_or(|y| !y.contains("creator")));
    }

    #[test]
    fn resolve_descendants_updates_only_descendants() {
        let mut all = map(vec![
            dc("", |d| d.raw_creator = Some("Alice".into())),
            dc("A", |_| {}),
            dc("A/B", |_| {}),
            dc("B", |_| {}),
        ]);
        resolve_fields_at("", &mut all);
        resolve_descendants("", &mut all);
        for p in ["A", "A/B", "B"] {
            assert_eq!(all[p].creator.as_deref(), Some("Alice"));
        }

        let mut all = map(vec![
            dc("", |_| {}),
            dc("A", |d| d.raw_creator = Some("Alice".into())),
            dc("A/B", |_| {}),
            dc("B", |d| d.raw_creator = Some("Bob".into())),
        ]);
        resolve_fields_at("A", &mut all);
        resolve_descendants("A", &mut all);
        assert_eq!(all["A/B"].creator.as_deref(), Some("Alice"));
        assert_eq!(all["B"].creator, None);
    }
}
