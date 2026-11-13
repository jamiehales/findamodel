//! Local mending of a shell crossing itself, before any box is remeshed: the faces that cross are
//! taken out with the rings round them, and each hole left is closed by the triangulation of its
//! rim with the least area. Every other face is kept exactly. A spot whose patch still crosses
//! the surface is tried again with a wider ring, and one that cannot be closed this way is left
//! for the box mend ([`super::union`]).
//!
//! Sculpting tools leave small pleats where their remesher folds a few faces over their
//! neighbours (Aradia's 24d8c4f8 base: 291 crossing pairs, all within a dozen rings of each other,
//! over 322 faces and 1.6 mm2): taking a pleat out and spanning its rim moves the surface by about
//! the pleat's height, where remeshing a box round it rebuilds tens of times that area.

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use super::mesh::Indexed;
use crate::geometry::support_reader::math::{BoxIndex, P, closest_on_tri, cross, dist, len, sub};

/// What [`mend`] did.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LocalStats {
    /// Spots (groups of crossing faces) mended, and those left for the box mend.
    pub mended: usize,
    pub left: usize,
    pub faces_removed: usize,
    pub faces_added: usize,
}

/// Rings tried round a spot's crossing faces, the narrowest first.
const RINGS: [usize; 5] = [0, 1, 2, 4, 8];

/// Holes up to this many corners are triangulated to stay close to the original surface; longer
/// ones by least area alone.
const DEVIATION_MAX_RIM: usize = 80;

/// The area term of a patch face's weight, against its mean squared distance from the surface
/// (mm2): a face within about 1 um of it is weighed by its area alone.
const AREA_WEIGHT_MM2: f64 = 1e-6;

/// Rounds of filling in a region and taking in the faces round its pinches.
const FILL_IN_ROUNDS: usize = 8;

/// Holes with more corners than this are not triangulated (the box mend takes the spot).
const MAX_RIM: usize = 400;

/// `m` with the self-crossings `pairs` (face pairs of one shell) mended where a patch can do it.
/// Works in rounds, the ring round the crossing faces widening each round: the crossing faces
/// still to mend, grown by the round's rings, make regions (spots close enough to touch go
/// together), and each region whose rim takes a patch that crosses nothing near it is replaced
/// by that patch. Faces never move; a mended region's faces go and the patch's are added.
pub fn mend(m: &Indexed, pairs: &[(u32, u32)]) -> (Indexed, LocalStats) {
    let mut st = LocalStats::default();
    let mut cur = m.clone();
    let mut removed = vec![false; cur.faces.len()];
    let mut pending: Vec<u32> = pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    pending.sort_unstable();
    pending.dedup();
    for (round, &rings) in RINGS.iter().enumerate() {
        if pending.is_empty() {
            break;
        }
        let adj = Adjacency::new(&cur, &removed);
        let grown = adj.fill_in(adj.grow(&pending, rings, &removed));
        let pending_set: FxHashSet<u32> = pending.iter().copied().collect();
        let mut failed: Vec<u32> = Vec::new();
        let mut patches: Vec<[u32; 3]> = Vec::new();
        let mut gone: Vec<u32> = Vec::new();
        // Regions are apart, so their patches are worked out side by side (each checked against the
        // surface as it was before this round; a crossing between two new patches is left for the
        // repair's follow-up passes, which check the whole result).
        let regions = adj.components(&grown);
        let found: Vec<Option<Vec<[u32; 3]>>> = regions
            .par_iter()
            .map(|region| {
                patch_for(&cur, &adj, region, &removed)
                    .filter(|patch| !crosses_near(&cur, &adj, region, patch, &removed))
            })
            .collect();
        for (region, patch) in regions.into_iter().zip(found) {
            match patch {
                Some(patch) => {
                    st.mended += 1;
                    gone.extend(&region);
                    patches.extend(patch);
                }
                None if round + 1 == RINGS.len() => st.left += 1,
                None => failed.extend(region.iter().copied().filter(|f| pending_set.contains(f))),
            }
        }
        for f in gone {
            removed[f as usize] = true;
        }
        cur.faces.extend(&patches);
        removed.resize(cur.faces.len(), false);
        pending = failed;
    }
    st.faces_removed = removed[..m.faces.len()].iter().filter(|r| **r).count();
    st.faces_added = removed[m.faces.len()..].iter().filter(|r| !**r).count();
    let faces = cur.faces.iter().zip(&removed).filter(|(_, r)| !**r).map(|(f, _)| *f).collect();
    (Indexed { verts: cur.verts, faces }, st)
}

