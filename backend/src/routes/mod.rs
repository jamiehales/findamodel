//! HTTP routing, authentication and shared request helpers.

mod explorer;
mod indexer;
mod models;
mod plate;
mod printers;
mod printing_lists;
mod settings;

use std::path::{Path as FsPath, PathBuf};

use axum::Router;
use axum::body::Body;
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use futures::StreamExt;
use serde_json::json;
use tower::ServiceExt;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

use crate::app::AppState;
use crate::config::RuntimeMode;
use crate::error::{AppError, AppResult};
use crate::services::printing_lists::Caller;
use crate::services::users;

pub const DESKTOP_TOKEN_HEADER: &str = "x-findamodel-desktop-token";

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .merge(models::routes())
        .merge(explorer::routes())
        .merge(indexer::routes())
        .merge(plate::routes())
        .merge(printers::routes())
        .merge(printing_lists::routes())
        .merge(settings::routes())
        .route("/api/users/me", get(current_user))
        .route_layer(middleware::from_fn_with_state(state.clone(), desktop_token_guard));

    let mut app = Router::new().route("/health", get(health)).merge(api);

    if let Some(web_root) = state.settings.web_root.clone().filter(|p| p.is_dir()) {
        let index = web_root.join("index.html");
        app = app.fallback_service(ServeDir::new(&web_root).fallback(ServeFile::new(index)));
    }

    let mut app = app.layer(CompressionLayer::new().compress_when(compressible));
    // The desktop window loads the frontend from the webview's own origin and calls this
    // backend on 127.0.0.1, so every request (and its token header) is cross-origin. Outermost,
    // so preflights are answered before the token guard sees them.
    if state.settings.mode == RuntimeMode::Desktop {
        app = app.layer(
            CorsLayer::new()
                .allow_origin(DESKTOP_WEBVIEW_ORIGINS.map(HeaderValue::from_static))
                .allow_headers(Any)
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::PATCH, Method::OPTIONS])
                .expose_headers(Any),
        );
    }
    if state.settings.is_development() && state.settings.mode == RuntimeMode::Server && !state.settings.disable_cors {
        app = app.layer(
            CorsLayer::new()
                .allow_origin(HeaderValue::from_static("http://localhost:5173"))
                .allow_headers(Any)
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::PATCH, Method::OPTIONS])
                .expose_headers(Any),
        );
    }
    app.with_state(state)
}

/// Origins the Tauri webview serves the desktop frontend from: `tauri://localhost` on macOS
/// and Linux, `http(s)://tauri.localhost` on Windows.
const DESKTOP_WEBVIEW_ORIGINS: [&str; 3] = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];

/// Compress JSON and mesh payloads only; exports and previews are already compressed and
/// downloads keep their Content-Length for progress reporting.
fn compressible(_: StatusCode, _: axum::http::Version, headers: &HeaderMap, _: &axum::http::Extensions) -> bool {
    let Some(content_type) = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let large_enough = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_none_or(|len| len > 256);
    large_enough
        && ["application/json", "model/gltf-binary", crate::geometry::mesh_transfer::CONTENT_TYPE]
            .iter()
            .any(|t| content_type.starts_with(t))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

/// In desktop mode every API request must carry the session token (header or query).
async fn desktop_token_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if state.settings.mode == RuntimeMode::Desktop
        && let Some(expected) = &state.settings.desktop_token
    {
        let header_token =
            request.headers().get(DESKTOP_TOKEN_HEADER).and_then(|v| v.to_str().ok()).filter(|v| !v.trim().is_empty());
        let query_token = request.uri().query().and_then(|q| serde_html_form_like_lookup(q, "desktopToken"));
        let provided = header_token.map(str::to_string).or(query_token);
        if provided.as_deref() != Some(expected.as_str()) {
            return (StatusCode::UNAUTHORIZED, "Missing or invalid desktop session token.").into_response();
        }
    }
    next.run(request).await
}

fn serde_html_form_like_lookup(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The authenticated user. All requests run as the seeded admin user.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: Uuid,
    pub username: String,
    pub is_admin: bool,
}

