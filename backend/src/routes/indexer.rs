//! /api/indexer endpoints.

use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use serde::Deserialize;

use super::{CurrentUser, IdPath};
use crate::app::AppState;
use crate::dto::EnqueueIndexRequest;
use crate::error::{AppResult, bad_request, not_found};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/indexer", get(get_status).post(enqueue))
        .route("/api/indexer/runs", get(get_runs))
        .route("/api/indexer/runs/{id}", get(get_run).delete(cancel_run))
}

async fn get_status(State(state): State<AppState>, _user: CurrentUser) -> Response {
    Json(state.indexer.get_status()).into_response()
}

/// Enqueues a request; a queued request for the same target is moved to the front with
/// merged flags.
async fn enqueue(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(body): Json<EnqueueIndexRequest>,
) -> AppResult<Response> {
    let has_filter = body.directory_filter.as_deref().is_some_and(|f| !f.trim().is_empty());
    let has_model = body.relative_model_path.as_deref().is_some_and(|f| !f.trim().is_empty());
    if has_filter && has_model {
        return Err(bad_request("Specify either directoryFilter or relativeModelPath, not both."));
    }
    let result =
        state.indexer.enqueue(body.directory_filter.as_deref(), body.relative_model_path.as_deref(), body.flags);
    Ok(Json(result).into_response())
}

#[derive(Deserialize)]
struct RunsQuery {
    days: Option<i64>,
}

async fn get_runs(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<RunsQuery>,
) -> AppResult<Response> {
    Ok(Json(state.indexer.get_history(q.days.unwrap_or(7), 250).await?).into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunQuery {
    files_page: Option<i64>,
    files_page_size: Option<i64>,
    files_view: Option<String>,
    events_page: Option<i64>,
    events_page_size: Option<i64>,
}

async fn get_run(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
    Query(q): Query<RunQuery>,
) -> AppResult<Response> {
    let detail = state
        .indexer
        .get_run_detail(
            id,
            q.files_page.unwrap_or(1),
            q.files_page_size.unwrap_or(200),
            q.files_view.as_deref().unwrap_or("all"),
            q.events_page.unwrap_or(1),
            q.events_page_size.unwrap_or(200),
        )
        .await?
        .ok_or_else(not_found)?;
    Ok(Json(detail).into_response())
}

async fn cancel_run(State(state): State<AppState>, _user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    if !state.indexer.cancel(id).await? {
        return Err(not_found());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
