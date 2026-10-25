//! /api/plate endpoints: synchronous plate export and background export jobs.

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};

use super::{CurrentUser, IdPath, accepted, serve_temp_download};
use crate::app::AppState;
use crate::dto::GeneratePlateRequest;
use crate::error::{AppResult, not_found};
use crate::services::plate_export;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/plate/generate", post(generate))
        .route("/api/plate/jobs", post(create_job))
        .route("/api/plate/jobs/{id}", get(get_job))
        .route("/api/plate/jobs/{id}/file", get(download_job_file))
}

fn warning_headers(warning: Option<&str>, skipped: &[String]) -> Vec<(&'static str, String)> {
    let mut headers = Vec::new();
    if let Some(w) = warning.filter(|w| !w.trim().is_empty()) {
        headers.push(("X-Plate-Warning", w.to_string()));
    }
    if !skipped.is_empty() {
        headers.push(("X-Plate-Skipped-Models", skipped.join(",")));
    }
    headers
}

async fn generate(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(request): Json<GeneratePlateRequest>,
) -> AppResult<Response> {
    let (out, result) =
        plate_export::generate_plate(&state.models, &request, std::io::Cursor::new(Vec::new()), None).await?;
    let mut response = (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(result.content_type)),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("attachment; filename=\"{}\"", result.file_name)).expect("header"),
            ),
        ],
        out.into_inner(),
    )
        .into_response();
    for (name, value) in warning_headers(result.warning.as_deref(), &result.skipped_models) {
        if let Ok(v) = HeaderValue::from_str(&value) {
            response.headers_mut().insert(name, v);
        }
    }
    Ok(response)
}

async fn create_job(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(request): Json<GeneratePlateRequest>,
) -> AppResult<Response> {
    let job = state.plate_jobs.create_job(request).await?;
    Ok(accepted(format!("/api/plate/jobs/{}", job.job_id), job))
}

async fn get_job(State(state): State<AppState>, _user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    Ok(Json(state.plate_jobs.get_job(id).ok_or_else(not_found)?).into_response())
}

/// Downloads a completed job's file; the job and its temp file are removed afterwards.
async fn download_job_file(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
) -> AppResult<Response> {
    let file = state.plate_jobs.completed_file(id).ok_or_else(not_found)?;
    let format = file.path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let content_type = plate_export::content_type(&format);
    let jobs = state.plate_jobs.clone();
    serve_temp_download(
        &file.path,
        content_type,
        &file.file_name,
        warning_headers(file.warning.as_deref(), &file.skipped_models),
        move || jobs.remove_job(id),
    )
    .await
}
