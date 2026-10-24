//! Print plate export: combines placed models into one 3MF (instanced), STL or GLB file.

use std::collections::{HashMap, HashSet};
use std::io::{Seek, Write};
use std::sync::Arc;

use futures::StreamExt;
use uuid::Uuid;

use crate::dto::GeneratePlateRequest;
use crate::error::{AppError, AppResult, bad_request};
use crate::geometry::saver::{self, MeshObject};
use crate::geometry::{LoadedGeometry, Triangle, Vec3, loader, swap_up_axis};
use crate::metadata::tags;
use crate::services::models::{ModelService, is_non_geometry_type};

const MAX_CONCURRENT_MODEL_LOADS: usize = 2;

/// Receives per-entry progress while a plate is generated.
pub trait PlateProgress: Send + Sync {
    fn mark_current_entry(&self, entry_name: &str);
    fn mark_entry_completed(&self);
}

#[derive(Debug, Clone)]
pub struct PlateExportResult {
    pub content_type: &'static str,
    pub file_name: &'static str,
    pub warning: Option<String>,
    pub skipped_models: Vec<String>,
}

pub fn normalize_format(format: Option<&str>) -> AppResult<String> {
    let normalized = format.unwrap_or("3mf").to_ascii_lowercase();
    if !matches!(normalized.as_str(), "3mf" | "stl" | "glb") {
        return Err(bad_request(format!(
            "Unsupported format '{}'. Supported: 3mf, stl, glb",
            format.unwrap_or_default()
        )));
    }
    Ok(normalized)
}

pub fn file_name(format: &str) -> &'static str {
    match format {
        "stl" => "plate.stl",
        "glb" => "plate.glb",
        _ => "plate.3mf",
    }
}

pub fn content_type(format: &str) -> &'static str {
    match format {
        "stl" => "model/stl",
        "glb" => "model/gltf-binary",
        _ => "application/vnd.ms-3mf",
    }
}

fn number(v: f64) -> String {
    if v == 0.0 { "0".to_string() } else { format!("{v}") }
}

fn number_f32(v: f32) -> String {
    if v == 0.0 { "0".to_string() } else { format!("{v}") }
}

/// 3MF transform (row-major 3x4) rotating about the vertical axis and translating on the
/// plate. Plate Y maps to 3MF Y; plate X is mirrored by the Y-up to Z-up conversion.
pub fn transform_3mf(angle_rad: f64, x_mm: f64, y_mm: f64) -> String {
    let (s, c) = (angle_rad as f32).sin_cos();
    format!(
        "{} {} 0 {} {} 0 0 0 1 {} {} 0",
        number_f32(c),
        number_f32(-s),
        number_f32(s),
        number_f32(c),
        number(-x_mm),
        number(y_mm)
    )
}

/// Column-major glTF node matrix: rotation about Y, then translation on the X-Z plane.
pub fn transform_glb(angle_rad: f64, x_mm: f64, y_mm: f64) -> [f32; 16] {
    let (s, c) = (angle_rad as f32).sin_cos();
    [c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, x_mm as f32, 0.0, y_mm as f32, 1.0]
}

fn place_vertex(v: Vec3, sin: f32, cos: f32, x: f32, y: f32) -> Vec3 {
    Vec3::new(v.x * cos - v.z * sin + x, v.y, v.x * sin + v.z * cos + y)
}

fn rotate_y(n: Vec3, sin: f32, cos: f32) -> Vec3 {
    Vec3::new(n.x * cos - n.z * sin, n.y, n.x * sin + n.z * cos)
}

struct Placement {
    model_id: Uuid,
    x_mm: f64,
    y_mm: f64,
    angle_rad: f64,
}

