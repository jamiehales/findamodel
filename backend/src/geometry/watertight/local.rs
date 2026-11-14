//! Local mending of a shell crossing itself, before any box is remeshed: the faces that cross are
//! taken out with the rings round them, and each hole left is closed by the triangulation of its
//! rim that keeps closest to the original surface. Every other face is kept exactly. A spot whose
//! patch crosses kept faces is tried again with those faces taken out too, then with a wider
//! ring, and one that cannot be closed this way is left for the box mend ([`super::union`]).
//!
//! Sculpting tools leave small pleats where their remesher folds a few faces over their
//! neighbours (Aradia's 24d8c4f8 base: 291 crossing pairs, all within a dozen rings of each other,
//! over 322 faces and 1.6 mm2): taking a pleat out and spanning its rim moves the surface by about
//! the pleat's height, where remeshing a box round it rebuilds tens of times that area.
//!
//! They also leave crumples: clumps of tiny faces folded every which way (an orc's arm, 363c280a:
//! 8,980 faces over 0.38 mm2 with 62k crossing pairs, most between faces more than a dozen rings
//! apart that lie side by side). Pieces that cross are taken out together; the clump's holes,
//! tunnels and tucked-in bits give its rim several loops, each spanned on its own once the bits it
//! closes off are taken in too; and corners where the surface touches itself go as well, since
//! the boolean splits them and rounding then folds the copies through each other.

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

/// The area term of a patch face's weight, against its mean squared distance from the surface
/// (mm2): a face within about 1 um of it is weighed by its area alone.
const AREA_WEIGHT_MM2: f64 = 1e-6;

/// Rounds of filling in a region and taking in the faces round its pinches.
const FILL_IN_ROUNDS: usize = 8;

/// Holes with more corners than this are not triangulated (the box mend takes the spot).
const MAX_RIM: usize = 400;

/// Tries at a region's patch, each taking in the kept faces the last one crossed.
const TAKE_CROSSED: usize = 6;

/// `m` with the self-crossings `pairs` (face pairs of one shell) mended where a patch can do it.
/// Works in rounds, the ring round the crossing faces widening each round: the crossing faces
/// still to mend (and the faces round corners where the surface touches itself), grown by the
/// round's rings, make regions (spots close enough to touch, or that cross each other, go
/// together), and each region whose rim takes a patch that crosses nothing near it is replaced
/// by that patch. Faces never move; a mended region's faces go and the patch's are added.
pub fn mend(m: &Indexed, pairs: &[(u32, u32)]) -> (Indexed, LocalStats) {
    let mut st = LocalStats::default();
    let mut cur = m.clone();
    let mut removed = vec![false; cur.faces.len()];
    let mut pending: Vec<u32> = pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    pending.sort_unstable();
    pending.dedup();
    // Built once and kept up to date as regions go and patches come (a rebuild over the whole
    // mesh each round costs more than the mending on a large one).
    let mut adj = if pending.is_empty() { None } else { Some(Adjacency::new(&cur, &removed)) };
    if let Some(adj) = adj.as_ref() {
        // Corners where the surface meets itself go too: the boolean splits such a corner, and
        // its faces, kept touching, cross once the copies are rounded apart.
        pending.extend(adj.pinched_faces(&cur));
        pending.sort_unstable();
        pending.dedup();
    }
    for (round, &rings) in RINGS.iter().enumerate() {
        if pending.is_empty() {
            break;
        }
        let adj = adj.as_mut().expect("built when anything is pending");
        let grown = adj.fill_in(adj.grow(&pending, rings, &removed));
        let pending_set: FxHashSet<u32> = pending.iter().copied().collect();
        let mut failed: Vec<u32> = Vec::new();
        let mut patches: Vec<[u32; 3]> = Vec::new();
        let mut gone: Vec<u32> = Vec::new();
        // Pieces that cross each other or whose boxes meet are one region (a crumple's folds lie
        // side by side in space however far apart they are along the surface). The regions'
        // patches are worked out side by side, each checked against the surface as it was before
        // this round.
        let regions = group(&cur, adj.components(&grown), pairs);
        let mut owner = vec![u32::MAX; cur.faces.len()];
        for (i, region) in regions.iter().enumerate() {
            for &f in region {
                owner[f as usize] = i as u32;
            }
        }
        let index = BoxIndex::new(&cur.tris(), cell_of(&cur));
        let found: Vec<Option<Mend>> = (0..regions.len())
            .into_par_iter()
            .map(|i| {
                let me = i as u32;
                // A patch that crosses kept faces (the folds round a crumple lie across its
                // rim) is tried again with those faces taken out too.
                let mut region = regions[i].clone();
                for step in 0..TAKE_CROSSED {
                    let (taken, rims) = close_off(&cur, adj, &region, me, &owner, &removed)?;
                    let patch = patch_for(&cur, adj, &taken, &rims, &removed, true)?;
                    let inside: FxHashSet<u32> = taken.iter().copied().collect();
                    let (crossed, itself) = crossed_near(&cur, &index, &inside, &patch, &removed);
                    if crossed.is_empty() && !itself {
                        return Some((taken, patch));
                    }
                    // Grown only while it keeps to faces of no other region.
                    let grown = (!crossed.is_empty() && step + 1 < TAKE_CROSSED)
                        .then(|| adj.fill_in(taken.iter().copied().chain(crossed).collect()))
                        .filter(|g| g.iter().all(|&f| [u32::MAX, me].contains(&owner[f as usize])));
                    if let Some(grown) = grown {
                        region = grown;
                        continue;
                    }
                    // A patch keeping close to a crumpled surface can fold with it: the flattest
                    // one (least area) is the last try.
                    let flat = patch_for(&cur, adj, &taken, &rims, &removed, false)?;
                    let (more, folds) = crossed_near(&cur, &index, &inside, &flat, &removed);
                    return (more.is_empty() && !folds).then_some((taken, flat));
                }
                None
            })
            .collect();
        // A patch stays within the box of the faces it replaces, so two regions whose boxes are
        // apart cannot make patches that cross. Regions start apart, but one grown past its box
        // can meet another: of two that took the same face or whose patches cross, the first is
        // mended and the other tried again next round.
        let mut claimed = vec![false; cur.faces.len()];
        let mut done: Vec<((P, P), Patch)> = Vec::new();
        for (region, patch) in regions.into_iter().zip(found) {
            let patch = patch.filter(|(taken, patch)| {
                let (lo, hi) = bounds_of(&cur, taken);
                let clear = !taken.iter().any(|&f| claimed[f as usize])
                    && done.iter().all(|((a, b), other)| {
                        (0..3).any(|k| lo[k] > b[k] || a[k] > hi[k]) || !patches_cross(&cur, patch, other)
                    });
                if clear {
                    taken.iter().for_each(|&f| claimed[f as usize] = true);
                    done.push(((lo, hi), patch.clone()));
                }
                clear
            });
            match patch {
                Some((taken, patch)) => {
                    st.mended += 1;
                    gone.extend(taken);
                    patches.extend(patch);
                }
                None if round + 1 == RINGS.len() => st.left += 1,
                None => failed.extend(region.iter().copied().filter(|f| pending_set.contains(f))),
            }
        }
        for &f in &gone {
            removed[f as usize] = true;
        }
        adj.remove(&gone);
        adj.add(&patches);
        cur.faces.extend(&patches);
        removed.resize(cur.faces.len(), false);
        pending = failed;
    }
    st.faces_removed = removed[..m.faces.len()].iter().filter(|r| **r).count();
    st.faces_added = removed[m.faces.len()..].iter().filter(|r| !**r).count();
    let faces = cur.faces.iter().zip(&removed).filter(|(_, r)| !**r).map(|(f, _)| *f).collect();
    (Indexed { verts: cur.verts, faces }, st)
}

