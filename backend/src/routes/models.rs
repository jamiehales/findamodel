//! /api/models and /api/query endpoints.

use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use super::{CurrentUser, IdPath, content_disposition, serve_file};
use crate::app::AppState;
use crate::dto::{ModelQueryRequest, ModelsByIdsRequest, UpdateModelMetadataRequest};
use crate::entities::ModelRow;
use crate::error::{AppError, AppResult, bad_request, not_found};
use crate::geometry::hull::DEFAULT_RAFT_HEIGHT_MM;
use crate::geometry::support_layout::{self, SupportLayout};
use crate::geometry::voxl::{self, VoxlMesh, VoxlPlacement};
use crate::geometry::{LoadedGeometry, Triangle, loader, mesh_transfer, support_separation, to_z_up};
use crate::services::voxl_cache::VoxlCacheKey;
use crate::services::{plate_export, query, scan_config, store};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/models", get(get_models))
        .route("/api/models/by-ids", post(get_models_by_ids))
        .route("/api/models/{id}", get(get_model))
        .route("/api/models/{id}/other-parts", get(get_other_parts))
        .route("/api/models/{id}/file", get(get_file))
        .route("/api/models/{id}/preview", get(get_preview))
        .route("/api/models/{id}/metadata", get(get_metadata).put(update_metadata))
        .route("/api/models/{id}/geometry", get(get_geometry))
        .route("/api/models/{id}/geometry/split", get(get_split_geometry))
        .route("/api/models/{id}/voxl", get(get_voxl))
        .route("/api/query", get(query_models))
        .route("/api/query/options", get(query_options))
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<i64>,
}

async fn get_models(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<LimitQuery>,
) -> AppResult<Response> {
    Ok(Json(state.models.get_models(q.limit).await?).into_response())
}

async fn get_models_by_ids(
    State(state): State<AppState>,
    _user: CurrentUser,
    body: Option<Json<ModelsByIdsRequest>>,
) -> AppResult<Response> {
    let ids = body.and_then(|Json(b)| b.ids).ok_or_else(|| bad_request("ids is required."))?;
    Ok(Json(state.models.get_models_by_ids(&ids).await?).into_response())
}

async fn get_model(State(state): State<AppState>, _user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    let model = state.models.get_model_dto(id).await?.ok_or_else(not_found)?;
    Ok(Json(model).into_response())
}

async fn get_other_parts(State(state): State<AppState>, _user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    Ok(Json(state.models.get_other_parts(id).await?).into_response())
}

async fn get_file(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
    headers: HeaderMap,
) -> AppResult<Response> {
    let model = state.models.get_model(id).await?.ok_or_else(not_found)?;
    let path = state
        .models
        .full_path(&model)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Models directory is not configured.")))?;
    if !path.is_file() {
        return Err(not_found());
    }
    let content_type = match model.file_type.as_str() {
        "stl" => "model/stl",
        "obj" => "model/obj",
        _ => "application/octet-stream",
    };
    serve_file(headers, &path, content_type, Some(&model.file_name), None).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewQuery {
    include_supports: Option<bool>,
}

async fn get_preview(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
    Query(q): Query<PreviewQuery>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let candidates =
        state.models.preview_path_candidates(id, q.include_supports.unwrap_or(true)).await?.ok_or_else(not_found)?;
    let renders = state.settings.renders_path();
    for (relative, preferred) in candidates.candidates() {
        let path = renders.join(&relative);
        if path.is_file() {
            // Exact-variant URLs are immutable; fallbacks stay short-lived because the
            // requested variant may be generated later under the same URL.
            let cache = if preferred { "public, max-age=31536000, immutable" } else { "public, max-age=60" };
            return serve_file(headers, &path, "image/png", None, Some(cache)).await;
        }
    }
    Err(not_found())
}

async fn get_metadata(State(state): State<AppState>, _user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    let metadata = state.models.get_model_metadata(id).await?.ok_or_else(not_found)?;
    Ok(Json(metadata).into_response())
}

async fn update_metadata(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
    Json(body): Json<UpdateModelMetadataRequest>,
) -> AppResult<Response> {
    let model = state.models.update_model_metadata(id, &body).await?.ok_or_else(not_found)?;
    Ok(Json(model).into_response())
}

async fn load_geometry(
    state: &AppState,
    id: uuid::Uuid,
) -> AppResult<(crate::entities::ModelRow, Option<LoadedGeometry>)> {
    let model = state.models.get_model(id).await?.ok_or_else(not_found)?;
    let path = state
        .models
        .full_path(&model)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Models directory is not configured.")))?;
    if !path.is_file() {
        return Err(not_found());
    }
    let geometry = loader::load_model_async(&path, &model.file_type).await;
    Ok((model, geometry))
}

#[derive(Deserialize)]
struct GeometryQuery {
    format: Option<String>,
}

fn prefers_binary_mesh(headers: &HeaderMap, q: &GeometryQuery) -> bool {
    if q.format.as_deref().is_some_and(|f| f.eq_ignore_ascii_case("json")) {
        return false;
    }
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.to_ascii_lowercase().contains(mesh_transfer::CONTENT_TYPE))
}

