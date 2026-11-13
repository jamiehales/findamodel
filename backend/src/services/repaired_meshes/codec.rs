//! The repaired mesh file: a header naming the repair version and the key of the mesh it was
//! repaired from, then the repaired mesh, indexed and compressed.
//!
//! Layout: `FMRM`, format version (u8), repair version (i32), the mesh key (32 bytes), the
//! triangle and vertex counts (u64 each), then a zlib stream of the vertices and the faces.
//! Little-endian. The vertices are in the order the faces first use them, stored as their f32
//! bits (so a mesh read back writes exactly the file it was repaired into), each of their 12 bytes
//! in a plane of its own: neighbouring vertices share their high bytes, which compress to almost
//! nothing that way. Each face corner is a LEB128 number: 0 for the next new vertex, else how
//! far back from it the vertex is. No normals are stored (a repaired mesh has none).

use std::io::{Read, Write};

use crate::geometry::watertight::MeshKey;
use crate::geometry::{Triangle, Vec3};

const MAGIC: &[u8; 4] = b"FMRM";
/// Increment when the encoding changes.
const FORMAT_VERSION: u8 = 1;
const HEADER_BYTES: usize = 4 + 1 + 4 + 32 + 8 + 8;

/// What a repaired mesh file says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub repair_version: i32,
    pub key: MeshKey,
}

pub fn encode(header: &Header, triangles: &[Triangle]) -> std::io::Result<Vec<u8>> {
    let bits = |v: Vec3| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()];
    let mut index: rustc_hash::FxHashMap<[u32; 3], u32> = rustc_hash::FxHashMap::default();
    let mut verts: Vec<[u32; 3]> = Vec::new();
    let mut codes = Vec::with_capacity(triangles.len() * 4);
    for t in triangles {
        for v in [t.v0, t.v1, t.v2] {
            let next = verts.len() as u32;
            let at = *index.entry(bits(v)).or_insert_with(|| {
                verts.push(bits(v));
                next
            });
            leb128(&mut codes, if at == next { 0 } else { next - at });
        }
    }

    let mut out = Vec::with_capacity(HEADER_BYTES);
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&header.repair_version.to_le_bytes());
    out.extend_from_slice(&header.key);
    out.extend_from_slice(&(triangles.len() as u64).to_le_bytes());
    out.extend_from_slice(&(verts.len() as u64).to_le_bytes());
    let mut z = flate2::write::ZlibEncoder::new(out, flate2::Compression::fast());
    let mut plane = vec![0u8; verts.len()];
    for axis in 0..3 {
        for byte in 0..4 {
            for (p, v) in plane.iter_mut().zip(&verts) {
                *p = (v[axis] >> (8 * byte)) as u8;
            }
            z.write_all(&plane)?;
        }
    }
    z.write_all(&codes)?;
    z.finish()
}

/// The header, read from the start of a file. None when the bytes are not a repaired mesh in the
/// current format.
pub fn decode_header(bytes: &[u8]) -> Option<Header> {
    let head = bytes.get(..HEADER_BYTES)?;
    if &head[..4] != MAGIC || head[4] != FORMAT_VERSION {
        return None;
    }
    Some(Header { repair_version: i32::from_le_bytes(head[5..9].try_into().ok()?), key: head[9..41].try_into().ok()? })
}

