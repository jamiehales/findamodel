//! Tag list normalisation: whitespace collapsing, case-insensitive de-duplication,
//! length limits and case-insensitive ordering.

use std::cmp::Ordering;

pub const MAX_TAG_COUNT: usize = 64;
pub const MAX_TAG_LENGTH: usize = 64;

fn fold_char(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// Ordinal, case-insensitive string comparison.
pub fn cmp_ignore_case(a: &str, b: &str) -> Ordering {
    a.chars().map(fold_char).cmp(b.chars().map(fold_char))
}

pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    cmp_ignore_case(a, b) == Ordering::Equal
}

/// Case-folded key for case-insensitive lookups and indexing.
pub fn fold_key(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

fn normalize_single(input: &str) -> Option<String> {
    let collapsed = input.split(' ').filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
    let collapsed = collapsed.trim();
    (!collapsed.is_empty()).then(|| collapsed.to_string())
}

/// Cleans, de-duplicates (case-insensitively) and sorts tags.
pub fn normalize<I, S>(tags: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut result: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in tags {
        if result.len() >= MAX_TAG_COUNT {
            break;
        }
        let Some(mut cleaned) = normalize_single(raw.as_ref()) else {
            continue;
        };
        if cleaned.chars().count() > MAX_TAG_LENGTH {
            cleaned = cleaned.chars().take(MAX_TAG_LENGTH).collect::<String>().trim().to_string();
        }
        if !seen.insert(fold_key(&cleaned)) {
            continue;
        }
        result.push(cleaned);
    }
    result.sort_by(|a, b| cmp_ignore_case(a, b));
    result
}

pub fn merge<'a>(sources: impl IntoIterator<Item = &'a [String]>) -> Vec<String> {
    normalize(sources.into_iter().flatten())
}

pub fn to_json_or_null<I, S>(tags: Option<I>) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let normalized = normalize(tags?);
    (!normalized.is_empty()).then(|| serde_json::to_string(&normalized).expect("serialising tags"))
}

pub fn from_json(json: Option<&str>) -> Vec<String> {
    let Some(json) = json.filter(|j| !j.trim().is_empty()) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(json).map(normalize).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_whitespace_dedupes_and_sorts() {
        let tags = normalize(["  big   dragon ", "Small", "small", "", "   ", "alpha"]);
        assert_eq!(tags, vec!["alpha", "big dragon", "Small"]);
    }

    #[test]
    fn normalize_truncates_long_tags_and_limits_count() {
        let long = "x".repeat(100);
        assert_eq!(normalize([long.as_str()])[0].len(), MAX_TAG_LENGTH);
        let many: Vec<String> = (0..100).map(|i| format!("tag{i:03}")).collect();
        assert_eq!(normalize(&many).len(), MAX_TAG_COUNT);
    }

    #[test]
    fn json_round_trip() {
        assert_eq!(to_json_or_null(Some(["b", "a"])), Some("[\"a\",\"b\"]".to_string()));
        assert_eq!(to_json_or_null(Some(Vec::<String>::new())), None);
        assert_eq!(from_json(Some("[\"b\",\"a\"]")), vec!["a", "b"]);
        assert!(from_json(Some("not json")).is_empty());
        assert!(from_json(None).is_empty());
    }
}
