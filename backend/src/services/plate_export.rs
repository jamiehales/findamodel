//! Print plate export: combines placed models into one 3MF (instanced), STL, GLB or VOXL
//! (instanced) file.

use std::collections::{HashMap, HashSet};
use std::io::{Seek, Write};
use std::sync::Arc;

use futures::StreamExt;
use rayon::prelude::*;
use uuid::Uuid;

use crate::dto::GeneratePlateRequest;
use crate::entities::ModelRow;
use crate::error::{AppError, AppResult, bad_request};
use crate::geometry::hull::DEFAULT_RAFT_HEIGHT_MM;
use crate::geometry::saver::{self, MeshObject};
use crate::geometry::support_layout::SupportLayout;
use crate::geometry::support_reader;
use crate::geometry::support_reader::CURRENT_SUPPORT_READER_VERSION;
use crate::geometry::voxl::{self, VoxlMesh, VoxlPlacement};
use crate::geometry::watertight;
use crate::geometry::{LoadedGeometry, Triangle, Vec3, loader, swap_up_axis, to_z_up};
use crate::metadata::tags;
use crate::services::models::{ModelService, is_non_geometry_type};
use crate::services::support_exports::{self, SupportExports};
use crate::services::{scan_config, store};

const MAX_CONCURRENT_MODEL_LOADS: usize = 2;

/// Receives per-entry progress while a plate is generated.
pub trait PlateProgress: Send + Sync {
    fn mark_current_entry(&self, entry_name: &str);
    fn mark_entry_completed(&self);
    /// A VOXL plate starts reading the supports of `total` pre-supported models.
    fn start_reading_supports(&self, total: usize);
    /// Reading the supports of one of them starts.
    fn mark_reading_supports(&self, entry_name: &str);
    /// The supports of one of them are read (or came from the cache).
    fn mark_supports_read(&self, entry_name: &str);
    /// A remeshed VOXL plate starts repairing `total` model meshes.
    fn start_repairing(&self, total: usize);
    /// Repairing one of them starts.
    fn mark_repairing(&self, entry_name: &str);
    /// One of them is repaired.
    fn mark_repaired(&self, entry_name: &str);
}

#[derive(Debug, Clone)]
pub struct PlateExportResult {
    pub content_type: &'static str,
    pub file_name: &'static str,
    pub warning: Option<String>,
    pub skipped_models: Vec<String>,
}

pub fn normalize_format(format: Option<&str>) -> AppResult<String> {
    let normalized = format.unwrap_or("voxl").to_ascii_lowercase();
    if !matches!(normalized.as_str(), "3mf" | "stl" | "glb" | "voxl" | REMESHED_VOXL) {
        return Err(bad_request(format!(
            "Unsupported format '{}'. Supported: 3mf, stl, glb, voxl, voxl-remeshed",
            format.unwrap_or_default()
        )));
    }
    Ok(normalized)
}

/// A VOXL plate whose model meshes are made watertight ([`watertight::repair_model`]); the
/// supports are the same DragonFruit supports.
pub const REMESHED_VOXL: &str = "voxl-remeshed";

pub fn file_name(format: &str) -> &'static str {
    match format {
        "stl" => "plate.stl",
        "glb" => "plate.glb",
        "voxl" => "plate.voxl",
        REMESHED_VOXL => "plate-remeshed.voxl",
        _ => "plate.3mf",
    }
}

