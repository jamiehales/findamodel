//! Measures for comparing a repaired mesh with its original: surface deviation both ways (the
//! original's exterior faces only, since internal faces are meant to go) and a voxel census of
//! material, sealed voids (resin traps) and drained cavities.

use rayon::prelude::*;

use super::mesh::Indexed;
use crate::geometry::support_reader::contacts::Wind;
use crate::geometry::support_reader::math::{BoxIndex, P, add, closest_on_tri, cross, dist, len, mul, sub};

/// Area-weighted sample points on faces, with their unit normals, deterministic.
pub fn samples(tris: &[[P; 3]], n: usize) -> Vec<(P, P)> {
    let areas: Vec<f64> = tris.iter().map(|t| 0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0])))).collect();
    let total: f64 = areas.iter().sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let step = total / n as f64;
    let mut out = Vec::with_capacity(n + tris.len().min(n));
    let mut acc = 0.0;
    let mut next = step * 0.5;
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    for (t, a) in tris.iter().zip(&areas) {
        acc += a;
        while next < acc {
            let (mut u, mut v) = (rnd(), rnd());
            if u + v > 1.0 {
                u = 1.0 - u;
                v = 1.0 - v;
            }
            let p = add(t[0], add(mul(sub(t[1], t[0]), u), mul(sub(t[2], t[0]), v)));
            let nn = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            out.push((p, mul(nn, 1.0 / len(nn).max(1e-30))));
            next += step;
        }
    }
    out
}

