//! Spin meshes: a small, compressed copy of a model's body (supports removed, exactly the body
//! the supports-removed preview draws) that the gallery turns in 3D while a tile is hovered.
//!
//! The body is reduced with meshoptimizer: very large bodies are first clustered down to
//! [`SLOPPY_TRIANGLES`], then simplified until the error reaches [`MAX_ERROR`] of the model's
//! extent, keeping at least [`MIN_TRIANGLES`] and at most [`MAX_TRIANGLES`]; bodies with no more
//! than [`MIN_TRIANGLES`] pass through. Positions are quantised to 12 bits over the body's
//! bounds and both buffers go through meshoptimizer's vertex and index codecs (no normals: the
//! viewer shades with face normals). Files are stored brotli-compressed, ready to be served
//! with `Content-Encoding: br`.
//!
//! File layout (little endian), before compression:
//!
//! | offset | type     | field                                                          |
//! |--------|----------|----------------------------------------------------------------|
//! | 0      | [u8; 4]  | magic `FSPN`                                                   |
//! | 4      | u32      | spin mesh version ([`CURRENT_SPIN_MESH_VERSION`])              |
//! | 8      | u32      | vertex count                                                   |
//! | 12     | u32      | index count                                                    |
//! | 16     | u32      | encoded vertex buffer length in bytes                          |
//! | 20     | f32 x 3  | quantisation offset                                            |
//! | 32     | f32      | quantisation scale: position = offset + q / 4095 * scale       |
//! | 36     | f32 x 6  | body bounds (min xyz, max xyz): the supports-removed preview frame |
//! | 60     | f32 x 6  | whole model bounds: the with-supports preview frame            |
//! | 84     | bytes    | encoded vertex buffer (4 x u16 per vertex: x, y, z, 0)         |
//! | ...    | bytes    | encoded index buffer                                           |
//!
//! Coordinates are the loaded model's (Y up, mm), the same space the previews are drawn in.

use std::io::{Read, Write};
use std::path::Path;

use meshopt::{SimplifyOptions, VertexDataAdapter};
use tracing::warn;
use uuid::Uuid;

use crate::geometry::{Triangle, support_separation};

/// Increment when the spin mesh output changes (reduction, encoding or layout), so the next
/// scan writes each model's spin mesh again. Previews are not redrawn.
pub const CURRENT_SPIN_MESH_VERSION: i32 = 1;

/// Media type of a spin mesh response.
pub const CONTENT_TYPE: &str = "application/vnd.findamodel.spin";
pub const MAGIC: &[u8; 4] = b"FSPN";
pub const HEADER_BYTES: usize = 84;
/// Bodies larger than this are clustered down to it before the simplifier runs.
pub const SLOPPY_TRIANGLES: usize = 200_000;
/// The simplifier keeps at least this many triangles; smaller bodies pass through.
pub const MIN_TRIANGLES: usize = 8_000;
/// And at most this many.
pub const MAX_TRIANGLES: usize = 20_000;
/// The simplifier stops at this error, relative to the model's extent.
pub const MAX_ERROR: f32 = 0.002;
pub const POSITION_BITS: u32 = 12;
const QUANT_MAX: f32 = ((1u32 << POSITION_BITS) - 1) as f32;
const BROTLI_QUALITY: u32 = 9;
const BROTLI_WINDOW: u32 = 22;

/// The spin mesh file name for a preview stem. The stem holds the checksum, raft height and
/// supported flag (see [`super::preview_stem`]), and the version is part of the name too, so a
/// file found under the name is always current.
pub fn relative_path(stem: &str) -> String {
    format!("{stem}_spin_v{CURRENT_SPIN_MESH_VERSION}.bin")
}

/// Whether a file name in the renders folder is a spin mesh (of any version).
pub fn is_spin_file(name: &str) -> bool {
    name.contains("_spin_v") && (name.ends_with(".bin") || name.ends_with(".partial"))
}

