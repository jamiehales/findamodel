//! Metadata rules. The only rule type is "regex", which applies a regular expression to a
//! value derived from the model file path.
//!
//! Options:
//!   source: full_path | folder | filename  (default: full_path)
//!   expression: a regex, or a sed-style substitution (s|pattern|replacement|flags).
//!     Sed-style returns the substituted string (all matches are replaced).
//!     Plain regex returns the first capture group, or the full match without groups.
//!     Bool fields return "true" when the regex matches and nothing otherwise, so a
//!     non-matching file keeps its folder value (or support detection).
//!   values: { EnumValue1: "regex1", EnumValue2: "regex2", ... }
//!     Enum fields return the key of the first matching regex; takes precedence over
//!     `expression`.
//! Matching is always case-insensitive.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use fancy_regex::Regex;
use parking_lot::Mutex;
use serde_json::{Map, Value};

use super::yaml;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFieldType {
    String,
    Bool,
    Enum,
}

pub const KNOWN_RULE_TYPES: [&str; 1] = ["regex"];

pub type RuleSet = Map<String, Value>;

static REGEX_CACHE: LazyLock<Mutex<HashMap<String, Option<Arc<Regex>>>>> = LazyLock::new(Default::default);

static SED_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^s(.)(.+?)\1(.*?)\1([gimsxy]*)$").expect("sed pattern"));

/// Compiles (and caches) a case-insensitive regex with optional extra inline flags.
fn compile(pattern: &str, extra_flags: &str) -> Option<Arc<Regex>> {
    let key = format!("{extra_flags}\u{0}{pattern}");
    let mut cache = REGEX_CACHE.lock();
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    if cache.len() > 4096 {
        cache.clear();
    }
    let compiled = Regex::new(&format!("(?i{extra_flags}){pattern}")).ok().map(Arc::new);
    cache.insert(key, compiled.clone());
    compiled
}

fn get_ci<'a>(options: &'a RuleSet, key: &str) -> Option<&'a Value> {
    options.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
}

fn as_str(value: &Value) -> Option<&str> {
    value.as_str()
}

