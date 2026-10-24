//! Catalog search: paginated, filtered model queries and facet options. All filtering and
//! pagination happens in SQL.

use sqlx::{QueryBuilder, Sqlite};

use crate::db::Db;
use crate::dto::{FilterOptionsDto, ModelQueryRequest, ModelQueryResult};
use crate::entities::ModelRow;
use crate::error::AppResult;
use crate::metadata::tags;
use crate::services::store;

const DEFAULT_LIMIT: i64 = 25;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Facet {
    Creator,
    Collection,
    Subcollection,
    Tags,
    Category,
    Type,
    Material,
    FileType,
}

fn like_term(term: &str) -> String {
    format!("%{}%", term.to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
}

fn push_in(qb: &mut QueryBuilder<'_, Sqlite>, column: &str, values: &[String]) {
    qb.push(format!(" AND {column} IN ("));
    let mut sep = qb.separated(", ");
    for v in values {
        sep.push_bind(v.clone());
    }
    sep.push_unseparated(")");
}

/// Appends WHERE conditions for every filter except `skip`.
fn push_filters(qb: &mut QueryBuilder<'_, Sqlite>, req: &ModelQueryRequest, skip: Option<Facet>) {
    qb.push(" WHERE 1 = 1");

    if let Some(search) = req.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let term = like_term(search);
        qb.push(" AND (lower(m.file_name) LIKE ")
            .push_bind(term.clone())
            .push(" ESCAPE '\\' OR lower(COALESCE(m.calculated_model_name, '')) LIKE ")
            .push_bind(term)
            .push(" ESCAPE '\\')");
    }

    if let Some(name) = req.model_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let term = like_term(name);
        qb.push(" AND (lower(COALESCE(m.calculated_model_name, '')) LIKE ")
            .push_bind(term.clone())
            .push(" ESCAPE '\\' OR lower(m.file_name) LIKE ")
            .push_bind(term)
            .push(" ESCAPE '\\')");
    }

    let facets: [(Facet, &str, &Vec<String>); 7] = [
        (Facet::Creator, "m.calculated_creator", &req.creator),
        (Facet::Collection, "m.calculated_collection", &req.collection),
        (Facet::Subcollection, "m.calculated_subcollection", &req.subcollection),
        (Facet::Category, "m.calculated_category", &req.category),
        (Facet::Type, "m.calculated_type", &req.item_type),
        (Facet::Material, "m.calculated_material", &req.material),
        (Facet::FileType, "m.file_type", &req.file_type),
    ];
    for (facet, column, values) in facets {
        if skip != Some(facet) && !values.is_empty() {
            push_in(qb, column, values);
        }
    }

    if skip != Some(Facet::Tags) {
        let requested: Vec<String> = tags::normalize(&req.tags).iter().map(|t| tags::fold_key(t)).collect();
        if !requested.is_empty() {
            qb.push(" AND EXISTS (SELECT 1 FROM model_tags t WHERE t.model_id = m.id");
            push_in(qb, "t.tag_key", &requested);
            qb.push(")");
        }
    }

    if let Some(supported) = req.supported {
        qb.push(" AND m.calculated_supported = ").push_bind(supported);
    }
}

fn merge_selected(options: Vec<String>, selected: &[String]) -> Vec<String> {
    let selected: Vec<String> = selected.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if selected.is_empty() {
        return options;
    }
    let mut merged: Vec<String> = Vec::new();
    for v in options.into_iter().chain(selected) {
        if !merged.iter().any(|m| tags::eq_ignore_case(m, &v)) {
            merged.push(v);
        }
    }
    merged.sort_by(|a, b| tags::cmp_ignore_case(a, b));
    merged
}

