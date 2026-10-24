//! Loads STL/OBJ model files into a unified [`LoadedGeometry`] representation.
//!
//! Transformations applied (in order):
//!   1. Z-up to Y-up: (x,y,z) -> (-x,z,y) via rotateX(pi/2) then rotateZ(pi)
//!   2. Scale to mm: factor = 1.0 (STL/OBJ carry no unit metadata; assumed mm)
//!   3. Centre: X/Z centred at origin; Y translated so the base face sits at Y = 0

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};
use tracing::{error, info, warn};

use super::{LoadedGeometry, Triangle, Vec3, swap_up_axis};

const LOG_TARGET: &str = "findamodel.Loader";

/// Loads, transforms, centres, and computes geometry metadata for a model file.
/// Returns `None` if parsing fails or yields no usable triangles. Blocking; call from a
/// blocking-friendly context for large files.
pub fn load_model(path: &Path, file_type: &str) -> Option<LoadedGeometry> {
    match load_model_inner(path, file_type) {
        Ok(geometry) => geometry,
        Err(err) => {
            error!(target: LOG_TARGET, "Failed to load model {}: {err:#}", path.display());
            None
        }
    }
}

/// Async wrapper that runs [`load_model`] on the blocking thread pool.
pub async fn load_model_async(path: &Path, file_type: &str) -> Option<LoadedGeometry> {
    let path = path.to_path_buf();
    let file_type = file_type.to_string();
    tokio::task::spawn_blocking(move || load_model(&path, &file_type)).await.ok().flatten()
}

fn load_model_inner(path: &Path, file_type: &str) -> Result<Option<LoadedGeometry>> {
    let raw = match file_type.to_ascii_lowercase().as_str() {
        "stl" => parse_stl(path)?,
        "obj" => parse_obj(path)?,
        _ => None,
    };

    let Some(raw) = raw.filter(|t| !t.is_empty()) else {
        warn!(target: LOG_TARGET, "No geometry found in {}", path.display());
        return Ok(None);
    };

    Ok(build_geometry(raw, path))
}

/// Applies the axis conversion, discards non-finite triangles, centres the mesh and
/// computes bounds. `source` is only used for log messages.
pub fn build_geometry(raw: Vec<Triangle>, source: &Path) -> Option<LoadedGeometry> {
    let (mut min_x, mut min_y, mut min_z) = (f32::MAX, f32::MAX, f32::MAX);
    let (mut max_x, mut max_y, mut max_z) = (f32::MIN, f32::MIN, f32::MIN);
    let mut valid = Vec::with_capacity(raw.len());
    let mut discarded = 0usize;

    for tri in raw {
        let tri =
            Triangle::new(swap_up_axis(tri.v0), swap_up_axis(tri.v1), swap_up_axis(tri.v2), swap_up_axis(tri.normal));
        if !tri.v0.is_finite() || !tri.v1.is_finite() || !tri.v2.is_finite() {
            discarded += 1;
            continue;
        }
        for v in [tri.v0, tri.v1, tri.v2] {
            min_x = min_x.min(v.x);
            max_x = max_x.max(v.x);
            min_y = min_y.min(v.y);
            max_y = max_y.max(v.y);
            min_z = min_z.min(v.z);
            max_z = max_z.max(v.z);
        }
        valid.push(tri);
    }

    if valid.is_empty() {
        warn!(target: LOG_TARGET, "No finite geometry found in {}", source.display());
        return None;
    }

    if discarded > 0 {
        warn!(
            target: LOG_TARGET,
            "Discarded {discarded} invalid triangles containing non-finite coordinates in {}",
            source.display()
        );
    }

    let dim_x = max_x - min_x;
    let dim_y = max_y - min_y;
    let dim_z = max_z - min_z;
    if !dim_x.is_finite() || !dim_y.is_finite() || !dim_z.is_finite() || dim_x < 0.0 || dim_y < 0.0 || dim_z < 0.0 {
        warn!(
            target: LOG_TARGET,
            "Computed invalid model dimensions for {}: X={dim_x}, Y={dim_y}, Z={dim_z}",
            source.display()
        );
        return None;
    }

    let offset = Vec3::new(-(min_x + max_x) * 0.5, -min_y, -(min_z + max_z) * 0.5);
    let mut centred = Vec::with_capacity(valid.len());
    for tri in valid {
        let v0 = tri.v0 + offset;
        let v1 = tri.v1 + offset;
        let v2 = tri.v2 + offset;
        if !v0.is_finite() || !v1.is_finite() || !v2.is_finite() {
            continue;
        }
        // Normals are directions, so translation does not affect them.
        centred.push(Triangle::new(v0, v1, v2, tri.normal));
    }

    if centred.is_empty() {
        warn!(target: LOG_TARGET, "No finite centred geometry found in {}", source.display());
        return None;
    }

    let sphere_centre = Vec3::new(0.0, dim_y * 0.5, 0.0);
    if !sphere_centre.is_finite() {
        warn!(target: LOG_TARGET, "Computed invalid sphere centre for {}", source.display());
        return None;
    }

    let mut sphere_radius = 0f32;
    let mut any_valid_distance = false;
    for tri in &centred {
        for v in [tri.v0, tri.v1, tri.v2] {
            if let Some(d) = try_distance(v, sphere_centre) {
                sphere_radius = sphere_radius.max(d);
                any_valid_distance = true;
            }
        }
    }
    if !any_valid_distance || !sphere_radius.is_finite() || sphere_radius < 1e-6 {
        sphere_radius = 1.0;
    }

    info!(
        target: LOG_TARGET,
        "Loaded {}: {} triangles, {:.2}x{:.2}x{:.2} mm, sphere r={:.2} mm",
        source.display(),
        centred.len(),
        dim_x,
        dim_y,
        dim_z,
        sphere_radius
    );

    Some(LoadedGeometry {
        triangles: centred,
        sphere_centre,
        sphere_radius,
        dimension_x_mm: dim_x,
        dimension_y_mm: dim_y,
        dimension_z_mm: dim_z,
    })
}