/// The whole file. None when the bytes are not a whole repaired mesh in the current format.
pub fn decode(bytes: &[u8]) -> Option<(Header, Vec<Triangle>)> {
    let header = decode_header(bytes)?;
    let count = |at: usize| usize::try_from(u64::from_le_bytes(bytes[at..at + 8].try_into().ok()?)).ok();
    let (triangles, verts) = (count(41)?, count(49)?);
    // Every vertex is used by a face, and every corner takes a byte at least.
    if verts > triangles.checked_mul(3)? {
        return None;
    }
    let mut payload = Vec::new();
    // Bounded, so a damaged count cannot ask for more than the stream holds.
    let most = verts.checked_mul(12)?.checked_add(triangles.checked_mul(3 * 5)?)?;
    let mut z = flate2::read::ZlibDecoder::new(&bytes[HEADER_BYTES..]);
    z.by_ref().take(most as u64 + 1).read_to_end(&mut payload).ok()?;
    // The whole stream, and nothing after it.
    if z.total_in() as usize != bytes.len() - HEADER_BYTES {
        return None;
    }
    let (planes, mut codes) = payload.split_at_checked(verts * 12)?;
    let mut points = vec![[0u32; 3]; verts];
    for axis in 0..3 {
        for byte in 0..4 {
            let plane = &planes[(axis * 4 + byte) * verts..][..verts];
            for (p, &b) in points.iter_mut().zip(plane) {
                p[axis] |= u32::from(b) << (8 * byte);
            }
        }
    }
    let point = |p: [u32; 3]| Vec3::new(f32::from_bits(p[0]), f32::from_bits(p[1]), f32::from_bits(p[2]));
    let mut next = 0usize;
    let mut corner = || -> Option<Vec3> {
        let code = read_leb128(&mut codes)? as usize;
        let at = if code == 0 {
            next += 1;
            next - 1
        } else {
            next.checked_sub(code)?
        };
        points.get(at).copied().map(point)
    };
    let mut out = Vec::with_capacity(triangles);
    for _ in 0..triangles {
        let (a, b, c) = (corner()?, corner()?, corner()?);
        out.push(Triangle::new(a, b, c, Vec3::default()));
    }
    (codes.is_empty() && next == verts).then_some((header, out))
}

fn leb128(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn read_leb128(bytes: &mut &[u8]) -> Option<u32> {
    let mut v = 0u32;
    for shift in (0..35).step_by(7) {
        let (&b, rest) = bytes.split_first()?;
        *bytes = rest;
        v |= u32::from(b & 0x7f).checked_shl(shift)?;
        if b < 0x80 {
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header { repair_version: 7, key: [3; 32] }
    }

    /// A strip of faces sharing corners, with values a float compare would call equal or unequal
    /// to themselves (-0.0 and 0.0, NaN).
    fn mesh(n: usize) -> Vec<Triangle> {
        let mut out: Vec<Triangle> = (0..n)
            .map(|i| {
                let x = i as f32 * 0.37;
                Triangle::new(
                    Vec3::new(x, 0.0, 1.5),
                    Vec3::new(x + 0.37, 0.0, 1.5),
                    Vec3::new(x, 2.0, -1.25e-7),
                    Vec3::default(),
                )
            })
            .collect();
        out.push(Triangle::new(
            Vec3::new(-0.0, 0.0, 0.0),
            Vec3::new(f32::NAN, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::default(),
        ));
        out
    }

    fn bits(triangles: &[Triangle]) -> Vec<u32> {
        triangles
            .iter()
            .flat_map(|t| [t.v0, t.v1, t.v2, t.normal])
            .flat_map(|v| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()])
            .collect()
    }

    #[test]
    fn a_mesh_reads_back_bit_for_bit() {
        for n in [0, 1, 200, 70_000] {
            let original = mesh(n);
            let bytes = encode(&header(), &original).unwrap();
            let (h, read) = decode(&bytes).unwrap();
            assert_eq!(h, header());
            assert_eq!(bits(&read), bits(&original), "{n} faces");
            assert_eq!(decode_header(&bytes), Some(header()));
        }
    }

    #[test]
    fn shared_corners_are_stored_once() {
        let original = mesh(70_000);
        let bytes = encode(&header(), &original).unwrap();
        // 36 bytes of corners a face as triangles; far less indexed and compressed.
        assert!(bytes.len() * 8 < original.len() * 36, "{} bytes", bytes.len());
    }

    #[test]
    fn a_damaged_or_foreign_file_is_not_read() {
        let bytes = encode(&header(), &mesh(500)).unwrap();
        assert!(decode(&bytes[..bytes.len() - 1]).is_none(), "cut short");
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode(&longer).is_none(), "trailing bytes");
        let mut flipped = bytes.clone();
        let mid = HEADER_BYTES + (bytes.len() - HEADER_BYTES) / 2;
        flipped[mid] ^= 0x55;
        assert!(decode(&flipped).is_none(), "a damaged stream");
        let mut counts = bytes.clone();
        counts[41] ^= 1;
        assert!(decode(&counts).is_none(), "another triangle count");
        let mut format = bytes.clone();
        format[4] = FORMAT_VERSION + 1;
        assert!(decode(&format).is_none() && decode_header(&format).is_none());
        assert!(decode(b"FMRM").is_none() && decode(b"").is_none());
    }
}
