//! Serialises centred Y-up mesh geometry into a compact binary format optimised for
//! frontend transfer: 16-bit quantised indexed positions plus 16/32-bit triangle indices.
//!
//! Layout ("FMSH" v1, little-endian, 56-byte header):
//!   0  u32 magic, 4 u8 version, 5 u8 index element size, 6 u8 quantisation bits, 7 u8 reserved
//!   8  u32 vertex count, 12 u32 triangle count
//!   16 f32 dimX, dimY, dimZ, 28 f32 sphere centre xyz, 40 f32 sphere radius
//!   44 u32 positions byte length, 48 u32 index offset, 52 u32 indices byte length

use rustc_hash::FxHashMap;

use super::LoadedGeometry;

pub const CONTENT_TYPE: &str = "application/vnd.findamodel.mesh";
pub const CONTENT_TYPE_SPLIT: &str = "application/vnd.findamodel.mesh-split";

const MAGIC: u32 = 0x4853_4D46; // "FMSH" little-endian
const VERSION: u8 = 1;
const QUANTIZATION_BITS: u8 = 16;
pub const HEADER_SIZE: usize = 56;

pub fn encode(geometry: &LoadedGeometry) -> Vec<u8> {
    let dim_x = geometry.dimension_x_mm;
    let dim_y = geometry.dimension_y_mm;
    let dim_z = geometry.dimension_z_mm;
    let min_x = -dim_x * 0.5;
    let min_y = 0.0;
    let min_z = -dim_z * 0.5;

    let mut vertices: Vec<[u16; 3]> = Vec::new();
    let mut vertex_map: FxHashMap<u64, u32> = FxHashMap::default();
    let mut indices: Vec<u32> = Vec::with_capacity(geometry.triangles.len() * 3);

    for tri in &geometry.triangles {
        for v in [tri.v0, tri.v1, tri.v2] {
            let q = [quantize(v.x, min_x, dim_x), quantize(v.y, min_y, dim_y), quantize(v.z, min_z, dim_z)];
            let key = (q[0] as u64) | ((q[1] as u64) << 16) | ((q[2] as u64) << 32);
            let index = *vertex_map.entry(key).or_insert_with(|| {
                vertices.push(q);
                (vertices.len() - 1) as u32
            });
            indices.push(index);
        }
    }

    let index_element_size: u8 = if vertices.len() <= u16::MAX as usize { 2 } else { 4 };
    let positions_byte_length = vertices.len() * 3 * 2;
    let index_offset = align4(HEADER_SIZE + positions_byte_length);
    let indices_byte_length = indices.len() * index_element_size as usize;

    let mut buffer = Vec::with_capacity(index_offset + indices_byte_length);
    buffer.extend_from_slice(&MAGIC.to_le_bytes());
    buffer.push(VERSION);
    buffer.push(index_element_size);
    buffer.push(QUANTIZATION_BITS);
    buffer.push(0);
    buffer.extend_from_slice(&(vertices.len() as u32).to_le_bytes());
    buffer.extend_from_slice(&(geometry.triangles.len() as u32).to_le_bytes());
    for value in [
        dim_x,
        dim_y,
        dim_z,
        geometry.sphere_centre.x,
        geometry.sphere_centre.y,
        geometry.sphere_centre.z,
        geometry.sphere_radius,
    ] {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    buffer.extend_from_slice(&(positions_byte_length as u32).to_le_bytes());
    buffer.extend_from_slice(&(index_offset as u32).to_le_bytes());
    buffer.extend_from_slice(&(indices_byte_length as u32).to_le_bytes());
    debug_assert_eq!(buffer.len(), HEADER_SIZE);

    for vertex in &vertices {
        for c in vertex {
            buffer.extend_from_slice(&c.to_le_bytes());
        }
    }
    buffer.resize(index_offset, 0);

    if index_element_size == 2 {
        for index in &indices {
            buffer.extend_from_slice(&(*index as u16).to_le_bytes());
        }
    } else {
        for index in &indices {
            buffer.extend_from_slice(&index.to_le_bytes());
        }
    }

    buffer
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

fn quantize(value: f32, min: f32, range: f32) -> u16 {
    if range <= 1e-8 {
        return 0;
    }
    let normalized = ((value - min) / range).clamp(0.0, 1.0);
    ((normalized * u16::MAX as f32).round_ties_even() as i32).clamp(0, u16::MAX as i32) as u16
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Triangle, Vec3};
    use std::collections::HashSet;

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn read_f32(bytes: &[u8], offset: usize) -> f32 {
        f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
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

    #[test]
    fn encode_deduplicates_vertices_and_uses_u16_indices() {
        let bytes = encode(&tetrahedron());
        assert_eq!(read_u32(&bytes, 0), MAGIC);
        assert_eq!(read_u32(&bytes, 8), 4);
        assert_eq!(read_u32(&bytes, 12), 4);
        assert_eq!(bytes[5], 2);

        let (dx, dy, dz) = (read_f32(&bytes, 16), read_f32(&bytes, 20), read_f32(&bytes, 24));
        let scale = |d: f32| if d == 0.0 { 0.0 } else { d / u16::MAX as f32 };
        let actual: HashSet<(i32, i32, i32)> = (0..4)
            .map(|i| {
                let o = HEADER_SIZE + i * 6;
                let q = |k: usize| u16::from_le_bytes([bytes[o + k], bytes[o + k + 1]]) as f32;
                let x = -dx / 2.0 + q(0) * scale(dx);
                let y = q(2) * scale(dy);
                let z = -dz / 2.0 + q(4) * scale(dz);
                ((x * 1000.0).round() as i32, (y * 1000.0).round() as i32, (z * 1000.0).round() as i32)
            })
            .collect();

        let expected: HashSet<(i32, i32, i32)> =
            [(-5000, 0, -5000), (5000, 0, -5000), (0, 0, 5000), (0, 10000, 0)].into_iter().collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn encode_uses_u32_indices_when_vertex_count_exceeds_u16() {
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
    }

    #[test]
    fn split_envelope_body_and_support_blobs_are_valid_payloads() {
        let body_triangles: Vec<Triangle> = (0..4).flat_map(|i| square(i as f32, 0.0)).collect();
        let support_triangles = square(0.0, 100.0);
        let body = encode(&geometry_for(body_triangles.clone()));
        let supports = encode(&geometry_for(support_triangles.clone()));
        let envelope = encode_split_envelope(&body, &supports);

        let body_len = read_u32(&envelope, 0) as usize;
        assert_eq!(body_len, body.len());
        assert_eq!(read_u32(&envelope, 4), MAGIC);
        assert_eq!(read_u32(&envelope, 4 + 12) as usize, body_triangles.len());

        let support_offset = 4 + body_len;
        let support_len = read_u32(&envelope, support_offset) as usize;
        assert_eq!(support_len, supports.len());
        assert_eq!(read_u32(&envelope, support_offset + 4), MAGIC);
        assert_eq!(read_u32(&envelope, support_offset + 4 + 12) as usize, support_triangles.len());
    }

    #[test]
    fn split_envelope_lengths_match_blob_sizes() {
        let payload = encode(&geometry_for(square(0.0, 0.0)));
        let envelope = encode_split_envelope(&payload, &payload);
        assert_eq!(envelope.len(), 4 + payload.len() + 4 + payload.len());
        assert_eq!(read_u32(&envelope, 0) as usize, payload.len());
        assert_eq!(read_u32(&envelope, 4 + payload.len()) as usize, payload.len());
    }
}
