//! Double-precision vector helpers shared by the reader's stages. The reader works in f64 on
//! `[f64; 3]` points: its fits and ray tests compare distances of a micron and less, which f32
//! cannot hold across a 200 mm plate.

use crate::geometry::Triangle;

pub type P = [f64; 3];

pub fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn mul(a: P, s: f64) -> P {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: P, b: P) -> P {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn len(a: P) -> f64 {
    dot(a, a).sqrt()
}
pub fn dist(a: P, b: P) -> f64 {
    len(sub(a, b))
}
/// The unit vector along `a` (a zero vector stays zero).
pub fn unit(a: P) -> P {
    mul(a, 1.0 / len(a).max(1e-30))
}
/// `a / s`, divided as numpy does (not multiplied by the reciprocal), so the classifier's
/// directions round as the prototype's did.
pub fn div(a: P, s: f64) -> P {
    [a[0] / s, a[1] / s, a[2] / s]
}
/// The unit vector along `a` by division (unchanged when shorter than 1e-12), as the prototype's
/// classifier takes it.
pub fn unit_div(a: P) -> P {
    let n = len(a);
    if n > 1e-12 { div(a, n) } else { a }
}
pub fn lerp(a: P, b: P, t: f64) -> P {
    add(a, mul(sub(b, a), t))
}
pub fn centroid(t: &[P; 3]) -> P {
    mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0)
}
/// Distance from `p` to the segment from `a` to `b`.
pub fn seg_dist(p: P, a: P, b: P) -> f64 {
    let d = sub(b, a);
    let t = (dot(sub(p, a), d) / dot(d, d).max(1e-18)).clamp(0.0, 1.0);
    dist(p, lerp(a, b, t))
}

/// The triangles' corners in f64.
pub fn points_of(triangles: &[Triangle]) -> Vec<[P; 3]> {
    let p = |v: crate::geometry::Vec3| [f64::from(v.x), f64::from(v.y), f64::from(v.z)];
    triangles.iter().map(|t| [p(t.v0), p(t.v1), p(t.v2)]).collect()
}