/// Geometry in Y-up, mm, centred coordinates: the binary FMSH format when requested via
/// Accept, otherwise legacy JSON with flat per-vertex arrays.
async fn get_geometry(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
    Query(q): Query<GeometryQuery>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let (_, geometry) = load_geometry(&state, id).await?;
    let geometry = geometry.ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to load model geometry")))?;

    if !prefers_binary_mesh(&headers, &q) {
        let body = tokio::task::spawn_blocking(move || {
            let mut positions = Vec::with_capacity(geometry.triangles.len() * 9);
            let mut normals = Vec::with_capacity(geometry.triangles.len() * 9);
            for t in &geometry.triangles {
                for v in [t.v0, t.v1, t.v2] {
                    positions.extend_from_slice(&[v.x, v.y, v.z]);
                    normals.extend_from_slice(&[t.normal.x, t.normal.y, t.normal.z]);
                }
            }
            json!({
                "positions": positions,
                "normals": normals,
                "triangleCount": geometry.triangles.len(),
                "sphereRadius": geometry.sphere_radius,
                "sphereCentre": { "x": geometry.sphere_centre.x, "y": geometry.sphere_centre.y, "z": geometry.sphere_centre.z },
                "dimensionXMm": geometry.dimension_x_mm,
                "dimensionYMm": geometry.dimension_y_mm,
                "dimensionZMm": geometry.dimension_z_mm,
            })
        })
        .await
        .map_err(anyhow::Error::from)?;
        return Ok(Json(body).into_response());
    }

    let payload =
        tokio::task::spawn_blocking(move || mesh_transfer::encode(&geometry)).await.map_err(anyhow::Error::from)?;
    Ok(([(header::CONTENT_TYPE, mesh_transfer::CONTENT_TYPE)], payload).into_response())
}

/// A pre-supported model's geometry split into body and supports.
struct SupportSplit {
    model: ModelRow,
    geometry: LoadedGeometry,
    body: Vec<Triangle>,
    supports: Vec<Triangle>,
    /// The supports' rebuilt layout (Z-up), when asked for or rebuilt to check the supports.
    layout: Option<SupportLayout>,
}

/// Splits the supports off a supported model and records whether support parts were found.
/// None when the model is not supported, cannot be loaded or has no supports to split, or
/// when its support parts are not supports (see
/// [`support_separation::Classification::holds_supports`]). A detection stored under the
/// current scan config was checked when the model was indexed; otherwise the supports are
/// rebuilt to check them, as they are whenever `with_layout` asks for the layout.
async fn split_supports(state: &AppState, id: uuid::Uuid, with_layout: bool) -> AppResult<Option<SupportSplit>> {
    let model = state.models.get_model(id).await?.ok_or_else(not_found)?;
    if model.supported() != Some(true) {
        return Ok(None);
    }
    let raft_height_mm = model.hull_raft_height_mm.unwrap_or(DEFAULT_RAFT_HEIGHT_MM);
    let checked = model.scan_config_checksum.as_deref() == Some(scan_config::compute(raft_height_mm).as_str());
    if checked && model.support_parts_found == Some(false) {
        return Ok(None);
    }
    let metadata_supported = model.calculated_supported;
    let (_, geometry) = load_geometry(state, id).await?;
    let Some(geometry) = geometry else {
        return Ok(None);
    };

    let (geometry, split, found) = tokio::task::spawn_blocking(move || {
        let classification = support_separation::classify(&geometry.triangles, raft_height_mm);
        let mut found = classification.is_some();
        let check = (with_layout || !checked) && classification.as_ref().is_some_and(|c| !c.generated);
        let split = classification
            .and_then(|c| c.mask_for(metadata_supported))
            .map(|mask| support_separation::split(&geometry.triangles, &mask))
            .filter(|(_, supports)| !supports.is_empty())
            .and_then(|(body, supports)| {
                if !with_layout && !check {
                    return Some((body, supports, None));
                }
                let z_up_supports = to_z_up(&supports);
                let layout = support_layout::reconstruct(&to_z_up(&body), &z_up_supports);
                if check {
                    found = layout.accounts_for(&z_up_supports, raft_height_mm);
                }
                found.then_some((body, supports, Some(layout)))
            });
        (geometry, split, found)
    })
    .await
    .map_err(anyhow::Error::from)?;

    if model.support_parts_found != Some(found) {
        state.models.set_support_parts_found(id, found).await?;
    }
    Ok(split.map(|(body, supports, layout)| SupportSplit { model, geometry, body, supports, layout }))
}