/// Distances from points to the nearest face of `tris`.
pub fn distances(points: &[P], tris: &[[P; 3]]) -> Vec<f64> {
    let mut edges: Vec<f64> = tris.iter().step_by((tris.len() / 4096).max(1)).map(|t| dist(t[0], t[1])).collect();
    let mid = edges.len() / 2;
    let typical = if edges.is_empty() { 1.0 } else { *edges.select_nth_unstable_by(mid, f64::total_cmp).1 };
    let cell = (2.0 * typical).max(0.01);
    let index = BoxIndex::new(tris, cell);
    points
        .par_iter()
        .map_init(Vec::new, |near, &p| {
            for r in [cell * 0.5, cell * 2.0, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 64.0] {
                index.query_into(sub(p, [r; 3]), add(p, [r; 3]), near);
                let best = near.iter().map(|&i| dist(p, closest_on_tri(p, &tris[i as usize]))).fold(f64::MAX, f64::min);
                if best <= r {
                    return best;
                }
            }
            64.0
        })
        .collect()
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Deviation {
    /// From the result's surface to the original.
    pub out_max: f64,
    pub out_p95: f64,
    /// From the original's exterior surface (faces with inside on one side and outside on the
    /// other) to the result.
    pub orig_max: f64,
    pub orig_p95: f64,
    pub orig_p99: f64,
    /// Share of the original's surface that is exterior.
    pub orig_exterior_share: f64,
    /// Where the largest distances are (result side, original side), for a look.
    pub out_worst: Vec<P>,
    pub orig_worst: Vec<P>,
}

/// The points of the `n` largest distances.
fn worst(points: &[P], d: &[f64], n: usize) -> Vec<P> {
    let mut order: Vec<usize> = (0..d.len()).collect();
    order.sort_by(|&a, &b| d[b].total_cmp(&d[a]));
    order.into_iter().take(n).map(|i| points[i]).collect()
}

fn pct(mut v: Vec<f64>, q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let k = ((v.len() - 1) as f64 * q).round() as usize;
    v.select_nth_unstable_by(k, f64::total_cmp);
    v[k]
}

pub fn deviation(orig: &[[P; 3]], orig_wind: &Wind, out: &Indexed, n: usize) -> Deviation {
    let out_tris = out.tris();
    let so: Vec<P> = samples(&out_tris, n).into_iter().map(|s| s.0).collect();
    let d_out = distances(&so, orig);
    let eps = 0.01;
    let all = samples(orig, n);
    let ext: Vec<P> = all
        .par_iter()
        .filter(|(p, nn)| {
            let a = orig_wind.w(add(*p, mul(*nn, eps))) > 0.5;
            let b = orig_wind.w(sub(*p, mul(*nn, eps))) > 0.5;
            a != b
        })
        .map(|s| s.0)
        .collect();
    let share = ext.len() as f64 / all.len().max(1) as f64;
    let d_orig = distances(&ext, &out_tris);
    Deviation {
        out_worst: worst(&so, &d_out, 5),
        orig_worst: worst(&ext, &d_orig, 5),
        out_max: d_out.iter().copied().fold(0.0, f64::max),
        out_p95: pct(d_out, 0.95),
        orig_max: d_orig.iter().copied().fold(0.0, f64::max),
        orig_p95: pct(d_orig.clone(), 0.95),
        orig_p99: pct(d_orig, 0.99),
        orig_exterior_share: share,
    }
}

/// Voxel classes.
pub const EXTERIOR: u8 = 0;
pub const MATERIAL: u8 = 1;
pub const SEALED: u8 = 2;
pub const CAVITY: u8 = 3;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Census {
    pub voxel_mm: f64,
    pub material_mm3: f64,
    /// Void with no way out: a resin trap.
    pub sealed_mm3: f64,
    /// Void open to the outside only through openings narrower than the probe: drained cavities
    /// (and deep narrow crevices, the same before and after).
    pub cavity_mm3: f64,
    pub probe_mm: f64,
    #[serde(skip)]
    pub dims: [usize; 3],
    #[serde(skip)]
    pub origin: P,
    #[serde(skip)]
    pub class: Vec<u8>,
}

impl Census {
    pub fn at(&self, i: [usize; 3]) -> u8 {
        self.class[(i[2] * self.dims[1] + i[1]) * self.dims[0] + i[0]]
    }
}

/// Whether each voxel centre is inside `tris`: the winding number up each voxel column from its
/// crossings (faces counted up or down by their normal), averaged with the count down from the
/// top so an open mesh leaks only near its holes.
pub fn solid_voxels(tris: &[[P; 3]], o: P, dims: [usize; 3], h: f64) -> Vec<bool> {
    // Off the voxel centres by an odd fraction, so columns miss the vertices of a mesh built on a
    // grid of the same pitch.
    let jitter = [h * 0.0173, h * 0.0291];
    let col = |x: usize, y: usize| [o[0] + (x as f64 + 0.5) * h + jitter[0], o[1] + (y as f64 + 0.5) * h + jitter[1]];
    let mut events: Vec<(u32, f64, i8)> = tris
        .par_iter()
        .flat_map_iter(|t| {
            let mut out = Vec::new();
            let nz = (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0]);
            if nz == 0.0 {
                return out;
            }
            let (a, b, c) = if nz > 0.0 { (t[0], t[1], t[2]) } else { (t[0], t[2], t[1]) };
            let lo = [0, 1].map(|k| a[k].min(b[k]).min(c[k]));
            let hi = [0, 1].map(|k| a[k].max(b[k]).max(c[k]));
            let first = |k: usize| ((lo[k] - o[k] - jitter[k]) / h - 0.5).ceil().max(0.0) as usize;
            let last = |k: usize| ((hi[k] - o[k] - jitter[k]) / h - 0.5).floor();
            let edge = |p: P, q: P, x: f64, y: f64| (q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0]);
            let owns = |p: P, q: P| (q[1] < p[1]) || (q[1] == p[1] && q[0] < p[0]);
            let (lx, ly) = (last(0), last(1));
            if lx < 0.0 || ly < 0.0 {
                return out;
            }
            for y in first(1)..=(ly as usize).min(dims[1] - 1) {
                for x in first(0)..=(lx as usize).min(dims[0] - 1) {
                    let q = col(x, y);
                    let w = [edge(b, c, q[0], q[1]), edge(c, a, q[0], q[1]), edge(a, b, q[0], q[1])];
                    let es = [(b, c), (c, a), (a, b)];
                    if !(0..3).all(|k| w[k] > 0.0 || (w[k] == 0.0 && owns(es[k].0, es[k].1))) {
                        continue;
                    }
                    let z = (w[0] * a[2] + w[1] * b[2] + w[2] * c[2]) / (w[0] + w[1] + w[2]);
                    out.push(((y * dims[0] + x) as u32, z, if nz > 0.0 { 1 } else { -1 }));
                }
            }
            out
        })
        .collect();
    events.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut solid = vec![false; dims[0] * dims[1] * dims[2]];
    let plane = dims[0] * dims[1];
    let mut i = 0;
    while i < events.len() {
        let c = events[i].0 as usize;
        let mut j = i;
        while j < events.len() && events[j].0 as usize == c {
            j += 1;
        }
        let run = &events[i..j];
        let total: i32 = run.iter().map(|e| i32::from(e.2)).sum();
        // Upward faces are entered from below with an outward normal pointing down: a face with
        // its normal down (sign -1) raises the winding when crossed going up.
        let mut k = 0;
        let mut below = 0i32;
        for z in 0..dims[2] {
            let zc = o[2] + (z as f64 + 0.5) * h;
            while k < run.len() && run[k].1 < zc {
                below -= i32::from(run[k].2);
                k += 1;
            }
            let above = below + total;
            if f64::from(below + above) * 0.5 > 0.5 {
                solid[z * plane + c] = true;
            }
        }
        i = j;
    }
    solid
}

