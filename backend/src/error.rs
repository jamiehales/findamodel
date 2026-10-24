//! Error type shared by services and HTTP handlers.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::error;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Invalid input (HTTP 400).
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Unprocessable(String),
    #[error("{0}")]
    Unauthorized(String),
    /// Unprocessable with a JSON body (e.g. per-field validation errors).
    #[error("validation failed")]
    UnprocessableJson(serde_json::Value),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Internal(err.into())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Internal(err.into())
    }
}

pub type AppResult<T> = Result<T, AppError>;

pub fn bad_request(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}

pub fn not_found() -> AppError {
    AppError::NotFound(String::new())
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m),
            AppError::Unprocessable(m) => (StatusCode::UNPROCESSABLE_ENTITY, m),
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            AppError::UnprocessableJson(v) => return (StatusCode::UNPROCESSABLE_ENTITY, Json(v)).into_response(),
            AppError::Internal(err) => {
                error!(target: "findamodel.Http", "Request failed: {err:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
            }
        };
        (status, body).into_response()
    }
}
