//! CPU software rasteriser, used when the GPU renderer is unavailable or disabled.
//!
//! - 2x SSAA: renders at twice the resolution then box-filters down for clean edges.
//! - Triangles are projected and shaded in parallel, binned into 64x64 tiles, and the tiles
//!   are rasterised in parallel; tiles own disjoint pixels so no locking is needed.
//! - Camera and lighting match the frontend viewer (see `camera`).

use rayon::prelude::*;

use super::camera::{self, FOV_Y};
use super::encode_png;
use crate::geometry::{Triangle, Vec3};

const SUPER_SAMPLE: usize = 2;
const TILE_SIZE: usize = 64;

#[derive(Clone, Copy)]
struct ProjectedTriangle {
    s: [[f32; 3]; 3],
    colour: [u8; 4],
    min_x: usize,
    max_x: usize,
    min_y: usize,
    max_y: usize,
    area: f32,
    inv_area: f32,
}

struct View {
    eye: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    tan_half_fov: f32,
    aspect: f32,
    width: usize,
    height: usize,
}

/// Renders the body (and optional supports) to a PNG of `width` x `height`.
pub fn render(
    body: &[Triangle],
    width: usize,
    height: usize,
    albedo: Vec3,
    supports: Option<&[Triangle]>,
    support_albedo: Vec3,
) -> Vec<u8> {
    let ss_w = width * SUPER_SAMPLE;
    let ss_h = height * SUPER_SAMPLE;
    let aspect = ss_w as f32 / ss_h as f32;

    // Frame on all geometry combined so supports aren't clipped.
    let supports = supports.filter(|s| !s.is_empty());
    let sets: Vec<&[Triangle]> = std::iter::once(body).chain(supports).collect();
    let (centre, eye, _) = camera::frame(sets.iter().copied(), aspect);

    let forward = (centre - eye).normalized();
    let world_up = if forward.dot(Vec3::UP).abs() > 0.99 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::UP };
    let right = forward.cross(world_up).normalized();
    let up = right.cross(forward).normalized();
    let view = View { eye, forward, right, up, tan_half_fov: (FOV_Y / 2.0).tan(), aspect, width: ss_w, height: ss_h };

    let mut projected: Vec<ProjectedTriangle> =
        body.par_iter().filter_map(|t| project_triangle(t, &view, albedo)).collect();
    if let Some(supports) = supports {
        projected.par_extend(supports.par_iter().filter_map(|t| project_triangle(t, &view, support_albedo)));
    }

    let tiles_x = ss_w.div_ceil(TILE_SIZE);
    let tiles_y = ss_h.div_ceil(TILE_SIZE);
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); tiles_x * tiles_y];
    for (i, p) in projected.iter().enumerate() {
        for ty in p.min_y / TILE_SIZE..=p.max_y / TILE_SIZE {
            for tx in p.min_x / TILE_SIZE..=p.max_x / TILE_SIZE {
                bins[ty * tiles_x + tx].push(i as u32);
            }
        }
    }

    let tiles: Vec<Vec<[u8; 4]>> = (0..tiles_x * tiles_y)
        .into_par_iter()
        .map(|tile| {
            let (tx, ty) = (tile % tiles_x, tile / tiles_x);
            rasterize_tile(&projected, &bins[tile], tx * TILE_SIZE, ty * TILE_SIZE, ss_w, ss_h)
        })
        .collect();

    let mut ss_pixels = vec![[0u8; 4]; ss_w * ss_h];
    for (tile, pixels) in tiles.iter().enumerate() {
        let (x0, y0) = ((tile % tiles_x) * TILE_SIZE, (tile / tiles_x) * TILE_SIZE);
        let tile_w = TILE_SIZE.min(ss_w - x0);
        let tile_h = TILE_SIZE.min(ss_h - y0);
        for y in 0..tile_h {
            let src = &pixels[y * TILE_SIZE..y * TILE_SIZE + tile_w];
            let dst = (y0 + y) * ss_w + x0;
            ss_pixels[dst..dst + tile_w].copy_from_slice(src);
        }
    }

    encode_png(&downsample(&ss_pixels, ss_w, width, height), width as u32, height as u32)
}

fn project(world: Vec3, v: &View) -> (f32, f32, f32) {
    let rel = world - v.eye;
    let vz = rel.dot(v.forward);
    if vz <= 0.001 {
        return (0.0, 0.0, -1.0);
    }
    let ndc_x = rel.dot(v.right) / (vz * v.tan_half_fov * v.aspect);
    let ndc_y = rel.dot(v.up) / (vz * v.tan_half_fov);
    ((ndc_x + 1.0) * 0.5 * v.width as f32, (1.0 - ndc_y) * 0.5 * v.height as f32, vz)
}

fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