/// Faces spanning a hole.
type Patch = Vec<[u32; 3]>;

/// The faces a region takes out and the patch that replaces them.
type Mend = (Vec<u32>, Patch);

/// Faces by edge, and each face's neighbours across its edges.
struct Adjacency {
    by_edge: FxHashMap<(u32, u32), EdgeFaces>,
    /// The live faces round each corner.
    by_vertex: VertexFaces,
    faces: Vec<[u32; 3]>,
}

/// The faces on one edge, ascending: two (as on most edges) are held in place, more in a list.
/// Reads as a slice.
#[derive(Debug, Default, Clone)]
struct EdgeFaces {
    two: [u32; 2],
    len: u32,
    /// Every face, when there are more than two (empty otherwise).
    more: Vec<u32>,
}

impl EdgeFaces {
    fn push(&mut self, f: u32) {
        if !self.more.is_empty() {
            self.more.push(f);
        } else if self.len < 2 {
            self.two[self.len as usize] = f;
            self.len += 1;
        } else {
            self.more = [self.two[0], self.two[1], f].to_vec();
        }
    }

    /// Takes `f` out wherever it is.
    fn remove(&mut self, f: u32) {
        if !self.contains(&f) {
            return;
        }
        let rest: Vec<u32> = self.iter().copied().filter(|&g| g != f).collect();
        *self = EdgeFaces::default();
        for g in rest {
            self.push(g);
        }
    }
}

impl std::ops::Deref for EdgeFaces {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        if self.more.is_empty() { &self.two[..self.len as usize] } else { &self.more }
    }
}

/// The faces round each corner, each corner's in order: in one array, each corner's run with
/// room for as many faces as it had at first, and a corner that comes to have more moved to a
/// list of its own. Indexed by corner, it reads as a slice.
#[derive(Debug, Default)]
struct VertexFaces {
    /// Where each corner's run starts (and the end of the last).
    start: Vec<u32>,
    /// How many of each run's places hold faces.
    len: Vec<u32>,
    items: Vec<u32>,
    moved: FxHashMap<u32, Vec<u32>>,
}

impl VertexFaces {
    /// `faces` round each of `verts` corners: face `f` once for each place a corner has in it.
    fn new<'a>(verts: usize, faces: impl Iterator<Item = (u32, &'a [u32; 3])> + Clone) -> Self {
        let mut start = vec![0u32; verts + 1];
        for (_, t) in faces.clone() {
            for &v in t {
                start[v as usize + 1] += 1;
            }
        }
        for v in 0..verts {
            start[v + 1] += start[v];
        }
        let mut len = vec![0u32; verts];
        let mut items = vec![0u32; start[verts] as usize];
        for (f, t) in faces {
            for &v in t {
                items[(start[v as usize] + len[v as usize]) as usize] = f;
                len[v as usize] += 1;
            }
        }
        VertexFaces { start, len, items, moved: FxHashMap::default() }
    }

    /// How many corners there are.
    fn corners(&self) -> usize {
        self.len.len()
    }

    fn push(&mut self, v: u32, f: u32) {
        let vi = v as usize;
        if let Some(list) = self.moved.get_mut(&v) {
            list.push(f);
        } else if self.start[vi] + self.len[vi] < self.start[vi + 1] {
            self.items[(self.start[vi] + self.len[vi]) as usize] = f;
            self.len[vi] += 1;
        } else {
            let mut list = self[vi].to_vec();
            list.push(f);
            self.moved.insert(v, list);
        }
    }

    /// Takes `f` out of corner `v`'s faces wherever it is.
    fn remove(&mut self, v: u32, f: u32) {
        let vi = v as usize;
        if let Some(list) = self.moved.get_mut(&v) {
            list.retain(|&g| g != f);
            return;
        }
        let (s, n) = (self.start[vi] as usize, self.len[vi] as usize);
        let run = &mut self.items[s..s + n];
        let mut kept = 0;
        for i in 0..n {
            if run[i] != f {
                run[kept] = run[i];
                kept += 1;
            }
        }
        self.len[vi] = kept as u32;
    }
}