/// A sum in numpy's pairwise order (`np.sum` on a float array), so thresholds on sums land as the
/// prototype's did.
pub fn pairwise_sum(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        a.iter().fold(0.0, |r, &x| r + x)
    } else if n <= 128 {
        let mut r = [0.0; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i < n - (n % 8) {
            for j in 0..8 {
                r[j] += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise_sum(&a[..n2]) + pairwise_sum(&a[n2..])
    }
}

/// `n` evenly spaced values from `a` to `b` inclusive (`np.linspace`).
pub fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    if n == 1 {
        return vec![a];
    }
    let step = (b - a) / (n - 1) as f64;
    let mut v: Vec<f64> = (0..n).map(|i| a + i as f64 * step).collect();
    v[n - 1] = b;
    v
}

/// Union-find root with path halving.
pub fn find(p: &mut [u32], mut x: u32) -> u32 {
    while p[x as usize] != x {
        p[x as usize] = p[p[x as usize] as usize];
        x = p[x as usize];
    }
    x
}

/// Triangles' bounding boxes in a uniform grid: every triangle whose box meets a query box, in
/// index order, as a scan of all the boxes would give them.
pub struct BoxIndex {
    /// Each triangle's (min, max) corner.
    pub bounds: Vec<(P, P)>,
    cell: f64,
    /// Each occupied cell's run in `items`.
    grid: rustc_hash::FxHashMap<[i32; 3], (u32, u32)>,
    items: Vec<u32>,
    /// Triangles spanning more than [`BoxIndex::MAX_CELLS`] cells, checked on every query.
    big: Vec<u32>,
}

impl BoxIndex {
    const MAX_CELLS: i64 = 64;

    /// Indexes `tris` in cells `cell` mm wide.
    pub fn new(tris: &[[P; 3]], cell: f64) -> Self {
        use rayon::prelude::*;
        let bounds: Vec<(P, P)> = tris
            .par_iter()
            .map(|t| {
                (
                    [0, 1, 2].map(|k| t[0][k].min(t[1][k]).min(t[2][k])),
                    [0, 1, 2].map(|k| t[0][k].max(t[1][k]).max(t[2][k])),
                )
            })
            .collect();
        // Each triangle's cells (or the big list), gathered in parallel and grouped by a sort:
        // the lists come out in index order, as a serial build would make them.
        let span = |i: usize| {
            let (lo, hi) = bounds[i];
            let (a, b) = (Self::key(lo, cell), Self::key(hi, cell));
            let cells: i64 = (0..3).map(|k| i64::from(b[k]) - i64::from(a[k]) + 1).product();
            (a, b, cells)
        };
        let big: Vec<u32> = (0..bounds.len())
            .into_par_iter()
            .filter(|&i| {
                let (_, _, cells) = span(i);
                cells > Self::MAX_CELLS || cells <= 0
            })
            .map(|i| i as u32)
            .collect();
        let mut entries: Vec<([i32; 3], u32)> = (0..bounds.len())
            .into_par_iter()
            .flat_map_iter(|i| {
                let (a, b, cells) = span(i);
                let ok = cells <= Self::MAX_CELLS && cells > 0;
                let n = if ok { cells as usize } else { 0 };
                let mut out = Vec::with_capacity(n);
                if ok {
                    for x in a[0]..=b[0] {
                        for y in a[1]..=b[1] {
                            for z in a[2]..=b[2] {
                                out.push(([x, y, z], i as u32));
                            }
                        }
                    }
                }
                out
            })
            .collect();
        entries.par_sort_unstable();
        let starts: Vec<u32> = (0..entries.len() as u32)
            .into_par_iter()
            .filter(|&i| i == 0 || entries[i as usize].0 != entries[i as usize - 1].0)
            .collect();
        let mut grid: rustc_hash::FxHashMap<[i32; 3], (u32, u32)> = rustc_hash::FxHashMap::default();
        grid.reserve(starts.len());
        for (k, &s) in starts.iter().enumerate() {
            let e = starts.get(k + 1).copied().unwrap_or(entries.len() as u32);
            grid.insert(entries[s as usize].0, (s, e));
        }
        let items = entries.par_iter().map(|e| e.1).collect();
        BoxIndex { bounds, cell, grid, items, big }
    }

    fn key(p: P, cell: f64) -> [i32; 3] {
        p.map(|x| (x / cell).floor().clamp(-1e9, 1e9) as i32)
    }

    /// Whether triangle `i`'s box meets the closed box `[lo, hi]`.
    pub fn meets(&self, i: u32, lo: P, hi: P) -> bool {
        let (a, b) = &self.bounds[i as usize];
        (0..3).all(|k| b[k] >= lo[k] && a[k] <= hi[k])
    }

    /// The triangles whose box meets the closed box `[lo, hi]`, ascending, into `out` (cleared).
    pub fn query_into(&self, lo: P, hi: P, out: &mut Vec<u32>) {
        out.clear();
        let (a, b) = (Self::key(lo, self.cell), Self::key(hi, self.cell));
        let cells: i64 = (0..3).map(|k| (i64::from(b[k]) - i64::from(a[k]) + 1).max(0)).product();
        if cells == 0 {
            return;
        }
        if cells as usize >= self.grid.len() || cells as usize * 4 >= self.bounds.len() {
            // A box spanning most of the grid: scanning every box is quicker.
            out.extend((0..self.bounds.len() as u32).filter(|&i| self.meets(i, lo, hi)));
            return;
        }
        for x in a[0]..=b[0] {
            for y in a[1]..=b[1] {
                for z in a[2]..=b[2] {
                    if let Some(&(s, e)) = self.grid.get(&[x, y, z]) {
                        let list = &self.items[s as usize..e as usize];
                        out.extend(list.iter().copied().filter(|&i| self.meets(i, lo, hi)));
                    }
                }
            }
        }
        out.extend(self.big.iter().copied().filter(|&i| self.meets(i, lo, hi)));
        out.sort_unstable();
        out.dedup();
    }

    /// [`BoxIndex::query_into`] into a new list.
    pub fn query(&self, lo: P, hi: P) -> Vec<u32> {
        let mut out = Vec::new();
        self.query_into(lo, hi, &mut out);
        out
    }
}

/// The closest point to `p` on triangle `t` (Ericson, Real-Time Collision Detection 5.1.5).
pub fn closest_on_tri(p: P, t: &[P; 3]) -> P {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add(a, mul(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add(a, mul(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return add(b, mul(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let den = 1.0 / (va + vb + vc).max(1e-30);
    add(a, add(mul(ab, vb * den), mul(ac, vc * den)))
}

/// Whether the ray from `o` along `d` crosses triangle `t` (Moller-Trumbore, beyond the origin).
pub fn ray_hits(o: P, d: P, t: &[P; 3]) -> bool {
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let h = cross(d, e2);
    let a = dot(e1, h);
    if a.abs() < 1e-14 {
        return false;
    }
    let f = 1.0 / a;
    let s = sub(o, t[0]);
    let u = f * dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = cross(s, e1);
    let v = f * dot(d, q);
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    f * dot(e2, q) > 1e-12
}

/// Symmetric 3x3 eigen decomposition (Jacobi): eigenvalues ascending, with their unit vectors.
#[allow(clippy::needless_range_loop)]
pub fn eig3(m: [[f64; 3]; 3]) -> ([f64; 3], [P; 3]) {
    let mut a = m;
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..60 {
        let (mut p, mut q, mut big) = (0, 1, 0.0);
        for i in 0..3 {
            for j in i + 1..3 {
                if a[i][j].abs() > big {
                    big = a[i][j].abs();
                    p = i;
                    q = j;
                }
            }
        }
        if big < 1e-18 {
            break;
        }
        let th = 0.5 * (2.0 * a[p][q]).atan2(a[q][q] - a[p][p]);
        let (c, s) = (th.cos(), th.sin());
        for row in a.iter_mut() {
            let (x, y) = (row[p], row[q]);
            row[p] = c * x - s * y;
            row[q] = s * x + c * y;
        }
        for k in 0..3 {
            let (x, y) = (a[p][k], a[q][k]);
            a[p][k] = c * x - s * y;
            a[q][k] = s * x + c * y;
        }
        for row in v.iter_mut() {
            let (x, y) = (row[p], row[q]);
            row[p] = c * x - s * y;
            row[q] = s * x + c * y;
        }
    }
    let mut idx = [0, 1, 2];
    idx.sort_by(|&i, &j| a[i][i].total_cmp(&a[j][j]));
    (idx.map(|i| a[i][i]), idx.map(|i| [v[0][i], v[1][i], v[2][i]]))
}

/// Solves `m x = b` for a 3x3 `m` given by its rows; None when singular.
pub fn solve3(m: [[f64; 3]; 3], b: P) -> Option<P> {
    let det = dot(m[0], cross(m[1], m[2]));
    if det.abs() < 1e-18 {
        return None;
    }
    let col = |k: usize| [m[0][k], m[1][k], m[2][k]];
    let (c0, c1, c2) = (col(0), col(1), col(2));
    Some([dot(b, cross(c1, c2)) / det, dot(c0, cross(b, c2)) / det, dot(c0, cross(c1, b)) / det])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eigen_decomposition_sorts_values_and_finds_axes() {
        let (values, vectors) = eig3([[3.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]]);
        assert_eq!(values, [1.0, 2.0, 3.0]);
        assert!((vectors[0][1].abs() - 1.0).abs() < 1e-12);
        assert!((vectors[2][0].abs() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn solve3_inverts_and_reports_singular_systems() {
        let x = solve3([[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [1.0, 0.0, 1.0]], [2.0, 8.0, 4.0]).unwrap();
        assert!(dist(x, [1.0, 2.0, 3.0]) < 1e-12);
        assert!(solve3([[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 0.0, 1.0]], [0.0; 3]).is_none());
    }

    #[test]
    fn closest_point_covers_corner_edge_and_face_regions() {
        let t = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        assert_eq!(closest_on_tri([-1.0, -1.0, 0.0], &t), [0.0, 0.0, 0.0]);
        assert!(dist(closest_on_tri([0.5, -1.0, 0.0], &t), [0.5, 0.0, 0.0]) < 1e-12);
        assert!(dist(closest_on_tri([0.2, 0.2, 5.0], &t), [0.2, 0.2, 0.0]) < 1e-12);
    }

    #[test]
    fn rays_hit_only_ahead_of_their_origin() {
        let t = [[0.0, 0.0, 1.0], [2.0, 0.0, 1.0], [0.0, 2.0, 1.0]];
        assert!(ray_hits([0.3, 0.3, 0.0], [0.0, 0.0, 1.0], &t));
        assert!(!ray_hits([0.3, 0.3, 2.0], [0.0, 0.0, 1.0], &t));
        assert!(!ray_hits([3.0, 3.0, 0.0], [0.0, 0.0, 1.0], &t));
    }
}