fn project_triangle(tri: &Triangle, view: &View, albedo: Vec3) -> Option<ProjectedTriangle> {
    let n = (tri.v1 - tri.v0).cross(tri.v2 - tri.v0);
    if n.length_sq() < 1e-12 {
        return None; // degenerate
    }
    let n = n.normalized();
    if n.dot(view.eye - tri.v0) < 0.0 {
        return None; // back face
    }

    let s = [project(tri.v0, view), project(tri.v1, view), project(tri.v2, view)];
    if s.iter().any(|p| p.2 <= 0.0) {
        return None;
    }

    let xs = [s[0].0, s[1].0, s[2].0];
    let ys = [s[0].1, s[1].1, s[2].1];
    let min_x = xs.iter().copied().fold(f32::MAX, f32::min).max(0.0);
    let max_x = xs.iter().copied().fold(f32::MIN, f32::max).ceil().min(view.width as f32 - 1.0);
    let min_y = ys.iter().copied().fold(f32::MAX, f32::min).max(0.0);
    let max_y = ys.iter().copied().fold(f32::MIN, f32::max).ceil().min(view.height as f32 - 1.0);
    if min_x > max_x || min_y > max_y {
        return None;
    }

    // Sub-pixel triangles are kept: a finely tessellated surface is made of nothing else,
    // and each covers the sample points that fall inside it.
    let area = edge(s[0].0, s[0].1, s[1].0, s[1].1, s[2].0, s[2].1);
    if area.abs() < 1e-6 {
        return None; // edge-on
    }

    let centroid = (tri.v0 + tri.v1 + tri.v2) * (1.0 / 3.0);
    let c = camera::shade(n, centroid, view.eye, albedo);
    let to_byte = |f: f32| ((f * 255.0 + 0.5) as i32).clamp(0, 255) as u8;

    Some(ProjectedTriangle {
        s: [[s[0].0, s[0].1, s[0].2], [s[1].0, s[1].1, s[1].2], [s[2].0, s[2].1, s[2].2]],
        colour: [to_byte(c.x), to_byte(c.y), to_byte(c.z), 255],
        min_x: min_x as usize,
        max_x: max_x as usize,
        min_y: min_y as usize,
        max_y: max_y as usize,
        area,
        inv_area: 1.0 / area,
    })
}

fn rasterize_tile(
    projected: &[ProjectedTriangle],
    bin: &[u32],
    x0: usize,
    y0: usize,
    width: usize,
    height: usize,
) -> Vec<[u8; 4]> {
    let mut pixels = vec![[0u8; 4]; TILE_SIZE * TILE_SIZE];
    let mut depth = vec![f32::MAX; TILE_SIZE * TILE_SIZE];
    let x1 = (x0 + TILE_SIZE).min(width) - 1;
    let y1 = (y0 + TILE_SIZE).min(height) - 1;

    for &i in bin {
        let p = &projected[i as usize];
        let (cx0, cx1) = (p.min_x.max(x0), p.max_x.min(x1));
        let (cy0, cy1) = (p.min_y.max(y0), p.max_y.min(y1));
        let [a, b, c] = p.s;
        for y in cy0..=cy1 {
            let py = y as f32 + 0.5;
            for x in cx0..=cx1 {
                let px = x as f32 + 0.5;
                let e0 = edge(b[0], b[1], c[0], c[1], px, py);
                let e1 = edge(c[0], c[1], a[0], a[1], px, py);
                let e2 = edge(a[0], a[1], b[0], b[1], px, py);
                let outside =
                    if p.area > 0.0 { e0 < 0.0 || e1 < 0.0 || e2 < 0.0 } else { e0 > 0.0 || e1 > 0.0 || e2 > 0.0 };
                if outside {
                    continue;
                }
                let b0 = e0 * p.inv_area;
                let b1 = e1 * p.inv_area;
                let z = b0 * a[2] + b1 * b[2] + (1.0 - b0 - b1) * c[2];
                let idx = (y - y0) * TILE_SIZE + (x - x0);
                if z >= depth[idx] {
                    continue;
                }
                depth[idx] = z;
                pixels[idx] = p.colour;
            }
        }
    }

    pixels
}

/// 2x2 box filter; RGB is averaged over opaque samples only so anti-aliased edges are not
/// darkened by the transparent background.
fn downsample(ss_pixels: &[[u8; 4]], ss_w: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let (mut r, mut g, mut b, mut a, mut hit) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for dy in 0..SUPER_SAMPLE {
                for dx in 0..SUPER_SAMPLE {
                    let c = ss_pixels[(y * SUPER_SAMPLE + dy) * ss_w + x * SUPER_SAMPLE + dx];
                    if c[3] > 0 {
                        r += c[0] as u32;
                        g += c[1] as u32;
                        b += c[2] as u32;
                        hit += 1;
                    }
                    a += c[3] as u32;
                }
            }
            let n = (SUPER_SAMPLE * SUPER_SAMPLE) as u32;
            let o = x * 4;
            if let (Some(r), Some(g), Some(b)) = (r.checked_div(hit), g.checked_div(hit), b.checked_div(hit)) {
                row[o] = r as u8;
                row[o + 1] = g as u8;
                row[o + 2] = b as u8;
            }
            row[o + 3] = (a / n) as u8;
        }
    });
    out
}