fn try_distance(a: Vec3, b: Vec3) -> Option<f32> {
    let dx = a.x as f64 - b.x as f64;
    let dy = a.y as f64 - b.y as f64;
    let dz = a.z as f64 - b.z as f64;
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    (dist.is_finite() && dist <= f32::MAX as f64).then_some(dist as f32)
}

// ── STL parsing ──────────────────────────────────────────────────────────────

fn parse_stl(path: &Path) -> Result<Option<Vec<Triangle>>> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let length = file.metadata()?.len();

    let mut header = [0u8; 84];
    if file.read_exact(&mut header).is_err() {
        return Ok(None);
    }
    let tri_count = u32::from_le_bytes([header[80], header[81], header[82], header[83]]);

    // Distinguish binary vs ASCII by exact file size (more reliable than text heuristics).
    let expected_binary_size = 84u64 + tri_count as u64 * 50;
    if length == expected_binary_size && tri_count > 0 {
        return parse_binary_stl(file, tri_count).map(Some);
    }

    file.seek(SeekFrom::Start(0))?;
    parse_ascii_stl(file).map(Some)
}

fn read_vec3(buf: &[u8]) -> Vec3 {
    let f = |i: usize| f32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]);
    Vec3::new(f(0), f(4), f(8))
}

fn parse_binary_stl(file: File, tri_count: u32) -> Result<Vec<Triangle>> {
    const BYTES_PER_TRIANGLE: usize = 50;
    const TRIANGLES_PER_CHUNK: usize = 8192;

    let mut reader = BufReader::with_capacity(BYTES_PER_TRIANGLE * TRIANGLES_PER_CHUNK, file);
    let mut triangles = Vec::with_capacity(tri_count.min(5_000_000) as usize);
    let mut buffer = vec![0u8; BYTES_PER_TRIANGLE * TRIANGLES_PER_CHUNK];
    let mut remaining = tri_count as usize;

    while remaining > 0 {
        let in_chunk = remaining.min(TRIANGLES_PER_CHUNK);
        let bytes = in_chunk * BYTES_PER_TRIANGLE;
        reader.read_exact(&mut buffer[..bytes])?;

        for chunk in buffer[..bytes].as_chunks::<BYTES_PER_TRIANGLE>().0 {
            triangles.push(Triangle::new(
                read_vec3(&chunk[12..24]),
                read_vec3(&chunk[24..36]),
                read_vec3(&chunk[36..48]),
                read_vec3(&chunk[0..12]),
            ));
        }
        remaining -= in_chunk;
    }

    Ok(triangles)
}

