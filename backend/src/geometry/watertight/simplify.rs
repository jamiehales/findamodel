//! Simplification after the remesh: quadric error edge collapses, cheapest first, in blocks on
//! their own threads. A collapse must keep the surface manifold (the link condition), must not
//! turn a face over, must not make the worst face round it a worse sliver, must not cross a face
//! two rings out, and must keep the area-weighted mean squared distance to the planes it has
//! absorbed within the bound.

use rayon::prelude::*;

use super::mesh::Indexed;
use crate::geometry::support_reader::math::{P, add, cross, dot, len, mul, solve3, sub};

/// A plane quadric: the symmetric 4x4 matrix as its ten distinct entries, then the summed weight.
#[derive(Clone, Copy, Default)]
struct Q([f64; 11]);

impl Q {
    /// A face's plane, weighted by its area (the last entry sums the weights).
    fn plane(n: P, d: f64, w: f64) -> Q {
        Q::unit(n, d).scale(w)
    }
    fn scale(self, w: f64) -> Q {
        Q(self.0.map(|x| x * w))
    }
    fn unit(n: P, d: f64) -> Q {
        let [a, b, c] = n;
        Q([a * a, a * b, a * c, a * d, b * b, b * c, b * d, c * c, c * d, d * d, 1.0])
    }
    fn add(self, o: Q) -> Q {
        let mut r = self.0;
        for (x, y) in r.iter_mut().zip(o.0) {
            *x += y;
        }
        Q(r)
    }
    /// The area-weighted mean squared distance from `p` to the planes.
    fn eval(&self, p: P) -> f64 {
        self.sum(p) / self.0[10].max(1e-30)
    }
    fn sum(&self, p: P) -> f64 {
        let q = &self.0;
        let [x, y, z] = p;
        q[0] * x * x
            + 2.0 * q[1] * x * y
            + 2.0 * q[2] * x * z
            + 2.0 * q[3] * x
            + q[4] * y * y
            + 2.0 * q[5] * y * z
            + 2.0 * q[6] * y
            + q[7] * z * z
            + 2.0 * q[8] * z
            + q[9]
    }
    fn optimum(&self) -> Option<P> {
        let q = &self.0;
        solve3([[q[0], q[1], q[2]], [q[1], q[4], q[5]], [q[2], q[5], q[7]]], [-q[3], -q[6], -q[8]])
    }
}

/// A face whose doubled area over its summed squared sides is below this is a sliver (an
/// equilateral face scores 0.29).
const SLIVER: f64 = 0.05;
/// A collapse may turn a face round it by up to a right angle.
const FLIP: f64 = 0.0;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SimplifyStats {
    pub rounds: usize,
    pub before: usize,
    pub after: usize,
}

fn normal(a: P, b: P, c: P) -> P {
    cross(sub(b, a), sub(c, a))
}

fn quality(t: &[P; 3], doubled_area: f64) -> f64 {
    doubled_area / (0..3).map(|k| dot(sub(t[k], t[(k + 1) % 3]), sub(t[k], t[(k + 1) % 3]))).sum::<f64>().max(1e-30)
}