impl std::ops::Index<usize> for VertexFaces {
    type Output = [u32];
    fn index(&self, v: usize) -> &[u32] {
        if !self.moved.is_empty()
            && let Some(list) = self.moved.get(&(v as u32))
        {
            return list;
        }
        let (s, n) = (self.start[v] as usize, self.len[v] as usize);
        &self.items[s..s + n]
    }
}

impl Adjacency {
    fn new(m: &Indexed, removed: &[bool]) -> Self {
        let live = m.faces.iter().enumerate().filter(|(fi, _)| !removed[*fi]).map(|(fi, f)| (fi as u32, f));
        // Sized for an edge per side and a half, as a closed mesh has.
        let mut by_edge: FxHashMap<(u32, u32), EdgeFaces> =
            FxHashMap::with_capacity_and_hasher(m.faces.len() * 3 / 2 + 16, Default::default());
        for (fi, f) in live.clone() {
            for k in 0..3 {
                let (a, b) = (f[k], f[(k + 1) % 3]);
                by_edge.entry((a.min(b), a.max(b))).or_default().push(fi);
            }
        }
        let by_vertex = VertexFaces::new(m.verts.len(), live);
        Adjacency { by_edge, by_vertex, faces: m.faces.clone() }
    }

    /// Takes `gone` out, as a rebuild without them would (the lists keep their order).
    fn remove(&mut self, gone: &[u32]) {
        for &f in gone {
            let t = self.faces[f as usize];
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if let Some(list) = self.by_edge.get_mut(&(a.min(b), a.max(b))) {
                    list.remove(f);
                }
                self.by_vertex.remove(a, f);
            }
        }
    }

    /// Adds `faces` after the faces there are, as a rebuild with them would (each new face's index
    /// is above every list's, so the lists stay ascending).
    fn add(&mut self, faces: &[[u32; 3]]) {
        for t in faces {
            let fi = self.faces.len() as u32;
            self.faces.push(*t);
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                self.by_edge.entry((a.min(b), a.max(b))).or_default().push(fi);
                self.by_vertex.push(a, fi);
            }
        }
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
            // A corner with more than one rim edge leaving it is a pinch, and so is one whose kept
            // faces are not one fan (the mesh meets itself there, as a crumple's folds often do:
            // a patch beside it touches the other fan at a point, which the boolean splits):
            // the faces round it go in.
            let mut out_edges: FxHashMap<u32, (usize, u32)> = FxHashMap::default();
            for &f in &set {
                let t = self.faces[f as usize];
                for k in 0..3 {
                    let (a, b) = (t[k], t[(k + 1) % 3]);
                    if self.by_edge[&(a.min(b), a.max(b))].iter().any(|g| !set.contains(g)) {
                        let e = out_edges.entry(a).or_insert((0, b));
                        e.0 += 1;
                    }
                }
            }
            let mut pinched: Vec<u32> = out_edges
                .into_iter()
                .filter(|&(v, (n, b))| n > 1 || !self.one_fan(&set, v, b))
                .map(|(v, _)| v)
                .collect();
            if pinched.is_empty() {
                break;
            }
            pinched.sort_unstable();
            for v in pinched {
                set.extend(self.by_vertex[v as usize].iter().copied());
            }
        }
        let mut out: Vec<u32> = set.into_iter().collect();
        out.sort_unstable();
        out
    }

    /// The faces round the corners of `m` where its surface meets itself (the faces round the
    /// corner are more than one fan) and the fans lie so close that moving one fan's copy of the
    /// corner by a rounding step folds it through another ([`Adjacency::folds_apart`]).
    fn pinched_faces(&self, m: &Indexed) -> Vec<u32> {
        // Two fans of a closed surface have at least three faces each.
        (0..self.by_vertex.corners() as u32)
            .into_par_iter()
            .filter(|&v| self.by_vertex[v as usize].len() >= 6)
            .filter(|&v| {
                let round = &self.by_vertex[v as usize];
                let one: FxHashSet<u32> = [round[0]].into_iter().collect();
                let t = self.faces[round[0] as usize];
                let b = t[(t.iter().position(|&x| x == v).unwrap_or(0) + 1) % 3];
                !self.one_fan(&one, v, b) && self.folds_apart(m, v)
            })
            .flat_map_iter(|v| self.by_vertex[v as usize].to_vec())
            .collect()
    }

    /// Whether a fan of faces round corner `v` crosses another once its copy of the corner is
    /// moved by a step a little over single-precision rounding, along any axis (the boolean gives
    /// each fan its own copy, and rounding nudges copies at one point apart).
    fn folds_apart(&self, m: &Indexed, v: u32) -> bool {
        let round = &self.by_vertex[v as usize];
        // Fans: faces round `v` joined across their sides at `v`.
        let mut fan: Vec<usize> = (0..round.len()).collect();
        fn root(fan: &mut [usize], mut i: usize) -> usize {
            while fan[i] != i {
                fan[i] = fan[fan[i]];
                i = fan[i];
            }
            i
        }
        let mut by_side: FxHashMap<u32, usize> = FxHashMap::default();
        for (i, &f) in round.iter().enumerate() {
            for x in self.faces[f as usize] {
                if x == v {
                    continue;
                }
                if let Some(&j) = by_side.get(&x) {
                    let (a, b) = (root(&mut fan, i), root(&mut fan, j));
                    fan[a.max(b)] = a.min(b);
                } else {
                    by_side.insert(x, i);
                }
            }
        }
        let fans: Vec<usize> = (0..round.len()).map(|i| root(&mut fan, i)).collect();
        let p = m.verts[v as usize];
        let step = 1e-6 * p.iter().fold(1.0f64, |a, x| a.max(x.abs()));
        let moved = |f: u32, d: P| {
            m.faces[f as usize]
                .map(|x| if x == v { [p[0] + d[0], p[1] + d[1], p[2] + d[2]] } else { m.verts[x as usize] })
        };
        (0..round.len()).any(|i| {
            (0..round.len()).filter(|&j| fans[j] != fans[i]).any(|j| {
                let (fi, fj) = (self.faces[round[i] as usize], self.faces[round[j] as usize]);
                if fi.iter().filter(|x| fj.contains(x)).count() != 1 {
                    return false;
                }
                (0..6).any(|k| {
                    let mut d = [0.0; 3];
                    d[k / 2] = if k % 2 == 0 { step } else { -step };
                    super::intersect::crosses(&moved(round[i], d), &m.tri(round[j] as usize))
                })
            })
        })
    }

    /// Whether the faces round corner `v` outside `set` are one fan, the one beyond the rim side
    /// `v`-`b` (a side of a face in `set`).
    fn one_fan(&self, set: &FxHashSet<u32>, v: u32, b: u32) -> bool {
        let kept = self.by_vertex[v as usize].iter().filter(|g| !set.contains(g)).count();
        let across = |g: u32, x: u32| {
            let faces = &self.by_edge[&(v.min(x), v.max(x))];
            (faces.len() == 2).then(|| if faces[0] == g { faces[1] } else { faces[0] })
        };
        // Round `v` from the side `v`-`b` through the kept faces, back to a face in `set`.
        let Some(mut g) = self.by_edge[&(v.min(b), v.max(b))].iter().copied().find(|g| !set.contains(g)) else {
            return false;
        };
        let (mut prev, mut seen) = (b, 0);
        while !set.contains(&g) {
            seen += 1;
            if seen > kept {
                return false;
            }
            let t = self.faces[g as usize];
            let Some(&other) = t.iter().find(|&&x| x != v && x != prev) else { return false };
            let Some(h) = across(g, other) else { return false };
            (g, prev) = (h, other);
        }
        seen == kept
    }

    /// Adds to `set` the faces with two or more neighbours in it, until none has.
    /// Each pass tries only the faces next to those the last pass added (a face's count of
    /// neighbours in the set grows only when one of them is added), which adds what trying every
    /// face next to the set would.
    fn fill_concave(&self, set: &mut FxHashSet<u32>) {
        let mut tried: Vec<u32> = set.iter().flat_map(|&f| self.neighbours(f)).filter(|g| !set.contains(g)).collect();
        loop {
            tried.sort_unstable();
            tried.dedup();
            let add: Vec<u32> = tried
                .iter()
                .copied()
                .filter(|&g| self.neighbours(g).filter(|h| set.contains(h)).count() >= 2)
                .collect();
            if add.is_empty() {
                break;
            }
            set.extend(add.iter().copied());
            tried = add.iter().flat_map(|&f| self.neighbours(f)).filter(|g| !set.contains(g)).collect();
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

/// `regions` with those that cross each other (a pair of `pairs` between them) or whose boxes
/// meet joined, each ascending, in order of their first face.
fn group(m: &Indexed, regions: Vec<Vec<u32>>, pairs: &[(u32, u32)]) -> Vec<Vec<u32>> {
    if regions.len() < 2 {
        return regions;
    }
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    fn join(parent: &mut [usize], a: usize, b: usize) {
        let (ra, rb) = (root(parent, a), root(parent, b));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    }
    let mut parent: Vec<usize> = (0..regions.len()).collect();
    let mut of: FxHashMap<u32, usize> = FxHashMap::default();
    for (i, r) in regions.iter().enumerate() {
        for &f in r {
            of.insert(f, i);
        }
    }
    for (a, b) in pairs {
        if let (Some(&i), Some(&j)) = (of.get(a), of.get(b)) {
            join(&mut parent, i, j);
        }
    }
    // Boxes meeting, swept along x.
    let bounds: Vec<(P, P)> = regions.iter().map(|r| bounds_of(m, r)).collect();
    let mut order: Vec<usize> = (0..regions.len()).collect();
    order.sort_by(|&a, &b| bounds[a].0[0].total_cmp(&bounds[b].0[0]).then(a.cmp(&b)));
    for (k, &i) in order.iter().enumerate() {
        for &j in &order[k + 1..] {
            if bounds[j].0[0] > bounds[i].1[0] {
                break;
            }
            if (0..3).all(|q| bounds[i].0[q] <= bounds[j].1[q] && bounds[j].0[q] <= bounds[i].1[q]) {
                join(&mut parent, i, j);
            }
        }
    }
    let mut joined: Vec<Vec<u32>> = vec![Vec::new(); regions.len()];
    for (i, r) in regions.into_iter().enumerate() {
        let to = root(&mut parent, i);
        joined[to].extend(r);
    }
    let mut out: Vec<Vec<u32>> = joined
        .into_iter()
        .filter(|r| !r.is_empty())
        .map(|mut r| {
            r.sort_unstable();
            r
        })
        .collect();
    out.sort_by_key(|r| r[0]);
    out
}

/// The box round faces `faces` of `m`.
fn bounds_of(m: &Indexed, faces: &[u32]) -> (P, P) {
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for &f in faces {
        for v in m.faces[f as usize] {
            let p = m.verts[v as usize];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    (lo, hi)
}

/// The faces `region` takes out and the loops of its rim. A clump of folds often closes off bits
/// of kept surface (a fold tucked under another, a tunnel through the clump) and so has several
/// rim loops: a bit closed off within the clump's box, holding no other region (`owner` gives each
/// face's region), goes too. Every loop left must lead to the one surface round the region, each
/// to be spanned on its own; None when one leads to a piece of its own (a limb the region rings,
/// which spanning would cut off), or the rim is not loops of plain corners ([`rim_loops`]).
fn close_off(
    m: &Indexed,
    adj: &Adjacency,
    region: &[u32],
    me: u32,
    owner: &[u32],
    removed: &[bool],
) -> Option<(Vec<u32>, Vec<Vec<u32>>)> {
    let mut inside: FxHashSet<u32> = region.iter().copied().collect();
    let rims = rim_loops(m, adj, region, &inside, removed)?;
    if rims.len() == 1 {
        return Some((region.to_vec(), rims));
    }
    let (lo, hi) = bounds_of(m, region);
    let pad = 0.25 * dist(lo, hi);
    let within = |piece: &[u32]| {
        let (a, b) = bounds_of(m, piece);
        (0..3).all(|k| a[k] >= lo[k] - pad && b[k] <= hi[k] + pad)
    };
    // The kept face across a rim's first side (none once a piece beyond it is taken in).
    let across = |inside: &FxHashSet<u32>, rim: &[u32]| {
        let (a, b) = (rim[0], rim[1]);
        adj.by_edge[&(a.min(b), a.max(b))].iter().copied().find(|g| !inside.contains(g))
    };
    // The kept faces reached from `seed` (BFS, at most `budget` past the first), and whether
    // that was all of them; stops early once `wanted` are all reached.
    let reach = |inside: &FxHashSet<u32>, seed: u32, budget: usize, wanted: &mut FxHashSet<u32>| {
        let stop = !wanted.is_empty();
        let mut seen: FxHashSet<u32> = [seed].into_iter().collect();
        let mut piece = vec![seed];
        let mut at = 0;
        while at < piece.len() && piece.len() <= budget && !(stop && wanted.is_empty()) {
            let f = piece[at];
            at += 1;
            for g in adj.neighbours(f) {
                if !inside.contains(&g) && !removed[g as usize] && seen.insert(g) {
                    wanted.remove(&g);
                    piece.push(g);
                }
            }
        }
        let whole = at == piece.len();
        (piece, whole)
    };
    let budget = 4 * region.len() + 256;
    let mut taken = region.to_vec();
    let mut grew = false;
    for rim in &rims {
        let Some(seed) = across(&inside, rim) else { continue };
        let (piece, whole) = reach(&inside, seed, budget, &mut FxHashSet::default());
        if !whole {
            continue;
        }
        let foreign = piece.iter().any(|&f| owner[f as usize] != u32::MAX && owner[f as usize] != me);
        if foreign || !within(&piece) {
            return None;
        }
        inside.extend(&piece);
        taken.extend(piece);
        grew = true;
    }
    let rims = if grew {
        taken.sort_unstable();
        rim_loops(m, adj, &taken, &inside, removed)?
    } else {
        rims
    };
    if rims.len() > 1 {
        // Every loop on the one surface: reached from the first one's kept side.
        let seeds: Vec<u32> = rims.iter().map(|r| across(&inside, r)).collect::<Option<_>>()?;
        let mut wanted: FxHashSet<u32> = seeds[1..].iter().copied().filter(|&s| s != seeds[0]).collect();
        reach(&inside, seeds[0], CONNECT_BUDGET, &mut wanted);
        if !wanted.is_empty() {
            return None;
        }
    }
    Some((taken, rims))
}

/// Kept faces walked to find that a region's rim loops all lead to one surface.
const CONNECT_BUDGET: usize = 200_000;

/// The loops of the rim `region` leaves (`inside` holds its faces), each walked the way the
/// region's faces run from its lowest corner, in order of those corners. None when a side of a
/// region face is not shared by exactly two faces or meets a removed face, a corner has two rim
/// sides leaving it, or a loop is too long.
fn rim_loops(
    m: &Indexed,
    adj: &Adjacency,
    region: &[u32],
    inside: &FxHashSet<u32>,
    removed: &[bool],
) -> Option<Vec<Vec<u32>>> {
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
            if removed[other as usize] {
                return None;
            }
            if !inside.contains(&other) && next.insert(a, b).is_some() {
                return None;
            }
        }
    }
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    let mut seen: FxHashSet<u32> = FxHashSet::default();
    let mut rims = Vec::new();
    for start in starts {
        if !seen.insert(start) {
            continue;
        }
        let mut rim = vec![start];
        let mut at = next[&start];
        while at != start {
            if rim.len() >= MAX_RIM || !seen.insert(at) {
                return None;
            }
            rim.push(at);
            at = *next.get(&at)?;
        }
        if rim.len() < 3 {
            return None;
        }
        rims.push(rim);
    }
    (!rims.is_empty()).then_some(rims)
}

/// The patch closing the hole `region` leaves, each of its rim loops `rims` spanned on its own
/// ([`span`]), keeping close to the original surface when `follow` is set and by least area
/// otherwise. None when a loop cannot be triangulated without making an edge the kept faces
/// already have.
fn patch_for(
    m: &Indexed,
    adj: &Adjacency,
    region: &[u32],
    rims: &[Vec<u32>],
    removed: &[bool],
    follow: bool,
) -> Option<Vec<[u32; 3]>> {
    let inside: FxHashSet<u32> = region.iter().copied().collect();
    // Edges the kept faces have, which the patch must not make again.
    let mut kept_edges: FxHashSet<(u32, u32)> = FxHashSet::default();
    let rim_set: FxHashSet<u32> = rims.iter().flatten().copied().collect();
    for rim in rims {
        for &v in rim {
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
    }
    // The original surface round the hole (the faces taken out and two rings of kept ones): a
    // patch face is weighed by how far it strays from it, so the patch follows a crease the hole
    // spans instead of cutting across it, and by its area, which settles the rest.
    let surface: Vec<[P; 3]> =
        if follow { adj.grow(region, 2, removed).iter().map(|&f| m.tri(f as usize)).collect() } else { Vec::new() };
    let mut out = Vec::new();
    for rim in rims {
        out.extend(span(m, rim, &kept_edges, &surface)?);
    }
    Some(out)
}

/// The triangulation of the loop `rim` with the least weight (dynamic programming over its
/// diagonals): each face weighed by its area and, when `surface` is given, by how far it strays
/// from it. None when every one makes an edge in `kept_edges`.
fn span(m: &Indexed, rim: &[u32], kept_edges: &FxHashSet<(u32, u32)>, surface: &[[P; 3]]) -> Option<Vec<[u32; 3]>> {
    let n = rim.len();
    let p = |i: usize| m.verts[rim[i] as usize];
    // Distances looked up in an index, as far as the rim's longest side reaches (every probe of a
    // patch face lies within that of a rim corner, which is on the surface).
    let reach = (0..n).map(|i| dist(p(i), p((i + 1) % n))).fold(0.0, f64::max).max(1e-6);
    let index = (!surface.is_empty()).then(|| {
        let cell = surface_cell(surface).min(reach);
        (BoxIndex::new(surface, cell), cell)
    });
    let near = std::cell::RefCell::new(Vec::new());
    // The distance to the surface, capped at `reach`: looked up in boxes doubling from one cell
    // wide, until the nearest face found lies well inside the box (no face outside it can then be
    // nearer); the last box is the whole reach, where the cap applies.
    let off = |q: P| {
        let Some((index, cell)) = &index else { return 0.0 };
        let mut near = near.borrow_mut();
        let mut r = *cell;
        loop {
            let last = r >= reach;
            if last {
                r = reach;
            }
            index.query_into(sub(q, [r; 3]), [q[0] + r, q[1] + r, q[2] + r], &mut near);
            // A face whose box lies clearly further off than the nearest found so far cannot be
            // nearer (the margin is far above the rounding in either distance).
            let mut best = f64::INFINITY;
            for &t in near.iter() {
                let (lo, hi) = index.bounds[t as usize];
                let gap2: f64 = (0..3).map(|k| (lo[k] - q[k]).max(q[k] - hi[k]).max(0.0).powi(2)).sum();
                let bar = best * (1.0 + 1e-6) + 1e-9;
                if gap2 > bar * bar {
                    continue;
                }
                best = best.min(dist(q, closest_on_tri(q, &surface[t as usize])));
            }
            if last {
                return best.min(reach);
            }
            if best <= r * 0.999 {
                return best;
            }
            r *= 2.0;
        }
    };
    // A side's midpoint is a probe of every patch face on that side: its distance is kept.
    let side_off: Vec<std::cell::Cell<f64>> =
        if index.is_some() { (0..n * n).map(|_| std::cell::Cell::new(-1.0)).collect() } else { Vec::new() };
    let mid_off = |i: usize, j: usize| {
        let (i, j) = (i.min(j), i.max(j));
        let kept = side_off[i * n + j].get();
        if kept >= 0.0 {
            return kept;
        }
        let (x, y) = (p(i), p(j));
        let d = off([0, 1, 2].map(|q| (x[q] + y[q]) / 2.0));
        side_off[i * n + j].set(d);
        d
    };
    // Face (i, j, k)'s weight added to `base`, or None when that is sure not to come below `bar`
    // (a sum of terms none of which is below zero cannot come below the sum of some of them, in
    // rounded sums too, so the centroid's distance is only looked up when it can matter).
    let weighed = |i: usize, j: usize, k: usize, base: f64, bar: f64| -> Option<f64> {
        let a = 0.5 * len(cross(sub(p(j), p(i)), sub(p(k), p(i))));
        if surface.is_empty() {
            return Some(base + a);
        }
        let weight = |probes: [f64; 4]| {
            let d2: f64 = probes.iter().map(|&d| d.powi(2)).sum::<f64>() / 4.0;
            base + a * (AREA_WEIGHT_MM2 + d2)
        };
        let sides = [mid_off(i, j), mid_off(j, k), mid_off(k, i)];
        if weight([0.0, sides[0], sides[1], sides[2]]) >= bar {
            return None;
        }
        let (x, y, z) = (p(i), p(j), p(k));
        // The centroid and the three side midpoints (a midpoint is the same point whichever way
        // round its side is taken).
        Some(weight([off([0, 1, 2].map(|q| (x[q] + y[q] + z[q]) / 3.0)), sides[0], sides[1], sides[2]]))
    };
    let blocked = |i: usize, k: usize| {
        let (a, b) = (rim[i], rim[k]);
        k - i > 1 && !(i == 0 && k == n - 1) && kept_edges.contains(&(a.min(b), a.max(b)))
    };
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
                // Only a split that could come below the best so far is weighed in full.
                let base = cost[i * n + j] + cost[j * n + k];
                if base >= cost[i * n + k] {
                    continue;
                }
                let Some(c) = weighed(i, j, k, base, cost[i * n + k]) else { continue };
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

/// A cell for indexing `surface` for nearest-face lookups: twice its median side.
fn surface_cell(surface: &[[P; 3]]) -> f64 {
    let mut side: Vec<f64> = surface.iter().map(|t| dist(t[0], t[1])).collect();
    let k = side.len() / 2;
    side.select_nth_unstable_by(k, f64::total_cmp);
    (side[k] * 2.0).max(1e-6)
}

/// The kept faces (not removed, not `inside` the region) that the patch crosses, ascending, and
/// whether the patch crosses itself. Every kept face whose box meets a patch face's is tried
/// (`index` holds the mesh's faces), so faces near in space but far along the surface count.
fn crossed_near(
    m: &Indexed,
    index: &BoxIndex,
    inside: &FxHashSet<u32>,
    patch: &[[u32; 3]],
    removed: &[bool],
) -> (Vec<u32>, bool) {
    let mut near: Vec<u32> = Vec::new();
    let mut found = Vec::new();
    for t in patch {
        let p = t.map(|v| m.verts[v as usize]);
        let lo = [0, 1, 2].map(|k| p[0][k].min(p[1][k]).min(p[2][k]));
        let hi = [0, 1, 2].map(|k| p[0][k].max(p[1][k]).max(p[2][k]));
        index.query_into(lo, hi, &mut found);
        near.extend(found.iter().copied().filter(|&f| !removed[f as usize] && !inside.contains(&f)));
    }
    near.sort_unstable();
    near.dedup();
    let kept = near.len();
    let mut faces: Vec<[u32; 3]> = near.iter().map(|&f| m.faces[f as usize]).collect();
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
    let mut crossed = Vec::new();
    let mut itself = false;
    for (a, b) in super::intersect::pairs(&local) {
        match (a as usize >= kept, b as usize >= kept) {
            (true, true) => itself = true,
            (true, false) => crossed.push(near[b as usize]),
            (false, true) => crossed.push(near[a as usize]),
            (false, false) => {}
        }
    }
    crossed.sort_unstable();
    crossed.dedup();
    (crossed, itself)
}

/// Whether a face of patch `a` crosses one of patch `b` (faces of `m`).
fn patches_cross(m: &Indexed, a: &[[u32; 3]], b: &[[u32; 3]]) -> bool {
    let faces: Vec<[u32; 3]> = a.iter().chain(b).copied().collect();
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
    let n = a.len() as u32;
    super::intersect::pairs(&Indexed { verts, faces }).iter().any(|&(x, y)| (x < n) != (y < n))
}

/// The cell of a face index over `m`'s live faces: twice its median edge.
fn cell_of(m: &Indexed) -> f64 {
    let mut edge: Vec<f64> = m.faces.iter().map(|f| dist(m.verts[f[0] as usize], m.verts[f[1] as usize])).collect();
    if edge.is_empty() {
        return 1.0;
    }
    let k = edge.len() / 2;
    edge.select_nth_unstable_by(k, f64::total_cmp);
    (edge[k] * 2.0).max(1e-3)
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

    #[test]
    fn edge_faces_hold_two_in_place_and_more_in_a_list() {
        let mut e = EdgeFaces::default();
        assert!(e.is_empty());
        e.push(4);
        e.push(7);
        assert_eq!((&e[..], e.more.is_empty()), (&[4, 7][..], true));
        e.push(9);
        assert_eq!(&e[..], &[4, 7, 9]);
        e.remove(7);
        assert_eq!((&e[..], e.more.is_empty()), (&[4, 9][..], true));
        e.remove(5);
        e.remove(4);
        assert_eq!(&e[..], &[9]);
    }

    #[test]
    fn vertex_faces_move_a_corner_that_outgrows_its_run() {
        let faces = [[0, 1, 2], [0, 2, 3]];
        let mut v = VertexFaces::new(4, faces.iter().enumerate().map(|(f, t)| (f as u32, t)));
        assert_eq!((&v[0], &v[1], &v[3]), (&[0, 1][..], &[0][..], &[1][..]));
        v.remove(0, 0);
        v.push(0, 5);
        assert_eq!(&v[0], &[1, 5]);
        assert!(v.moved.is_empty());
        v.push(0, 6);
        assert_eq!((&v[0], &v[2]), (&[1, 5, 6][..], &[0, 1][..]));
        v.remove(0, 5);
        assert_eq!(&v[0], &[1, 6]);
    }

    /// Every list of `a` against those of `b`, leaving out edges no face has any more.
    fn same_adjacency(a: &Adjacency, b: &Adjacency, verts: usize) {
        let lists = |x: &Adjacency| {
            let mut l: Vec<((u32, u32), Vec<u32>)> =
                x.by_edge.iter().filter(|(_, f)| !f.is_empty()).map(|(k, f)| (*k, f.to_vec())).collect();
            l.sort_unstable();
            l
        };
        assert_eq!(lists(a), lists(b));
        for v in 0..verts {
            assert_eq!(&a.by_vertex[v], &b.by_vertex[v], "corner {v}");
        }
        assert_eq!(a.faces, b.faces);
    }

    #[test]
    fn adjacency_kept_up_to_date_is_what_a_rebuild_makes() {
        let mut m = ball();
        let mut removed = vec![false; m.faces.len()];
        let mut adj = Adjacency::new(&m, &removed);
        // A band of faces taken out, and faces round one corner added (more than its run holds).
        let gone: Vec<u32> = (300..340).collect();
        let hub = m.faces[320][0];
        let ring: Vec<u32> = m.faces[300..340].iter().flatten().copied().filter(|&v| v != hub).collect();
        let added: Vec<[u32; 3]> = ring.windows(2).map(|w| [hub, w[0], w[1]]).filter(|t| t[1] != t[2]).collect();
        for &f in &gone {
            removed[f as usize] = true;
        }
        adj.remove(&gone);
        adj.add(&added);
        m.faces.extend(&added);
        removed.resize(m.faces.len(), false);
        assert!(!adj.by_vertex.moved.is_empty());
        same_adjacency(&adj, &Adjacency::new(&m, &removed), m.verts.len());
        // Then some of the added faces taken out again.
        let again: Vec<u32> = (m.faces.len() as u32 - 5..m.faces.len() as u32).chain([10, 11]).collect();
        for &f in &again {
            removed[f as usize] = true;
        }
        adj.remove(&again);
        same_adjacency(&adj, &Adjacency::new(&m, &removed), m.verts.len());
    }

    #[test]
    fn filling_in_from_the_faces_last_added_is_filling_in_from_all() {
        let m = ball();
        let adj = Adjacency::new(&m, &vec![false; m.faces.len()]);
        // Scattered faces with gaps between them for the filling to close.
        let seed: FxHashSet<u32> = (0..m.faces.len() as u32).filter(|f| f % 7 == 0 || f % 11 == 0).collect();
        let mut quick = seed.clone();
        adj.fill_concave(&mut quick);
        let mut every = seed.clone();
        loop {
            let add: Vec<u32> = every
                .iter()
                .flat_map(|&f| adj.neighbours(f))
                .filter(|g| !every.contains(g))
                .filter(|&g| adj.neighbours(g).filter(|h| every.contains(h)).count() >= 2)
                .collect();
            if add.is_empty() {
                break;
            }
            every.extend(add);
        }
        assert!(quick.len() > seed.len());
        assert_eq!(quick, every);
    }

    /// Whether `out` is closed, manifold, consistently oriented and free of crossings.
    fn sound(out: &Indexed) {
        assert!(super::super::intersect::pairs(out).is_empty());
        let c = super::super::mesh::check(out, false);
        assert!(c.boundary_edges + c.non_manifold_edges + c.misoriented_edges == 0, "{c:?}");
        assert_eq!(c.shells, 1, "{c:?}");
    }

    #[test]
    fn a_crumpled_clump_is_mended_in_place_and_everything_else_kept() {
        let mut m = ball();
        // The corners round a point on the equator scattered by more than their spacing (about
        // 0.65 mm), so the faces between them fold through each other every which way.
        let c = [5.0, 0.0, 0.0];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut jitter = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 2001) as f64 / 1000.0 - 1.0
        };
        let near: Vec<usize> = (0..m.verts.len()).filter(|&v| dist(m.verts[v], c) < 1.6).collect();
        for &v in &near {
            let d = [0.4 * jitter(), 0.9 * jitter(), 0.9 * jitter()];
            m.verts[v] = [0, 1, 2].map(|k| m.verts[v][k] + d[k]);
        }
        let pairs = super::super::intersect::pairs(&m);
        assert!(pairs.len() > 20, "{} crossings", pairs.len());
        let (out, st) = mend(&m, &pairs);
        assert_eq!(st.left, 0, "{st:?}");
        sound(&out);
        // Every face away from the clump is kept as it was.
        let after: FxHashSet<[u32; 3]> = out.faces.iter().copied().collect();
        for f in &m.faces {
            if f.iter().all(|&v| dist(m.verts[v as usize], c) > 3.5) {
                assert!(after.contains(f), "{f:?} lost, {st:?}");
            }
        }
    }

    #[test]
    fn a_region_closing_off_a_bit_of_surface_takes_it_in() {
        let m = ball();
        let adj = Adjacency::new(&m, &vec![false; m.faces.len()]);
        // An annulus round one corner: its rim is two loops, the inner round the corner's faces.
        let v = (0..m.verts.len()).find(|&v| m.verts[v][2].abs() < 1e-9).unwrap() as u32;
        let star = adj.by_vertex[v as usize].to_vec();
        let star_set: FxHashSet<u32> = star.iter().copied().collect();
        let ring: Vec<u32> =
            adj.grow(&star, 3, &vec![false; m.faces.len()]).into_iter().filter(|f| !star_set.contains(f)).collect();
        let mut owner = vec![u32::MAX; m.faces.len()];
        for &f in &ring {
            owner[f as usize] = 0;
        }
        let (taken, rims) = close_off(&m, &adj, &ring, 0, &owner, &vec![false; m.faces.len()]).unwrap();
        assert_eq!(rims.len(), 1);
        assert_eq!(taken.len(), ring.len() + star.len());
    }

    #[test]
    fn a_region_ringing_the_shell_is_not_spanned() {
        let m = ball();
        let adj = Adjacency::new(&m, &vec![false; m.faces.len()]);
        // A belt round the equator: spanning both its loops would cut the ball in two.
        let belt: Vec<u32> =
            (0..m.faces.len() as u32).filter(|&f| m.tri(f as usize).iter().all(|p| p[2].abs() < 0.8)).collect();
        let mut owner = vec![u32::MAX; m.faces.len()];
        for &f in &belt {
            owner[f as usize] = 0;
        }
        assert!(
            rim_loops(&m, &adj, &belt, &belt.iter().copied().collect(), &vec![false; m.faces.len()]).unwrap().len()
                == 2
        );
        assert!(close_off(&m, &adj, &belt, 0, &owner, &vec![false; m.faces.len()]).is_none());
    }

    #[test]
    fn folds_far_apart_on_the_surface_but_crossing_in_space_are_mended_together() {
        let mut m = ball();
        // A corner on the equator pushed in through the ball and out past the far side: its
        // faces cross the far wall, two dozen rings of faces away along the surface.
        let v = (0..m.verts.len()).find(|&v| m.verts[v][2].abs() < 1e-9 && m.verts[v][0] > 4.9).unwrap();
        m.verts[v] = [-5.6, 0.05, 0.02];
        let pairs = super::super::intersect::pairs(&m);
        assert!(!pairs.is_empty());
        let (out, st) = mend(&m, &pairs);
        assert_eq!(st.left, 0, "{st:?}");
        sound(&out);
    }
}