fn parse_floats<const N: usize>(parts: &[&str]) -> Result<[f32; N]> {
    let mut out = [0f32; N];
    for (slot, part) in out.iter_mut().zip(parts) {
        *slot = part.parse::<f32>().with_context(|| format!("invalid number '{part}'"))?;
    }
    Ok(out)
}

fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn parse_ascii_stl(file: File) -> Result<Vec<Triangle>> {
    let reader = BufReader::new(file);
    let mut triangles = Vec::new();
    let mut normal = Vec3::default();
    let mut verts = [Vec3::default(); 3];
    let mut vert_idx = 0usize;

    for line in reader.split(b'\n') {
        let line = line?;
        let line = String::from_utf8_lossy(&line);
        let trimmed = line.trim();

        if starts_with_ignore_case(trimmed, "facet normal") {
            let parts: Vec<&str> = trimmed.split_ascii_whitespace().collect();
            if parts.len() >= 5 {
                let [x, y, z] = parse_floats::<3>(&parts[2..5])?;
                normal = Vec3::new(x, y, z);
            }
            vert_idx = 0;
        } else if starts_with_ignore_case(trimmed, "vertex ") && vert_idx < 3 {
            let parts: Vec<&str> = trimmed.split_ascii_whitespace().collect();
            if parts.len() >= 4 {
                let [x, y, z] = parse_floats::<3>(&parts[1..4])?;
                verts[vert_idx] = Vec3::new(x, y, z);
                vert_idx += 1;
            }
        } else if starts_with_ignore_case(trimmed, "endfacet") && vert_idx == 3 {
            triangles.push(Triangle::new(verts[0], verts[1], verts[2], normal));
        }
    }

    Ok(triangles)
}

// ── OBJ parsing ──────────────────────────────────────────────────────────────

fn parse_obj(path: &Path) -> Result<Option<Vec<Triangle>>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut vertices: Vec<Vec3> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    let mut triangles = Vec::new();

    for line in reader.split(b'\n') {
        let line = line?;
        let line = String::from_utf8_lossy(&line);
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("v ") {
            let p: Vec<&str> = rest.split_ascii_whitespace().collect();
            if p.len() >= 3 {
                let [x, y, z] = parse_floats::<3>(&p[0..3])?;
                vertices.push(Vec3::new(x, y, z));
            }
        } else if let Some(rest) = line.strip_prefix("vn ") {
            let p: Vec<&str> = rest.split_ascii_whitespace().collect();
            if p.len() >= 3 {
                let [x, y, z] = parse_floats::<3>(&p[0..3])?;
                normals.push(Vec3::new(x, y, z));
            }
        } else if let Some(rest) = line.strip_prefix("f ") {
            let p: Vec<&str> = rest.split_ascii_whitespace().collect();
            if p.len() < 3 {
                continue;
            }

            let mut face = Vec::with_capacity(p.len());
            for token in &p {
                face.push(parse_face_vertex(token, vertices.len(), normals.len())?);
            }

            for i in 1..face.len() - 1 {
                let (vi0, ni0) = face[0];
                let (vi1, ni1) = face[i];
                let (vi2, ni2) = face[i + 1];
                let in_range = |v: i64| v >= 0 && (v as usize) < vertices.len();
                if !in_range(vi0) || !in_range(vi1) || !in_range(vi2) {
                    continue;
                }

                let v0 = vertices[vi0 as usize];
                let v1 = vertices[vi1 as usize];
                let v2 = vertices[vi2 as usize];

                let normal_in_range = |n: i64| n >= 0 && (n as usize) < normals.len();
                let n = if normal_in_range(ni0) && normal_in_range(ni1) && normal_in_range(ni2) {
                    ((normals[ni0 as usize] + normals[ni1 as usize] + normals[ni2 as usize]) * (1.0 / 3.0)).normalized()
                } else {
                    (v1 - v0).cross(v2 - v0).normalized()
                };

                triangles.push(Triangle::new(v0, v1, v2, n));
            }
        }
    }

    Ok(Some(triangles))
}

