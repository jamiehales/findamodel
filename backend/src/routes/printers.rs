//! /api/printers endpoints.

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post, put};

use super::{CurrentUser, IdPath};
use crate::app::AppState;
use crate::dto::PrinterConfigRequest;
use crate::error::AppResult;
use crate::services::printers;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/printers", get(get_all).post(create))
        .route("/api/printers/{id}", put(update).delete(delete))
        .route("/api/printers/{id}/set-default", post(set_default))
}

async fn get_all(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Response> {
    Ok(Json(printers::get_all(&state.db).await?).into_response())
}

async fn create(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(body): Json<PrinterConfigRequest>,
) -> AppResult<Response> {
    Ok(Json(printers::create(&state.db, &body).await?).into_response())
}

async fn update(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
    Json(body): Json<PrinterConfigRequest>,
) -> AppResult<Response> {
    Ok(Json(printers::update(&state.db, id, &body).await?).into_response())
}

async fn delete(State(state): State<AppState>, _user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    printers::delete(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn set_default(State(state): State<AppState>, _user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    printers::set_default(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
