//! Self-intersections: pairs of faces that cross. Faces sharing an edge, or lying in one plane,
//! are not counted (welded neighbours, and coplanar overlaps that a slicer fills either way);
//! faces sharing a corner cross when the side of one away from it passes through the other.

use rayon::prelude::*;

use super::mesh::Indexed;
use crate::geometry::support_reader::math::{BoxIndex, P, cross, dot, len, sub};

fn orient(a: P, b: P, c: P, d: P) -> f64 {
    dot(sub(a, d), cross(sub(b, d), sub(c, d)))
}

/// Whether segment `p`-`q` crosses triangle `t` strictly inside.
fn segment_crosses(p: P, q: P, t: &[P; 3]) -> bool {
    let (sp, sq) = (orient(t[0], t[1], t[2], p), orient(t[0], t[1], t[2], q));
    if sp == 0.0 || sq == 0.0 || (sp > 0.0) == (sq > 0.0) {
        return false;
    }
    let s = [orient(p, q, t[0], t[1]), orient(p, q, t[1], t[2]), orient(p, q, t[2], t[0])];
    (s[0] > 0.0 && s[1] > 0.0 && s[2] > 0.0) || (s[0] < 0.0 && s[1] < 0.0 && s[2] < 0.0)
}

/// Where segment `p`-`q` passes through triangle `t`, if it does.
fn segment_point(p: P, q: P, t: &[P; 3]) -> Option<P> {
    if !segment_crosses(p, q, t) {
        return None;
    }
    let (sp, sq) = (orient(t[0], t[1], t[2], p), orient(t[0], t[1], t[2], q));
    let u = sp / (sp - sq);
    Some([0, 1, 2].map(|k| p[k] + (q[k] - p[k]) * u))
}

/// The points where the sides of each triangle pass through the other: the ends of the segment
/// along which they cross.
pub fn crossing_points(a: &[P; 3], b: &[P; 3]) -> Vec<P> {
    (0..3)
        .filter_map(|k| segment_point(a[k], a[(k + 1) % 3], b))
        .chain((0..3).filter_map(|k| segment_point(b[k], b[(k + 1) % 3], a)))
        .collect()
}

/// Whether two triangles cross (an edge of one through the other).
pub fn crosses(a: &[P; 3], b: &[P; 3]) -> bool {
    (0..3).any(|k| segment_crosses(a[k], a[(k + 1) % 3], b) || segment_crosses(b[k], b[(k + 1) % 3], a))
}

/// Whether two faces sharing one corner cross: the side of either away from the shared corner
/// passes through the other.
pub fn crosses_at_corner(fi: &[u32; 3], ti: &[P; 3], fj: &[u32; 3], tj: &[P; 3]) -> bool {
    let opposite = |f: &[u32; 3], g: &[u32; 3], t: &[P; 3]| {
        let k = (0..3).find(|&k| g.contains(&f[k])).expect("a shared corner");
        (t[(k + 1) % 3], t[(k + 2) % 3])
    };
    let (a, b) = opposite(fi, fj, ti);
    let (c, d) = opposite(fj, fi, tj);
    segment_crosses(a, b, tj) || segment_crosses(c, d, ti)
}

/// The crossing face pairs, each once (lower face first), ascending.
pub fn pairs(m: &Indexed) -> Vec<(u32, u32)> {
    let tris = m.tris();
    if tris.is_empty() {
        return Vec::new();
    }
    let mut edge: Vec<f64> = tris.par_iter().map(|t| len(sub(t[1], t[0]))).collect();
    let k = edge.len() / 2;
    edge.select_nth_unstable_by(k, f64::total_cmp);
    let cell = (edge[k] * 2.0).max(1e-3);
    let index = BoxIndex::new(&tris, cell);
    let mut out: Vec<(u32, u32)> = (0..tris.len())
        .into_par_iter()
        .fold(
            || (Vec::new(), Vec::new()),
            |(mut acc, mut near): (Vec<(u32, u32)>, Vec<u32>), i| {
                let (lo, hi) = index.bounds[i];
                index.query_into(lo, hi, &mut near);
                let fi = m.faces[i];
                for &j in near.iter().filter(|&&j| j as usize > i) {
                    let fj = m.faces[j as usize];
                    let shared = fi.iter().filter(|v| fj.contains(v)).count();
                    let hit = match shared {
                        0 => crosses(&tris[i], &tris[j as usize]),
                        // Sharing a corner: they cross when the side facing it in one passes
                        // through the other.
                        1 => crosses_at_corner(&fi, &tris[i], &fj, &tris[j as usize]),
                        _ => false,
                    };
                    if hit {
                        acc.push((i as u32, j));
                    }
                }
                (acc, near)
            },
        )
        .map(|(acc, _)| acc)
        .reduce(Vec::new, |mut a, b| {
            a.extend(b);
            a
        });
    out.par_sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    use super::super::mesh::tests::cube;
    use super::*;

    #[test]
    fn the_crossing_points_are_the_ends_of_the_crossing_segment() {
        let a = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]];
        let b = [[1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [3.0, 3.0, 0.5]];
        let pts = crossing_points(&a, &b);
        assert!(!pts.is_empty() && pts.iter().all(|p| p[2].abs() < 1e-12), "{pts:?}");
    }

    #[test]
    fn two_overlapping_cubes_cross_and_two_apart_do_not() {
        let mut tris = cube([0.0; 3], [1.0; 3]);
        tris.extend(cube([0.5, 0.3, 0.2], [1.5, 1.3, 1.2]));
        assert!(!pairs(&Indexed::weld(&tris)).is_empty());
        let mut tris = cube([0.0; 3], [1.0; 3]);
        tris.extend(cube([2.0; 3], [3.0; 3]));
        assert!(pairs(&Indexed::weld(&tris)).is_empty());
    }
}
