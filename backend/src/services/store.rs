//! Persistence helpers for model and directory config rows.

use std::path::Path;

use sqlx::{QueryBuilder, Sqlite, SqliteConnection};
use uuid::Uuid;

use crate::db::{Db, param_chunks, placeholders};
use crate::dto::{ModelDto, RelatedModelDto};
use crate::entities::{DirectoryConfig, ModelRow};
use crate::geometry::hull::DEFAULT_RAFT_HEIGHT_MM;
use crate::metadata::inheritance::DirectoryMap;
use crate::metadata::model_metadata::ComputedMetadata;
use crate::metadata::tags;
use crate::preview;

/// Preview URL for a model. The stored path is part of it because a different support
/// split is a different image, and exact-variant previews are cached as immutable.
pub fn preview_url(model_id: Uuid, preview_generation_version: Option<i32>, preview_image_path: &str) -> String {
    format!(
        "/api/models/{model_id}/preview?v={}&key={}&includeSupports=true",
        preview_generation_version.unwrap_or(0),
        preview::stem_of(preview_image_path)
    )
}

pub fn relative_path(directory: &str, file_name: &str) -> String {
    if directory.is_empty() { file_name.to_string() } else { format!("{directory}/{file_name}") }
}

pub fn file_stem(file_name: &str) -> String {
    Path::new(file_name).file_stem().map_or_else(|| file_name.to_string(), |s| s.to_string_lossy().into_owned())
}

pub fn to_model_dto(m: &ModelRow) -> ModelDto {
    ModelDto {
        id: m.id,
        name: m.calculated_model_name.clone().unwrap_or_else(|| file_stem(&m.file_name)),
        part_name: m.calculated_part_name.clone(),
        relative_path: relative_path(&m.directory, &m.file_name),
        file_type: m.file_type.clone(),
        can_export_to_plate: matches!(m.file_type.as_str(), "stl" | "obj"),
        file_size: m.file_size,
        file_url: format!("/api/models/{}/file", m.id),
        has_preview: m.preview_image_path.is_some(),
        preview_url: m.preview_image_path.as_deref().map(|p| preview_url(m.id, m.preview_generation_version, p)),
        creator: m.calculated_creator.clone(),
        collection: m.calculated_collection.clone(),
        subcollection: m.calculated_subcollection.clone(),
        tags: tags::from_json(m.calculated_tags_json.as_deref()),
        category: m.calculated_category.clone(),
        item_type: m.calculated_type.clone(),
        material: m.calculated_material.clone(),
        supported: m.supported(),
        convex_hull: m.convex_hull.clone(),
        concave_hull: m.concave_hull.clone(),
        convex_sans_raft_hull: m.convex_sans_raft_hull.clone(),
        raft_height_mm: m.hull_raft_height_mm.unwrap_or(DEFAULT_RAFT_HEIGHT_MM),
        dimension_x_mm: m.dimension_x_mm,
        dimension_y_mm: m.dimension_y_mm,
        dimension_z_mm: m.dimension_z_mm,
        sphere_centre_x: m.sphere_centre_x,
        sphere_centre_y: m.sphere_centre_y,
        sphere_centre_z: m.sphere_centre_z,
        sphere_radius: m.sphere_radius,
    }
}

pub fn to_related_dto(m: &ModelRow) -> RelatedModelDto {
    RelatedModelDto {
        id: m.id,
        name: m.calculated_model_name.clone().unwrap_or_else(|| file_stem(&m.file_name)),
        relative_path: relative_path(&m.directory, &m.file_name),
        file_type: m.file_type.clone(),
        file_size: m.file_size,
        preview_url: m.preview_image_path.as_deref().map(|p| preview_url(m.id, m.preview_generation_version, p)),
    }
}

pub fn apply_calculated_metadata(model: &mut ModelRow, metadata: &ComputedMetadata) {
    model.calculated_creator = metadata.creator.clone();
    model.calculated_collection = metadata.collection.clone();
    model.calculated_subcollection = metadata.subcollection.clone();
    model.calculated_tags_json = tags::to_json_or_null(Some(&metadata.tags));
    model.calculated_category = metadata.category.clone();
    model.calculated_type = metadata.item_type.clone();
    model.calculated_material = metadata.material.clone();
    model.calculated_supported = metadata.supported;
    model.calculated_model_name = metadata.model_name.clone();
    model.calculated_part_name = metadata.part_name.clone();
}