/// A reduced, indexed mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Reduced {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl Reduced {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Axis-aligned bounds (min, max).
pub type Bounds = ([f32; 3], [f32; 3]);

fn bounds_of(points: impl IntoIterator<Item = [f32; 3]>) -> Option<Bounds> {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    let mut any = false;
    for p in points {
        any = true;
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    any.then_some((lo, hi))
}

pub fn triangle_bounds(triangles: &[Triangle]) -> Option<Bounds> {
    bounds_of(triangles.iter().flat_map(|t| [t.v0, t.v1, t.v2]).map(|v| [v.x, v.y, v.z]))
}

fn adapter(positions: &[[f32; 3]]) -> VertexDataAdapter<'_> {
    VertexDataAdapter::new(bytemuck::cast_slice(positions), 12, 0).expect("a tightly packed position buffer")
}

/// Drops the vertices `indices` don't use and orders both for the codecs.
fn compact(positions: &[[f32; 3]], indices: &[u32]) -> Reduced {
    let mut indices = meshopt::optimize_vertex_cache(indices, positions.len());
    let positions = meshopt::optimize_vertex_fetch(&mut indices, positions);
    Reduced { positions, indices }
}

/// Welds the triangle soup and reduces it (see the module docs). None for an empty body.
pub fn reduce(body: &[Triangle]) -> Option<Reduced> {
    if body.is_empty() {
        return None;
    }
    // `+ 0.0` folds -0.0 into 0.0 so welding sees them as the same position.
    let soup: Vec<[f32; 3]> =
        body.iter().flat_map(|t| [t.v0, t.v1, t.v2]).map(|v| [v.x + 0.0, v.y + 0.0, v.z + 0.0]).collect();
    let (count, remap) = meshopt::generate_vertex_remap(&soup, None);
    let mut positions = meshopt::remap_vertex_buffer(&soup, count, &remap);
    let mut indices = remap;
    drop(soup);

    if indices.len() / 3 > SLOPPY_TRIANGLES {
        let clustered = meshopt::simplify_sloppy(&indices, &adapter(&positions), SLOPPY_TRIANGLES * 3, 1.0, None);
        let r = compact(&positions, &clustered);
        (positions, indices) = (r.positions, r.indices);
    }
    if indices.len() / 3 > MIN_TRIANGLES {
        let a = adapter(&positions);
        let mut simplified =
            meshopt::simplify(&indices, &a, MIN_TRIANGLES * 3, MAX_ERROR, SimplifyOptions::Prune, None);
        if simplified.len() / 3 > MAX_TRIANGLES {
            simplified = meshopt::simplify(&simplified, &a, MAX_TRIANGLES * 3, 1.0, SimplifyOptions::Prune, None);
        }
        indices = simplified;
    }
    let reduced = compact(&positions, &indices);
    (!reduced.indices.is_empty()).then_some(reduced)
}

/// Quantisation offset and (uniform) scale over the mesh's bounds.
fn quantisation(positions: &[[f32; 3]]) -> ([f32; 3], f32) {
    let (lo, hi) = bounds_of(positions.iter().copied()).unwrap_or(([0.0; 3], [0.0; 3]));
    let extent = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
    (lo, if extent > 0.0 { extent } else { 1.0 })
}