/// Faces by edge, and each face's neighbours across its edges.
struct Adjacency {
    by_edge: FxHashMap<(u32, u32), Vec<u32>>,
    /// The live faces round each corner.
    by_vertex: Vec<Vec<u32>>,
    faces: Vec<[u32; 3]>,
}

impl Adjacency {
    fn new(m: &Indexed, removed: &[bool]) -> Self {
        let mut by_edge: FxHashMap<(u32, u32), Vec<u32>> = FxHashMap::default();
        let mut by_vertex: Vec<Vec<u32>> = vec![Vec::new(); m.verts.len()];
        for (fi, f) in m.faces.iter().enumerate().filter(|(fi, _)| !removed[*fi]) {
            for k in 0..3 {
                let (a, b) = (f[k], f[(k + 1) % 3]);
                by_edge.entry((a.min(b), a.max(b))).or_default().push(fi as u32);
                by_vertex[a as usize].push(fi as u32);
            }
        }
        Adjacency { by_edge, by_vertex, faces: m.faces.clone() }
    }

    fn neighbours(&self, f: u32) -> impl Iterator<Item = u32> + '_ {
        let t = self.faces[f as usize];
        (0..3).flat_map(move |k| {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            self.by_edge[&(a.min(b), a.max(b))].iter().copied().filter(move |&g| g != f)
        })
    }

    /// `seed` and the faces within `rings` of it, leaving out faces already removed.
    fn grow(&self, seed: &[u32], rings: usize, removed: &[bool]) -> Vec<u32> {
        let mut seen: FxHashSet<u32> = seed.iter().copied().collect();
        let mut front: Vec<u32> = seed.to_vec();
        for _ in 0..rings {
            let mut next = Vec::new();
            for &f in &front {
                for g in self.neighbours(f) {
                    if !removed[g as usize] && seen.insert(g) {
                        next.push(g);
                    }
                }
            }
            front = next;
        }
        let mut out: Vec<u32> = seen.into_iter().collect();
        out.sort_unstable();
        out
    }

    /// `faces` with the faces that have two or more neighbours among them added, until none has:
    /// a region grown ring by ring can enclose a face or meet itself at a corner, and its rim is
    /// then not one simple loop.
    fn fill_in(&self, faces: Vec<u32>) -> Vec<u32> {
        let mut set: FxHashSet<u32> = faces.into_iter().collect();
        for _ in 0..FILL_IN_ROUNDS {
            self.fill_concave(&mut set);
            // A corner with more than one rim edge leaving it is a pinch: the faces round it go in.
            let mut out_edges: FxHashMap<u32, usize> = FxHashMap::default();
            for &f in &set {
                let t = self.faces[f as usize];
                for k in 0..3 {
                    let (a, b) = (t[k], t[(k + 1) % 3]);
                    if self.by_edge[&(a.min(b), a.max(b))].iter().any(|g| !set.contains(g)) {
                        *out_edges.entry(a).or_default() += 1;
                    }
                }
            }
            let pinched: FxHashSet<u32> = out_edges.into_iter().filter(|&(_, n)| n > 1).map(|(v, _)| v).collect();
            if pinched.is_empty() {
                break;
            }
            let add: Vec<u32> = set
                .iter()
                .flat_map(|&f| self.neighbours(f))
                .filter(|g| self.faces[*g as usize].iter().any(|v| pinched.contains(v)))
                .collect();
            set.extend(add);
        }
        let mut out: Vec<u32> = set.into_iter().collect();
        out.sort_unstable();
        out
    }

    /// Adds to `set` the faces with two or more neighbours in it, until none has.
    fn fill_concave(&self, set: &mut FxHashSet<u32>) {
        loop {
            let mut add: Vec<u32> = set
                .iter()
                .flat_map(|&f| self.neighbours(f))
                .filter(|g| !set.contains(g))
                .filter(|&g| self.neighbours(g).filter(|h| set.contains(h)).count() >= 2)
                .collect();
            add.sort_unstable();
            add.dedup();
            if add.is_empty() {
                break;
            }
            set.extend(add);
        }
    }

    /// `faces` split into groups joined across edges, each ascending, in order of their first face.
    fn components(&self, faces: &[u32]) -> Vec<Vec<u32>> {
        let set: FxHashSet<u32> = faces.iter().copied().collect();
        let mut seen: FxHashSet<u32> = FxHashSet::default();
        let mut out = Vec::new();
        for &f in faces {
            if !seen.insert(f) {
                continue;
            }
            let mut group = vec![f];
            let mut stack = vec![f];
            while let Some(g) = stack.pop() {
                for h in self.neighbours(g) {
                    if set.contains(&h) && seen.insert(h) {
                        group.push(h);
                        stack.push(h);
                    }
                }
            }
            group.sort_unstable();
            out.push(group);
        }
        out
    }
}

