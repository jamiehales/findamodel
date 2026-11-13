//! Serialises centred Y-up mesh geometry into a compact binary format for frontend transfer:
//! 16-bit quantised indexed positions and triangle indices, both compressed with
//! meshoptimizer's vertex and index codecs. Encoded per request (nothing is cached).
//!
//! Layout ("FMSH" v2, little-endian, 56-byte header):
//!   0  u32 magic, 4 u8 version, 5 u8 decoded index element size (2 or 4), 6 u8 quantisation
//!      bits, 7 u8 reserved
//!   8  u32 vertex count, 12 u32 triangle count
//!   16 f32 dimX, dimY, dimZ, 28 f32 sphere centre xyz, 40 f32 sphere radius
//!   44 u32 encoded vertex buffer length, 48 u32 encoded index buffer length, 52 u32 reserved
//!   56 encoded vertex buffer (4 x u16 per vertex: x, y, z, 0), then the encoded index buffer
//!
//! Position = min + q / 65535 * dim per axis, with min = (-dimX / 2, 0, -dimZ / 2). Vertices
//! are welded by their quantised position. Triangles are reordered for the vertex cache and
//! vertices for fetch order, which is what lets the codecs compress them well; the index codec
//! keeps each triangle's winding but may rotate its corners.

use rustc_hash::FxHashMap;

use super::{LoadedGeometry, Triangle, Vec3};

pub const CONTENT_TYPE: &str = "application/vnd.findamodel.mesh";
pub const CONTENT_TYPE_SPLIT: &str = "application/vnd.findamodel.mesh-split";
/// A mesh with a flag per face ([`encode_flagged`]).
pub const CONTENT_TYPE_REPAIRED: &str = "application/vnd.findamodel.mesh-repaired";
/// Every content type a mesh is sent with.
pub const CONTENT_TYPES: [&str; 3] = [CONTENT_TYPE, CONTENT_TYPE_SPLIT, CONTENT_TYPE_REPAIRED];

const MAGIC: u32 = 0x4853_4D46; // "FMSH" little-endian
pub const VERSION: u8 = 2;
const QUANTIZATION_BITS: u8 = 16;
pub const HEADER_SIZE: usize = 56;
/// Cache size the triangle order is tuned for. The FIFO optimiser leaves the codecs output about
/// 4% larger than the full vertex cache optimiser, at about half its time on large meshes.
const VERTEX_CACHE_SIZE: u32 = 16;

/// A mesh quantised and welded, before it is ordered and encoded.
struct Quantised {
    vertices: Vec<[u16; 4]>,
    /// Three per triangle, in the geometry's triangle order.
    indices: Vec<u32>,
}

fn quantise(geometry: &LoadedGeometry) -> Quantised {
    let (dim_x, dim_y, dim_z) = (geometry.dimension_x_mm, geometry.dimension_y_mm, geometry.dimension_z_mm);
    let (min_x, min_y, min_z) = (-dim_x * 0.5, 0.0, -dim_z * 0.5);
    let mut vertices: Vec<[u16; 4]> = Vec::new();
    let mut vertex_map: FxHashMap<u64, u32> = FxHashMap::default();
    let mut indices: Vec<u32> = Vec::with_capacity(geometry.triangles.len() * 3);
    for tri in &geometry.triangles {
        for v in [tri.v0, tri.v1, tri.v2] {
            let q = [quantize(v.x, min_x, dim_x), quantize(v.y, min_y, dim_y), quantize(v.z, min_z, dim_z)];
            let key = (q[0] as u64) | ((q[1] as u64) << 16) | ((q[2] as u64) << 32);
            let index = *vertex_map.entry(key).or_insert_with(|| {
                vertices.push([q[0], q[1], q[2], 0]);
                (vertices.len() - 1) as u32
            });
            indices.push(index);
        }
    }
    Quantised { vertices, indices }
}