const MODEL_COLUMNS: [&str; 38] = [
    "id",
    "checksum",
    "file_name",
    "directory",
    "file_type",
    "file_size",
    "file_modified_at",
    "cached_at",
    "preview_image_path",
    "preview_generated_at",
    "preview_generation_version",
    "directory_config_id",
    "calculated_creator",
    "calculated_collection",
    "calculated_subcollection",
    "calculated_tags_json",
    "calculated_category",
    "calculated_type",
    "calculated_material",
    "calculated_supported",
    "detected_supported",
    "calculated_model_name",
    "calculated_part_name",
    "convex_hull",
    "concave_hull",
    "convex_sans_raft_hull",
    "hull_generation_version",
    "hull_raft_height_mm",
    "scan_config_checksum",
    "hull_generated_at",
    "dimension_x_mm",
    "dimension_y_mm",
    "dimension_z_mm",
    "sphere_centre_x",
    "sphere_centre_y",
    "sphere_centre_z",
    "sphere_radius",
    "geometry_calculated_at",
];

fn bind_model<'q>(q: &mut sqlx::query_builder::Separated<'_, 'q, Sqlite, &'static str>, m: &'q ModelRow) {
    q.push_bind(m.id)
        .push_bind(&m.checksum)
        .push_bind(&m.file_name)
        .push_bind(&m.directory)
        .push_bind(&m.file_type)
        .push_bind(m.file_size)
        .push_bind(m.file_modified_at)
        .push_bind(m.cached_at)
        .push_bind(&m.preview_image_path)
        .push_bind(m.preview_generated_at)
        .push_bind(m.preview_generation_version)
        .push_bind(m.directory_config_id)
        .push_bind(&m.calculated_creator)
        .push_bind(&m.calculated_collection)
        .push_bind(&m.calculated_subcollection)
        .push_bind(&m.calculated_tags_json)
        .push_bind(&m.calculated_category)
        .push_bind(&m.calculated_type)
        .push_bind(&m.calculated_material)
        .push_bind(m.calculated_supported)
        .push_bind(m.detected_supported)
        .push_bind(&m.calculated_model_name)
        .push_bind(&m.calculated_part_name)
        .push_bind(&m.convex_hull)
        .push_bind(&m.concave_hull)
        .push_bind(&m.convex_sans_raft_hull)
        .push_bind(m.hull_generation_version)
        .push_bind(m.hull_raft_height_mm)
        .push_bind(&m.scan_config_checksum)
        .push_bind(m.hull_generated_at)
        .push_bind(m.dimension_x_mm)
        .push_bind(m.dimension_y_mm)
        .push_bind(m.dimension_z_mm)
        .push_bind(m.sphere_centre_x)
        .push_bind(m.sphere_centre_y)
        .push_bind(m.sphere_centre_z)
        .push_bind(m.sphere_radius)
        .push_bind(m.geometry_calculated_at);
}

/// Inserts or fully replaces a model row and rewrites its tag index.
pub async fn save_model(conn: &mut SqliteConnection, model: &ModelRow) -> sqlx::Result<()> {
    let columns = &MODEL_COLUMNS;
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new(format!("INSERT INTO models ({}) VALUES (", columns.join(", ")));
    {
        let mut sep = qb.separated(", ");
        bind_model(&mut sep, model);
    }
    qb.push(") ON CONFLICT(id) DO UPDATE SET ");
    qb.push(columns[1..].iter().map(|c| format!("{c} = excluded.{c}")).collect::<Vec<_>>().join(", "));
    qb.build().execute(&mut *conn).await?;
    set_model_tags(conn, model.id, &tags::from_json(model.calculated_tags_json.as_deref())).await
}