/// The patch closing the hole `region` leaves: its rim walked the way the region's faces run, and
/// triangulated with the least area. None when the rim is not one simple loop, is too long, or
/// cannot be triangulated without making an edge the kept faces already have.
fn patch_for(m: &Indexed, adj: &Adjacency, region: &[u32], removed: &[bool]) -> Option<Vec<[u32; 3]>> {
    let inside: FxHashSet<u32> = region.iter().copied().collect();
    // Rim half-edges: sides of region faces whose other face is kept.
    let mut next: FxHashMap<u32, u32> = FxHashMap::default();
    for &f in region {
        let t = m.faces[f as usize];
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let faces = &adj.by_edge[&(a.min(b), a.max(b))];
            if faces.len() != 2 {
                return None;
            }
            let other = if faces[0] == f { faces[1] } else { faces[0] };
            if !inside.contains(&other) && !removed[other as usize] {
                if next.insert(a, b).is_some() {
                    return None;
                }
            } else if removed[other as usize] {
                return None;
            }
        }
    }
    let &start = next.keys().min()?;
    let mut rim = vec![start];
    let mut at = next[&start];
    while at != start {
        if rim.len() > MAX_RIM {
            return None;
        }
        rim.push(at);
        at = *next.get(&at)?;
    }
    if rim.len() != next.len() || rim.len() < 3 {
        return None;
    }
    // Edges the kept faces have, which the patch must not make again.
    let mut kept_edges: FxHashSet<(u32, u32)> = FxHashSet::default();
    let rim_set: FxHashSet<u32> = rim.iter().copied().collect();
    for &v in &rim {
        for &g in &adj.by_vertex[v as usize] {
            if inside.contains(&g) {
                continue;
            }
            let t = m.faces[g as usize];
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if rim_set.contains(&a) && rim_set.contains(&b) {
                    kept_edges.insert((a.min(b), a.max(b)));
                }
            }
        }
    }
    let n = rim.len();
    let p = |i: usize| m.verts[rim[i] as usize];
    // The original surface round the hole (the faces taken out and two rings of kept ones): a
    // patch face is weighed by how far it strays from it, so the patch follows a crease the hole
    // spans instead of cutting across it, and by its area, which settles the rest.
    let surface: Vec<[P; 3]> = if n <= DEVIATION_MAX_RIM {
        adj.grow(region, 2, removed).iter().map(|&f| m.tri(f as usize)).collect()
    } else {
        Vec::new()
    };
    // Distances looked up in an index, as far as the rim's longest side reaches (every probe of a
    // patch face lies within that of a rim corner, which is on the surface).
    let reach = (0..n).map(|i| dist(p(i), p((i + 1) % n))).fold(0.0, f64::max).max(1e-6);
    let index = (!surface.is_empty()).then(|| BoxIndex::new(&surface, reach));
    let near = std::cell::RefCell::new(Vec::new());
    let off = |q: P| {
        let Some(index) = &index else { return 0.0 };
        let mut near = near.borrow_mut();
        index.query_into(sub(q, [reach; 3]), [q[0] + reach, q[1] + reach, q[2] + reach], &mut near);
        near.iter().map(|&t| dist(q, closest_on_tri(q, &surface[t as usize]))).fold(reach, f64::min)
    };
    let area = |i: usize, j: usize, k: usize| {
        let a = 0.5 * len(cross(sub(p(j), p(i)), sub(p(k), p(i))));
        if surface.is_empty() {
            return a;
        }
        let (x, y, z) = (p(i), p(j), p(k));
        let probes = [
            [0, 1, 2].map(|q| (x[q] + y[q] + z[q]) / 3.0),
            [0, 1, 2].map(|q| (x[q] + y[q]) / 2.0),
            [0, 1, 2].map(|q| (y[q] + z[q]) / 2.0),
            [0, 1, 2].map(|q| (z[q] + x[q]) / 2.0),
        ];
        let d2: f64 = probes.iter().map(|&q| off(q).powi(2)).sum::<f64>() / 4.0;
        a * (AREA_WEIGHT_MM2 + d2)
    };
    let blocked = |i: usize, k: usize| {
        let (a, b) = (rim[i], rim[k]);
        k - i > 1 && !(i == 0 && k == n - 1) && kept_edges.contains(&(a.min(b), a.max(b)))
    };
    // Least-area triangulation of the polygon (dynamic programming over its diagonals).
    let mut cost = vec![f64::INFINITY; n * n];
    let mut pick = vec![usize::MAX; n * n];
    for i in 0..n - 1 {
        cost[i * n + i + 1] = 0.0;
    }
    for gap in 2..n {
        for i in 0..n - gap {
            let k = i + gap;
            if blocked(i, k) {
                continue;
            }
            for j in i + 1..k {
                let c = cost[i * n + j] + cost[j * n + k] + area(i, j, k);
                if c < cost[i * n + k] {
                    cost[i * n + k] = c;
                    pick[i * n + k] = j;
                }
            }
        }
    }
    if !cost[n - 1].is_finite() {
        return None;
    }
    let mut out = Vec::with_capacity(n - 2);
    let mut stack = vec![(0usize, n - 1)];
    while let Some((i, k)) = stack.pop() {
        if k - i < 2 {
            continue;
        }
        let j = pick[i * n + k];
        out.push([rim[i], rim[j], rim[k]]);
        stack.push((i, j));
        stack.push((j, k));
    }
    Some(out)
}