/// The path rules match against: the file's path relative to the library root with a
/// leading slash (e.g. "/Creator/Collection/model.stl"), so rules never depend on where the
/// library lives. `directory` is library-relative ("" for the root).
pub fn rule_path(directory: &str, file_name: &str) -> String {
    let directory = directory.replace('\\', "/");
    let directory = directory.trim_matches('/');
    if directory.is_empty() { format!("/{file_name}") } else { format!("/{directory}/{file_name}") }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn has_extension(path: &str) -> bool {
    let name = file_name(path);
    matches!(name.rfind('.'), Some(i) if i + 1 < name.len())
}

fn directory_name(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

fn input_for(file_path: &str, options: &RuleSet) -> Option<String> {
    let normalized = file_path.replace('\\', "/");
    let source = match get_ci(options, "source") {
        Some(v) => as_str(v)?.to_ascii_lowercase(),
        None => "full_path".to_string(),
    };
    Some(match source.as_str() {
        "folder" => {
            if has_extension(&normalized) {
                directory_name(&normalized)
            } else {
                normalized
            }
        }
        "filename" => file_name(&normalized).to_string(),
        _ => normalized,
    })
}

/// Why the first regex in a rule config (its `expression` or a `values` pattern) doesn't
/// compile; `None` when every pattern is valid.
pub fn pattern_error(rule_config: &Value) -> Option<String> {
    let Value::Object(config) = rule_config else {
        return None;
    };
    // fancy_regex reports "Parsing error at position N: <reason>", where N counts the
    // "(?i)" prefix; only the reason means anything to whoever wrote the rule.
    let check = |pattern: &str, flags: &str| -> Option<String> {
        Regex::new(&format!("(?i{flags}){pattern}")).err().map(|err| {
            let message = err.to_string();
            message.split_once(": ").map_or(message.clone(), |(_, reason)| reason.to_string())
        })
    };
    if let Some(expression) = get_ci(config, "expression").and_then(as_str).filter(|e| !e.is_empty()) {
        let error = match SED_PATTERN.captures(expression) {
            Ok(Some(sed)) => check(sed.get(2).map_or("", |m| m.as_str()), ""),
            _ => check(expression, ""),
        };
        if error.is_some() {
            return error;
        }
    }
    if let Some(Value::Object(values)) = get_ci(config, "values") {
        return values.values().filter_map(Value::as_str).filter(|p| !p.is_empty()).find_map(|p| check(p, ""));
    }
    None
}

/// Keys of every `values` entry whose regex matches, in declaration order.
pub fn parse_matching_value_keys(file_path: &str, options: &RuleSet) -> Vec<String> {
    let Some(input) = input_for(file_path, options) else {
        return Vec::new();
    };
    let Some(Value::Object(values)) = get_ci(options, "values") else {
        return Vec::new();
    };

    values
        .iter()
        .filter(|(_, pattern)| {
            let Some(pattern) = pattern.as_str().filter(|p| !p.is_empty()) else {
                return false;
            };
            compile(pattern, "").is_some_and(|re| re.is_match(&input).unwrap_or(false))
        })
        .map(|(key, _)| key.clone())
        .collect()
}

pub fn parse_value(file_path: &str, field_type: RuleFieldType, options: &RuleSet) -> Option<String> {
    let input = input_for(file_path, options)?;

    if field_type == RuleFieldType::Enum && matches!(get_ci(options, "values"), Some(Value::Object(_))) {
        return parse_matching_value_keys(file_path, options).into_iter().next();
    }

    let expression = get_ci(options, "expression").and_then(as_str).filter(|e| !e.is_empty())?;

    if let Ok(Some(sed)) = SED_PATTERN.captures(expression) {
        let pattern = sed.get(2).map_or("", |m| m.as_str());
        let replacement = convert_sed_replacement(sed.get(3).map_or("", |m| m.as_str()));
        let flags: String = sed
            .get(4)
            .map_or("", |m| m.as_str())
            .chars()
            .map(|c| c.to_ascii_lowercase())
            .filter(|c| matches!(c, 'm' | 's' | 'x'))
            .collect();
        let re = compile(pattern, &flags)?;
        return Some(re.replace_all(&input, replacement.as_str()).into_owned());
    }

    let re = compile(expression, "")?;
    if field_type == RuleFieldType::Bool {
        return re.is_match(&input).unwrap_or(false).then(|| "true".to_string());
    }

    let captures = re.captures(&input).ok()??;
    if captures.len() > 1 {
        Some(captures.get(1).map_or("", |m| m.as_str()).to_string())
    } else {
        captures.get(0).map(|m| m.as_str().to_string())
    }
}

/// Converts sed backreferences (`\1`) to replacement syntax (`${1}`), escaping literal `$`.
fn convert_sed_replacement(sed: &str) -> String {
    let mut out = String::with_capacity(sed.len());
    let chars: Vec<char> = sed.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && chars[end].is_ascii_digit() {
                end += 1;
            }
            out.push_str("${");
            out.extend(&chars[start..end]);
            out.push('}');
            i = end;
        } else if chars[i] == '$' && i + 1 < chars.len() && (chars[i + 1].is_ascii_digit() || chars[i + 1] == '{') {
            // Already in replacement syntax (e.g. "$1").
            out.push('$');
            i += 1;
        } else if chars[i] == '$' {
            out.push_str("$$");
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Evaluates a single rule config for a model file. A missing `rule` defaults to "regex";
/// unknown rule types return `None`.
pub fn evaluate(file_path: &str, rule_config: &Value, field_type: RuleFieldType) -> Option<String> {
    let Value::Object(config) = rule_config else {
        return None;
    };

    let rule_name = match config.get("rule") {
        Some(Value::String(name)) if !name.trim().is_empty() => name.to_ascii_lowercase(),
        Some(_) => return None,
        None => "regex".to_string(),
    };

    let options: RuleSet =
        config.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("rule")).map(|(k, v)| (k.clone(), v.clone())).collect();

    match rule_name.as_str() {
        "regex" => parse_value(file_path, field_type, &options),
        _ => None,
    }
}

/// Parses a rules YAML document (`field: { rule config }`). Returns an empty set when the
/// YAML is missing or invalid. All scalar values become strings.
pub fn deserialize_rules(yaml_text: Option<&str>) -> RuleSet {
    let Some(text) = yaml_text.filter(|t| !t.is_empty()) else {
        return RuleSet::new();
    };
    match yaml::parse_mapping(text) {
        Ok(mapping) => match yaml::to_loose_json(&serde_yaml::Value::Mapping(mapping)) {
            Value::Object(map) => map,
            _ => RuleSet::new(),
        },
        Err(_) => RuleSet::new(),
    }
}

pub fn serialize_rules(rules: &RuleSet) -> String {
    yaml::to_string(&yaml::from_json(&Value::Object(rules.clone())))
}

/// Inner YAML of one rule (without the field key), for editing in the UI.
pub fn rule_contents_yaml(rule: &Value) -> String {
    yaml::to_string(&yaml::from_json(rule)).trim_end().to_string()
}

/// Compact display snippet:
/// ```text
/// field:
///   source: filename
///   expression: ^(.*)$
/// ```
pub fn rule_config_to_yaml_snippet(field_name: &str, rule: &Value) -> String {
    let mut out = format!("{field_name}:\n");
    if let Value::Object(map) = rule {
        for (key, value) in map {
            let rendered = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            out.push_str(&format!("  {key}: {rendered}\n"));
        }
    }
    out.trim_end().to_string()
}

/// Validates each field's rule YAML; returns `field -> error message` for failures.
pub fn validate_rules<'a>(field_rules: impl IntoIterator<Item = (&'a String, &'a String)>) -> Vec<(String, String)> {
    let mut errors = Vec::new();
    for (field, rule_yaml) in field_rules {
        if rule_yaml.trim().is_empty() {
            continue;
        }
        let mapping = match yaml::parse_mapping(rule_yaml) {
            Ok(m) => m,
            Err(err) => {
                errors.push((field.clone(), format!("Invalid YAML: {err}")));
                continue;
            }
        };
        if mapping.is_empty() {
            errors.push((field.clone(), "Invalid YAML: rule is empty".to_string()));
            continue;
        }
        let rule_name =
            yaml::string_key(&mapping, "rule").and_then(yaml::scalar_to_string).unwrap_or_else(|| "regex".to_string());
        if !KNOWN_RULE_TYPES.iter().any(|k| k.eq_ignore_ascii_case(&rule_name)) {
            errors.push((
                field.clone(),
                format!("Unknown rule type \"{rule_name}\". Valid types: {}", KNOWN_RULE_TYPES.join(", ")),
            ));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn opts(value: Value) -> RuleSet {
        match value {
            Value::Object(m) => m,
            _ => panic!("object expected"),
        }
    }

    fn eval(path: &str, options: Value) -> Option<String> {
        parse_value(path, RuleFieldType::String, &opts(options))
    }

    // ── Rule paths ───────────────────────────────────────────────────────────

    #[test]
    fn rule_path_is_library_relative_with_leading_slash() {
        assert_eq!(rule_path("", "dragon.stl"), "/dragon.stl");
        assert_eq!(rule_path("Creator/Set", "elf.stl"), "/Creator/Set/elf.stl");
        assert_eq!(rule_path("/Creator\\Set/", "elf.stl"), "/Creator/Set/elf.stl");
    }

    /// The patterns the folder editor's presets generate, run against library-relative paths.
    #[test]
    fn preset_patterns_extract_expected_values() {
        let path = "/Loot Studios/Fantasy/Elf Rangers (2024)/Supported/elf_ps.stl";
        let cases = [
            ("^/([^/]+)/", Some("Loot Studios")),
            ("^/[^/]+/([^/]+)/", Some("Fantasy")),
            ("^/[^/]+/[^/]+/([^/]+)/", Some("Elf Rangers (2024)")),
            (r"^/[^/]+/[^/]+/([^/]+?)(?: \([^/]*\))?/", Some("Elf Rangers")),
            (r"^/[^/]+/[^/]+/([^/]+?)(?: - [^/]*)?/", Some("Elf Rangers (2024)")),
            ("/([^/]+)/[^/]+$", Some("Supported")),
            (r"/([^/]+)\.[^./]+$", Some("elf_ps")),
            ("^/[^/]+/[^/]+/[^/]+/[^/]+/[^/]+/([^/]+)/", None),
        ];
        for (expression, expected) in cases {
            assert_eq!(eval(path, json!({ "expression": expression })).as_deref(), expected, "{expression}");
        }
        assert_eq!(
            eval("/Epic Basing/Ruins - Set 2/x.stl", json!({ "expression": r"^/[^/]+/([^/]+?)(?: - [^/]*)?/" }))
                .as_deref(),
            Some("Ruins")
        );
    }

    #[test]
    fn supported_keywords_preset_skips_unsupported() {
        let rule = json!({ "expression": "(?<!un)supported|_ps(?![a-z0-9])" });
        let supported = |path: &str| parse_value(path, RuleFieldType::Bool, &opts(rule.clone())).is_some();
        assert!(supported("/A/Supported/elf.stl"));
        assert!(supported("/A/elf_presupported.stl"));
        assert!(supported("/A/elf-pre-supported.stl"));
        assert!(supported("/A/elf_ps.stl"));
        assert!(supported("/A/elf_ps_v2.stl"));
        assert!(!supported("/A/Unsupported/elf_UNSUPPORTED.stl"));
        assert!(!supported("/A/elf_psx.stl"));
    }

    #[test]
    fn keyword_presets_treat_underscores_as_separators() {
        let scale = json!({ "expression": "(?<![a-z0-9])(\\d+mm)(?![a-z0-9])" });
        assert_eq!(eval("/A/elf_32mm_ps.stl", scale.clone()).as_deref(), Some("32mm"));
        assert_eq!(eval("/A/75mm Hero/x.stl", scale.clone()).as_deref(), Some("75mm"));
        assert_eq!(eval("/A/v132mmx.stl", scale), None);

        let tags =
            opts(json!({ "values": { "Elf": "(?<![a-z0-9])Elf(?![a-z0-9])", "Orc": "(?<![a-z0-9])Orc(?![a-z0-9])" } }));
        assert_eq!(parse_matching_value_keys("/A/dark_elf_bust.stl", &tags), vec!["Elf"]);
        assert!(parse_matching_value_keys("/A/self_orcish.stl", &tags).is_empty());
    }

    #[test]
    fn pattern_error_reports_invalid_regexes_only() {
        assert_eq!(pattern_error(&json!({ "expression": "^/([^/]+)/" })), None);
        assert_eq!(
            pattern_error(&json!({ "expression": "([^/]+" })).as_deref(),
            Some("Opening parenthesis without closing parenthesis")
        );
        assert!(pattern_error(&json!({ "expression": "s|([a-z|x|" })).is_some());
        assert!(pattern_error(&json!({ "values": { "Bust": "bust", "Bad": "(" } })).is_some());
        assert_eq!(pattern_error(&json!("not a rule")), None);
    }

    // ── Regex parser ─────────────────────────────────────────────────────────

    #[test]
    fn plain_regex_returns_first_capture_group() {
        assert_eq!(
            eval("/models/Fantasy/elf.stl", json!({"source": "full_path", "expression": "/(Fantasy|SciFi)/"}))
                .as_deref(),
            Some("Fantasy")
        );
    }

    #[test]
    fn plain_regex_returns_full_match_without_groups() {
        assert_eq!(eval("/models/dragon.stl", json!({"expression": "dragon"})).as_deref(), Some("dragon"));
    }

    #[test]
    fn plain_regex_returns_none_without_match() {
        assert_eq!(eval("/models/dragon.stl", json!({"expression": "^unicorn"})), None);
    }

    #[test]
    fn plain_regex_is_case_insensitive() {
        assert_eq!(eval("/models/Dragon.stl", json!({"expression": "dragon"})).as_deref(), Some("Dragon"));
    }

    #[test]
    fn folder_source_uses_parent_directory_only() {
        assert_eq!(
            eval("/models/Fantasy/Elves/warrior.stl", json!({"source": "folder", "expression": "Elves"})).as_deref(),
            Some("Elves")
        );
        assert_eq!(eval("/models/Fantasy/warrior.stl", json!({"source": "folder", "expression": "warrior"})), None);
    }

    #[test]
    fn filename_source_uses_filename_only() {
        assert_eq!(
            eval("/models/Fantasy/elf_warrior.stl", json!({"source": "filename", "expression": "^(elf)"})).as_deref(),
            Some("elf")
        );
        assert_eq!(eval("/models/Fantasy/dragon.stl", json!({"source": "filename", "expression": "Fantasy"})), None);
    }

    #[test]
    fn full_path_source_is_default() {
        assert_eq!(
            eval("/models/Fantasy/warrior.stl", json!({"source": "full_path", "expression": "Fantasy"})).as_deref(),
            Some("Fantasy")
        );
        assert_eq!(eval("/models/Fantasy/warrior.stl", json!({"expression": "Fantasy"})).as_deref(), Some("Fantasy"));
    }

    #[test]
    fn bool_field_returns_true_or_nothing() {
        let o = opts(json!({"expression": "supported"}));
        assert_eq!(parse_value("/models/supported_dragon.stl", RuleFieldType::Bool, &o).as_deref(), Some("true"));
        assert_eq!(parse_value("/models/dragon.stl", RuleFieldType::Bool, &o), None);
    }

    #[test]
    fn enum_values_return_first_matching_key() {
        let o = opts(json!({"values": {"Fantasy": "fantasy", "SciFi": "sci.?fi"}}));
        assert_eq!(parse_value("/models/fantasy/elf.stl", RuleFieldType::Enum, &o).as_deref(), Some("Fantasy"));
        assert_eq!(parse_value("/models/historical/knight.stl", RuleFieldType::Enum, &o), None);
        let both = opts(json!({"values": {"Fantasy": "fantasy", "SciFi": "scifi"}}));
        assert_eq!(
            parse_value("/models/fantasy-scifi/model.stl", RuleFieldType::Enum, &both).as_deref(),
            Some("Fantasy")
        );
    }

    #[test]
    fn matching_value_keys_returns_all_matches_in_order() {
        let o = opts(
            json!({"source": "full_path", "values": {"Supported": "supported", "Miniature": "mini", "Bust": "bust"}}),
        );
        assert_eq!(
            parse_matching_value_keys("/models/fantasy/mini_knight_supported.stl", &o),
            vec!["Supported", "Miniature"]
        );
    }

    #[test]
    fn sed_substitution_replaces_all_matches() {
        assert_eq!(eval("fantasy-model.stl", json!({"expression": "s/-/_/"})).as_deref(), Some("fantasy_model.stl"));
        assert_eq!(
            eval("/fantasy-foo-bar.stl", json!({"expression": "s|-|_|g"})).as_deref(),
            Some("/fantasy_foo_bar.stl")
        );
    }

    #[test]
    fn sed_substitution_supports_backreferences_and_flags() {
        assert_eq!(
            eval("/models/Fantasy/elf.stl", json!({"source": "full_path", "expression": r"s/.*(Fantasy|SciFi).*/\1/"}))
                .as_deref(),
            Some("Fantasy")
        );
        assert_eq!(
            eval("DRAGON.stl", json!({"source": "filename", "expression": "s|dragon|replaced|i"})).as_deref(),
            Some("replaced.stl")
        );
    }

    #[test]
    fn missing_expression_returns_none() {
        assert_eq!(eval("/models/dragon.stl", json!({"source": "full_path"})), None);
    }

    // ── Rule registry ────────────────────────────────────────────────────────

    #[test]
    fn deserialize_rules_handles_missing_and_invalid_yaml() {
        assert!(deserialize_rules(None).is_empty());
        assert!(deserialize_rules(Some("")).is_empty());
        assert!(deserialize_rules(Some("{ invalid yaml [[[")).is_empty());
        assert!(deserialize_rules(Some("   \n\n ")).is_empty());
    }

    #[test]
    fn deserialize_rules_parses_field_keys() {
        let yaml = "creator:\n                source: folder\n                expression: \"^([^/]+)\"\ncollection:\n  rule: regex\n  source: folder\n  expression: \"^([^/]+)\"\n";
        let rules = deserialize_rules(Some(yaml));
        assert!(rules.contains_key("creator"));
        assert!(rules.contains_key("collection"));
        let parsed = deserialize_rules(Some("creator:\n  rule: regex\n"));
        assert_eq!(parsed["creator"]["rule"], json!("regex"));
    }

    #[test]
    fn evaluate_handles_rule_names() {
        assert_eq!(evaluate("/model.stl", &json!({"rule": "unknowntype"}), RuleFieldType::String), None);
        assert_eq!(evaluate("/model.stl", &json!({"source": "folder"}), RuleFieldType::String), None);
        assert_eq!(
            evaluate(
                "/models/dragon.stl",
                &json!({"source": "folder", "expression": "^/?([^/]+)"}),
                RuleFieldType::String
            )
            .as_deref(),
            Some("models")
        );
        assert_eq!(
            evaluate(
                "/models/Fantasy/warrior.stl",
                &json!({"rule": "regex", "expression": "Fantasy"}),
                RuleFieldType::String
            )
            .as_deref(),
            Some("Fantasy")
        );
        assert_eq!(
            evaluate(
                "Fantasy/Elves/warrior.stl",
                &json!({"rule": "regex", "source": "folder", "expression": "^([^/]+)"}),
                RuleFieldType::String
            )
            .as_deref(),
            Some("Fantasy")
        );
        assert_eq!(evaluate("/models/dragon.stl", &json!({"rule": "filename"}), RuleFieldType::String), None);
    }

    #[test]
    fn evaluate_bool_and_enum_rules() {
        let bool_rule = json!({"rule": "regex", "expression": "supported"});
        assert_eq!(evaluate("/models/supported_dragon.stl", &bool_rule, RuleFieldType::Bool).as_deref(), Some("true"));
        assert_eq!(evaluate("/models/dragon.stl", &bool_rule, RuleFieldType::Bool), None);
        let enum_rule = json!({"rule": "regex", "values": {"Fantasy": "fantasy", "SciFi": "scifi"}});
        assert_eq!(evaluate("/models/fantasy/elf.stl", &enum_rule, RuleFieldType::Enum).as_deref(), Some("Fantasy"));
    }

    // ── Validation ───────────────────────────────────────────────────────────

    fn validate(rules: &[(&str, &str)]) -> Vec<(String, String)> {
        let owned: Vec<(String, String)> = rules.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        validate_rules(owned.iter().map(|(a, b)| (a, b)))
    }

    fn has_error(errors: &[(String, String)], field: &str) -> bool {
        errors.iter().any(|(f, _)| f == field)
    }

    #[test]
    fn validate_accepts_regex_rules_with_or_without_rule_key() {
        assert!(validate(&[]).is_empty());
        assert!(validate(&[("creator", "source: folder\nexpression: ^([^/]+)")]).is_empty());
        assert!(validate(&[("creator", "rule: regex\nsource: folder\nexpression: ^([^/]+)")]).is_empty());
    }

    #[test]
    fn validate_rejects_unknown_rule_types() {
        let errors = validate(&[("creator", "rule: filename")]);
        assert!(errors[0].1.contains("filename"));
        let errors = validate(&[("creator", "rule: magic\nsource: folder")]);
        assert!(errors[0].1.contains("magic"));
    }

    #[test]
    fn validate_rejects_invalid_or_empty_yaml_and_skips_blank() {
        assert!(has_error(&validate(&[("creator", "{ this is: [invalid yaml")]), "creator"));
        assert!(has_error(&validate(&[("creator", "{}")]), "creator"));
        for blank in ["", "   ", "\t"] {
            assert!(validate(&[("creator", blank)]).is_empty());
        }
    }

    #[test]
    fn validate_checks_fields_independently() {
        let errors = validate(&[
            ("creator", "source: folder\nexpression: ^([^/]+)"),
            ("collection", "source: folder\nexpression: ([^/]+)$"),
            ("category", "rule: unknowntype"),
        ]);
        assert!(!has_error(&errors, "creator"));
        assert!(!has_error(&errors, "collection"));
        assert!(has_error(&errors, "category"));
        assert!(validate(&[("Creator", "source: folder")]).is_empty());
    }
}