/// A census of the solid `tris` make, on voxels `h` mm wide over the box `lo`-`hi`, cavities
/// probed with a cube `probe` mm across.
pub fn census(tris: &[[P; 3]], lo: P, hi: P, h: f64, probe: f64) -> Census {
    let o = sub(lo, [2.0 * h; 3]);
    let dims = [0, 1, 2].map(|k| ((hi[k] - lo[k]) / h).ceil() as usize + 4);
    let n = dims[0] * dims[1] * dims[2];
    let idx = |x: usize, y: usize, z: usize| (z * dims[1] + y) * dims[0] + x;
    let solid = solid_voxels(tris, o, dims, h);
    let flood = |open: &dyn Fn(usize) -> bool| -> Vec<bool> {
        let mut seen = vec![false; n];
        let mut stack: Vec<usize> = Vec::new();
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let border = x == 0 || y == 0 || z == 0 || x + 1 == dims[0] || y + 1 == dims[1] || z + 1 == dims[2];
                    if border && open(idx(x, y, z)) {
                        seen[idx(x, y, z)] = true;
                        stack.push(idx(x, y, z));
                    }
                }
            }
        }
        while let Some(i) = stack.pop() {
            let (x, y, z) = (i % dims[0], (i / dims[0]) % dims[1], i / (dims[0] * dims[1]));
            let mut go = |j: usize| {
                if !seen[j] && open(j) {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                go(i - 1);
            }
            if x + 1 < dims[0] {
                go(i + 1);
            }
            if y > 0 {
                go(i - dims[0]);
            }
            if y + 1 < dims[1] {
                go(i + dims[0]);
            }
            if z > 0 {
                go(i - dims[0] * dims[1]);
            }
            if z + 1 < dims[2] {
                go(i + dims[0] * dims[1]);
            }
        }
        seen
    };
    let dilate = |src: &[bool], k: usize| -> Vec<bool> {
        let mut a = src.to_vec();
        for axis in 0..3 {
            let stride = [1, dims[0], dims[0] * dims[1]][axis];
            let len_axis = dims[axis];
            let b: Vec<bool> = (0..n)
                .into_par_iter()
                .map(|i| {
                    let c = (i / stride) % len_axis;
                    let from = c.saturating_sub(k);
                    let to = (c + k).min(len_axis - 1);
                    (from..=to).any(|j| a[i - c * stride + j * stride])
                })
                .collect();
            a = b;
        }
        a
    };
    let exterior = flood(&|i| !solid[i]);
    let k = ((probe / h) / 2.0).round().max(1.0) as usize;
    let thick = dilate(&solid, k);
    let wide = flood(&|i| !thick[i]);
    let reached = dilate(&wide, k);
    let class: Vec<u8> = (0..n)
        .map(|i| {
            if solid[i] {
                MATERIAL
            } else if !exterior[i] {
                SEALED
            } else if !reached[i] {
                CAVITY
            } else {
                EXTERIOR
            }
        })
        .collect();
    let v = h * h * h;
    let count = |c: u8| class.iter().filter(|&&x| x == c).count() as f64 * v;
    Census {
        voxel_mm: h,
        material_mm3: count(MATERIAL),
        sealed_mm3: count(SEALED),
        cavity_mm3: count(CAVITY),
        probe_mm: probe,
        dims,
        origin: o,
        class,
    }
}

#[cfg(test)]
mod tests {
    use super::super::mesh::tests::cube;
    use super::*;

    #[test]
    fn a_sealed_hollow_box_counts_its_void_as_sealed() {
        let mut t = cube([0.0; 3], [10.0; 3]);
        let mut inner = cube([2.0; 3], [8.0; 3]);
        for f in &mut inner {
            f.swap(1, 2);
        }
        t.extend(inner);
        let c = census(&t, [0.0; 3], [10.0; 3], 0.5, 2.0);
        assert!((c.sealed_mm3 - 216.0).abs() < 1.0, "{c:?}");
        assert!((c.material_mm3 - 784.0).abs() < 1.0, "{c:?}");
    }

    #[test]
    fn a_remeshed_cube_has_the_cube_s_material() {
        let t = cube([0.0; 3], [10.0; 3]);
        let (m, _) = super::super::remesh::remesh(&t, 0.05);
        let c = census(&m.tris(), [0.0; 3], [10.0; 3], 0.2, 2.0);
        assert!((c.material_mm3 - 1000.0).abs() < 20.0, "{}", c.material_mm3);
    }

    #[test]
    fn deviation_of_a_mesh_from_itself_is_zero() {
        let t = cube([0.0; 3], [1.0; 3]);
        let w = Wind::new(t.clone());
        let d = deviation(&t, &w, &Indexed::weld(&t), 2000);
        assert!(d.out_max < 1e-9 && d.orig_max < 1e-9 && d.orig_exterior_share > 0.99, "{d:?}");
    }
}
