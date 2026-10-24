//! Instance statistics for the settings page.

use crate::db::Db;
use crate::dto::InstanceStatsDto;
use crate::error::AppResult;
use crate::geometry::hull::CURRENT_HULL_GENERATION_VERSION;
use crate::preview::{CURRENT_PREVIEW_GENERATION_VERSION, GpuState};

pub struct PreviewRuntimeInfo {
    pub gpu_enabled: bool,
    pub gpu_available: bool,
    pub renderer_name: String,
}

impl PreviewRuntimeInfo {
    pub fn from_state(gpu_enabled: bool, gpu: &GpuState) -> Self {
        Self { gpu_enabled, gpu_available: gpu.is_available(), renderer_name: gpu.renderer_name() }
    }
}

fn operating_system() -> String {
    let pretty = std::fs::read_to_string("/etc/os-release").ok().and_then(|text| {
        text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")).map(|v| v.trim_matches('"').to_string())
    });
    match pretty {
        Some(name) => format!("{name} ({})", std::env::consts::ARCH),
        None => format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH),
    }
}

async fn count(db: &Db, sql: &str) -> AppResult<i64> {
    Ok(sqlx::query_scalar(sql).fetch_one(db).await?)
}

pub async fn get(db: &Db, environment: &str, preview: PreviewRuntimeInfo) -> AppResult<InstanceStatsDto> {
    Ok(InstanceStatsDto {
        application_version: env!("CARGO_PKG_VERSION").to_string(),
        environment: environment.to_string(),
        framework_version: env!("FINDAMODEL_RUSTC_VERSION").to_string(),
        operating_system: operating_system(),
        preview_generation_version: CURRENT_PREVIEW_GENERATION_VERSION,
        hull_generation_version: CURRENT_HULL_GENERATION_VERSION,
        preview_gpu_enabled: preview.gpu_enabled,
        preview_gpu_available: preview.gpu_available,
        preview_renderer: preview.renderer_name,
        model_count: count(db, "SELECT COUNT(*) FROM models").await?,
        models_with_previews: count(db, "SELECT COUNT(*) FROM models WHERE preview_image_path IS NOT NULL").await?,
        directory_config_count: count(db, "SELECT COUNT(*) FROM directory_configs").await?,
        printing_list_count: count(db, "SELECT COUNT(*) FROM printing_lists").await?,
        metadata_dictionary_value_count: count(db, "SELECT COUNT(*) FROM metadata_dictionary_values").await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDb, model};

    #[tokio::test]
    async fn returns_counts_versions_and_gpu_flags() {
        let t = TestDb::new().await;
        let mut with_preview = model("model-a.stl", "heroes");
        with_preview.preview_image_path = Some("a.png".into());
        t.insert_model(&with_preview).await;
        t.insert_model(&model("model-b.obj", "terrain")).await;
        let owner = t.add_user("admin", true).await;
        crate::services::printing_lists::ensure_default_list(&t.db, owner).await.unwrap();

        let preview = PreviewRuntimeInfo {
            gpu_enabled: true,
            gpu_available: false,
            renderer_name: "llvmpipe (LLVM 15.0.6, 256 bits)".into(),
        };
        let stats = get(&t.db, "Development", preview).await.unwrap();

        assert!(!stats.application_version.is_empty());
        assert_eq!(stats.environment, "Development");
        assert!(stats.framework_version.starts_with("rustc"));
        assert!(!stats.operating_system.is_empty());
        assert_eq!(stats.preview_generation_version, CURRENT_PREVIEW_GENERATION_VERSION);
        assert_eq!(stats.hull_generation_version, CURRENT_HULL_GENERATION_VERSION);
        assert!(stats.preview_gpu_enabled && !stats.preview_gpu_available);
        assert_eq!(stats.preview_renderer, "llvmpipe (LLVM 15.0.6, 256 bits)");
        assert_eq!((stats.model_count, stats.models_with_previews), (2, 1));
        assert_eq!(stats.directory_config_count, 0);
        assert_eq!(stats.printing_list_count, 1);
        assert_eq!(stats.metadata_dictionary_value_count, 8);
    }
}
