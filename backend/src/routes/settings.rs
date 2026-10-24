//! /api/settings endpoints: app config, initial setup, metadata dictionary, stats and logs.

use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post, put};
use serde::Deserialize;

use super::{CurrentUser, IdPath};
use crate::app::AppState;
use crate::dto::{
    ApplicationLogEntryDto, ApplicationLogsResponseDto, CreateMetadataDictionaryValueRequest, InitialSetupRequest,
    UpdateAppConfigRequest, UpdateMetadataDictionaryValueRequest,
};
use crate::error::{AppResult, bad_request};
use crate::logging::{SEVERITIES, severity_rank};
use crate::services::metadata_dictionary;
use crate::services::stats::{self, PreviewRuntimeInfo};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/config", get(get_config).put(update_config))
        .route("/api/settings/setup-status", get(get_setup_status))
        .route("/api/settings/setup-defaults", get(get_setup_defaults))
        .route("/api/settings/setup", post(complete_setup))
        .route("/api/settings/metadata-dictionary", get(get_dictionary).post(create_dictionary_value))
        .route("/api/settings/metadata-dictionary/{id}", put(update_dictionary_value).delete(delete_dictionary_value))
        .route("/api/settings/stats", get(get_stats))
        .route("/api/settings/logs", get(get_logs))
}

async fn get_config(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Response> {
    Ok(Json(state.app_config.get().await?).into_response())
}

async fn update_config(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(body): Json<UpdateAppConfigRequest>,
) -> AppResult<Response> {
    Ok(Json(state.app_config.update(&body).await?).into_response())
}

async fn get_setup_status(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Response> {
    Ok(Json(state.app_config.get_setup_status().await?).into_response())
}

async fn get_setup_defaults(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Response> {
    Ok(Json(state.app_config.get_initial_setup_defaults().await?).into_response())
}

async fn complete_setup(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(body): Json<InitialSetupRequest>,
) -> AppResult<Response> {
    Ok(Json(state.app_config.complete_initial_setup(&body).await?).into_response())
}

async fn get_dictionary(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Response> {
    Ok(Json(metadata_dictionary::get_overview(&state.db).await?).into_response())
}

async fn create_dictionary_value(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(body): Json<CreateMetadataDictionaryValueRequest>,
) -> AppResult<Response> {
    Ok(Json(metadata_dictionary::create(&state.db, &body.field, &body.value).await?).into_response())
}

async fn update_dictionary_value(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
    Json(body): Json<UpdateMetadataDictionaryValueRequest>,
) -> AppResult<Response> {
    Ok(Json(metadata_dictionary::update(&state.db, id, &body.value).await?).into_response())
}

async fn delete_dictionary_value(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
) -> AppResult<Response> {
    metadata_dictionary::delete(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn get_stats(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Response> {
    let preview = PreviewRuntimeInfo::from_state(state.preview.gpu_enabled(), state.preview.gpu());
    Ok(Json(stats::get(&state.db, &state.settings.environment, preview).await?).into_response())
}

#[derive(Deserialize)]
struct LogsQuery {
    channel: Option<String>,
    severity: Option<String>,
    limit: Option<usize>,
}

async fn get_logs(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<LogsQuery>,
) -> AppResult<Response> {
    let minimum = match q.severity.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(severity_rank(s).ok_or_else(|| bad_request("Invalid severity filter."))?),
        None => None,
    };
    let entries = state
        .log_buffer
        .get(q.channel.as_deref(), minimum, q.limit.unwrap_or(500))
        .into_iter()
        .map(|e| ApplicationLogEntryDto {
            timestamp: e.timestamp,
            severity: e.severity.to_string(),
            channel: e.channel,
            message: e.message,
            exception: e.exception,
        })
        .collect();
    Ok(Json(ApplicationLogsResponseDto {
        entries,
        available_channels: state.log_buffer.channels(),
        available_severities: SEVERITIES.iter().map(|s| s.to_string()).collect(),
    })
    .into_response())
}
