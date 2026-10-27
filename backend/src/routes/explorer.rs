//! /api/explorer endpoints.

use axum::Router;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use serde::Deserialize;

use super::{CurrentUser, models_root, serve_file};
use crate::app::AppState;
use crate::dto::{RulePreviewRequest, UpdateDirectoryConfigRequest};
use crate::error::AppResult;
use crate::services::explorer;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/explorer", get(get_directory))
        .route("/api/explorer/file", get(get_file))
        .route("/api/explorer/config", get(get_config).put(update_config))
        .route("/api/explorer/rule-preview", post(preview_rule))
}

#[derive(Deserialize)]
struct PathQuery {
    #[serde(default)]
    path: String,
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_string()
}

async fn get_directory(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<PathQuery>,
) -> AppResult<Response> {
    let root = models_root(&state)?;
    Ok(Json(explorer::get_directory_contents(&state.db, &root, &normalize(&q.path)).await?).into_response())
}

/// Streams an explorer-previewable file (image or text) from the models root.
async fn get_file(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<PathQuery>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let root = models_root(&state)?;
    let (path, content_type) = explorer::resolve_previewable_file(&root, &q.path)?;
    serve_file(headers, &path, content_type, None, None).await
}

async fn get_config(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<PathQuery>,
) -> AppResult<Response> {
    Ok(Json(state.metadata_config.get_directory_config_detail(&normalize(&q.path)).await?).into_response())
}

/// Replaces a directory's local config: writes findamodel.yaml, updates the database and
/// re-resolves all descendants.
async fn update_config(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<PathQuery>,
    Json(body): Json<UpdateDirectoryConfigRequest>,
) -> AppResult<Response> {
    models_root(&state)?;
    Ok(Json(state.metadata_config.update_directory_config(&normalize(&q.path), &body).await?).into_response())
}

/// Runs a field's rule over the models under a folder, for previewing while it's edited.
async fn preview_rule(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(body): Json<RulePreviewRequest>,
) -> AppResult<Response> {
    let preview = explorer::preview_rule(&state.db, &normalize(&body.path), &body.field, &body.rule).await?;
    Ok(Json(preview).into_response())
}