/// The file for a reduced body, uncompressed (see the module docs for the layout).
pub fn encode(reduced: &Reduced, body_bounds: Bounds, model_bounds: Bounds) -> Vec<u8> {
    let (offset, scale) = quantisation(&reduced.positions);
    let quantised: Vec<[u16; 4]> = reduced
        .positions
        .iter()
        .map(|p| {
            let q = |k: usize| ((p[k] - offset[k]) / scale * QUANT_MAX).round().clamp(0.0, QUANT_MAX) as u16;
            [q(0), q(1), q(2), 0]
        })
        .collect();
    let vertices = meshopt::encode_vertex_buffer(&quantised).expect("vertex buffer encodes");
    let indices =
        meshopt::encode_index_buffer(&reduced.indices, reduced.positions.len()).expect("index buffer encodes");

    let mut out = Vec::with_capacity(HEADER_BYTES + vertices.len() + indices.len());
    out.extend_from_slice(MAGIC);
    for v in [CURRENT_SPIN_MESH_VERSION as u32, reduced.positions.len() as u32, reduced.indices.len() as u32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(vertices.len() as u32).to_le_bytes());
    let floats = offset
        .into_iter()
        .chain([scale])
        .chain(body_bounds.0)
        .chain(body_bounds.1)
        .chain(model_bounds.0)
        .chain(model_bounds.1);
    for f in floats {
        out.extend_from_slice(&f.to_le_bytes());
    }
    debug_assert_eq!(out.len(), HEADER_BYTES);
    out.extend_from_slice(&vertices);
    out.extend_from_slice(&indices);
    out
}

pub fn compress(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut w = brotli::CompressorWriter::new(&mut out, 4096, BROTLI_QUALITY, BROTLI_WINDOW);
        w.write_all(bytes).expect("writing to memory");
    }
    out
}

pub fn decompress(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    brotli::Decompressor::new(bytes, 4096).read_to_end(&mut out)?;
    Ok(out)
}

/// The compressed spin mesh file for a model: `body` is the supports-removed body and `model`
/// all of its triangles. None when the body is empty.
pub fn build(body: &[Triangle], model: &[Triangle]) -> Option<Vec<u8>> {
    let body_bounds = triangle_bounds(body)?;
    let model_bounds = triangle_bounds(model).unwrap_or(body_bounds);
    let reduced = reduce(body)?;
    Some(compress(&encode(&reduced, body_bounds, model_bounds)))
}

/// The support split the previews use: the scan's checked classification of the model's
/// triangles, masked by its supported metadata. None when nothing is split off.
pub fn support_mask(triangles: &[Triangle], raft_height_mm: f32, supported: Option<bool>) -> Option<Vec<bool>> {
    checked_classification(triangles, raft_height_mm).and_then(|c| c.mask_for(supported))
}

/// The support classification the scan keeps: only splits that hold supports.
pub fn checked_classification(
    triangles: &[Triangle],
    raft_height_mm: f32,
) -> Option<support_separation::Classification> {
    support_separation::classify(triangles, raft_height_mm).filter(|c| c.holds_supports(triangles, raft_height_mm))
}

/// Writes a model's spin mesh (see [`store`]) from all of its loaded triangles and the support
/// mask its previews are split by: the body is what the supports-removed preview draws. The one
/// path both the scan and on-demand builds take, so they write the same file. Blocking.
pub fn store_for_model(dir: &Path, relative: &str, triangles: &[Triangle], mask: Option<&[bool]>) -> Option<String> {
    let path = dir.join(relative);
    if path.is_file() {
        return Some(relative.to_string());
    }
    let body: std::borrow::Cow<[Triangle]> = match mask {
        Some(mask) => support_separation::split(triangles, mask).0.into(),
        None => triangles.into(),
    };
    store(dir, relative, &body, triangles)
}

/// Writes the spin mesh for `body` (and `model`, all of its triangles) at `relative` under `dir`,
/// unless a file is already there: its name holds the version, so a file found is current.
/// Returns the relative path; None when the body gives no mesh or the file could not be written.
/// Blocking.
pub fn store(dir: &Path, relative: &str, body: &[Triangle], model: &[Triangle]) -> Option<String> {
    let path = dir.join(relative);
    if path.is_file() {
        return Some(relative.to_string());
    }
    let bytes = build(body, model)?;
    // Written beside the file and renamed into place, so readers never see part of it.
    let partial = dir.join(format!("{relative}.{}.partial", Uuid::new_v4().simple()));
    let written = std::fs::write(&partial, &bytes).and_then(|()| std::fs::rename(&partial, &path));
    match written {
        Ok(()) => Some(relative.to_string()),
        Err(err) => {
            let _ = std::fs::remove_file(&partial);
            warn!(target: "findamodel.Preview", "Failed to save spin mesh to {}: {err}", path.display());
            None
        }
    }
}

