//! API request and response types. JSON uses camelCase field names.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use uuid::Uuid;

pub use crate::metadata::config_reader::UpdateDirectoryConfigRequest;
pub use crate::metadata::model_metadata::ModelMetadataEntry;

pub type StringMap = Map<String, serde_json::Value>;

pub fn string_map(pairs: impl IntoIterator<Item = (String, String)>) -> StringMap {
    pairs.into_iter().map(|(k, v)| (k, serde_json::Value::String(v))).collect()
}

// ── Models ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelDto {
    pub id: Uuid,
    pub name: String,
    pub part_name: Option<String>,
    pub relative_path: String,
    pub file_type: String,
    pub can_export_to_plate: bool,
    pub file_size: i64,
    pub file_url: String,
    pub has_preview: bool,
    pub preview_url: Option<String>,
    pub creator: Option<String>,
    pub collection: Option<String>,
    pub subcollection: Option<String>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    pub material: Option<String>,
    pub supported: Option<bool>,
    pub convex_hull: Option<String>,
    pub concave_hull: Option<String>,
    pub convex_sans_raft_hull: Option<String>,
    pub raft_height_mm: f32,
    pub dimension_x_mm: Option<f32>,
    pub dimension_y_mm: Option<f32>,
    pub dimension_z_mm: Option<f32>,
    pub sphere_centre_x: Option<f32>,
    pub sphere_centre_y: Option<f32>,
    pub sphere_centre_z: Option<f32>,
    pub sphere_radius: Option<f32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelatedModelDto {
    pub id: Uuid,
    pub name: String,
    pub relative_path: String,
    pub file_type: String,
    pub file_size: i64,
    pub preview_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelQueryRequest {
    pub search: Option<String>,
    pub model_name: Option<String>,
    pub creator: Vec<String>,
    pub collection: Vec<String>,
    pub subcollection: Vec<String>,
    pub tags: Vec<String>,
    pub category: Vec<String>,
    #[serde(rename = "type")]
    pub item_type: Vec<String>,
    pub material: Vec<String>,
    pub file_type: Vec<String>,
    pub supported: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelQueryResult {
    pub models: Vec<ModelDto>,
    pub total_count: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptionsDto {
    pub creators: Vec<String>,
    pub collections: Vec<String>,
    pub subcollections: Vec<String>,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub types: Vec<String>,
    pub materials: Vec<String>,
    pub file_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsByIdsRequest {
    pub ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateModelMetadataRequest {
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

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelMetadataDetail {
    pub local_values: ModelMetadataEntry,
    pub inherited_values: Option<ModelMetadataEntry>,
}

// ── Explorer ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFieldsDto {
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
    pub model_name: Option<String>,
    pub part_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryConfigDetailDto {
    pub directory_path: String,
    pub local_values: ConfigFieldsDto,
    pub parent_resolved_values: Option<ConfigFieldsDto>,
    pub parent_path: Option<String>,
    pub local_rule_fields: Option<Vec<String>>,
    pub local_rule_contents: Option<StringMap>,
    pub parent_resolved_rules: Option<StringMap>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderItemDto {
    pub name: String,
    pub path: String,
    pub subdirectory_count: i64,
    pub model_count: i64,
    pub resolved_values: ConfigFieldsDto,
    pub rule_configs: Option<StringMap>,
    pub local_values: Option<ConfigFieldsDto>,
    pub local_rule_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerModelItemDto {
    pub id: Option<String>,
    pub file_name: String,
    pub relative_path: String,
    pub file_type: String,
    pub file_size: Option<i64>,
    pub has_preview: bool,
    pub preview_url: Option<String>,
    pub resolved_metadata: Option<ConfigFieldsDto>,
    pub rule_configs: Option<StringMap>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerFileItemDto {
    pub file_name: String,
    pub relative_path: String,
    pub file_type: String,
    pub file_size: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerResponseDto {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub folders: Vec<FolderItemDto>,
    pub models: Vec<ExplorerModelItemDto>,
    pub files: Vec<ExplorerFileItemDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigValidationErrorResponse {
    pub field_errors: StringMap,
}

// ── Indexer ──────────────────────────────────────────────────────────────────

pub mod index_flags {
    pub const DIRECTORIES: i32 = 1;
    pub const MODELS: i32 = 2;
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexRequestDto {
    pub id: Uuid,
    pub run_id: Option<Uuid>,
    pub directory_filter: Option<String>,
    pub relative_model_path: Option<String>,
    pub flags: i32,
    pub requested_at: DateTime<Utc>,
    pub total_files: Option<i32>,
    pub processed_files: i32,
    /// "queued" | "running"
    pub status: String,
    pub is_cancellation_requested: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedIndexRequestDto {
    pub id: Uuid,
    pub run_id: Option<Uuid>,
    pub directory_filter: Option<String>,
    pub relative_model_path: Option<String>,
    pub flags: i32,
    pub requested_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: f64,
    pub outcome: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRunSummaryDto {
    pub id: Uuid,
    pub directory_filter: Option<String>,
    pub relative_model_path: Option<String>,
    pub flags: i32,
    pub requested_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub total_files: Option<i32>,
    pub processed_files: i32,
    pub status: String,
    pub outcome: Option<String>,
    pub error: Option<String>,
    pub is_cancellation_requested: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRunFileDto {
    pub relative_path: String,
    pub file_type: String,
    pub status: String,
    pub is_new: bool,
    pub was_updated: bool,
    pub generated_preview: bool,
    pub generated_hull: bool,
    pub message: Option<String>,
    pub duration_ms: Option<f64>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedResultDto<T> {
    pub items: Vec<T>,
    pub page: i64,
    pub page_size: i64,
    pub total_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRunEventDto {
    pub created_at: DateTime<Utc>,
    pub level: String,
    pub message: String,
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRunDetailDto {
    pub run: IndexRunSummaryDto,
    pub files: PagedResultDto<IndexRunFileDto>,
    pub events: PagedResultDto<IndexRunEventDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueIndexRequest {
    pub directory_filter: Option<String>,
    pub relative_model_path: Option<String>,
    #[serde(default)]
    pub flags: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexerStatusDto {
    pub is_running: bool,
    pub current_request: Option<IndexRequestDto>,
    pub queue: Vec<IndexRequestDto>,
    pub recent: Vec<CompletedIndexRequestDto>,
}

// ── Plate export ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementDto {
    pub model_id: String,
    #[serde(default)]
    pub instance_index: i32,
    pub x_mm: f64,
    pub y_mm: f64,
    #[serde(default)]
    pub angle_rad: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratePlateRequest {
    pub placements: Vec<PlacementDto>,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlateGenerationJobDto {
    pub job_id: Uuid,
    pub file_name: String,
    pub format: String,
    pub status: String,
    pub total_entries: i64,
    pub completed_entries: i64,
    pub progress_percent: i64,
    pub current_entry_name: Option<String>,
    pub error_message: Option<String>,
    pub warning: Option<String>,
    pub skipped_models: Vec<String>,
}

// ── Printers and printing lists ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrinterConfigDto {
    pub id: Uuid,
    pub name: String,
    pub bed_width_mm: f32,
    pub bed_depth_mm: f32,
    pub is_built_in: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterConfigRequest {
    pub name: String,
    pub bed_width_mm: f32,
    pub bed_depth_mm: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrintingListSummaryDto {
    pub id: Uuid,
    pub name: String,
    pub is_active: bool,
    pub is_default: bool,
    pub spawn_type: String,
    pub hull_mode: String,
    pub created_at: DateTime<Utc>,
    pub owner_username: Option<String>,
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrintingListPrinterInfoDto {
    pub id: Uuid,
    pub name: String,
    pub bed_width_mm: f32,
    pub bed_depth_mm: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrintingListItemDto {
    pub id: Uuid,
    pub model_id: Uuid,
    pub quantity: i32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrintingListDetailDto {
    pub id: Uuid,
    pub name: String,
    pub is_active: bool,
    pub is_default: bool,
    pub spawn_type: String,
    pub hull_mode: String,
    pub created_at: DateTime<Utc>,
    pub owner_username: Option<String>,
    pub items: Vec<PrintingListItemDto>,
    pub printer: Option<PrintingListPrinterInfoDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameRequest {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePrintingListSettingsRequest {
    #[serde(default)]
    pub spawn_type: String,
    #[serde(default)]
    pub hull_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePrintingListPrinterRequest {
    pub printer_config_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertPrintingListItemRequest {
    pub quantity: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintingListArchiveJobDto {
    pub job_id: Uuid,
    pub file_name: String,
    pub status: String,
    pub total_entries: i64,
    pub completed_entries: i64,
    pub progress_percent: i64,
    pub current_entry_name: Option<String>,
    pub error_message: Option<String>,
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigDto {
    pub default_raft_height_mm: f32,
    pub theme: String,
    pub generate_previews_enabled: bool,
    pub minimum_preview_generation_version: i32,
    pub setup_completed: bool,
    pub models_directory_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppConfigRequest {
    pub default_raft_height_mm: f32,
    pub theme: String,
    pub generate_previews_enabled: bool,
    pub minimum_preview_generation_version: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialSetupRequest {
    pub models_directory_path: String,
    pub default_raft_height_mm: f32,
    pub theme: String,
    pub generate_previews_enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatusDto {
    pub setup_completed: bool,
    pub requires_wizard: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitialSetupDefaultsDto {
    pub models_directory_path: Option<String>,
    pub default_raft_height_mm: f32,
    pub theme: String,
    pub generate_previews_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationLogEntryDto {
    pub timestamp: DateTime<Utc>,
    pub severity: String,
    pub channel: String,
    pub message: String,
    pub exception: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationLogsResponseDto {
    pub entries: Vec<ApplicationLogEntryDto>,
    pub available_channels: Vec<String>,
    pub available_severities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStatsDto {
    pub application_version: String,
    pub environment: String,
    pub framework_version: String,
    pub operating_system: String,
    pub preview_generation_version: i32,
    pub hull_generation_version: i32,
    pub preview_gpu_enabled: bool,
    pub preview_gpu_available: bool,
    pub preview_renderer: String,
    pub model_count: i64,
    pub models_with_previews: i64,
    pub directory_config_count: i64,
    pub printing_list_count: i64,
    pub metadata_dictionary_value_count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetadataDictionaryValueDto {
    pub id: Uuid,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataDictionaryFieldDto {
    pub configured: Vec<MetadataDictionaryValueDto>,
    pub observed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataDictionaryOverviewDto {
    pub category: MetadataDictionaryFieldDto,
    #[serde(rename = "type")]
    pub item_type: MetadataDictionaryFieldDto,
    pub material: MetadataDictionaryFieldDto,
    pub tags: MetadataDictionaryFieldDto,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMetadataDictionaryValueRequest {
    pub field: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadataDictionaryValueRequest {
    pub value: String,
}
