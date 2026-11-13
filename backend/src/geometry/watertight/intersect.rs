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
    segment_inside(p, q, t)
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
    // Each corner's side of the other's plane, worked out once ([`segment_crosses`] does the same
    // sums); only sides that pass through the plane go on to the full test.
    let sa = a.map(|p| orient(b[0], b[1], b[2], p));
    let sb = b.map(|p| orient(a[0], a[1], a[2], p));
    let through = |s: &[f64; 3], k: usize| {
        let (x, y) = (s[k], s[(k + 1) % 3]);
        x != 0.0 && y != 0.0 && (x > 0.0) != (y > 0.0)
    };
    (0..3).any(|k| {
        (through(&sa, k) && segment_inside(a[k], a[(k + 1) % 3], b))
            || (through(&sb, k) && segment_inside(b[k], b[(k + 1) % 3], a))
    })
}

/// The second half of [`segment_crosses`]: whether segment `p`-`q`, which passes through the
/// plane of `t`, passes inside `t`.
fn segment_inside(p: P, q: P, t: &[P; 3]) -> bool {
    let s = [orient(p, q, t[0], t[1]), orient(p, q, t[1], t[2]), orient(p, q, t[2], t[0])];
    (s[0] > 0.0 && s[1] > 0.0 && s[2] > 0.0) || (s[0] < 0.0 && s[1] < 0.0 && s[2] < 0.0)
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

/// Whether faces `i` and `j` cross ([`pairs`]' test).
fn hit(m: &Indexed, tris: &[[P; 3]], i: usize, j: usize) -> bool {
    let (fi, fj) = (m.faces[i], m.faces[j]);
    let shared = fi.iter().filter(|v| fj.contains(v)).count();
    match shared {
        0 => crosses(&tris[i], &tris[j]),
        // Sharing a corner: they cross when the side facing it in one passes through the other.
        1 => crosses_at_corner(&fi, &tris[i], &fj, &tris[j]),
        _ => false,
    }
}

/// The index cell for a mesh's crossing search: twice its median edge.
fn cell_of(tris: &[[P; 3]]) -> f64 {
    let mut edge: Vec<f64> = tris.par_iter().map(|t| len(sub(t[1], t[0]))).collect();
    let k = edge.len() / 2;
    edge.select_nth_unstable_by(k, f64::total_cmp);
    (edge[k] * 2.0).max(1e-3)
}

/// Faces spanning more cells than this are tried against every face instead of by cell.
const MAX_CELLS: i64 = 64;

/// The crossing face pairs, each once (lower face first), ascending: every pair of faces whose
/// boxes meet is tried. The faces go in a uniform grid; each pair is tried in the first cell both
/// cover (the one at the larger of their low corners), so only once.
pub fn pairs(m: &Indexed) -> Vec<(u32, u32)> {
    let tris = m.tris();
    if tris.is_empty() {
        return Vec::new();
    }
    let cell = cell_of(&tris);
    let key = |p: P| p.map(|x| (x / cell).floor().clamp(-1e9, 1e9) as i32);
    let bounds: Vec<(P, P)> = tris
        .par_iter()
        .map(|t| {
            ([0, 1, 2].map(|k| t[0][k].min(t[1][k]).min(t[2][k])), [0, 1, 2].map(|k| t[0][k].max(t[1][k]).max(t[2][k])))
        })
        .collect();
    let keys: Vec<([i32; 3], [i32; 3])> = bounds.par_iter().map(|&(lo, hi)| (key(lo), key(hi))).collect();
    let spans = |i: usize| -> i64 {
        let (a, b) = keys[i];
        (0..3).map(|k| i64::from(b[k]) - i64::from(a[k]) + 1).product()
    };
    let is_big = |i: usize| {
        let n = spans(i);
        n > MAX_CELLS || n <= 0
    };
    let meets = |i: usize, j: usize| {
        let ((a, b), (c, d)) = (bounds[i], bounds[j]);
        (0..3).all(|k| b[k] >= c[k] && a[k] <= d[k])
    };
    let mut entries: Vec<([i32; 3], u32)> = (0..tris.len())
        .into_par_iter()
        .filter(|&i| !is_big(i))
        .flat_map_iter(|i| {
            let (a, b) = keys[i];
            (a[0]..=b[0])
                .flat_map(move |x| (a[1]..=b[1]).flat_map(move |y| (a[2]..=b[2]).map(move |z| ([x, y, z], i as u32))))
        })
        .collect();
    entries.par_sort_unstable();
    let starts: Vec<usize> =
        (0..entries.len()).into_par_iter().filter(|&i| i == 0 || entries[i].0 != entries[i - 1].0).collect();
    let mut out: Vec<(u32, u32)> = (0..starts.len())
        .into_par_iter()
        .fold(Vec::new, |mut acc, r| {
            let (s, e) = (starts[r], starts.get(r + 1).copied().unwrap_or(entries.len()));
            let c = entries[s].0;
            let run = &entries[s..e];
            for (x, &(_, i)) in run.iter().enumerate() {
                let (i, ki) = (i as usize, keys[i as usize].0);
                for &(_, j) in &run[x + 1..] {
                    let j = j as usize;
                    let kj = keys[j].0;
                    if (0..3).any(|k| ki[k].max(kj[k]) != c[k]) || !meets(i, j) {
                        continue;
                    }
                    if hit(m, &tris, i, j) {
                        acc.push((i as u32, j as u32));
                    }
                }
            }
            acc
        })
        .reduce(Vec::new, |mut a, b| {
            a.extend(b);
            a
        });
    // Faces too big for the grid, in a coarser index of their own that every face looks up.
    let big: Vec<u32> = (0..tris.len() as u32).into_par_iter().filter(|&i| is_big(i as usize)).collect();
    if !big.is_empty() {
        let big_tris: Vec<[P; 3]> = big.iter().map(|&i| tris[i as usize]).collect();
        let coarse = BoxIndex::new(&big_tris, cell * 16.0);
        let more: Vec<(u32, u32)> = (0..tris.len())
            .into_par_iter()
            .fold(
                || (Vec::new(), Vec::new()),
                |(mut acc, mut near): (Vec<(u32, u32)>, Vec<u32>), j| {
                    let (lo, hi) = bounds[j];
                    coarse.query_into(lo, hi, &mut near);
                    for &n in &near {
                        let i = big[n as usize] as usize;
                        if i == j || (is_big(j) && i > j) {
                            continue;
                        }
                        let (a, b) = (i.min(j), i.max(j));
                        if hit(m, &tris, a, b) {
                            acc.push((a as u32, b as u32));
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
        out.extend(more);
    }
    out.par_sort_unstable();
    out
}

/// The crossing face pairs whose faces lie in different groups (`group` of each face), as
/// [`pairs`] finds them, without looking at pairs inside group `skip` (a large shell whose own
/// crossings do not matter).
pub fn pairs_across(m: &Indexed, group: &[usize], skip: usize) -> Vec<(u32, u32)> {
    let tris = m.tris();
    if tris.is_empty() {
        return Vec::new();
    }
    let cell = cell_of(&tris);
    // Only the faces outside `skip` are indexed; every face looks them up.
    let others: Vec<u32> = (0..tris.len() as u32).filter(|&f| group[f as usize] != skip).collect();
    if others.is_empty() {
        return Vec::new();
    }
    let sub_tris: Vec<[P; 3]> = others.iter().map(|&f| tris[f as usize]).collect();
    let index = BoxIndex::new(&sub_tris, cell);
    let (mut all_lo, mut all_hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for (lo, hi) in &index.bounds {
        for k in 0..3 {
            all_lo[k] = all_lo[k].min(lo[k]);
            all_hi[k] = all_hi[k].max(hi[k]);
        }
    }
    let mut out: Vec<(u32, u32)> = (0..tris.len())
        .into_par_iter()
        .fold(
            || (Vec::new(), Vec::new()),
            |(mut acc, mut near): (Vec<(u32, u32)>, Vec<u32>), i| {
                let t = &tris[i];
                let lo = [0, 1, 2].map(|k| t[0][k].min(t[1][k]).min(t[2][k]));
                let hi = [0, 1, 2].map(|k| t[0][k].max(t[1][k]).max(t[2][k]));
                if (0..3).any(|k| hi[k] < all_lo[k] || lo[k] > all_hi[k]) {
                    return (acc, near);
                }
                index.query_into(lo, hi, &mut near);
                for &n in &near {
                    let j = others[n as usize] as usize;
                    if group[j] == group[i] || (group[i] != skip && j <= i) {
                        continue;
                    }
                    let (a, b) = (i.min(j), i.max(j));
                    if hit(m, &tris, a, b) {
                        acc.push((a as u32, b as u32));
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
    out.dedup();
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

    /// Every pair of faces tried, the plain way.
    fn brute(m: &Indexed) -> Vec<(u32, u32)> {
        let tris = m.tris();
        let mut out = Vec::new();
        for i in 0..tris.len() {
            for j in i + 1..tris.len() {
                let (fi, fj) = (m.faces[i], m.faces[j]);
                let hit = match fi.iter().filter(|v| fj.contains(v)).count() {
                    0 => (0..3).any(|k| {
                        segment_crosses(tris[i][k], tris[i][(k + 1) % 3], &tris[j])
                            || segment_crosses(tris[j][k], tris[j][(k + 1) % 3], &tris[i])
                    }),
                    1 => crosses_at_corner(&fi, &tris[i], &fj, &tris[j]),
                    _ => false,
                };
                if hit {
                    out.push((i as u32, j as u32));
                }
            }
        }
        out
    }

    #[test]
    fn the_grid_search_finds_the_pairs_every_pair_tried_finds() {
        for seed in [3, 11, 29] {
            let m = Indexed::weld(&super::super::mesh::tests::soup(1500, seed));
            let all = brute(&m);
            assert!(!all.is_empty());
            assert_eq!(pairs(&m), all, "seed {seed}");
            // Across groups, the largest group's own pairs left out.
            let group: Vec<usize> = (0..m.faces.len()).map(|f| f % 3).collect();
            let want: Vec<(u32, u32)> =
                all.iter().copied().filter(|&(a, b)| group[a as usize] != group[b as usize]).collect();
            assert_eq!(pairs_across(&m, &group, 0), want, "seed {seed}");
        }
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
