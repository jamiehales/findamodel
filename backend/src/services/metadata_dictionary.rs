//! Configured metadata values (categories, types, materials, tags) plus the values
//! observed across models and directory configs.

use uuid::Uuid;

use crate::db::{Db, now};
use crate::dto::{MetadataDictionaryFieldDto, MetadataDictionaryOverviewDto, MetadataDictionaryValueDto};
use crate::entities::MetadataDictionaryValue;
use crate::error::{AppError, AppResult, bad_request};
use crate::metadata::tags;

const ALLOWED_FIELDS: [&str; 4] = ["category", "type", "material", "tags"];

fn normalize_field(field: &str) -> AppResult<String> {
    let normalized = field.trim().to_ascii_lowercase();
    if !ALLOWED_FIELDS.contains(&normalized.as_str()) {
        return Err(bad_request(format!("Unsupported field '{field}'.")));
    }
    Ok(normalized)
}

fn normalize_value(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(bad_request("Value cannot be empty."));
    }
    Ok(trimmed.to_lowercase())
}

async fn observed(db: &Db, model_column: &str, dir_raw: &str, dir_resolved: &str) -> AppResult<Vec<String>> {
    let sql = format!(
        "SELECT {model_column} FROM models WHERE {model_column} IS NOT NULL AND {model_column} != ''
         UNION SELECT {dir_raw} FROM directory_configs WHERE {dir_raw} IS NOT NULL AND {dir_raw} != ''
         UNION SELECT {dir_resolved} FROM directory_configs WHERE {dir_resolved} IS NOT NULL AND {dir_resolved} != ''
         ORDER BY 1"
    );
    Ok(sqlx::query_scalar(&sql).fetch_all(db).await?)
}

async fn observed_tags(db: &Db) -> AppResult<Vec<String>> {
    let mut values: Vec<String> =
        sqlx::query_scalar("SELECT MIN(tag) FROM model_tags GROUP BY tag_key").fetch_all(db).await?;
    let dir_json: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT raw_tags_json FROM directory_configs WHERE raw_tags_json IS NOT NULL
         UNION SELECT tags_json FROM directory_configs WHERE tags_json IS NOT NULL",
    )
    .fetch_all(db)
    .await?;
    for json in dir_json {
        values.extend(tags::from_json(json.as_deref()));
    }
    let mut merged: Vec<String> = Vec::new();
    for v in values {
        if !merged.iter().any(|m| tags::eq_ignore_case(m, &v)) {
            merged.push(v);
        }
    }
    merged.sort_by(|a, b| tags::cmp_ignore_case(a, b));
    Ok(merged)
}

pub async fn get_overview(db: &Db) -> AppResult<MetadataDictionaryOverviewDto> {
    let configured: Vec<MetadataDictionaryValue> = sqlx::query_as(
        "SELECT * FROM metadata_dictionary_values WHERE field IN ('category', 'type', 'material', 'tags') ORDER BY field, value",
    )
    .fetch_all(db)
    .await?;
    let field = |name: &str, observed: Vec<String>| MetadataDictionaryFieldDto {
        configured: configured
            .iter()
            .filter(|v| v.field.eq_ignore_ascii_case(name))
            .map(|v| MetadataDictionaryValueDto { id: v.id, value: v.value.clone() })
            .collect(),
        observed,
    };

    Ok(MetadataDictionaryOverviewDto {
        category: field("category", observed(db, "calculated_category", "raw_category", "category").await?),
        item_type: field("type", observed(db, "calculated_type", "raw_type", "item_type").await?),
        material: field("material", observed(db, "calculated_material", "raw_material", "material").await?),
        tags: field("tags", observed_tags(db).await?),
    })
}

pub async fn create(db: &Db, field: &str, value: &str) -> AppResult<MetadataDictionaryValueDto> {
    let field = normalize_field(field)?;
    let normalized = normalize_value(value)?;
    let existing: Option<MetadataDictionaryValue> =
        sqlx::query_as("SELECT * FROM metadata_dictionary_values WHERE field = ? AND normalized_value = ?")
            .bind(&field)
            .bind(&normalized)
            .fetch_optional(db)
            .await?;
    if let Some(e) = existing {
        return Ok(MetadataDictionaryValueDto { id: e.id, value: e.value });
    }
    let id = Uuid::new_v4();
    let ts = now();
    sqlx::query(
        "INSERT INTO metadata_dictionary_values (id, field, value, normalized_value, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&field)
    .bind(value.trim())
    .bind(&normalized)
    .bind(ts)
    .bind(ts)
    .execute(db)
    .await?;
    Ok(MetadataDictionaryValueDto { id, value: value.trim().to_string() })
}

pub async fn update(db: &Db, id: Uuid, value: &str) -> AppResult<MetadataDictionaryValueDto> {
    let normalized = normalize_value(value)?;
    let entity: MetadataDictionaryValue = sqlx::query_as("SELECT * FROM metadata_dictionary_values WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(crate::error::not_found)?;
    let conflict: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM metadata_dictionary_values WHERE id != ? AND field = ? AND normalized_value = ?)",
    )
    .bind(id)
    .bind(&entity.field)
    .bind(&normalized)
    .fetch_one(db)
    .await?;
    if conflict {
        return Err(AppError::Conflict(format!("Value '{}' already exists for {}.", value.trim(), entity.field)));
    }
    sqlx::query("UPDATE metadata_dictionary_values SET value = ?, normalized_value = ?, updated_at = ? WHERE id = ?")
        .bind(value.trim())
        .bind(&normalized)
        .bind(now())
        .bind(id)
        .execute(db)
        .await?;
    Ok(MetadataDictionaryValueDto { id, value: value.trim().to_string() })
}

pub async fn delete(db: &Db, id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM metadata_dictionary_values WHERE id = ?").bind(id).execute(db).await?;
    if result.rows_affected() == 0 {
        return Err(crate::error::not_found());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDb, model};

    #[tokio::test]
    async fn overview_combines_configured_and_observed_values() {
        let t = TestDb::new().await;
        let mut m = model("a.stl", "");
        m.calculated_category = Some("Statue".into());
        m.calculated_tags_json = Some(r#"["Hero","small"]"#.into());
        t.insert_model(&m).await;

        let overview = get_overview(&t.db).await.unwrap();
        let configured: Vec<&str> = overview.category.configured.iter().map(|v| v.value.as_str()).collect();
        assert_eq!(configured, vec!["Bust", "Miniature", "Uncategorized"]);
        assert_eq!(overview.category.observed, vec!["Statue"]);
        assert_eq!(overview.tags.observed, vec!["Hero", "small"]);
        assert_eq!(overview.item_type.configured.len(), 2);
    }

    #[tokio::test]
    async fn create_update_delete_with_validation() {
        let t = TestDb::new().await;
        let created = create(&t.db, " Tags ", " Hero ").await.unwrap();
        assert_eq!(created.value, "Hero");
        assert_eq!(create(&t.db, "tags", "hero").await.unwrap().id, created.id);
        assert!(matches!(create(&t.db, "colour", "red").await, Err(AppError::BadRequest(_))));
        assert!(matches!(create(&t.db, "tags", "  ").await, Err(AppError::BadRequest(_))));

        let other = create(&t.db, "tags", "Villain").await.unwrap();
        assert!(matches!(update(&t.db, other.id, "HERO").await, Err(AppError::Conflict(_))));
        assert_eq!(update(&t.db, other.id, "Anti-hero").await.unwrap().value, "Anti-hero");

        delete(&t.db, other.id).await.unwrap();
        assert!(matches!(delete(&t.db, other.id).await, Err(AppError::NotFound(_))));
    }
}