/// Collapses edges while each stays within `tol` mm of the planes it absorbs. The mesh is cut
/// into blocks, each simplified on its own thread (only vertices whose faces all lie in the
/// block move), and the blocks are shifted between passes so their borders get done too.
pub fn simplify(m: &Indexed, tol: f64) -> (Indexed, SimplifyStats) {
    let mut verts = m.verts.clone();
    let mut faces = m.faces.clone();
    let mut st = SimplifyStats { before: faces.len(), ..Default::default() };
    let mut quad = vec![Q::default(); verts.len()];
    for f in &faces {
        let [a, b, c] = f.map(|v| verts[v as usize]);
        let n = normal(a, b, c);
        let l = len(n);
        if l < 1e-30 {
            continue;
        }
        let n = mul(n, 1.0 / l);
        let q = Q::plane(n, -dot(n, a), l * 0.5);
        for &v in f {
            quad[v as usize] = quad[v as usize].add(q);
        }
    }
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for v in &verts {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let span = (0..3).map(|k| hi[k] - lo[k]).fold(1e-9, f64::max);
    let threads = rayon::current_num_threads().max(1);
    // Blocks of about the same share of the surface: a surface's faces fill about k^2 of k^3 cells.
    let k = ((threads * 16) as f64).sqrt().ceil().clamp(2.0, 64.0);
    // At least a few dozen faces across, so most vertices are free to move.
    let step = (faces.len() / 1000).max(1);
    let edge = faces.iter().step_by(step).map(|f| len(sub(verts[f[0] as usize], verts[f[1] as usize]))).sum::<f64>()
        / faces.iter().step_by(step).count().max(1) as f64;
    let cell = (span / k).max(40.0 * edge);
    let tol2 = tol * tol;
    let shifts = [0.0, 0.5, 0.25, 0.75];
    for pass in 0..16 {
        let shift = shifts[pass % 4];
        let before_pass = faces.len();
        st.rounds += 1;
        let block_of = |p: P| -> u64 {
            let c = [0, 1, 2].map(|q| (((p[q] - lo[q]) / cell + shift).floor() as i64 + 1) as u64);
            c[0] | (c[1] << 20) | (c[2] << 40)
        };
        let vb: Vec<u64> = verts.par_iter().map(|&p| block_of(p)).collect();
        let mut degree = vec![0u32; verts.len()];
        for f in &faces {
            for &v in f {
                degree[v as usize] += 1;
            }
        }
        // Faces with all corners in one block, grouped by block; the rest stay as they are.
        let mut keyed: Vec<(u64, u32)> = faces
            .par_iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let b = vb[f[0] as usize];
                (vb[f[1] as usize] == b && vb[f[2] as usize] == b).then_some((b, i as u32))
            })
            .collect();
        keyed.par_sort_unstable();
        let mut runs: Vec<&[(u64, u32)]> = Vec::new();
        let mut start = 0;
        for i in 1..=keyed.len() {
            if i == keyed.len() || keyed[i].0 != keyed[start].0 {
                runs.push(&keyed[start..i]);
                start = i;
            }
        }
        let results: Vec<Block> =
            runs.par_iter().map(|run| collapse_block(run, &faces, &verts, &quad, &degree, tol2)).collect();
        let mut in_block = vec![false; faces.len()];
        for &(_, f) in &keyed {
            in_block[f as usize] = true;
        }
        let mut next: Vec<[u32; 3]> = faces.iter().zip(&in_block).filter(|(_, b)| !**b).map(|(f, _)| *f).collect();
        for r in results {
            for (v, p, q) in r.moved {
                verts[v as usize] = p;
                quad[v as usize] = q;
            }
            next.extend(r.faces);
        }
        faces = next;
        if pass >= 3 && faces.len() * 50 > before_pass * 49 {
            break;
        }
    }
    // Drop unused vertices.
    let mut used = vec![u32::MAX; verts.len()];
    let mut out_verts = Vec::new();
    for f in &mut faces {
        for v in f.iter_mut() {
            if used[*v as usize] == u32::MAX {
                used[*v as usize] = out_verts.len() as u32;
                out_verts.push(verts[*v as usize]);
            }
            *v = used[*v as usize];
        }
    }
    st.after = faces.len();
    (Indexed { verts: out_verts, faces }, st)
}

/// Simplifies a whole mesh on one thread, with the `locked` vertices kept where they are (they
/// are shared with a neighbouring piece).
pub(crate) fn simplify_locked(m: &Indexed, locked: &[bool], tol: f64) -> Indexed {
    let mut quad = vec![Q::default(); m.verts.len()];
    let mut degree = vec![0u32; m.verts.len()];
    for f in &m.faces {
        let [a, b, c] = f.map(|v| m.verts[v as usize]);
        let n = normal(a, b, c);
        let l = len(n);
        for &v in f {
            degree[v as usize] += 1;
        }
        if l < 1e-30 {
            continue;
        }
        let n = mul(n, 1.0 / l);
        let q = Q::plane(n, -dot(n, a), l * 0.5);
        for &v in f {
            quad[v as usize] = quad[v as usize].add(q);
        }
    }
    for (d, &l) in degree.iter_mut().zip(locked) {
        if l {
            *d = u32::MAX;
        }
    }
    let run: Vec<(u64, u32)> = (0..m.faces.len() as u32).map(|f| (0, f)).collect();
    let r = collapse_block(&run, &m.faces, &m.verts, &quad, &degree, tol * tol);
    let mut verts = m.verts.clone();
    for (v, p, _) in r.moved {
        verts[v as usize] = p;
    }
    Indexed { verts, faces: r.faces }
}