/// Generates the plate into `out`. Models that carry no mesh (LYS/LYT/CTB) are skipped
/// and reported.
pub async fn generate_plate<W: Write + Seek + Send + 'static>(
    models: &ModelService,
    request: &GeneratePlateRequest,
    out: W,
    progress: Option<Arc<dyn PlateProgress>>,
) -> AppResult<(W, PlateExportResult)> {
    let root =
        models.root().ok_or_else(|| AppError::Internal(anyhow::anyhow!("Models directory is not configured")))?;
    let format = normalize_format(request.format.as_deref())?;

    let mut requested: Vec<(Uuid, &crate::dto::PlacementDto)> = Vec::with_capacity(request.placements.len());
    for placement in &request.placements {
        let id = Uuid::parse_str(&placement.model_id)
            .map_err(|_| bad_request(format!("Invalid model ID: {}", placement.model_id)))?;
        requested.push((id, placement));
    }
    let mut unique_ids: Vec<Uuid> = requested.iter().map(|(id, _)| *id).collect();
    unique_ids.sort();
    unique_ids.dedup();

    let infos = models.get_models_by_id_map(&unique_ids).await?;
    if let Some(missing) = unique_ids.iter().find(|id| !infos.contains_key(id)) {
        return Err(AppError::NotFound(format!("Model not found: {missing}")));
    }

    let mut placements = Vec::with_capacity(requested.len());
    let mut skipped: Vec<String> = Vec::new();
    let mut geometry_ids: Vec<Uuid> = Vec::new();
    let mut seen = HashSet::new();
    for (id, placement) in &requested {
        let info = &infos[id];
        if is_non_geometry_type(&info.file_type) {
            if !skipped.iter().any(|s| tags::eq_ignore_case(s, &info.file_name)) {
                skipped.push(info.file_name.clone());
            }
            if let Some(p) = &progress {
                p.mark_entry_completed();
            }
            continue;
        }
        placements.push(Placement {
            model_id: *id,
            x_mm: placement.x_mm,
            y_mm: placement.y_mm,
            angle_rad: placement.angle_rad,
        });
        if seen.insert(*id) {
            geometry_ids.push(*id);
        } else if let Some(p) = &progress {
            p.mark_entry_completed();
        }
    }
    skipped.sort_by(|a, b| tags::cmp_ignore_case(a, b));

    let object_ids: HashMap<Uuid, u32> = geometry_ids.iter().enumerate().map(|(i, id)| (*id, i as u32 + 1)).collect();

    let load_futures: Vec<_> = geometry_ids
        .iter()
        .copied()
        .map(|id: Uuid| {
            let info = infos[&id].clone();
            let path = crate::metadata::inheritance::build_full_file_path(&root, &info.directory, &info.file_name);
            let progress = progress.clone();
            async move {
                if let Some(p) = &progress {
                    p.mark_current_entry(&info.file_name);
                }
                if !path.is_file() {
                    return Err(AppError::NotFound(format!("Model file not found on disk: {}", info.file_name)));
                }
                let geometry = loader::load_model_async(&path, &info.file_type).await.ok_or_else(|| {
                    AppError::Internal(anyhow::anyhow!("Failed to parse geometry for: {}", info.file_name))
                })?;
                if let Some(p) = &progress {
                    p.mark_entry_completed();
                }
                Ok((info.id, geometry))
            }
        })
        .collect();
    let loads =
        futures::stream::iter(load_futures).buffer_unordered(MAX_CONCURRENT_MODEL_LOADS).collect::<Vec<_>>().await;

    let mut geometries: HashMap<Uuid, LoadedGeometry> = HashMap::with_capacity(loads.len());
    for load in loads {
        let (id, geometry) = load?;
        geometries.insert(id, geometry);
    }

    if placements.is_empty() {
        return Err(AppError::Unprocessable("No geometry-based models were included in the export request.".into()));
    }

    let warning = (!skipped.is_empty())
        .then(|| "Some models were skipped because they do not contain exportable geometry (LYS/LYT/CTB).".to_string());
    let result = PlateExportResult {
        content_type: content_type(&format),
        file_name: file_name(&format),
        warning,
        skipped_models: skipped,
    };

    let out = tokio::task::spawn_blocking(move || -> anyhow::Result<W> {
        let mut out = out;
        match format.as_str() {
            "3mf" => {
                let z_up: Vec<(u32, Vec<Triangle>)> = geometry_ids
                    .iter()
                    .map(|id| {
                        let tris = geometries[id]
                            .triangles
                            .iter()
                            .map(|t| {
                                Triangle::new(
                                    swap_up_axis(t.v0),
                                    swap_up_axis(t.v1),
                                    swap_up_axis(t.v2),
                                    swap_up_axis(t.normal),
                                )
                            })
                            .collect();
                        (object_ids[id], tris)
                    })
                    .collect();
                let objects: Vec<MeshObject<'_>> =
                    z_up.iter().map(|(id, tris)| MeshObject { id: *id, triangles: tris }).collect();
                let items: Vec<(u32, String)> = placements
                    .iter()
                    .map(|p| (object_ids[&p.model_id], transform_3mf(p.angle_rad, p.x_mm, p.y_mm)))
                    .collect();
                out = saver::write_3mf(out, &objects, &items)?;
            }
            "stl" => {
                let total: u32 = placements.iter().map(|p| geometries[&p.model_id].triangles.len() as u32).sum();
                let triangles = placements.iter().flat_map(|p| {
                    let (sin, cos) = (p.angle_rad as f32).sin_cos();
                    let (x, y) = (p.x_mm as f32, p.y_mm as f32);
                    geometries[&p.model_id].triangles.iter().map(move |t| {
                        Triangle::new(
                            swap_up_axis(place_vertex(t.v0, sin, cos, x, y)),
                            swap_up_axis(place_vertex(t.v1, sin, cos, x, y)),
                            swap_up_axis(place_vertex(t.v2, sin, cos, x, y)),
                            swap_up_axis(rotate_y(t.normal, sin, cos)),
                        )
                    })
                });
                let mut buffered = std::io::BufWriter::new(&mut out);
                saver::write_stl(&mut buffered, total, triangles, Some("findamodel plate"))?;
                buffered.flush()?;
            }
            _ => {
                let objects: Vec<MeshObject<'_>> = geometry_ids
                    .iter()
                    .map(|id| MeshObject { id: object_ids[id], triangles: &geometries[id].triangles })
                    .collect();
                let items: Vec<(u32, [f32; 16])> = placements
                    .iter()
                    .map(|p| (object_ids[&p.model_id], transform_glb(p.angle_rad, p.x_mm, p.y_mm)))
                    .collect();
                out.write_all(&saver::save_glb(&objects, &items))?;
            }
        }
        out.flush()?;
        Ok(out)
    })
    .await
    .map_err(anyhow::Error::from)??;

    Ok((out, result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_validation_and_names() {
        assert_eq!(normalize_format(None).unwrap(), "3mf");
        assert_eq!(normalize_format(Some("STL")).unwrap(), "stl");
        assert!(matches!(normalize_format(Some("obj")), Err(AppError::BadRequest(_))));
        assert_eq!(file_name("glb"), "plate.glb");
        assert_eq!(content_type("3mf"), "application/vnd.ms-3mf");
    }

    #[test]
    fn transforms_encode_rotation_and_translation() {
        assert_eq!(transform_3mf(0.0, 10.0, 20.0), "1 0 0 0 1 0 0 0 1 -10 20 0");
        let quarter = transform_3mf(std::f64::consts::FRAC_PI_2, 0.0, 0.0);
        let parts: Vec<f64> = quarter.split(' ').map(|p| p.parse().unwrap()).collect();
        assert_eq!(parts.len(), 12);
        assert!((parts[1] + 1.0).abs() < 1e-6 && (parts[3] - 1.0).abs() < 1e-6);

        let m = transform_glb(0.0, 5.0, 7.0);
        assert_eq!((m[0], m[12], m[14], m[15]), (1.0, 5.0, 7.0, 1.0));
    }
}