pub async fn query_models(db: &Db, req: &ModelQueryRequest) -> AppResult<ModelQueryResult> {
    let limit = req.limit.unwrap_or(DEFAULT_LIMIT).max(0);
    let offset = req.offset.unwrap_or(0).max(0);

    let mut count_qb = QueryBuilder::new("SELECT COUNT(*) FROM models m");
    push_filters(&mut count_qb, req, None);
    let total_count: i64 = count_qb.build_query_scalar().fetch_one(db).await?;

    let mut qb = QueryBuilder::new("SELECT m.* FROM models m");
    push_filters(&mut qb, req, None);
    qb.push(" ORDER BY m.directory, m.file_name LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
    let rows: Vec<ModelRow> = qb.build_query_as().fetch_all(db).await?;

    Ok(ModelQueryResult {
        models: rows.iter().map(store::to_model_dto).collect(),
        total_count,
        has_more: offset + limit < total_count,
    })
}

async fn distinct_column(db: &Db, req: &ModelQueryRequest, facet: Facet, column: &str) -> AppResult<Vec<String>> {
    let mut qb = QueryBuilder::new(format!("SELECT DISTINCT {column} FROM models m"));
    push_filters(&mut qb, req, Some(facet));
    qb.push(format!(" AND {column} IS NOT NULL ORDER BY {column}"));
    Ok(qb.build_query_scalar().fetch_all(db).await?)
}

/// Facet values available under the current filters. Each facet ignores its own
/// constraint (so other choices stay visible) and always includes selected values.
pub async fn get_filter_options(db: &Db, req: &ModelQueryRequest) -> AppResult<FilterOptionsDto> {
    let creators =
        merge_selected(distinct_column(db, req, Facet::Creator, "m.calculated_creator").await?, &req.creator);
    let collections =
        merge_selected(distinct_column(db, req, Facet::Collection, "m.calculated_collection").await?, &req.collection);
    let subcollections = merge_selected(
        distinct_column(db, req, Facet::Subcollection, "m.calculated_subcollection").await?,
        &req.subcollection,
    );
    let categories =
        merge_selected(distinct_column(db, req, Facet::Category, "m.calculated_category").await?, &req.category);
    let types = merge_selected(distinct_column(db, req, Facet::Type, "m.calculated_type").await?, &req.item_type);
    let materials =
        merge_selected(distinct_column(db, req, Facet::Material, "m.calculated_material").await?, &req.material);
    let file_types = merge_selected(distinct_column(db, req, Facet::FileType, "m.file_type").await?, &req.file_type);

    let mut qb = QueryBuilder::new("SELECT MIN(t.tag) FROM model_tags t JOIN models m ON m.id = t.model_id");
    push_filters(&mut qb, req, Some(Facet::Tags));
    qb.push(" GROUP BY t.tag_key");
    let mut tag_values: Vec<String> = qb.build_query_scalar().fetch_all(db).await?;
    tag_values.sort_by(|a, b| tags::cmp_ignore_case(a, b));
    let tag_options = tags::merge([tag_values.as_slice(), req.tags.as_slice()]);

    Ok(FilterOptionsDto {
        creators,
        collections,
        subcollections,
        tags: tag_options,
        categories,
        types,
        materials,
        file_types,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDb, model};

    fn with_tags(file: &str, dir: &str, tags_json: &str) -> ModelRow {
        let mut m = model(file, dir);
        m.calculated_tags_json = Some(tags_json.into());
        m
    }

    fn names(result: &ModelQueryResult) -> Vec<String> {
        result.models.iter().map(|m| m.name.clone()).collect()
    }

    #[tokio::test]
    async fn tags_filter_matches_any_selected_tag() {
        let t = TestDb::new().await;
        for m in [
            with_tags("dragon.stl", "fantasy", r#"["monster","small"]"#),
            with_tags("tank.stl", "scifi", r#"["vehicle","large"]"#),
            with_tags("base.stl", "bits", r#"["small","terrain"]"#),
        ] {
            t.insert_model(&m).await;
        }
        let req =
            ModelQueryRequest { tags: vec!["monster".into(), "VEHICLE".into()], limit: Some(50), ..Default::default() };
        let result = query_models(&t.db, &req).await.unwrap();
        assert_eq!(result.total_count, 2);
        assert!(!result.has_more);
        assert_eq!(names(&result), vec!["dragon", "tank"]);
    }

    #[tokio::test]
    async fn filter_options_include_distinct_tags() {
        let t = TestDb::new().await;
        t.insert_model(&with_tags("one.stl", "a", r#"["small","monster"]"#)).await;
        t.insert_model(&with_tags("two.stl", "b", r#"["large","monster"]"#)).await;
        let options = get_filter_options(&t.db, &ModelQueryRequest::default()).await.unwrap();
        assert_eq!(options.tags, vec!["large", "monster", "small"]);
    }

    #[tokio::test]
    async fn search_and_model_name_filters_combine() {
        let t = TestDb::new().await;
        for (file, name) in [
            ("orc-captain-axe.stl", "Orc Captain Axe"),
            ("orc-captain-banner.stl", "Orc Captain Banner"),
            ("elf-ranger-axe.stl", "Elf Ranger Axe"),
        ] {
            let mut m = model(file, "warband");
            m.calculated_model_name = Some(name.into());
            t.insert_model(&m).await;
        }
        let req = ModelQueryRequest {
            search: Some("axe".into()),
            model_name: Some("orc captain".into()),
            ..Default::default()
        };
        let result = query_models(&t.db, &req).await.unwrap();
        assert_eq!(names(&result), vec!["Orc Captain Axe"]);
    }

    #[tokio::test]
    async fn search_treats_like_wildcards_literally() {
        let t = TestDb::new().await;
        t.insert_model(&model("100%_done.stl", "")).await;
        t.insert_model(&model("other.stl", "")).await;
        let result =
            query_models(&t.db, &ModelQueryRequest { search: Some("%_".into()), ..Default::default() }).await.unwrap();
        assert_eq!(result.total_count, 1);
    }

    #[tokio::test]
    async fn pagination_reports_has_more() {
        let t = TestDb::new().await;
        for i in 0..5 {
            t.insert_model(&model(&format!("m{i}.stl"), "")).await;
        }
        let result = query_models(&t.db, &ModelQueryRequest { limit: Some(2), offset: Some(2), ..Default::default() })
            .await
            .unwrap();
        assert_eq!((result.models.len(), result.total_count, result.has_more), (2, 5, true));
        assert_eq!(names(&result), vec!["m2", "m3"]);
    }

    fn creator_model(file: &str, dir: &str, creator: &str, category: &str) -> ModelRow {
        let mut m = model(file, dir);
        m.calculated_creator = Some(creator.into());
        m.calculated_category = Some(category.into());
        m
    }

    #[tokio::test]
    async fn filter_options_scope_by_other_filters() {
        let t = TestDb::new().await;
        t.insert_model(&creator_model("red-dragon.stl", "a", "joe", "red")).await;
        t.insert_model(&creator_model("blue-tank.stl", "b", "alice", "blue")).await;
        let options =
            get_filter_options(&t.db, &ModelQueryRequest { category: vec!["red".into()], ..Default::default() })
                .await
                .unwrap();
        assert_eq!(options.creators, vec!["joe"]);
    }

    #[tokio::test]
    async fn filter_options_exclude_own_constraint_and_keep_selected_values() {
        let t = TestDb::new().await;
        t.insert_model(&creator_model("red-dragon.stl", "a", "joe", "red")).await;
        t.insert_model(&creator_model("blue-tank.stl", "b", "alice", "blue")).await;
        let req =
            ModelQueryRequest { creator: vec!["alice".into()], category: vec!["red".into()], ..Default::default() };
        let options = get_filter_options(&t.db, &req).await.unwrap();
        assert_eq!(options.creators, vec!["alice", "joe"]);
        assert_eq!(options.categories, vec!["blue", "red"]);
    }

    #[tokio::test]
    async fn filter_option_tags_are_case_insensitive_and_keep_selection() {
        let t = TestDb::new().await;
        let mut a = with_tags("red-dragon.stl", "a", r#"["Monster","Small"]"#);
        a.calculated_category = Some("red".into());
        let mut b = with_tags("red-knight.stl", "a", r#"["monster","printed"]"#);
        b.calculated_category = Some("red".into());
        let mut c = with_tags("blue-tank.stl", "b", r#"["vehicle"]"#);
        c.calculated_category = Some("blue".into());
        for m in [&a, &b, &c] {
            t.insert_model(m).await;
        }
        let req = ModelQueryRequest { category: vec!["red".into()], tags: vec!["ghost".into()], ..Default::default() };
        let options = get_filter_options(&t.db, &req).await.unwrap();
        assert!(options.tags.contains(&"ghost".to_string()));
        assert_eq!(options.tags.iter().filter(|v| v.eq_ignore_ascii_case("monster")).count(), 1);
        assert!(!options.tags.iter().any(|v| v.eq_ignore_ascii_case("vehicle")));
    }
}