impl CurrentUser {
    pub fn caller(&self) -> Caller {
        Caller { user_id: self.id, is_admin: self.is_admin }
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(_parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let user = users::get_admin_user(&state.db)
            .await?
            .ok_or_else(|| AppError::Unauthorized("No admin user found".into()))?;
        Ok(CurrentUser { id: user.id, username: user.username, is_admin: user.is_admin })
    }
}

async fn current_user(user: CurrentUser) -> Json<serde_json::Value> {
    Json(json!({ "id": user.id, "username": user.username, "isAdmin": user.is_admin }))
}

/// A `{id}` path segment that must be a GUID; anything else is a 404, like a route
/// constraint.
pub struct IdPath(pub Uuid);

impl<S: Send + Sync> FromRequestParts<S> for IdPath {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(raw) =
            Path::<String>::from_request_parts(parts, state).await.map_err(|_| crate::error::not_found())?;
        Uuid::parse_str(&raw).map(IdPath).map_err(|_| crate::error::not_found())
    }
}

pub fn parse_id(raw: &str) -> AppResult<Uuid> {
    Uuid::parse_str(raw).map_err(|_| crate::error::not_found())
}

pub fn models_root(state: &AppState) -> AppResult<PathBuf> {
    state.models_root().ok_or_else(|| AppError::Internal(anyhow::anyhow!("Models directory is not configured.")))
}

fn content_disposition(file_name: &str) -> HeaderValue {
    let ascii: String =
        file_name.chars().map(|c| if c.is_ascii() && c != '"' && !c.is_control() { c } else { '_' }).collect();
    let encoded: String = file_name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// Serves a file from disk (with range support), optionally as a named download.
pub async fn serve_file(
    request_headers: HeaderMap,
    path: &FsPath,
    content_type: &str,
    download_name: Option<&str>,
    cache_control: Option<&str>,
) -> AppResult<Response> {
    let mime: mime::Mime = content_type.parse().unwrap_or(mime::APPLICATION_OCTET_STREAM);
    let mut request = Request::builder().uri("/").body(Body::empty()).map_err(anyhow::Error::from)?;
    *request.headers_mut() = request_headers;
    let response = ServeFile::new_with_mime(path, &mime).oneshot(request).await.map_err(anyhow::Error::from)?;
    let mut response = response.map(Body::new);
    if let Some(name) = download_name {
        response.headers_mut().insert(header::CONTENT_DISPOSITION, content_disposition(name));
    }
    if let Some(cache) = cache_control
        && let Ok(v) = HeaderValue::from_str(cache)
    {
        response.headers_mut().insert(header::CACHE_CONTROL, v);
    }
    Ok(response)
}

/// Streams a file as a download and runs `on_complete` once the body is finished or
/// dropped (used to delete temporary export files).
pub async fn serve_temp_download(
    path: &FsPath,
    content_type: &str,
    download_name: &str,
    extra_headers: Vec<(&'static str, String)>,
    on_complete: impl FnOnce() + Send + 'static,
) -> AppResult<Response> {
    struct Guard<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for Guard<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f();
            }
        }
    }

    let file = tokio::fs::File::open(path).await?;
    let length = file.metadata().await?.len();
    let guard = Guard(Some(on_complete));
    let stream = tokio_util::io::ReaderStream::with_capacity(file, 1 << 16).map(move |chunk| {
        let _keep = &guard;
        chunk
    });

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, length)
        .header(header::CONTENT_DISPOSITION, content_disposition(download_name));
    for (name, value) in extra_headers {
        if let Ok(v) = HeaderValue::from_str(&value) {
            builder = builder.header(name, v);
        }
    }
    Ok(builder.body(Body::from_stream(stream)).map_err(anyhow::Error::from)?)
}

pub fn created(location: String, body: impl serde::Serialize) -> Response {
    let mut response = (StatusCode::CREATED, Json(body)).into_response();
    if let Ok(v) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, v);
    }
    response
}

pub fn accepted(location: String, body: impl serde::Serialize) -> Response {
    let mut response = (StatusCode::ACCEPTED, Json(body)).into_response();
    if let Ok(v) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, v);
    }
    response
}