/// A decoded spin mesh file (for tests and tools; the browser decodes its own).
#[derive(Debug, Clone)]
pub struct Decoded {
    pub version: u32,
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub body_bounds: Bounds,
    pub model_bounds: Bounds,
    /// The largest error quantisation can add to a coordinate.
    pub quantisation_step: f32,
}

/// Decodes an uncompressed spin mesh file. None when it isn't one.
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    if bytes.len() < HEADER_BYTES || &bytes[..4] != MAGIC {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let f32_at = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let (version, vertex_count, index_count, vertex_bytes) =
        (u32_at(4), u32_at(8) as usize, u32_at(12) as usize, u32_at(16) as usize);
    let offset = [f32_at(20), f32_at(24), f32_at(28)];
    let scale = f32_at(32);
    let v3 = |o: usize| [f32_at(o), f32_at(o + 4), f32_at(o + 8)];
    let body_bounds = (v3(36), v3(48));
    let model_bounds = (v3(60), v3(72));
    let vertex_end = HEADER_BYTES.checked_add(vertex_bytes).filter(|&e| e <= bytes.len())?;
    let quantised: Vec<[u16; 4]> =
        meshopt::decode_vertex_buffer(&bytes[HEADER_BYTES..vertex_end], vertex_count).ok()?;
    let indices: Vec<u32> = meshopt::decode_index_buffer(&bytes[vertex_end..], index_count).ok()?;
    let positions = quantised.iter().map(|q| [0, 1, 2].map(|k| offset[k] + q[k] as f32 / QUANT_MAX * scale)).collect();
    Some(Decoded { version, positions, indices, body_bounds, model_bounds, quantisation_step: scale / QUANT_MAX })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Vec3;

    /// A UV sphere of radius `r` with about `2 * rings * segments` triangles.
    fn sphere(rings: usize, segments: usize, r: f32, centre: Vec3) -> Vec<Triangle> {
        let point = |i: usize, j: usize| {
            let theta = std::f32::consts::PI * i as f32 / rings as f32;
            let phi = std::f32::consts::TAU * (j % segments) as f32 / segments as f32;
            centre + Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin()) * r
        };
        let mut out = Vec::new();
        for i in 0..rings {
            for j in 0..segments {
                let (a, b, c, d) = (point(i, j), point(i + 1, j), point(i + 1, j + 1), point(i, j + 1));
                if i != 0 {
                    out.push(Triangle::new(a, b, d, Vec3::UP));
                }
                if i != rings - 1 {
                    out.push(Triangle::new(b, c, d, Vec3::UP));
                }
            }
        }
        out
    }

    #[test]
    fn small_bodies_pass_through_unreduced() {
        let body = sphere(20, 40, 10.0, Vec3::new(0.0, 10.0, 0.0));
        assert!(body.len() <= MIN_TRIANGLES);
        let reduced = reduce(&body).unwrap();
        assert_eq!(reduced.triangle_count(), body.len());
    }

    #[test]
    fn large_bodies_are_reduced_to_the_budget() {
        // About 320k triangles: clustered down to 200k first, then simplified. A smooth sphere
        // reaches the error limit late, so it lands between the floor and the cap.
        let body = sphere(400, 400, 25.0, Vec3::new(0.0, 25.0, 0.0));
        assert!(body.len() > SLOPPY_TRIANGLES);
        let reduced = reduce(&body).unwrap();
        let n = reduced.triangle_count();
        assert!(n <= MAX_TRIANGLES, "{n} triangles is over the cap");
        assert!(n >= MIN_TRIANGLES / 2, "{n} triangles is far below the floor");
        assert!(reduced.indices.iter().all(|&i| (i as usize) < reduced.positions.len()));
    }

    #[test]
    fn simple_shapes_stop_at_the_floor() {
        // A finely tessellated flat-sided box simplifies with almost no error, so the floor
        // (not the error limit) decides where it stops.
        let body = sphere(4, 4, 10.0, Vec3::new(0.0, 0.0, 0.0)); // coarse: a few dozen triangles
        let mut fine = Vec::new();
        for t in &body {
            // Split each triangle into many coplanar ones.
            let n = 40;
            for i in 0..n {
                for j in 0..n - i {
                    let p = |a: usize, b: usize| {
                        let (u, v) = (a as f32 / n as f32, b as f32 / n as f32);
                        t.v0 + (t.v1 - t.v0) * u + (t.v2 - t.v0) * v
                    };
                    fine.push(Triangle::new(p(i, j), p(i + 1, j), p(i, j + 1), Vec3::UP));
                    if i + j + 1 < n {
                        fine.push(Triangle::new(p(i + 1, j), p(i + 1, j + 1), p(i, j + 1), Vec3::UP));
                    }
                }
            }
        }
        assert!(fine.len() > MIN_TRIANGLES * 2);
        let n = reduce(&fine).unwrap().triangle_count();
        assert!(n <= MIN_TRIANGLES, "{n} triangles: a flat-sided shape stops at the floor");
    }

    #[test]
    fn empty_bodies_have_no_spin_mesh() {
        assert!(reduce(&[]).is_none());
        assert!(build(&[], &[]).is_none());
    }

    #[test]
    fn positions_round_trip_within_the_quantisation_error() {
        let body = sphere(30, 60, 12.0, Vec3::new(3.0, 12.0, -2.0));
        let model = sphere(10, 10, 30.0, Vec3::new(0.0, 30.0, 0.0));
        let reduced = reduce(&body).unwrap();
        let file = build(&body, &model).unwrap();
        let raw = decompress(&file).unwrap();
        assert!(file.len() < raw.len(), "brotli shrinks the file");
        let decoded = decode(&raw).unwrap();

        assert_eq!(decoded.version, CURRENT_SPIN_MESH_VERSION as u32);
        // The index codec keeps the triangle order and winding but may rotate each triangle.
        let canonical = |indices: &[u32]| -> Vec<[u32; 3]> {
            indices
                .chunks(3)
                .map(|t| {
                    let first = (0..3).min_by_key(|&k| t[k]).unwrap();
                    [t[first], t[(first + 1) % 3], t[(first + 2) % 3]]
                })
                .collect()
        };
        assert_eq!(canonical(&decoded.indices), canonical(&reduced.indices));
        assert_eq!(decoded.positions.len(), reduced.positions.len());
        // Rounding moves a coordinate by at most half a step (plus float noise).
        let tolerance = decoded.quantisation_step * 0.5 + 1e-4;
        assert!((decoded.quantisation_step - 24.0 / QUANT_MAX).abs() < 1e-3);
        for (a, b) in decoded.positions.iter().zip(&reduced.positions) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() <= tolerance, "{a:?} vs {b:?}");
            }
        }
        assert_eq!(decoded.body_bounds, triangle_bounds(&body).unwrap());
        assert_eq!(decoded.model_bounds, triangle_bounds(&model).unwrap());
    }

    #[test]
    fn decode_rejects_other_files() {
        assert!(decode(b"not a spin mesh").is_none());
        assert!(decode(&[0u8; HEADER_BYTES]).is_none());
    }

    #[test]
    fn file_names_hold_the_stem_and_version() {
        let path = relative_path("abc_r0.00a");
        assert_eq!(path, format!("abc_r0.00a_spin_v{CURRENT_SPIN_MESH_VERSION}.bin"));
        assert!(is_spin_file(&path));
        assert!(!is_spin_file("abc_r0.00a.png"));
        assert!(!is_spin_file("abc_r0.00a_supportsremoved.png"));
    }
}