/// Parses an OBJ face vertex token (`v`, `v/vt`, `v//vn`, `v/vt/vn`) into zero-based
/// vertex and normal indices. Negative indices are relative to the current list end.
fn parse_face_vertex(token: &str, v_count: usize, n_count: usize) -> Result<(i64, i64)> {
    let resolve = |raw: &str, count: usize| -> Result<i64> {
        let value: i64 = raw.parse().with_context(|| format!("invalid face index '{raw}'"))?;
        Ok(if value > 0 { value - 1 } else { count as i64 + value })
    };

    let mut parts = token.split('/');
    let vi = resolve(parts.next().unwrap_or_default(), v_count)?;
    let _texture = parts.next();
    let ni = match parts.next() {
        Some(raw) if !raw.is_empty() => resolve(raw, n_count)?,
        _ => -1,
    };

    Ok((vi, ni))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn loads_binary_stl() {
        let mut bytes = vec![0u8; 80];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        for f in [0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0] {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
        let (_dir, path) = write_temp("binary.stl", &bytes);

        let geometry = load_model(&path, "stl").expect("geometry");
        assert_eq!(geometry.triangles.len(), 1);
        assert!(geometry.dimension_x_mm > 0.0);
        assert!(geometry.dimension_z_mm > 0.0);
        assert!(geometry.sphere_radius.is_finite());
    }

    #[test]
    fn loads_ascii_stl_and_converts_to_centred_y_up() {
        let (_dir, path) = write_temp(
            "ascii.stl",
            b"solid t\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 0\n   vertex 10 0 0\n   vertex 0 10 5\n  endloop\n endfacet\nendsolid t\n",
        );
        let geometry = load_model(&path, "STL").expect("geometry");
        assert_eq!(geometry.triangles.len(), 1);
        // Z-up source height (5) becomes the Y dimension; the base sits at Y = 0.
        assert_eq!(geometry.dimension_y_mm, 5.0);
        let min_y = geometry.triangles.iter().flat_map(|t| [t.v0.y, t.v1.y, t.v2.y]).fold(f32::MAX, f32::min);
        assert_eq!(min_y, 0.0);
        assert_eq!(geometry.sphere_centre, Vec3::new(0.0, 2.5, 0.0));
        assert_eq!(geometry.triangles[0].normal, Vec3::new(-0.0, 1.0, 0.0));
    }

    #[test]
    fn ignores_triangles_with_nan_vertices() {
        let (_dir, path) = write_temp("nan.obj", b"v 0 0 0\nv 10 0 0\nv 0 10 0\nv NaN 0 0\nf 1 2 3\nf 1 4 3\n");
        let geometry = load_model(&path, "obj").expect("geometry");
        assert_eq!(geometry.triangles.len(), 1);
        assert!(geometry.sphere_radius.is_finite());
        assert!(geometry.sphere_centre.is_finite());
    }

    #[test]
    fn returns_none_when_all_geometry_is_non_finite() {
        let (_dir, path) = write_temp("invalid.obj", b"v NaN 0 0\nv NaN 1 0\nv NaN 0 1\nf 1 2 3\n");
        assert!(load_model(&path, "obj").is_none());
    }

    #[test]
    fn obj_supports_polygons_relative_indices_and_normals() {
        let (_dir, path) = write_temp(
            "quad.obj",
            b"# quad\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvn 0 0 1\nf -4//1 -3//1 -2//1 -1//1\n",
        );
        let geometry = load_model(&path, "obj").expect("geometry");
        assert_eq!(geometry.triangles.len(), 2);
        assert_eq!(geometry.triangles[0].normal, Vec3::new(-0.0, 1.0, 0.0));
    }

    #[test]
    fn unsupported_types_and_missing_files_return_none() {
        let (_dir, path) = write_temp("x.ctb", b"data");
        assert!(load_model(&path, "ctb").is_none());
        assert!(load_model(Path::new("/definitely/missing.stl"), "stl").is_none());
    }
}