/// A block's result: its faces after collapsing (global vertex ids), and the vertices that moved
/// with their new position and quadric.
struct Block {
    faces: Vec<[u32; 3]>,
    moved: Vec<(u32, P, Q)>,
}

/// A collapse's cost, ordered totally.
struct Cost(f64);
impl PartialEq for Cost {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o).is_eq()
    }
}
impl Eq for Cost {}
impl PartialOrd for Cost {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cost {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

fn collapse_block(run: &[(u64, u32)], faces: &[[u32; 3]], verts: &[P], quad: &[Q], degree: &[u32], tol2: f64) -> Block {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let mut local: rustc_hash::FxHashMap<u32, u32> = rustc_hash::FxHashMap::default();
    let mut global: Vec<u32> = Vec::new();
    let mut lf: Vec<[u32; 3]> = run
        .iter()
        .map(|&(_, f)| {
            faces[f as usize].map(|v| {
                *local.entry(v).or_insert_with(|| {
                    global.push(v);
                    global.len() as u32 - 1
                })
            })
        })
        .collect();
    let n = global.len();
    let mut pos: Vec<P> = global.iter().map(|&g| verts[g as usize]).collect();
    let mut q: Vec<Q> = global.iter().map(|&g| quad[g as usize]).collect();
    let mut vf: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (i, f) in lf.iter().enumerate() {
        for &v in f {
            vf[v as usize].push(i as u32);
        }
    }
    // A vertex may move when every face round it is in the block.
    let whole: Vec<bool> = (0..n).map(|v| vf[v].len() as u32 == degree[global[v] as usize]).collect();
    // And so does every neighbour, so the faces two rings out (checked for folds) are all here.
    let free: Vec<bool> =
        (0..n).map(|v| whole[v] && vf[v].iter().all(|&f| lf[f as usize].iter().all(|&w| whole[w as usize]))).collect();
    let mut alive = vec![true; lf.len()];
    let mut dead_v = vec![false; n];
    let mut stamp = vec![0u32; n];
    let mut moved = vec![false; n];

    let ring = |vf: &Vec<Vec<u32>>, lf: &Vec<[u32; 3]>, v: u32| -> Vec<u32> {
        let mut r: Vec<u32> = vf[v as usize].iter().flat_map(|&f| lf[f as usize]).filter(|&w| w != v).collect();
        r.sort_unstable();
        r.dedup();
        r
    };
    // The cost and target of collapsing a-b, if allowed.
    let consider =
        |a: u32, b: u32, pos: &Vec<P>, q: &Vec<Q>, vf: &Vec<Vec<u32>>, lf: &Vec<[u32; 3]>| -> Option<(f64, P)> {
            let (pa, pb) = (pos[a as usize], pos[b as usize]);
            let qq = q[a as usize].add(q[b as usize]);
            let mid = mul(add(pa, pb), 0.5);
            let el = len(sub(pa, pb));
            let mut best = [pa, pb, mid]
                .into_iter()
                .map(|p| (qq.eval(p), p))
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .expect("three points");
            if let Some(p) = qq.optimum().filter(|p| len(sub(*p, mid)) <= el) {
                let e = qq.eval(p);
                if e < best.0 {
                    best = (e, p);
                }
            }
            if best.0 > tol2 {
                return None;
            }
            let (na, nb) = (ring(vf, lf, a), ring(vf, lf, b));
            let common = na.iter().filter(|v| nb.binary_search(v).is_ok()).count();
            if common != 2 || na.len() <= 3 || nb.len() <= 3 {
                return None;
            }
            let (mut worst_old, mut worst_new) = (f64::MAX, f64::MAX);
            for &f in vf[a as usize].iter().chain(&vf[b as usize]) {
                let t = lf[f as usize];
                if t.contains(&a) && t.contains(&b) {
                    let old = t.map(|v| pos[v as usize]);
                    worst_old = worst_old.min(quality(&old, len(normal(old[0], old[1], old[2]))));
                    continue;
                }
                let old = t.map(|v| pos[v as usize]);
                let new = t.map(|v| if v == a || v == b { best.1 } else { pos[v as usize] });
                let (no, nn) = (normal(old[0], old[1], old[2]), normal(new[0], new[1], new[2]));
                let (l0, l1) = (len(no), len(nn));
                if l1 < 1e-30 || dot(no, nn) < FLIP * l0 * l1 {
                    return None;
                }
                worst_new = worst_new.min(quality(&new, l1));
                worst_old = worst_old.min(quality(&old, l0));
            }
            // The worst face round the collapse may not get worse, unless it stays fair.
            if worst_new < SLIVER && worst_new < worst_old {
                return None;
            }
            // No new face may cross a face two rings out (a local fold).
            let mut near: Vec<u32> = na.iter().chain(&nb).flat_map(|&w| vf[w as usize].iter().copied()).collect();
            near.sort_unstable();
            near.dedup();
            for &f in vf[a as usize].iter().chain(&vf[b as usize]) {
                let t = lf[f as usize];
                if t.contains(&a) && t.contains(&b) {
                    continue;
                }
                let tn = t.map(|v| if v == b { a } else { v });
                let new = tn.map(|v| if v == a { best.1 } else { pos[v as usize] });
                for &g in &near {
                    let u = lf[g as usize];
                    if u.contains(&a) || u.contains(&b) {
                        continue;
                    }
                    let shared = tn.iter().filter(|v| u.contains(v)).count();
                    let other = u.map(|v| pos[v as usize]);
                    let hit = match shared {
                        0 => super::intersect::crosses(&new, &other),
                        1 => super::intersect::crosses_at_corner(&tn, &new, &u, &other),
                        _ => false,
                    };
                    if hit {
                        return None;
                    }
                }
            }
            Some(best)
        };
    // Cost, the edge's ends, and their stamps when it was costed.
    type Entry = Reverse<(Cost, u32, u32, u32, u32)>;
    let mut heap: BinaryHeap<Entry> = BinaryHeap::new();
    for f in &lf {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            if a < b
                && free[a as usize]
                && free[b as usize]
                && let Some((c, _)) = consider(a, b, &pos, &q, &vf, &lf)
            {
                heap.push(Reverse((Cost(c), a, b, 0, 0)));
            }
        }
    }
    while let Some(Reverse((_, a, b, sa, sb))) = heap.pop() {
        if dead_v[a as usize] || dead_v[b as usize] || stamp[a as usize] != sa || stamp[b as usize] != sb {
            continue;
        }
        let Some((_, p)) = consider(a, b, &pos, &q, &vf, &lf) else { continue };
        // b into a.
        pos[a as usize] = p;
        q[a as usize] = q[a as usize].add(q[b as usize]);
        dead_v[b as usize] = true;
        moved[a as usize] = true;
        let fb = std::mem::take(&mut vf[b as usize]);
        for f in fb {
            let t = lf[f as usize];
            if t.contains(&a) {
                alive[f as usize] = false;
                for &v in &t {
                    if v != b {
                        vf[v as usize].retain(|&g| g != f);
                    }
                }
            } else {
                lf[f as usize] = t.map(|v| if v == b { a } else { v });
                vf[a as usize].push(f);
            }
        }
        // Edges at a are costed again; edges further out keep their cost (their ends did not
        // change) and are checked again when they come up.
        stamp[a as usize] += 1;
        if free[a as usize] {
            for w in ring(&vf, &lf, a) {
                if !free[w as usize] {
                    continue;
                }
                let (x, y) = (a.min(w), a.max(w));
                if let Some((c, _)) = consider(x, y, &pos, &q, &vf, &lf) {
                    heap.push(Reverse((Cost(c), x, y, stamp[x as usize], stamp[y as usize])));
                }
            }
        }
    }
    Block {
        faces: lf.iter().zip(&alive).filter(|(_, a)| **a).map(|(f, _)| f.map(|v| global[v as usize])).collect(),
        moved: (0..n).filter(|&v| moved[v] && !dead_v[v]).map(|v| (global[v], pos[v], q[v])).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::mesh::{self, tests::cube};
    use super::*;

    #[test]
    fn a_remeshed_cube_simplifies_to_a_few_faces_and_stays_watertight() {
        let (m, _) = super::super::remesh::remesh(&cube([0.0; 3], [2.0; 3]), 0.1);
        let (s, st) = simplify(&m, 0.01);
        let c = mesh::check(&s, true);
        assert!(c.watertight() && c.shells == 1, "{c:?}");
        assert!(st.after * 10 < st.before, "{st:?}");
        assert!((c.volume - 8.0).abs() < 0.2, "{}", c.volume);
    }
}
