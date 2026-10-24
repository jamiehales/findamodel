//! /api/printing-lists endpoints. List ids may be "active" for item and download routes.

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, post, put};
use serde::Deserialize;
use uuid::Uuid;

use super::{CurrentUser, IdPath, accepted, created, parse_id, serve_temp_download};
use crate::app::AppState;
use crate::dto::{
    NameRequest, UpdatePrintingListPrinterRequest, UpdatePrintingListSettingsRequest, UpsertPrintingListItemRequest,
};
use crate::error::{AppResult, bad_request, not_found};
use crate::services::printing_lists;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/printing-lists", get(get_lists).post(create_list))
        .route("/api/printing-lists/active", get(get_active_list))
        .route("/api/printing-lists/{id}", get(get_list).put(rename_list).delete(delete_list))
        .route("/api/printing-lists/{id}/activate", post(activate_list))
        .route("/api/printing-lists/{id}/settings", put(update_settings))
        .route("/api/printing-lists/{id}/printer", put(update_printer))
        .route("/api/printing-lists/{id}/items/{model_id}", put(upsert_item).delete(remove_item))
        .route("/api/printing-lists/{id}/items", delete(clear_items))
        .route("/api/printing-lists/{id}/download-jobs", post(create_download_job))
        .route("/api/printing-lists/download-jobs/{id}", get(get_download_job))
        .route("/api/printing-lists/download-jobs/{id}/file", get(download_job_file))
}

async fn resolve(state: &AppState, user: &CurrentUser, raw: &str) -> AppResult<Uuid> {
    printing_lists::resolve_list_id(&state.db, raw, user.id).await?.ok_or_else(not_found)
}

async fn get_lists(State(state): State<AppState>, user: CurrentUser) -> AppResult<Response> {
    Ok(Json(printing_lists::get_lists(&state.db, user.caller()).await?).into_response())
}

async fn get_active_list(State(state): State<AppState>, user: CurrentUser) -> AppResult<Response> {
    Ok(match printing_lists::get_active_list(&state.db, user.id).await? {
        Some(list) => Json(list).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    })
}

async fn get_list(State(state): State<AppState>, user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    Ok(Json(printing_lists::get_list(&state.db, id, user.caller()).await?).into_response())
}

async fn create_list(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<NameRequest>,
) -> AppResult<Response> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(bad_request("Name is required."));
    }
    let list = printing_lists::create_list(&state.db, user.id, name).await?;
    Ok(created(format!("/api/printing-lists/{}", list.id), list))
}

async fn rename_list(
    State(state): State<AppState>,
    user: CurrentUser,
    IdPath(id): IdPath,
    Json(body): Json<NameRequest>,
) -> AppResult<Response> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(bad_request("Name is required."));
    }
    Ok(Json(printing_lists::rename_list(&state.db, id, user.caller(), name).await?).into_response())
}

async fn delete_list(State(state): State<AppState>, user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    printing_lists::delete_list(&state.db, id, user.caller()).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn activate_list(State(state): State<AppState>, user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    printing_lists::activate_list(&state.db, id, user.caller()).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn update_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    IdPath(id): IdPath,
    Json(body): Json<UpdatePrintingListSettingsRequest>,
) -> AppResult<Response> {
    Ok(Json(printing_lists::update_settings(&state.db, id, user.caller(), &body.spawn_type, &body.hull_mode).await?)
        .into_response())
}

async fn update_printer(
    State(state): State<AppState>,
    user: CurrentUser,
    IdPath(id): IdPath,
    Json(body): Json<UpdatePrintingListPrinterRequest>,
) -> AppResult<Response> {
    Ok(Json(printing_lists::update_printer(&state.db, id, user.caller(), body.printer_config_id).await?)
        .into_response())
}

async fn upsert_item(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, model_id)): Path<(String, String)>,
    Json(body): Json<UpsertPrintingListItemRequest>,
) -> AppResult<Response> {
    let model_id = parse_id(&model_id)?;
    let list_id = resolve(&state, &user, &id).await?;
    Ok(Json(printing_lists::upsert_item(&state.db, list_id, user.caller(), model_id, body.quantity).await?)
        .into_response())
}

async fn remove_item(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, model_id)): Path<(String, String)>,
) -> AppResult<Response> {
    let model_id = parse_id(&model_id)?;
    let list_id = resolve(&state, &user, &id).await?;
    Ok(Json(printing_lists::upsert_item(&state.db, list_id, user.caller(), model_id, 0).await?).into_response())
}

async fn clear_items(State(state): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> AppResult<Response> {
    let list_id = resolve(&state, &user, &id).await?;
    Ok(Json(printing_lists::clear_items(&state.db, list_id, user.caller()).await?).into_response())
}

#[derive(Deserialize)]
struct DownloadQuery {
    #[serde(default)]
    flatten: bool,
}

async fn create_download_job(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Query(q): Query<DownloadQuery>,
) -> AppResult<Response> {
    let list_id = resolve(&state, &user, &id).await?;
    let job = state.archive_jobs.create_job(list_id, user.caller(), q.flatten).await?.ok_or_else(not_found)?;
    Ok(accepted(format!("/api/printing-lists/download-jobs/{}", job.job_id), job))
}

async fn get_download_job(State(state): State<AppState>, user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    Ok(Json(state.archive_jobs.get_job(id, user.caller()).ok_or_else(not_found)?).into_response())
}

/// Downloads a completed archive; the job and its temp file are removed afterwards.
async fn download_job_file(
    State(state): State<AppState>,
    user: CurrentUser,
    IdPath(id): IdPath,
) -> AppResult<Response> {
    let file = state.archive_jobs.completed_file(id, user.caller()).ok_or_else(not_found)?;
    let jobs = state.archive_jobs.clone();
    serve_temp_download(&file.path, "application/zip", &file.file_name, Vec::new(), move || jobs.remove_job(id)).await
}
