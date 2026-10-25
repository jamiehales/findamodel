//! Database row types. Column names match field names (snake_case).

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Default, sqlx::FromRow)]
pub struct DirectoryConfig {
    pub id: Uuid,
    /// Relative to the models root, forward slashes, "" for the root.
    pub directory_path: String,
    pub parent_id: Option<Uuid>,

    // Raw (local) values, sourced exclusively from this directory's findamodel.yaml.
    pub raw_creator: Option<String>,
    pub raw_collection: Option<String>,
    pub raw_subcollection: Option<String>,
    pub raw_tags_json: Option<String>,
    pub raw_category: Option<String>,
    pub raw_type: Option<String>,
    pub raw_material: Option<String>,
    pub raw_supported: Option<bool>,
    pub raw_model_name: Option<String>,
    pub raw_part_name: Option<String>,
    pub raw_raft_height_mm: Option<f32>,

    // Resolved values: computed by walking ancestors; closest non-null wins.
    pub creator: Option<String>,
    pub collection: Option<String>,
    pub subcollection: Option<String>,
    pub model_name: Option<String>,
    pub part_name: Option<String>,
    pub tags_json: Option<String>,
    pub category: Option<String>,
    pub item_type: Option<String>,
    pub material: Option<String>,
    pub supported: Option<bool>,
    pub raft_height_mm: Option<f32>,

    /// Rule definitions as YAML (`field: { rule config }`), local and after inheritance.
    pub raw_rules_yaml: Option<String>,
    pub resolved_rules_yaml: Option<String>,

    /// SHA-256 of this directory's findamodel.yaml (None when no config file exists).
    pub local_config_file_hash: Option<String>,
    /// JSON object keyed by file name holding per-model overrides from `model_metadata`.
    pub raw_model_metadata_json: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Default, sqlx::FromRow)]
pub struct ModelRow {
    pub id: Uuid,
    /// SHA-256 of the file content (64 lowercase hex chars).
    pub checksum: String,
    pub file_name: String,
    /// Relative folder from the models root (forward slashes, "" for the root).
    pub directory: String,
    pub file_type: String,
    pub file_size: i64,
    pub file_modified_at: DateTime<Utc>,
    pub cached_at: DateTime<Utc>,
    /// Preview PNG path relative to the renders cache folder (None = not generated).
    pub preview_image_path: Option<String>,
    pub preview_generated_at: Option<DateTime<Utc>>,
    pub preview_generation_version: Option<i32>,
    pub directory_config_id: Option<Uuid>,

    // Metadata computed from the directory config with rules applied to this file.
    pub calculated_creator: Option<String>,
    pub calculated_collection: Option<String>,
    pub calculated_subcollection: Option<String>,
    pub calculated_tags_json: Option<String>,
    pub calculated_category: Option<String>,
    pub calculated_type: Option<String>,
    pub calculated_material: Option<String>,
    pub calculated_supported: Option<bool>,
    /// Supports found in the geometry (true only when confident, otherwise None). Shares the
    /// scan config checksum with the hulls.
    pub detected_supported: Option<bool>,
    pub calculated_model_name: Option<String>,
    pub calculated_part_name: Option<String>,

    // Hulls as JSON arrays of [x, z] pairs on the X-Z plane (bird's eye view, Y up).
    pub convex_hull: Option<String>,
    pub concave_hull: Option<String>,
    pub convex_sans_raft_hull: Option<String>,
    pub hull_generation_version: Option<i32>,
    pub hull_raft_height_mm: Option<f32>,
    /// Hash of every config input affecting hull generation (None = needs regeneration).
    pub scan_config_checksum: Option<String>,
    pub hull_generated_at: Option<DateTime<Utc>>,

    // Geometry metadata: Y-up, mm, centred (X/Z at origin, base at Y=0).
    pub dimension_x_mm: Option<f32>,
    pub dimension_y_mm: Option<f32>,
    pub dimension_z_mm: Option<f32>,
    pub sphere_centre_x: Option<f32>,
    pub sphere_centre_y: Option<f32>,
    pub sphere_centre_z: Option<f32>,
    pub sphere_radius: Option<f32>,
    pub geometry_calculated_at: Option<DateTime<Utc>>,
}

impl ModelRow {
    /// Whether the model has supports: its metadata when set, else the geometry detection.
    /// Mirrors the `effective_supported` column.
    pub fn supported(&self) -> Option<bool> {
        self.calculated_supported.or(self.detected_supported)
    }
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub is_admin: bool,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct AppConfigRow {
    pub id: i64,
    pub default_raft_height_mm: f32,
    pub generate_previews_enabled: bool,
    pub minimum_preview_generation_version: i32,
    pub setup_completed: bool,
    pub models_directory_path: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct PrinterConfig {
    pub id: Uuid,
    pub name: String,
    pub bed_width_mm: f32,
    pub bed_depth_mm: f32,
    pub is_built_in: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct PrintingList {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub is_active: bool,
    pub is_default: bool,
    pub spawn_type: String,
    pub hull_mode: String,
    pub printer_config_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct PrintingListItem {
    pub id: Uuid,
    pub printing_list_id: Uuid,
    pub model_id: Uuid,
    pub quantity: i32,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct MetadataDictionaryValue {
    pub id: Uuid,
    pub field: String,
    pub value: String,
    pub normalized_value: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct IndexRun {
    pub id: Uuid,
    pub directory_filter: Option<String>,
    pub relative_model_path: Option<String>,
    pub flags: i32,
    pub requested_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<f64>,
    /// running | queued | success | failed | cancelled
    pub status: String,
    /// success | failed | cancelled
    pub outcome: Option<String>,
    pub error: Option<String>,
    pub total_files: Option<i32>,
    pub processed_files: i32,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct IndexRunFile {
    pub id: Uuid,
    pub index_run_id: Uuid,
    pub relative_path: String,
    pub file_type: String,
    /// pending | processed | skipped | failed
    pub status: String,
    pub is_new: bool,
    pub was_updated: bool,
    pub generated_preview: bool,
    pub generated_hull: bool,
    pub message: Option<String>,
    pub duration_ms: Option<f64>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct IndexRunEvent {
    pub id: Uuid,
    pub index_run_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub level: String,
    pub message: String,
    pub relative_path: Option<String>,
}