pub fn content_type(format: &str) -> &'static str {
    match format {
        "stl" => "model/stl",
        "glb" => "model/gltf-binary",
        "voxl" | REMESHED_VOXL => "application/vnd.dragonfruit.voxl",
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

/// VOXL placement: the same rotation and translation as `transform_3mf` (a rotation about Z
/// by -angle, plate X mirrored), shifted so the bed centre is the origin, as DragonFruit expects.
pub fn voxl_placement(
    mesh: usize,
    angle_rad: f64,
    x_mm: f64,
    y_mm: f64,
    bed_width_mm: f64,
    bed_depth_mm: f64,
) -> VoxlPlacement {
    VoxlPlacement {
        mesh,
        rotation_z: -angle_rad,
        translation: [bed_width_mm / 2.0 - x_mm, y_mm - bed_depth_mm / 2.0, 0.0],
    }
}

fn place_vertex(v: Vec3, sin: f32, cos: f32, x: f32, y: f32) -> Vec3 {
    Vec3::new(v.x * cos - v.z * sin + x, v.y, v.x * sin + v.z * cos + y)
}

fn rotate_y(n: Vec3, sin: f32, cos: f32) -> Vec3 {
    Vec3::new(n.x * cos - n.z * sin, n.y, n.x * sin + n.z * cos)
}

/// The body (Z-up) and rebuilt supports of a pre-supported model, as the model's own VOXL has
/// them. None when the model is not supported or holds nothing that reads as supports. A
/// detection stored under the current scan config saves looking when it found no supports.
/// The split and the rebuilt supports come from the model's export entry when it is current, and
/// are read and stored as its entry otherwise (the entry's relative path is returned then).
fn supported_export(
    exports: Option<&SupportExports>,
    info: &ModelRow,
    triangles: &[Triangle],
) -> (support_exports::Export, Option<String>) {
    if info.supported() != Some(true) {
        return (None, None);
    }
    let raft_height_mm = info.hull_raft_height_mm.unwrap_or(DEFAULT_RAFT_HEIGHT_MM);
    let checked = info.scan_config_checksum.as_deref() == Some(scan_config::compute(raft_height_mm).as_str());
    if checked && info.support_parts_found == Some(false) {
        return (None, None);
    }
    let key = support_exports::key_for(info, raft_height_mm, checked);
    if let Some(export) = exports.and_then(|e| e.read(&key, triangles)) {
        return (export, None);
    }
    let export = support_reader::model_export(triangles, raft_height_mm, info.calculated_supported, checked);
    let written =
        exports.and_then(|e| e.store(&key, export.as_ref().map(|(mask, _, layout)| (mask.as_slice(), layout))));
    (export.map(|(_, body, layout)| (body, layout)), written)
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
    let names: HashMap<Uuid, String> = geometry_ids.iter().map(|id| (*id, infos[id].file_name.clone())).collect();
    let (bed_width_mm, bed_depth_mm) = (request.bed_width_mm.unwrap_or(0.0), request.bed_depth_mm.unwrap_or(0.0));
    let plate_name = request.plate_name.clone();
    let exports = models.support_exports().cloned();
    let written: Arc<parking_lot::Mutex<Vec<(Uuid, String)>>> = Arc::default();
    let written_by_export = written.clone();
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
                let z_up: Vec<(u32, Vec<Triangle>)> =
                    geometry_ids.iter().map(|id| (object_ids[id], to_z_up(&geometries[id].triangles))).collect();
                let objects: Vec<MeshObject<'_>> =
                    z_up.iter().map(|(id, tris)| MeshObject { id: *id, triangles: tris }).collect();
                let items: Vec<(u32, String)> = placements
                    .iter()
                    .map(|p| (object_ids[&p.model_id], transform_3mf(p.angle_rad, p.x_mm, p.y_mm)))
                    .collect();
                out = saver::write_3mf(out, &objects, &items)?;
            }
            "voxl" | REMESHED_VOXL => {
                // A pre-supported model goes in without its supports, rebuilt as DragonFruit
                // supports, as the model's own VOXL does; every other model as it is.
                let reading = geometry_ids.iter().filter(|id| infos[*id].supported() == Some(true)).count();
                if let Some(p) = &progress {
                    p.start_reading_supports(reading);
                }
                let prepared: Vec<(Vec<Triangle>, Option<SupportLayout>)> = geometry_ids
                    .par_iter()
                    .map(|id| {
                        let (info, triangles) = (&infos[id], &geometries[id].triangles);
                        let reports = progress.as_ref().filter(|_| info.supported() == Some(true));
                        if let Some(p) = reports {
                            p.mark_reading_supports(&info.file_name);
                        }
                        let (export, stored) = supported_export(exports.as_ref(), info, triangles);
                        if let Some(path) = stored {
                            written_by_export.lock().push((*id, path));
                        }
                        if let Some(p) = reports {
                            p.mark_supports_read(&info.file_name);
                        }
                        match export {
                            Some((body, layout)) => (body, Some(layout)),
                            None => (to_z_up(triangles), None),
                        }
                    })
                    .collect();
                // The remeshed plate: every model mesh made watertight (never the supports).
                let prepared: Vec<(Vec<Triangle>, Option<SupportLayout>)> = if format == REMESHED_VOXL {
                    if let Some(p) = &progress {
                        p.start_repairing(prepared.len());
                    }
                    geometry_ids
                        .par_iter()
                        .zip(prepared.into_par_iter())
                        .map(|(id, (body, layout))| {
                            if let Some(p) = &progress {
                                p.mark_repairing(&infos[id].file_name);
                            }
                            let body = watertight::repair_model(&body);
                            if let Some(p) = &progress {
                                p.mark_repaired(&infos[id].file_name);
                            }
                            (body, layout)
                        })
                        .collect()
                } else {
                    prepared
                };
                let meshes: Vec<VoxlMesh<'_>> = geometry_ids
                    .iter()
                    .zip(&prepared)
                    .map(|(id, (triangles, layout))| VoxlMesh {
                        name: &names[id],
                        triangles,
                        supports: layout.as_ref(),
                    })
                    .collect();
                let items: Vec<VoxlPlacement> = placements
                    .iter()
                    .map(|p| {
                        let mesh = object_ids[&p.model_id] as usize - 1;
                        voxl_placement(mesh, p.angle_rad, p.x_mm, p.y_mm, bed_width_mm, bed_depth_mm)
                    })
                    .collect();
                out = voxl::write_voxl(out, &meshes, &items, plate_name.as_deref())?;
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

    // The entries this export wrote, indexed for the scan.
    let written = std::mem::take(&mut *written.lock());
    for (id, path) in written {
        match store::set_support_export_index(&models.db, id, &path, CURRENT_SUPPORT_READER_VERSION).await {
            Ok(Some(previous)) if previous != path => {
                if let Some(exports) = models.support_exports() {
                    exports.release(&models.db, &previous).await;
                }
            }
            Ok(_) => {}
            Err(err) => tracing::warn!("Failed to index the support export of {id}: {err}"),
        }
    }

    Ok((out, result))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::geometry::support_layout::SupportPath;

    fn triangle(z: f32) -> Triangle {
        Triangle::new(Vec3::new(0.0, z, 0.0), Vec3::new(1.0, z, 0.0), Vec3::new(0.0, z, 1.0), Vec3::new(0.0, 1.0, 0.0))
    }

    fn supported_model() -> ModelRow {
        ModelRow { calculated_supported: Some(true), ..crate::test_support::model("knight.stl", "Mini") }
    }

    fn layout_with_one_support() -> SupportLayout {
        let mut layout = SupportLayout::default();
        layout.supports.push(SupportPath {
            tip: crate::geometry::support_layout::ContactTip {
                contact: Vec3::new(1.0, 2.0, 3.0),
                surface_normal: Vec3::new(0.0, 0.0, -1.0),
                from_outside: true,
                socket: Vec3::new(1.0, 2.0, 2.0),
                contact_diameter_mm: 0.3,
                body_diameter_mm: 0.6,
                ball: None,
            },
            base: crate::geometry::support_layout::SupportBase::Plate { x: 1.0, y: 2.0, diameter_mm: 3.0 },
            joints: vec![],
            shaft_diameter_mm: 0.6,
        });
        layout
    }

    #[test]
    fn a_current_entry_is_used_instead_of_reading_the_supports() {
        let dir = tempfile::tempdir().unwrap();
        let exports = SupportExports::new(dir.path().to_path_buf());
        let info = supported_model();
        let triangles: Vec<Triangle> = (0..4).map(|i| triangle(i as f32)).collect();
        let key = support_exports::key_for(&info, DEFAULT_RAFT_HEIGHT_MM, false);
        let layout = layout_with_one_support();
        let mask = [false, true, false, true];
        exports.write(&key, Some((&mask, &layout))).unwrap();

        let (export, stored) = supported_export(Some(&exports), &info, &triangles);
        assert_eq!(export, Some((to_z_up(&[triangles[0], triangles[2]]), layout.clone())));
        assert_eq!(stored, None, "a current entry is not written again");

        // A model whose detection was checked has its own entry.
        let checked = ModelRow {
            scan_config_checksum: Some(scan_config::compute(DEFAULT_RAFT_HEIGHT_MM)),
            support_parts_found: Some(true),
            ..info.clone()
        };
        let checked_key = support_exports::key_for(&checked, DEFAULT_RAFT_HEIGHT_MM, true);
        let other = SupportLayout::default();
        exports.write(&checked_key, Some((&mask, &other))).unwrap();
        assert_eq!(supported_export(Some(&exports), &checked, &triangles).0.map(|(_, l)| l), Some(other));
        // Without an entry store, the supports are read every time.
        assert_ne!(supported_export(None, &info, &triangles).0.map(|(_, l)| l), Some(layout));
    }

    #[test]
    fn an_entry_from_an_older_reader_is_read_again_and_rewritten_at_the_current_version() {
        let dir = tempfile::tempdir().unwrap();
        let exports = SupportExports::new(dir.path().to_path_buf());
        let info = supported_model();
        let triangles: Vec<Triangle> = (0..4).map(|i| triangle(i as f32)).collect();
        let key = support_exports::key_for(&info, DEFAULT_RAFT_HEIGHT_MM, false);
        let old = support_exports::Header {
            reader_version: CURRENT_SUPPORT_READER_VERSION - 1,
            detection_version: crate::geometry::support_separation::CURRENT_SUPPORT_DETECTION_VERSION,
            key: key.clone(),
        };
        let path = dir.path().join(support_exports::relative_path(&key));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let layout = layout_with_one_support();
        std::fs::write(&path, support_exports::codec::encode(&old, Some((&[false, true, false, true], &layout))))
            .unwrap();

        let (export, stored) = supported_export(Some(&exports), &info, &triangles);
        assert_ne!(export.map(|(_, l)| l), Some(layout), "the old entry is not used");
        assert_eq!(stored, Some(support_exports::relative_path(&key)));
        assert_eq!(exports.stored_version(&key), Some(CURRENT_SUPPORT_READER_VERSION));
    }

    #[test]
    fn a_model_without_supports_to_read_is_stored_as_such() {
        let dir = tempfile::tempdir().unwrap();
        let exports = SupportExports::new(dir.path().to_path_buf());
        let info = supported_model();
        let triangles: Vec<Triangle> = (0..4).map(|i| triangle(i as f32)).collect();
        assert_eq!(supported_export(Some(&exports), &info, &triangles).0, None);
        let key = support_exports::key_for(&info, DEFAULT_RAFT_HEIGHT_MM, false);
        assert_eq!(exports.read(&key, &triangles), Some(None));
        // An unsupported model stores nothing.
        let unsupported = ModelRow { calculated_supported: Some(false), ..info };
        assert_eq!(supported_export(Some(&exports), &unsupported, &triangles), (None, None));
    }

    #[test]
    fn format_validation_and_names() {
        assert_eq!(normalize_format(None).unwrap(), "voxl");
        assert_eq!(normalize_format(Some("STL")).unwrap(), "stl");
        assert!(matches!(normalize_format(Some("obj")), Err(AppError::BadRequest(_))));
        assert_eq!(file_name("glb"), "plate.glb");
        assert_eq!(normalize_format(Some("VOXL")).unwrap(), "voxl");
        assert_eq!(file_name("voxl"), "plate.voxl");
        assert_eq!(content_type("voxl"), "application/vnd.dragonfruit.voxl");
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

    #[test]
    fn voxl_placement_matches_3mf_transform_on_a_centred_bed() {
        let (angle, x, y) = (0.6, 40.0, 25.0);
        let p = voxl_placement(3, angle, x, y, 200.0, 120.0);
        assert_eq!(p.mesh, 3);
        let m: Vec<f64> = transform_3mf(angle, x, y).split(' ').map(|v| v.parse().unwrap()).collect();
        // 3MF uses row vectors: v' = v * M, so a point (1, 0) maps to the first row plus translation.
        let (s, c) = p.rotation_z.sin_cos();
        assert!((c - m[0]).abs() < 1e-6 && (s - m[1]).abs() < 1e-6);
        assert_eq!(p.translation, [m[9] + 100.0, m[10] - 60.0, 0.0]);
    }
}