/// The mesh in FMSH form. `runs` splits the triangles (in order) into runs of that many
/// triangles that are each reordered within themselves and kept in that order, so a per-run
/// flag stays aligned.
fn encode_runs(geometry: &LoadedGeometry, quantised: Quantised, runs: &[usize]) -> Vec<u8> {
    let Quantised { vertices, indices } = quantised;
    let vertex_count = vertices.len();
    let mut ordered = Vec::with_capacity(indices.len());
    let mut start = 0;
    for &run in runs {
        let run = &indices[start * 3..(start + run) * 3];
        ordered.extend(meshopt::optimize_vertex_cache_fifo(run, vertex_count, VERTEX_CACHE_SIZE));
        start += run.len() / 3;
    }
    debug_assert_eq!(ordered.len(), indices.len());
    drop(indices);
    let vertices = meshopt::optimize_vertex_fetch(&mut ordered, &vertices);

    let encoded_vertices = meshopt::encode_vertex_buffer(&vertices).expect("vertex buffer encodes");
    let encoded_indices = meshopt::encode_index_buffer(&ordered, vertex_count).expect("index buffer encodes");
    let index_element_size: u8 = if vertex_count <= u16::MAX as usize { 2 } else { 4 };

    let mut buffer = Vec::with_capacity(HEADER_SIZE + encoded_vertices.len() + encoded_indices.len());
    buffer.extend_from_slice(&MAGIC.to_le_bytes());
    buffer.extend_from_slice(&[VERSION, index_element_size, QUANTIZATION_BITS, 0]);
    buffer.extend_from_slice(&(vertex_count as u32).to_le_bytes());
    buffer.extend_from_slice(&((ordered.len() / 3) as u32).to_le_bytes());
    for value in [
        geometry.dimension_x_mm,
        geometry.dimension_y_mm,
        geometry.dimension_z_mm,
        geometry.sphere_centre.x,
        geometry.sphere_centre.y,
        geometry.sphere_centre.z,
        geometry.sphere_radius,
    ] {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    for length in [encoded_vertices.len() as u32, encoded_indices.len() as u32, 0] {
        buffer.extend_from_slice(&length.to_le_bytes());
    }
    debug_assert_eq!(buffer.len(), HEADER_SIZE);
    buffer.extend_from_slice(&encoded_vertices);
    buffer.extend_from_slice(&encoded_indices);
    buffer
}

pub fn encode(geometry: &LoadedGeometry) -> Vec<u8> {
    let quantised = quantise(geometry);
    let triangles = quantised.indices.len() / 3;
    encode_runs(geometry, quantised, &[triangles])
}

/// Wraps two encoded meshes: `[bodyLength:u32][body][supportLength:u32][support]`.
pub fn encode_split_envelope(body: &[u8], supports: &[u8]) -> Vec<u8> {
    let mut envelope = Vec::with_capacity(8 + body.len() + supports.len());
    envelope.extend_from_slice(&(body.len() as u32).to_le_bytes());
    envelope.extend_from_slice(body);
    envelope.extend_from_slice(&(supports.len() as u32).to_le_bytes());
    envelope.extend_from_slice(supports);
    envelope
}

/// A mesh with one flag per triangle: `[meshLength:u32][FMSH][faceCount:u32][flags]`, the flags
/// packed eight to a byte, the first triangle's in the lowest bit. `flags` follows the
/// geometry's triangles; the mesh sends the unflagged triangles first, then the flagged ones
/// (each run reordered within itself), and the flags follow the mesh's order.
pub fn encode_flagged(geometry: &LoadedGeometry, flags: &[bool]) -> Vec<u8> {
    debug_assert_eq!(flags.len(), geometry.triangles.len());
    let mut quantised = quantise(geometry);
    let triangles = quantised.indices.len() / 3;
    let flagged = |t: usize| flags.get(t).copied().unwrap_or(false);
    let partitioned: Vec<u32> = (0..triangles)
        .filter(|&t| !flagged(t))
        .chain((0..triangles).filter(|&t| flagged(t)))
        .flat_map(|t| quantised.indices[t * 3..t * 3 + 3].to_vec())
        .collect();
    let kept = (0..triangles).filter(|&t| !flagged(t)).count();
    quantised.indices = partitioned;
    let mesh = encode_runs(geometry, quantised, &[kept, triangles - kept]);

    let mut envelope = Vec::with_capacity(8 + mesh.len() + triangles.div_ceil(8));
    envelope.extend_from_slice(&(mesh.len() as u32).to_le_bytes());
    envelope.extend_from_slice(&mesh);
    envelope.extend_from_slice(&(triangles as u32).to_le_bytes());
    let mut bits = vec![0u8; triangles.div_ceil(8)];
    for t in kept..triangles {
        bits[t / 8] |= 1 << (t % 8);
    }
    envelope.extend_from_slice(&bits);
    envelope
}

/// Triangles already in the frontend frame (Y-up, X/Z centred on the model, the plate at Y = 0)
/// with bounds that cover them all, for [`encode`]: a mesh built in the model's frame (rebuilt
/// supports, a repaired body) need not span the model's own box.
pub fn covering(triangles: Vec<Triangle>) -> LoadedGeometry {
    let (mut x, mut y, mut z) = (0.0f32, 0.0f32, 0.0f32);
    for v in triangles.iter().flat_map(|t| [t.v0, t.v1, t.v2]) {
        x = x.max(v.x.abs());
        y = y.max(v.y);
        z = z.max(v.z.abs());
    }
    let centre = Vec3::new(0.0, y / 2.0, 0.0);
    LoadedGeometry {
        triangles,
        sphere_centre: centre,
        sphere_radius: Vec3::new(x, y / 2.0, z).length(),
        dimension_x_mm: 2.0 * x,
        dimension_y_mm: y,
        dimension_z_mm: 2.0 * z,
    }
}

fn quantize(value: f32, min: f32, range: f32) -> u16 {
    if range <= 1e-8 {
        return 0;
    }
    let normalized = ((value - min) / range).clamp(0.0, 1.0);
    ((normalized * u16::MAX as f32).round_ties_even() as i32).clamp(0, u16::MAX as i32) as u16
}

/// A decoded FMSH mesh (for tests and tools; the browser decodes its own).
#[derive(Debug, Clone)]
pub struct Decoded {
    pub index_element_size: u8,
    pub dimensions: [f32; 3],
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl Decoded {
    /// The largest error quantisation can add to each coordinate.
    pub fn quantisation_step(&self) -> [f32; 3] {
        self.dimensions.map(|d| d / u16::MAX as f32)
    }

    /// The triangles' corners.
    pub fn triangles(&self) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
        self.indices.chunks(3).map(|t| [0, 1, 2].map(|k| self.positions[t[k] as usize]))
    }
}

/// Decodes an FMSH mesh. None when it isn't one (of this version).
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    if bytes.len() < HEADER_SIZE {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let f32_at = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    if u32_at(0) != MAGIC || bytes[4] != VERSION || bytes[6] != QUANTIZATION_BITS {
        return None;
    }
    let (vertex_count, triangle_count) = (u32_at(8) as usize, u32_at(12) as usize);
    let dimensions = [f32_at(16), f32_at(20), f32_at(24)];
    let (vertex_bytes, index_bytes) = (u32_at(44) as usize, u32_at(48) as usize);
    let vertex_end = HEADER_SIZE.checked_add(vertex_bytes)?;
    let index_end = vertex_end.checked_add(index_bytes).filter(|&e| e == bytes.len())?;
    let quantised: Vec<[u16; 4]> = meshopt::decode_vertex_buffer(&bytes[HEADER_SIZE..vertex_end], vertex_count).ok()?;
    let indices: Vec<u32> = meshopt::decode_index_buffer(&bytes[vertex_end..index_end], triangle_count * 3).ok()?;
    let min = [-dimensions[0] / 2.0, 0.0, -dimensions[2] / 2.0];
    let positions =
        quantised.iter().map(|q| [0, 1, 2].map(|k| min[k] + q[k] as f32 / u16::MAX as f32 * dimensions[k])).collect();
    Some(Decoded { index_element_size: bytes[5], dimensions, positions, indices })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Triangle, Vec3};

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn tetrahedron() -> LoadedGeometry {
        let up = Vec3::UP;
        LoadedGeometry {
            triangles: vec![
                Triangle::new(Vec3::new(-5.0, 0.0, -5.0), Vec3::new(5.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 5.0), up),
                Triangle::new(Vec3::new(-5.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 10.0, 0.0), up),
                Triangle::new(Vec3::new(5.0, 0.0, -5.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 5.0), up),
                Triangle::new(Vec3::new(-5.0, 0.0, -5.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(5.0, 0.0, -5.0), up),
            ],
            sphere_centre: Vec3::new(0.0, 5.0, 0.0),
            sphere_radius: 8.660_254_5,
            dimension_x_mm: 10.0,
            dimension_y_mm: 10.0,
            dimension_z_mm: 10.0,
        }
    }

    fn square(x_offset: f32, z_offset: f32) -> Vec<Triangle> {
        let v0 = Vec3::new(x_offset, 0.0, z_offset);
        let v1 = Vec3::new(x_offset + 1.0, 0.0, z_offset);
        let v2 = Vec3::new(x_offset + 1.0, 0.0, z_offset + 1.0);
        let v3 = Vec3::new(x_offset, 0.0, z_offset + 1.0);
        vec![Triangle::new(v0, v1, v2, Vec3::UP), Triangle::new(v0, v2, v3, Vec3::UP)]
    }

    fn geometry_for(triangles: Vec<Triangle>) -> LoadedGeometry {
        LoadedGeometry {
            triangles,
            sphere_centre: Vec3::new(0.0, 0.5, 0.0),
            sphere_radius: 8.0,
            dimension_x_mm: 10.0,
            dimension_y_mm: 1.0,
            dimension_z_mm: 10.0,
        }
    }

    /// A bumpy sheet of `n` x `n` quads (2 n^2 triangles) in the model frame, as a soup.
    fn sheet(n: usize) -> LoadedGeometry {
        let p = |i: usize, j: usize| {
            let (x, z) = (i as f32 * 0.37 - n as f32 * 0.185, j as f32 * 0.37 - n as f32 * 0.185);
            Vec3::new(x, 2.0 + (x * 0.7).sin() + (z * 0.4).cos(), z)
        };
        let mut triangles = Vec::new();
        for i in 0..n {
            for j in 0..n {
                triangles.push(Triangle::new(p(i, j), p(i + 1, j), p(i + 1, j + 1), Vec3::UP));
                triangles.push(Triangle::new(p(i, j), p(i + 1, j + 1), p(i, j + 1), Vec3::UP));
            }
        }
        covering(triangles)
    }

    fn corners(t: &Triangle) -> [[f32; 3]; 3] {
        [t.v0, t.v1, t.v2].map(|v| [v.x, v.y, v.z])
    }

    /// A triangle's corners quantised as the encoder does (over `dims`) and rotated to start at
    /// the smallest, so a rotated triangle compares equal; and the corners in that rotation.
    fn canonical(t: [[f32; 3]; 3], dims: [f32; 3]) -> ([[u16; 3]; 3], [[f32; 3]; 3]) {
        let min = [-dims[0] / 2.0, 0.0, -dims[2] / 2.0];
        let q = t.map(|p| [0, 1, 2].map(|k| quantize(p[k], min[k], dims[k])));
        let first = (0..3).min_by_key(|&k| q[k]).unwrap();
        ([0, 1, 2].map(|k| q[(first + k) % 3]), [0, 1, 2].map(|k| t[(first + k) % 3]))
    }

    fn sorted_triangles(triangles: impl Iterator<Item = [[f32; 3]; 3]>, dims: [f32; 3]) -> Vec<[[u16; 3]; 3]> {
        let mut out: Vec<_> = triangles.map(|t| canonical(t, dims).0).collect();
        out.sort();
        out
    }

    #[test]
    fn round_trip_keeps_positions_within_quantisation_and_every_triangle() {
        let geometry = sheet(60);
        let bytes = encode(&geometry);
        assert_eq!(read_u32(&bytes, 0), MAGIC);
        assert_eq!(bytes[4], VERSION);
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.indices.len(), geometry.triangles.len() * 3);
        assert_eq!(decoded.positions.len(), 61 * 61, "vertices are welded");
        assert!(decoded.indices.iter().all(|&i| (i as usize) < decoded.positions.len()));

        // The triangles are the same set, winding kept, and each decoded corner is within half
        // a step (plus float noise) of its source corner.
        let dims = decoded.dimensions;
        let source = sorted_triangles(geometry.triangles.iter().map(corners), dims);
        assert_eq!(sorted_triangles(decoded.triangles(), dims), source);
        let by_key: std::collections::HashMap<_, _> =
            geometry.triangles.iter().map(|t| canonical(corners(t), dims)).collect();
        let step = decoded.quantisation_step();
        for t in decoded.triangles() {
            let (key, sent) = canonical(t, dims);
            let original = by_key[&key];
            for (a, b) in sent.iter().zip(original) {
                assert!((0..3).all(|k| (a[k] - b[k]).abs() <= step[k] * 0.5 + 1e-4), "{a:?} vs {b:?}");
            }
        }
        // The codecs pay: well under the 6 bytes per vertex and 2 per index the raw form takes.
        assert!(bytes.len() < HEADER_SIZE + (61 * 61 * 6 + geometry.triangles.len() * 6) / 2, "{}", bytes.len());
    }

    #[test]
    fn small_meshes_use_u16_indices() {
        let bytes = encode(&tetrahedron());
        assert_eq!(read_u32(&bytes, 8), 4);
        assert_eq!(read_u32(&bytes, 12), 4);
        assert_eq!(bytes[5], 2);
        let decoded = decode(&bytes).unwrap();
        let dims = decoded.dimensions;
        let expected = sorted_triangles(tetrahedron().triangles.iter().map(corners), dims);
        assert_eq!(sorted_triangles(decoded.triangles(), dims), expected);
    }

    #[test]
    fn large_meshes_use_u32_indices() {
        const VERTEX_COUNT: usize = 65_538;
        let grid = |index: usize| {
            let x = (index & 255) as f32;
            let z = ((index >> 8) & 255) as f32;
            let y = ((index >> 16) & 1) as f32;
            Vec3::new(x - 128.0, y, z - 128.0)
        };
        let triangles =
            (0..VERTEX_COUNT).step_by(3).map(|i| Triangle::new(grid(i), grid(i + 1), grid(i + 2), Vec3::UP)).collect();
        let geometry = LoadedGeometry {
            triangles,
            sphere_centre: Vec3::new(0.0, 0.5, 0.0),
            sphere_radius: 512.0,
            dimension_x_mm: 512.0,
            dimension_y_mm: 1.0,
            dimension_z_mm: 512.0,
        };

        let bytes = encode(&geometry);
        assert!(read_u32(&bytes, 8) > u16::MAX as u32);
        assert_eq!(bytes[5], 4);
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.positions.len(), read_u32(&bytes, 8) as usize);
        assert!(decoded.indices.iter().all(|&i| (i as usize) < decoded.positions.len()));
    }

    #[test]
    fn empty_meshes_round_trip() {
        let decoded = decode(&encode(&geometry_for(Vec::new()))).unwrap();
        assert!(decoded.positions.is_empty() && decoded.indices.is_empty());
    }

    #[test]
    fn decode_rejects_other_payloads() {
        assert!(decode(b"not a mesh").is_none());
        let mut bytes = encode(&tetrahedron());
        bytes[4] = 1;
        assert!(decode(&bytes).is_none(), "the raw v1 form is gone");
    }

    #[test]
    fn split_envelope_body_and_support_blobs_are_valid_payloads() {
        let body_triangles: Vec<Triangle> = (0..4).flat_map(|i| square(i as f32, 0.0)).collect();
        let support_triangles = square(0.0, 3.0);
        let body = encode(&geometry_for(body_triangles.clone()));
        let supports = encode(&geometry_for(support_triangles.clone()));
        let envelope = encode_split_envelope(&body, &supports);

        let body_len = read_u32(&envelope, 0) as usize;
        assert_eq!(body_len, body.len());
        assert_eq!(decode(&envelope[4..4 + body_len]).unwrap().indices.len(), body_triangles.len() * 3);
        let support_offset = 4 + body_len;
        let support_len = read_u32(&envelope, support_offset) as usize;
        assert_eq!(support_len, supports.len());
        let supports = decode(&envelope[support_offset + 4..support_offset + 4 + support_len]).unwrap();
        assert_eq!(supports.indices.len(), support_triangles.len() * 3);
        assert_eq!(envelope.len(), support_offset + 4 + support_len);
    }

    /// The flags of a flagged envelope, unpacked, and its decoded mesh.
    fn unpack_flagged(envelope: &[u8]) -> (Decoded, Vec<bool>) {
        let mesh_len = read_u32(envelope, 0) as usize;
        let mesh = decode(&envelope[4..4 + mesh_len]).unwrap();
        let at = 4 + mesh_len;
        let count = read_u32(envelope, at) as usize;
        assert_eq!(envelope.len(), at + 4 + count.div_ceil(8));
        let flags = (0..count).map(|t| envelope[at + 4 + t / 8] >> (t % 8) & 1 == 1).collect();
        (mesh, flags)
    }

    #[test]
    fn flags_follow_their_triangles_through_the_reordering() {
        let geometry = sheet(30);
        // Flag a scattered set of triangles: every seventh, and a patch in the middle.
        let flags: Vec<bool> = (0..geometry.triangles.len()).map(|t| t % 7 == 3 || (850..900).contains(&t)).collect();
        let (mesh, sent_flags) = unpack_flagged(&encode_flagged(&geometry, &flags));
        assert_eq!(sent_flags.len(), geometry.triangles.len());
        assert_eq!(sent_flags.iter().filter(|&&f| f).count(), flags.iter().filter(|&&f| f).count());

        let dims = mesh.dimensions;
        let source = |flag: bool| {
            sorted_triangles(
                geometry.triangles.iter().zip(&flags).filter(|(_, f)| **f == flag).map(|(t, _)| corners(t)),
                dims,
            )
        };
        let sent = |flag: bool| {
            sorted_triangles(mesh.triangles().zip(&sent_flags).filter(|(_, f)| **f == flag).map(|(t, _)| t), dims)
        };
        assert_eq!(sent(true), source(true), "the flagged triangles are the ones flagged");
        assert_eq!(sent(false), source(false));
        // Unflagged first, then flagged: the viewer draws them as two runs.
        let first_flagged = sent_flags.iter().position(|&f| f).unwrap();
        assert!(sent_flags[first_flagged..].iter().all(|&f| f));
    }

    #[test]
    fn flagged_meshes_without_flags_round_trip() {
        let geometry = geometry_for((0..5).flat_map(|i| square(i as f32, 0.0)).collect());
        let (mesh, flags) = unpack_flagged(&encode_flagged(&geometry, &[false; 10]));
        assert_eq!(mesh.indices.len(), 30);
        assert!(flags.iter().all(|&f| !f));
        let (_, flags) = unpack_flagged(&encode_flagged(&geometry, &[true; 10]));
        assert!(flags.iter().all(|&f| f));
    }

    #[test]
    fn covering_bounds_span_every_vertex() {
        let tri =
            Triangle::new(Vec3::new(-3.0, 0.0, 1.0), Vec3::new(1.0, 7.0, 0.0), Vec3::new(0.0, 2.0, -5.0), Vec3::UP);
        let g = covering(vec![tri]);
        assert_eq!((g.dimension_x_mm, g.dimension_y_mm, g.dimension_z_mm), (6.0, 7.0, 10.0));
        // Every vertex quantises inside the box: none is clamped.
        let decoded = decode(&encode(&g)).unwrap();
        let mut found = decoded.positions.clone();
        found.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let expected = [[-3.0, 0.0, 1.0], [0.0, 2.0, -5.0], [1.0, 7.0, 0.0]];
        for (a, b) in found.iter().zip(expected) {
            assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1e-3), "{a:?} vs {b:?}");
        }
    }
}