/// Whether the patch crosses itself or any kept face near the region (within a few rings).
fn crosses_near(m: &Indexed, adj: &Adjacency, region: &[u32], patch: &[[u32; 3]], removed: &[bool]) -> bool {
    let near = adj.grow(region, 6, removed);
    let inside: FxHashSet<u32> = region.iter().copied().collect();
    let mut faces: Vec<[u32; 3]> = near.iter().filter(|f| !inside.contains(f)).map(|&f| m.faces[f as usize]).collect();
    let kept = faces.len();
    faces.extend_from_slice(patch);
    // Only the corners these faces use, renumbered (the order of the faces is kept).
    let mut at: FxHashMap<u32, u32> = FxHashMap::default();
    let mut verts = Vec::new();
    let faces = faces
        .iter()
        .map(|f| {
            f.map(|v| {
                *at.entry(v).or_insert_with(|| {
                    verts.push(m.verts[v as usize]);
                    verts.len() as u32 - 1
                })
            })
        })
        .collect();
    let local = Indexed { verts, faces };
    super::intersect::pairs(&local).iter().any(|&(a, b)| a as usize >= kept || b as usize >= kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A finely split ball of radius 5 (48 x 48), welded.
    fn ball() -> Indexed {
        let (r, n) = (5.0, 48);
        let at = |i: usize, j: usize| -> P {
            if i == 0 || i == n {
                return [0.0, 0.0, if i == 0 { r } else { -r }];
            }
            let (t, q) =
                (std::f64::consts::PI * i as f64 / n as f64, std::f64::consts::TAU * (j % n) as f64 / n as f64);
            [r * t.sin() * q.cos(), r * t.sin() * q.sin(), r * t.cos()]
        };
        let mut t: Vec<[P; 3]> = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                if i > 0 {
                    t.push([a, b, d]);
                }
                if i + 1 < n {
                    t.push([b, c, d]);
                }
            }
        }
        Indexed::weld(&t)
    }

    #[test]
    fn a_pleat_is_cut_out_and_spanned_and_everything_else_kept() {
        let mut m = ball();
        // One point on the equator dragged sideways past its neighbours: its faces fold over them.
        let v = (0..m.verts.len()).find(|&v| m.verts[v][2].abs() < 1e-9 && m.verts[v][0] > 4.9).unwrap();
        m.verts[v][1] += 1.6;
        let pairs = super::super::intersect::pairs(&m);
        assert!(!pairs.is_empty());
        let (out, st) = mend(&m, &pairs);
        assert_eq!((st.mended, st.left), (1, 0), "{st:?}");
        assert!(super::super::intersect::pairs(&out).is_empty());
        let c = super::super::mesh::check(&out, false);
        assert!(c.boundary_edges + c.non_manifold_edges + c.misoriented_edges == 0, "{c:?}");
        // Only the faces round the pleat changed.
        let before: FxHashSet<[u32; 3]> = m.faces.iter().copied().collect();
        let kept = out.faces.iter().filter(|f| before.contains(*f)).count();
        assert!(kept + 60 > m.faces.len() && st.faces_removed < 60, "{kept} of {}, {st:?}", m.faces.len());
    }

    #[test]
    fn a_mesh_without_crossings_is_left_as_it_is() {
        let m = ball();
        let (out, st) = mend(&m, &[]);
        assert_eq!(out.faces, m.faces);
        assert_eq!(st.mended + st.left, 0);
    }
}