pub async fn set_model_tags(conn: &mut SqliteConnection, model_id: Uuid, model_tags: &[String]) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM model_tags WHERE model_id = ?").bind(model_id).execute(&mut *conn).await?;
    for tag in model_tags {
        sqlx::query("INSERT OR IGNORE INTO model_tags (model_id, tag, tag_key) VALUES (?, ?, ?)")
            .bind(model_id)
            .bind(tag)
            .bind(tags::fold_key(tag))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

pub async fn get_model(db: &Db, id: Uuid) -> sqlx::Result<Option<ModelRow>> {
    sqlx::query_as("SELECT * FROM models WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn get_models_by_ids(db: &Db, ids: &[Uuid]) -> sqlx::Result<Vec<ModelRow>> {
    let mut rows = Vec::new();
    for chunk in param_chunks(ids) {
        let sql = format!("SELECT * FROM models WHERE id IN ({})", placeholders(chunk.len()));
        let mut q = sqlx::query_as::<_, ModelRow>(&sql);
        for id in chunk {
            q = q.bind(id);
        }
        rows.extend(q.fetch_all(db).await?);
    }
    Ok(rows)
}

pub async fn models_in_directories(conn: &mut SqliteConnection, dirs: &[String]) -> sqlx::Result<Vec<ModelRow>> {
    let mut rows = Vec::new();
    for chunk in param_chunks(dirs) {
        let sql = format!("SELECT * FROM models WHERE directory IN ({})", placeholders(chunk.len()));
        let mut q = sqlx::query_as::<_, ModelRow>(&sql);
        for d in chunk {
            q = q.bind(d);
        }
        rows.extend(q.fetch_all(&mut *conn).await?);
    }
    Ok(rows)
}

pub async fn load_directory_configs(conn: &mut SqliteConnection) -> sqlx::Result<DirectoryMap> {
    let rows: Vec<DirectoryConfig> = sqlx::query_as("SELECT * FROM directory_configs").fetch_all(&mut *conn).await?;
    Ok(rows.into_iter().map(|r| (r.directory_path.clone(), r)).collect())
}

pub async fn get_directory_config(db: &Db, path: &str) -> sqlx::Result<Option<DirectoryConfig>> {
    sqlx::query_as("SELECT * FROM directory_configs WHERE directory_path = ?").bind(path).fetch_optional(db).await
}

pub async fn save_directory_config(conn: &mut SqliteConnection, d: &DirectoryConfig) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO directory_configs (id, directory_path, parent_id, raw_creator, raw_collection, raw_subcollection,
            raw_tags_json, raw_category, raw_type, raw_material, raw_supported, raw_model_name, raw_part_name,
            raw_raft_height_mm, creator, collection, subcollection, model_name, part_name, tags_json, category,
            item_type, material, supported, raft_height_mm, raw_rules_yaml, resolved_rules_yaml,
            local_config_file_hash, raw_model_metadata_json, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            directory_path = excluded.directory_path, parent_id = excluded.parent_id,
            raw_creator = excluded.raw_creator, raw_collection = excluded.raw_collection,
            raw_subcollection = excluded.raw_subcollection, raw_tags_json = excluded.raw_tags_json,
            raw_category = excluded.raw_category, raw_type = excluded.raw_type, raw_material = excluded.raw_material,
            raw_supported = excluded.raw_supported, raw_model_name = excluded.raw_model_name,
            raw_part_name = excluded.raw_part_name, raw_raft_height_mm = excluded.raw_raft_height_mm,
            creator = excluded.creator, collection = excluded.collection, subcollection = excluded.subcollection,
            model_name = excluded.model_name, part_name = excluded.part_name, tags_json = excluded.tags_json,
            category = excluded.category, item_type = excluded.item_type, material = excluded.material,
            supported = excluded.supported, raft_height_mm = excluded.raft_height_mm,
            raw_rules_yaml = excluded.raw_rules_yaml, resolved_rules_yaml = excluded.resolved_rules_yaml,
            local_config_file_hash = excluded.local_config_file_hash,
            raw_model_metadata_json = excluded.raw_model_metadata_json, updated_at = excluded.updated_at",
    )
    .bind(d.id)
    .bind(&d.directory_path)
    .bind(d.parent_id)
    .bind(&d.raw_creator)
    .bind(&d.raw_collection)
    .bind(&d.raw_subcollection)
    .bind(&d.raw_tags_json)
    .bind(&d.raw_category)
    .bind(&d.raw_type)
    .bind(&d.raw_material)
    .bind(d.raw_supported)
    .bind(&d.raw_model_name)
    .bind(&d.raw_part_name)
    .bind(d.raw_raft_height_mm)
    .bind(&d.creator)
    .bind(&d.collection)
    .bind(&d.subcollection)
    .bind(&d.model_name)
    .bind(&d.part_name)
    .bind(&d.tags_json)
    .bind(&d.category)
    .bind(&d.item_type)
    .bind(&d.material)
    .bind(d.supported)
    .bind(d.raft_height_mm)
    .bind(&d.raw_rules_yaml)
    .bind(&d.resolved_rules_yaml)
    .bind(&d.local_config_file_hash)
    .bind(&d.raw_model_metadata_json)
    .bind(d.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::CURRENT_PREVIEW_GENERATION_VERSION;

    fn full_model() -> ModelRow {
        ModelRow {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            file_name: "dragon.stl".into(),
            directory: "Fantasy/Creatures".into(),
            file_type: "stl".into(),
            file_size: 1024,
            preview_image_path: Some("preview/abc.png".into()),
            preview_generation_version: Some(CURRENT_PREVIEW_GENERATION_VERSION),
            calculated_creator: Some("Alice".into()),
            calculated_collection: Some("Fantasy".into()),
            calculated_subcollection: Some("Creatures".into()),
            calculated_category: Some("miniature".into()),
            calculated_type: Some("whole".into()),
            calculated_material: Some("resin".into()),
            calculated_supported: Some(true),
            calculated_model_name: Some("Dragon".into()),
            convex_hull: Some("[[0,0],[1,0],[1,1]]".into()),
            concave_hull: Some("[[0,0],[0.5,0.1],[1,0],[1,1]]".into()),
            convex_sans_raft_hull: Some("[[0,0],[1,0]]".into()),
            hull_raft_height_mm: Some(2.0),
            dimension_x_mm: Some(50.0),
            dimension_y_mm: Some(30.0),
            dimension_z_mm: Some(20.0),
            sphere_centre_x: Some(0.0),
            sphere_centre_y: Some(15.0),
            sphere_centre_z: Some(0.0),
            sphere_radius: Some(25.0),
            ..Default::default()
        }
    }

    #[test]
    fn model_dto_maps_identity_paths_and_urls() {
        let dto = to_model_dto(&full_model());
        assert_eq!(dto.id.to_string(), "11111111-1111-1111-1111-111111111111");
        assert_eq!(dto.name, "Dragon");
        assert_eq!(dto.relative_path, "Fantasy/Creatures/dragon.stl");
        assert_eq!(dto.file_type, "stl");
        assert_eq!(dto.file_size, 1024);
        assert_eq!(dto.file_url, "/api/models/11111111-1111-1111-1111-111111111111/file");
        assert!(dto.has_preview);
        assert_eq!(
            dto.preview_url.as_deref(),
            Some(format!(
                "/api/models/11111111-1111-1111-1111-111111111111/preview?v={CURRENT_PREVIEW_GENERATION_VERSION}&key=preview/abc&includeSupports=true"
            ))
            .as_deref()
        );
    }

    #[test]
    fn model_dto_falls_back_to_file_stem_and_root_paths() {
        let mut model = full_model();
        model.calculated_model_name = None;
        model.file_name = "my_dragon.stl".into();
        model.directory = String::new();
        model.preview_image_path = None;
        let dto = to_model_dto(&model);
        assert_eq!(dto.name, "my_dragon");
        assert_eq!(dto.relative_path, "my_dragon.stl");
        assert!(!dto.has_preview);
        assert_eq!(dto.preview_url, None);
    }

    #[test]
    fn model_dto_maps_metadata_hulls_and_geometry() {
        let dto = to_model_dto(&full_model());
        assert_eq!(dto.creator.as_deref(), Some("Alice"));
        assert_eq!(dto.collection.as_deref(), Some("Fantasy"));
        assert_eq!(dto.subcollection.as_deref(), Some("Creatures"));
        assert_eq!(dto.category.as_deref(), Some("miniature"));
        assert_eq!(dto.item_type.as_deref(), Some("whole"));
        assert_eq!(dto.material.as_deref(), Some("resin"));
        assert_eq!(dto.supported, Some(true));
        assert_eq!(dto.convex_hull.as_deref(), Some("[[0,0],[1,0],[1,1]]"));
        assert_eq!(dto.concave_hull.as_deref(), Some("[[0,0],[0.5,0.1],[1,0],[1,1]]"));
        assert_eq!(dto.convex_sans_raft_hull.as_deref(), Some("[[0,0],[1,0]]"));
        assert_eq!(dto.raft_height_mm, 2.0);
        assert_eq!((dto.dimension_x_mm, dto.dimension_y_mm, dto.dimension_z_mm), (Some(50.0), Some(30.0), Some(20.0)));
        assert_eq!((dto.sphere_centre_y, dto.sphere_radius), (Some(15.0), Some(25.0)));

        let mut model = full_model();
        model.hull_raft_height_mm = None;
        assert_eq!(to_model_dto(&model).raft_height_mm, DEFAULT_RAFT_HEIGHT_MM);
    }

    #[test]
    fn model_dto_supported_prefers_metadata_over_detection() {
        let mut model = full_model();
        for (calculated, detected, expected) in [
            (Some(true), None, Some(true)),
            (Some(false), Some(true), Some(false)),
            (None, Some(true), Some(true)),
            (None, None, None),
        ] {
            model.calculated_supported = calculated;
            model.detected_supported = detected;
            assert_eq!(to_model_dto(&model).supported, expected, "{calculated:?} {detected:?}");
        }
    }

    #[tokio::test]
    async fn effective_supported_column_prefers_metadata_over_detection() {
        let t = crate::test_support::TestDb::new().await;
        for (name, calculated, detected) in
            [("a.stl", Some(false), Some(true)), ("b.stl", None, Some(true)), ("c.stl", None, None)]
        {
            let mut m = crate::test_support::model(name, "");
            m.calculated_supported = calculated;
            m.detected_supported = detected;
            t.insert_model(&m).await;
        }
        let rows: Vec<(String, Option<bool>)> =
            sqlx::query_as("SELECT file_name, effective_supported FROM models ORDER BY file_name")
                .fetch_all(&t.db)
                .await
                .unwrap();
        assert_eq!(rows, vec![("a.stl".into(), Some(false)), ("b.stl".into(), Some(true)), ("c.stl".into(), None)]);
    }

    #[test]
    fn model_dto_serialises_type_field_name() {
        let json = serde_json::to_value(to_model_dto(&full_model())).unwrap();
        assert_eq!(json["type"], "whole");
        assert!(json.get("itemType").is_none());
    }

    #[test]
    fn apply_calculated_metadata_sets_and_clears_fields() {
        let mut model = ModelRow::default();
        let metadata = ComputedMetadata {
            creator: Some("Bob".into()),
            collection: Some("SciFi".into()),
            subcollection: Some("Robots".into()),
            category: Some("bust".into()),
            item_type: Some("part".into()),
            material: Some("fdm".into()),
            supported: Some(false),
            model_name: Some("Robot Head".into()),
            ..Default::default()
        };
        apply_calculated_metadata(&mut model, &metadata);
        assert_eq!(model.calculated_creator.as_deref(), Some("Bob"));
        assert_eq!(model.calculated_type.as_deref(), Some("part"));
        assert_eq!(model.calculated_supported, Some(false));
        assert_eq!(model.calculated_model_name.as_deref(), Some("Robot Head"));

        apply_calculated_metadata(&mut model, &ComputedMetadata::default());
        assert_eq!(model.calculated_creator, None);
        assert_eq!(model.calculated_collection, None);
    }
}