/// Body and support geometry of a pre-supported model in one envelope:
/// `[bodyLength:u32][body FMSH][supportLength:u32][support FMSH]`. 204 when the model is
/// not supported (by metadata or detection), has no detectable supports, or cannot be
/// loaded.
async fn get_split_geometry(
    State(state): State<AppState>,
    _user: CurrentUser,
    IdPath(id): IdPath,
) -> AppResult<Response> {
    let Some(split) = split_supports(&state, id, false).await? else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let envelope = tokio::task::spawn_blocking(move || {
        let body_payload = mesh_transfer::encode(&split.geometry.with_triangles(split.body));
        let support_payload = mesh_transfer::encode(&split.geometry.with_triangles(split.supports));
        mesh_transfer::encode_split_envelope(&body_payload, &support_payload)
    })
    .await
    .map_err(anyhow::Error::from)?;
    Ok(([(header::CONTENT_TYPE, mesh_transfer::CONTENT_TYPE_SPLIT)], envelope).into_response())
}

/// The model stripped of its supports, with their layout rebuilt as DragonFruit supports,
/// as a VOXL scene. 404 when the supports cannot be split off. Served from the VOXL cache
/// when the model, its raft height and the support layout are unchanged since it was written.
async fn get_voxl(State(state): State<AppState>, _user: CurrentUser, IdPath(id): IdPath) -> AppResult<Response> {
    let model = state.models.get_model(id).await?.ok_or_else(not_found)?;
    let download_name = format!("{}.voxl", store::file_stem(&model.file_name));
    let raft_height_mm = model.hull_raft_height_mm.unwrap_or(DEFAULT_RAFT_HEIGHT_MM);
    let key = (model.supported() == Some(true)).then(|| VoxlCacheKey::for_model(&model, raft_height_mm)).flatten();
    if let Some(key) = key.clone() {
        let cache = state.voxl_cache.clone();
        let cached = tokio::task::spawn_blocking(move || cache.get(&key)).await.map_err(anyhow::Error::from)?;
        if let Some(bytes) = cached {
            return Ok(voxl_response(&download_name, bytes));
        }
    }

    let split = split_supports(&state, id, true).await?.ok_or_else(not_found)?;
    let cache = state.voxl_cache.clone();
    let bytes = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let body = to_z_up(&split.body);
        let layout = split.layout.as_ref().expect("split_supports rebuilds the layout when asked");
        let mesh = VoxlMesh { name: &split.model.file_name, triangles: &body, supports: Some(layout) };
        let placement = VoxlPlacement { mesh: 0, rotation_z: 0.0, translation: [0.0; 3] };
        let bytes = voxl::write_voxl(std::io::Cursor::new(Vec::new()), &[mesh], &[placement], None)?.into_inner();
        if let Some(key) = key {
            cache.put(&key, &bytes);
        }
        Ok(bytes)
    })
    .await
    .map_err(anyhow::Error::from)??;
    Ok(voxl_response(&download_name, bytes))
}

fn voxl_response(download_name: &str, bytes: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, plate_export::content_type("voxl").parse().expect("valid header")),
            (header::CONTENT_DISPOSITION, content_disposition(download_name)),
        ],
        bytes,
    )
        .into_response()
}

async fn query_models(
    State(state): State<AppState>,
    _user: CurrentUser,
    axum_extra::extract::Query(q): axum_extra::extract::Query<ModelQueryRequest>,
) -> AppResult<Response> {
    Ok(Json(query::query_models(&state.db, &q).await?).into_response())
}

async fn query_options(
    State(state): State<AppState>,
    _user: CurrentUser,
    axum_extra::extract::Query(q): axum_extra::extract::Query<ModelQueryRequest>,
) -> AppResult<Response> {
    Ok(Json(query::get_filter_options(&state.db, &q).await?).into_response())
}
