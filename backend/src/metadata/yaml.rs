//! YAML helpers. Config scalars are treated as strings (like an untyped YAML reader), so
//! `supported: true` and `supported: "true"` behave the same.

use anyhow::{Result, anyhow};
use serde_yaml::{Mapping, Value};

/// Parses a YAML document whose root must be a mapping; an empty document is an empty
/// mapping.
pub fn parse_mapping(text: &str) -> Result<Mapping> {
    match serde_yaml::from_str::<Value>(text)? {
        Value::Mapping(m) => Ok(m),
        Value::Null => Ok(Mapping::new()),
        Value::Tagged(t) => match t.value {
            Value::Mapping(m) => Ok(m),
            _ => Err(anyhow!("expected a mapping")),
        },
        _ => Err(anyhow!("expected a mapping")),
    }
}

pub fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Tagged(t) => scalar_to_string(&t.value),
        _ => None,
    }
}

/// Converts YAML to JSON with every scalar as a string.
pub fn to_loose_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Mapping(m) => serde_json::Value::Object(
            m.iter().filter_map(|(k, v)| Some((scalar_to_string(k)?, to_loose_json(v)))).collect(),
        ),
        Value::Sequence(s) => serde_json::Value::Array(s.iter().map(to_loose_json).collect()),
        Value::Tagged(t) => to_loose_json(&t.value),
        scalar => scalar_to_string(scalar).map_or(serde_json::Value::Null, serde_json::Value::String),
    }
}

pub fn from_json(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => serde_yaml::to_value(n).unwrap_or(Value::Null),
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(a) => Value::Sequence(a.iter().map(from_json).collect()),
        serde_json::Value::Object(o) => {
            Value::Mapping(o.iter().map(|(k, v)| (Value::String(k.clone()), from_json(v))).collect())
        }
    }
}

/// First entry whose key matches case-insensitively.
pub fn get_ci<'a>(mapping: &'a Mapping, key: &str) -> Option<(&'a str, &'a Value)> {
    mapping.iter().find_map(|(k, v)| match k {
        Value::String(s) if s.eq_ignore_ascii_case(key) => Some((s.as_str(), v)),
        _ => None,
    })
}

pub fn string_key<'a>(mapping: &'a Mapping, key: &str) -> Option<&'a Value> {
    mapping.get(Value::String(key.to_string()))
}

/// A float as a YAML number using its shortest round-trip form (so 0.1f32 stays 0.1).
pub fn f32_value(v: f32) -> Value {
    let shortest: f64 = format!("{v}").parse().unwrap_or(v as f64);
    serde_yaml::to_value(shortest).unwrap_or(Value::Null)
}

pub fn to_string(value: &Value) -> String {
    serde_yaml::to_string(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loose_json_turns_scalars_into_strings_and_keeps_order() {
        let mapping = parse_mapping("b: true\na: 1.5\nc:\n  - x\n  - 2\n").unwrap();
        let json = to_loose_json(&Value::Mapping(mapping));
        assert_eq!(json.to_string(), r#"{"b":"true","a":"1.5","c":["x","2"]}"#);
    }

    #[test]
    fn parse_mapping_rejects_scalars_and_accepts_empty() {
        assert!(parse_mapping("just a string").is_err());
        assert!(parse_mapping("").unwrap().is_empty());
        assert!(parse_mapping("{ invalid yaml [[[").is_err());
    }

    #[test]
    fn f32_values_serialise_in_shortest_form() {
        assert_eq!(to_string(&f32_value(0.1)).trim(), "0.1");
        assert_eq!(to_string(&f32_value(2.0)).trim(), "2.0");
    }
}
