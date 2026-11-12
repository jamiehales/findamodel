//! An indexed mesh welded from a triangle soup, its edges, its shells and the checks that say
//! whether it is watertight: closed (no boundary edge), manifold (no edge on more than two
//! faces), consistently oriented (each edge crossed in opposite directions by its two faces).

use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::geometry::support_reader::math::{P, add, cross, dot, find, mul, sub};

/// Vertices and faces, welded by exact position.
#[derive(Debug, Clone, Default)]
pub struct Indexed {
    pub verts: Vec<P>,
    pub faces: Vec<[u32; 3]>,
}

impl Indexed {
    /// Welds corners at the same position (bit for bit, as STL writers repeat them) and drops
    /// faces with a repeated corner.
    pub fn weld(tris: &[[P; 3]]) -> Self {
        let mut at: FxHashMap<[u64; 3], u32> = FxHashMap::default();
        let mut verts = Vec::new();
        let mut faces = Vec::with_capacity(tris.len());
        for t in tris {
            let f = t.map(|v| {
                *at.entry(v.map(|x| (x + 0.0).to_bits())).or_insert_with(|| {
                    verts.push(v);
                    verts.len() as u32 - 1
                })
            });
            if f[0] != f[1] && f[1] != f[2] && f[2] != f[0] {
                faces.push(f);
            }
        }
        Indexed { verts, faces }
    }

    pub fn tris(&self) -> Vec<[P; 3]> {
        self.faces.par_iter().map(|f| f.map(|i| self.verts[i as usize])).collect()
    }

    pub fn tri(&self, f: usize) -> [P; 3] {
        self.faces[f].map(|i| self.verts[i as usize])
    }

    /// The faces' signed volume (positive for an outward-facing closed surface).
    pub fn volume_of(&self, faces: &[u32]) -> f64 {
        let Some(&first) = faces.first() else { return 0.0 };
        let o = self.verts[self.faces[first as usize][0] as usize];
        faces
            .iter()
            .map(|&f| {
                let [a, b, c] = self.tri(f as usize).map(|v| sub(v, o));
                dot(a, cross(b, c)) / 6.0
            })
            .sum()
    }

    pub fn area_of(&self, faces: &[u32]) -> f64 {
        faces
            .iter()
            .map(|&f| {
                let [a, b, c] = self.tri(f as usize);
                0.5 * crate::geometry::support_reader::math::len(cross(sub(b, a), sub(c, a)))
            })
            .sum()
    }
}

/// How each undirected edge is used.
#[derive(Debug, Clone, Default)]
pub struct Edges {
    /// Edges with one face.
    pub boundary: Vec<[u32; 2]>,
    /// Edges with more than two faces.
    pub non_manifold: Vec<[u32; 2]>,
    /// Edges whose two faces cross them in the same direction.
    pub misoriented: Vec<[u32; 2]>,
    /// Pairs of faces across each two-face edge, with whether they agree in orientation.
    pub pairs: Vec<(u32, u32, bool)>,
    /// Faces on a boundary or non-manifold edge.
    pub bad_faces: Vec<u32>,
}

pub fn edges(m: &Indexed) -> Edges {
    // Half-edges keyed by their undirected edge, with their face and direction.
    let mut half: Vec<(u64, u32, bool)> = m
        .faces
        .par_iter()
        .enumerate()
        .flat_map_iter(|(f, t)| {
            (0..3).map(move |k| {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = (u64::from(a.min(b)) << 32) | u64::from(a.max(b));
                (key, f as u32, a < b)
            })
        })
        .collect();
    half.par_sort_unstable();
    let mut e = Edges::default();
    let mut bad = Vec::new();
    let mut i = 0;
    while i < half.len() {
        let mut j = i + 1;
        while j < half.len() && half[j].0 == half[i].0 {
            j += 1;
        }
        let key = half[i].0;
        let edge = [(key >> 32) as u32, key as u32];
        match j - i {
            1 => {
                e.boundary.push(edge);
                bad.push(half[i].1);
            }
            2 => {
                let agree = half[i].2 != half[i + 1].2;
                if !agree {
                    e.misoriented.push(edge);
                }
                e.pairs.push((half[i].1, half[i + 1].1, agree));
            }
            _ => {
                e.non_manifold.push(edge);
                bad.extend(half[i..j].iter().map(|h| h.1));
            }
        }
        i = j;
    }
    bad.sort_unstable();
    bad.dedup();
    e.bad_faces = bad;
    e
}

/// Connected components of faces across two-face edges (a non-manifold edge does not join).
pub fn shells(m: &Indexed, e: &Edges) -> Vec<Vec<u32>> {
    let mut parent: Vec<u32> = (0..m.faces.len() as u32).collect();
    for &(a, b, _) in &e.pairs {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[ra.max(rb) as usize] = ra.min(rb);
        }
    }
    let mut by_root: FxHashMap<u32, usize> = FxHashMap::default();
    let mut out: Vec<Vec<u32>> = Vec::new();
    for f in 0..m.faces.len() as u32 {
        let r = find(&mut parent, f);
        let k = *by_root.entry(r).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[k].push(f);
    }
    out
}

/// Makes every shell consistently oriented where it can be: a breadth-first walk sets each face to
/// agree with its neighbours, and the shell then keeps the orientation most of its area had
/// (so a mostly right inner skin stays inward). Returns the number of faces flipped and the
/// number of shells that are not orientable.
pub fn orient(m: &mut Indexed) -> (usize, usize) {
    let e = edges(m);
    let mut adj: Vec<Vec<(u32, bool)>> = vec![Vec::new(); m.faces.len()];
    for &(a, b, agree) in &e.pairs {
        adj[a as usize].push((b, agree));
        adj[b as usize].push((a, agree));
    }
    let mut flip: Vec<Option<bool>> = vec![None; m.faces.len()];
    let mut flipped = 0;
    let mut non_orientable = 0;
    for s in 0..m.faces.len() {
        if flip[s].is_some() {
            continue;
        }
        flip[s] = Some(false);
        let mut queue = vec![s as u32];
        let mut members = Vec::new();
        let mut bad = false;
        while let Some(f) = queue.pop() {
            members.push(f);
            let ff = flip[f as usize].unwrap_or(false);
            for &(g, agree) in &adj[f as usize] {
                let want = if agree { ff } else { !ff };
                match flip[g as usize] {
                    None => {
                        flip[g as usize] = Some(want);
                        queue.push(g);
                    }
                    Some(have) if have != want => bad = true,
                    _ => {}
                }
            }
        }
        if bad {
            non_orientable += 1;
        }
        let flipped_area: f64 =
            members.iter().filter(|&&f| flip[f as usize] == Some(true)).map(|&f| m.area_of(&[f])).sum();
        if flipped_area * 2.0 > m.area_of(&members) {
            for &f in &members {
                flip[f as usize] = flip[f as usize].map(|x| !x);
            }
        }
    }
    for (f, fl) in flip.iter().enumerate() {
        if *fl == Some(true) {
            m.faces[f].swap(1, 2);
            flipped += 1;
        }
    }
    (flipped, non_orientable)
}

/// Flips the faces given.
pub fn flip(m: &mut Indexed, faces: &[u32]) {
    for &f in faces {
        m.faces[f as usize].swap(1, 2);
    }
}

/// Splits the corners of non-manifold edges: each fan of faces round such a corner (faces joined
/// across two-face edges) gets its own copy of it. Sheets that only touched along the edge come
/// apart, each closed if it was; returns the number of corners added.
pub fn split_non_manifold(m: &mut Indexed) -> usize {
    let e = edges(m);
    if e.non_manifold.is_empty() {
        return 0;
    }
    let mut count: FxHashMap<(u32, u32), u32> = FxHashMap::default();
    for t in &m.faces {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut corners: Vec<u32> = e.non_manifold.iter().flatten().copied().collect();
    corners.sort_unstable();
    corners.dedup();
    let mut around: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for (f, t) in m.faces.iter().enumerate() {
        for &v in t {
            if corners.binary_search(&v).is_ok() {
                around.entry(v).or_default().push(f as u32);
            }
        }
    }
    let mut added = 0;
    // Each corner's original, for counting faces on an edge whose far corner was split already.
    let mut orig: Vec<u32> = (0..m.verts.len() as u32).collect();
    for v in corners {
        let fs = &around[&v];
        let mut parent: Vec<u32> = (0..fs.len() as u32).collect();
        let mut by_other: FxHashMap<u32, u32> = FxHashMap::default();
        for (k, &f) in fs.iter().enumerate() {
            for &w in &m.faces[f as usize] {
                let w0 = orig[w as usize];
                if w == v || count.get(&(v.min(w0), v.max(w0))) != Some(&2) {
                    continue;
                }
                if let Some(&j) = by_other.get(&w) {
                    let (a, b) = (find(&mut parent, j), find(&mut parent, k as u32));
                    parent[a.max(b) as usize] = a.min(b);
                } else {
                    by_other.insert(w, k as u32);
                }
            }
        }
        let mut copy: FxHashMap<u32, u32> = FxHashMap::default();
        for (k, &f) in fs.iter().enumerate() {
            let root = find(&mut parent, k as u32);
            if root == 0 {
                continue;
            }
            let nv = *copy.entry(root).or_insert_with(|| {
                m.verts.push(m.verts[v as usize]);
                orig.push(v);
                added += 1;
                m.verts.len() as u32 - 1
            });
            for c in m.faces[f as usize].iter_mut() {
                if *c == v {
                    *c = nv;
                }
            }
        }
    }
    added
}

/// What [`close_holes`] did.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Closing {
    /// Corners of non-manifold edges split by fan ([`split_non_manifold`]).
    pub corners_split: usize,
    /// Faces on edges still with more than two faces after that, taken out (their holes are then
    /// filled).
    pub non_manifold_faces_removed: usize,
    pub holes_filled: usize,
    /// The longest hole filled, in edges.
    pub longest_hole: usize,
    /// Boundary edges left (loops that could not be walked).
    pub boundary_left: usize,
    /// Where each hole was, for showing the repairs.
    #[serde(skip)]
    pub hole_centres: Vec<P>,
}

/// Closes the mesh: corners of non-manifold edges are split by fan, faces still on non-manifold
/// edges come out, then every boundary loop is filled with a fan round its centre (a triangle for
/// a loop of three).
pub fn close_holes(m: &mut Indexed) -> Closing {
    let mut c = Closing { corners_split: split_non_manifold(m), ..Default::default() };
    let e = edges(m);
    if !e.non_manifold.is_empty() {
        let nm: rustc_hash::FxHashSet<[u32; 2]> = e.non_manifold.iter().copied().collect();
        let before = m.faces.len();
        m.faces.retain(|t| {
            !(0..3).any(|k| {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                nm.contains(&[a.min(b), a.max(b)])
            })
        });
        c.non_manifold_faces_removed = before - m.faces.len();
    }
    // Boundary half-edges, each walked the opposite way to its face.
    let mut count: FxHashMap<(u32, u32), i32> = FxHashMap::default();
    for t in &m.faces {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut next: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    let mut left = 0;
    for t in &m.faces {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if count[&(a.min(b), a.max(b))] == 1 {
                next.entry(b).or_default().push(a);
                left += 1;
            }
        }
    }
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    for s in starts {
        while next.get(&s).is_some_and(|v| !v.is_empty()) {
            let mut lp = vec![s];
            let mut at = s;
            let mut ok = false;
            while let Some(n) = next.get_mut(&at).and_then(|v| v.pop()) {
                if n == s {
                    ok = true;
                    break;
                }
                lp.push(n);
                at = n;
                if lp.len() > 1_000_000 {
                    break;
                }
            }
            if !ok || lp.len() < 3 {
                continue;
            }
            left -= lp.len();
            c.holes_filled += 1;
            let centre = lp.iter().fold([0.0; 3], |acc, &v| add(acc, m.verts[v as usize]));
            c.hole_centres.push(mul(centre, 1.0 / lp.len() as f64));
            c.longest_hole = c.longest_hole.max(lp.len());
            if lp.len() == 3 {
                m.faces.push([lp[0], lp[1], lp[2]]);
                continue;
            }
            let centre = lp.iter().fold([0.0; 3], |acc, &v| add(acc, m.verts[v as usize]));
            let centre = mul(centre, 1.0 / lp.len() as f64);
            m.verts.push(centre);
            let ci = m.verts.len() as u32 - 1;
            for k in 0..lp.len() {
                m.faces.push([lp[k], lp[(k + 1) % lp.len()], ci]);
            }
        }
    }
    c.boundary_left = left;
    c
}

/// A shell's summary.
#[derive(Debug, Clone)]
pub struct Shell {
    pub faces: Vec<u32>,
    pub volume: f64,
    pub area: f64,
    pub lo: P,
    pub hi: P,
    /// No boundary or non-manifold edge and no misoriented edge.
    pub closed: bool,
}

pub fn shell_summaries(m: &Indexed, e: &Edges) -> Vec<Shell> {
    let mut bad = vec![false; m.faces.len()];
    for &f in &e.bad_faces {
        bad[f as usize] = true;
    }
    for &(a, b, agree) in &e.pairs {
        if !agree {
            bad[a as usize] = true;
            bad[b as usize] = true;
        }
    }
    shells(m, e)
        .into_par_iter()
        .map(|faces| {
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for &f in &faces {
                for v in m.tri(f as usize) {
                    for k in 0..3 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                }
            }
            let closed = faces.iter().all(|&f| !bad[f as usize]);
            Shell { volume: m.volume_of(&faces), area: m.area_of(&faces), faces, lo, hi, closed }
        })
        .collect()
}

/// A watertightness report.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Check {
    pub triangles: usize,
    pub vertices: usize,
    pub shells: usize,
    pub boundary_edges: usize,
    pub non_manifold_edges: usize,
    pub misoriented_edges: usize,
    /// Vertices whose faces form more than one fan (shells touching at a point).
    pub pinched_vertices: usize,
    pub self_intersecting_pairs: usize,
    pub volume: f64,
    pub area: f64,
}

impl Check {
    pub fn watertight(&self) -> bool {
        self.boundary_edges == 0
            && self.non_manifold_edges == 0
            && self.misoriented_edges == 0
            && self.self_intersecting_pairs == 0
    }
}

pub fn check(m: &Indexed, with_intersections: bool) -> Check {
    let e = edges(m);
    let all: Vec<u32> = (0..m.faces.len() as u32).collect();
    Check {
        triangles: m.faces.len(),
        vertices: m.verts.len(),
        shells: shells(m, &e).len(),
        boundary_edges: e.boundary.len(),
        non_manifold_edges: e.non_manifold.len(),
        misoriented_edges: e.misoriented.len(),
        pinched_vertices: pinched(m),
        self_intersecting_pairs: if with_intersections { super::intersect::pairs(m).len() } else { 0 },
        volume: m.volume_of(&all),
        area: m.area_of(&all),
    }
}

/// Vertices whose surrounding faces split into more than one edge-connected fan.
fn pinched(m: &Indexed) -> usize {
    let mut around: Vec<Vec<u32>> = vec![Vec::new(); m.verts.len()];
    for (f, t) in m.faces.iter().enumerate() {
        for &v in t {
            around[v as usize].push(f as u32);
        }
    }
    around
        .par_iter()
        .enumerate()
        .filter(|(v, fs)| {
            if fs.len() < 2 {
                return false;
            }
            // Fans: faces around the vertex joined when they share the edge from it.
            let mut parent: Vec<u32> = (0..fs.len() as u32).collect();
            let mut other: FxHashMap<u32, u32> = FxHashMap::default();
            for (k, &f) in fs.iter().enumerate() {
                for &w in &m.faces[f as usize] {
                    if w as usize == *v {
                        continue;
                    }
                    if let Some(&j) = other.get(&w) {
                        let (a, b) = (find(&mut parent, j), find(&mut parent, k as u32));
                        parent[a.max(b) as usize] = a.min(b);
                    } else {
                        other.insert(w, k as u32);
                    }
                }
            }
            (0..fs.len() as u32).filter(|&k| find(&mut parent, k) == k).count() > 1
        })
        .count()
}

/// What [`finish`] did.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Finish {
    /// Edges under [`SHORT_EDGE_MM`] collapsed first.
    pub short_edges_collapsed: usize,
    /// Corners no face uses, dropped.
    pub merged_vertices: usize,
    /// Corners nudged by a single-precision step so they stay apart when rounded (as STL and
    /// VOXL store them).
    pub nudged_vertices: usize,
    pub degenerate_faces: usize,
    /// Pairs of faces on the same corners facing opposite ways (a membrane), removed.
    pub membrane_faces: usize,
    /// Shells with next to no volume and area, removed.
    pub specks: usize,
    /// Where membranes and specks were removed, for showing the repairs.
    #[serde(skip)]
    pub membrane_centres: Vec<P>,
    #[serde(skip)]
    pub speck_centres: Vec<P>,
}

/// Readies a repaired mesh for export: tiny edges collapsed, corners rounded to single precision
/// (kept apart), membranes (a face and its reverse) dropped, and specks (shells under
/// [`SPECK_MM3`] and [`SPECK_MM2`], and sheets) dropped.
pub fn finish(m: &Indexed) -> (Indexed, Finish) {
    let mut st = Finish::default();
    let mut m = m.clone();
    st.short_edges_collapsed = collapse_short_edges(&mut m, SHORT_EDGE_MM);
    // Unused corners go; the rest are rounded to single precision, nudged apart by a step where
    // two would land on one point (or already share one), so the topology survives a reader that
    // welds by position.
    let mut w = compact(&m);
    let before = w.verts.len();
    let mut taken: FxHashMap<[u32; 3], u32> = FxHashMap::default();
    for (i, v) in w.verts.iter_mut().enumerate() {
        let mut r = v.map(|x| x as f32);
        while let Some(&other) = taken.get(&r.map(f32::to_bits)) {
            if other == i as u32 {
                break;
            }
            r[0] = r[0].next_up();
            st.nudged_vertices += 1;
        }
        taken.insert(r.map(f32::to_bits), i as u32);
        *v = r.map(f64::from);
    }
    st.merged_vertices = m.verts.len().saturating_sub(before);
    st.degenerate_faces = m.faces.len().saturating_sub(w.faces.len());
    // A face and its reverse cancel.
    // Each face by its sorted corners, its orientation and its index.
    type Keyed = ([u32; 3], bool, u32);
    let mut keyed: Vec<Keyed> = w
        .faces
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut k = *f;
            k.sort_unstable();
            // The orientation: whether the face is an even permutation of its sorted corners.
            let even =
                (f[0] == k[0] && f[1] == k[1]) || (f[1] == k[0] && f[2] == k[1]) || (f[2] == k[0] && f[0] == k[1]);
            (k, even, i as u32)
        })
        .collect();
    keyed.sort_unstable();
    let mut drop = vec![false; w.faces.len()];
    let mut i = 0;
    while i < keyed.len() {
        let mut j = i + 1;
        while j < keyed.len() && keyed[j].0 == keyed[i].0 {
            j += 1;
        }
        let (even, odd): (Vec<&Keyed>, Vec<&Keyed>) = keyed[i..j].iter().partition(|x| x.1);
        for (a, b) in even.iter().zip(&odd) {
            st.membrane_centres.push(centre_of(&w, a.2 as usize));
            drop[a.2 as usize] = true;
            drop[b.2 as usize] = true;
            st.membrane_faces += 2;
        }
        // Repeats facing the same way: keep one.
        for x in even.iter().skip(odd.len() + 1).chain(odd.iter().skip(even.len() + 1)) {
            drop[x.2 as usize] = true;
        }
        i = j;
    }
    let mut k = 0;
    w.faces.retain(|_| {
        k += 1;
        !drop[k - 1]
    });
    let e = edges(&w);
    let shells = shell_summaries(&w, &e);
    let mut keep = vec![true; w.faces.len()];
    for s in &shells {
        if negligible(s.volume, s.area) {
            st.specks += 1;
            st.speck_centres.push(mul(add(s.lo, s.hi), 0.5));
            for &f in &s.faces {
                keep[f as usize] = false;
            }
        }
    }
    let mut k = 0;
    w.faces.retain(|_| {
        k += 1;
        keep[k - 1]
    });
    (w, st)
}

/// The mesh without corners no face uses (and faces with a repeated corner).
pub fn compact(m: &Indexed) -> Indexed {
    let mut at = vec![u32::MAX; m.verts.len()];
    let mut verts = Vec::new();
    let faces = m
        .faces
        .iter()
        .filter(|f| f[0] != f[1] && f[1] != f[2] && f[2] != f[0])
        .map(|f| {
            f.map(|v| {
                if at[v as usize] == u32::MAX {
                    at[v as usize] = verts.len() as u32;
                    verts.push(m.verts[v as usize]);
                }
                at[v as usize]
            })
        })
        .collect();
    Indexed { verts, faces }
}

/// Edges shorter than this (well under what single precision holds apart at model sizes) are
/// collapsed before rounding, so the rounding does not pinch the surface.
pub const SHORT_EDGE_MM: f64 = 1e-4;

/// Collapses edges shorter than `eps` to their midpoints where that keeps the surface manifold
/// (the ends share exactly the two corners across the edge). Returns the number collapsed.
pub fn collapse_short_edges(m: &mut Indexed, eps: f64) -> usize {
    let mut total = 0;
    for _ in 0..8 {
        let mut around: Vec<Vec<u32>> = vec![Vec::new(); m.verts.len()];
        for (f, t) in m.faces.iter().enumerate() {
            for &v in t {
                around[v as usize].push(f as u32);
            }
        }
        let ring = |v: u32, faces: &[[u32; 3]]| -> Vec<u32> {
            let mut r: Vec<u32> =
                around[v as usize].iter().flat_map(|&f| faces[f as usize]).filter(|&w| w != v).collect();
            r.sort_unstable();
            r.dedup();
            r
        };
        let mut touched = vec![false; m.verts.len()];
        let mut into: Vec<u32> = (0..m.verts.len() as u32).collect();
        let mut done = 0;
        for f in 0..m.faces.len() {
            for k in 0..3 {
                let (a, b) = (m.faces[f][k], m.faces[f][(k + 1) % 3]);
                if touched[a as usize] || touched[b as usize] {
                    continue;
                }
                let d = crate::geometry::support_reader::math::dist(m.verts[a as usize], m.verts[b as usize]);
                if d >= eps {
                    continue;
                }
                let (ra, rb) = (ring(a, &m.faces), ring(b, &m.faces));
                let common = ra.iter().filter(|v| rb.binary_search(v).is_ok()).count();
                if common != 2 || (ra.len() <= 3 && rb.len() <= 3) {
                    continue;
                }
                // Neither end nor any neighbour moves again this pass.
                for &w in ra.iter().chain(&rb).chain([a, b].iter()) {
                    touched[w as usize] = true;
                }
                let mid = mul(add(m.verts[a as usize], m.verts[b as usize]), 0.5);
                m.verts[a as usize] = mid;
                into[b as usize] = a;
                done += 1;
            }
        }
        if done == 0 {
            break;
        }
        total += done;
        for t in &mut m.faces {
            *t = t.map(|v| into[v as usize]);
        }
        m.faces.retain(|t| t[0] != t[1] && t[1] != t[2] && t[2] != t[0]);
    }
    total
}

/// Shells smaller than both of these are specks.
pub const SPECK_MM3: f64 = 1e-3;
pub const SPECK_MM2: f64 = 0.05;
/// A closed shell whose volume over area is under this (a plate about twice as thick) is a sheet
/// with no inside to print.
pub const SHEET_MM: f64 = 0.005;

/// Whether a shell of this volume and area is a speck or a sheet: nothing a printer makes.
pub fn negligible(volume: f64, area: f64) -> bool {
    (volume.abs() < SPECK_MM3 && area < SPECK_MM2) || volume.abs() < SHEET_MM * area
}

/// An axis-aligned box's faces, facing out.
pub fn box_tris(lo: P, hi: P) -> Vec<[P; 3]> {
    let c = |i: usize| {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ]
    };
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    quads.iter().flat_map(|q| [[c(q[0]), c(q[1]), c(q[2])], [c(q[0]), c(q[2]), c(q[3])]]).collect()
}

fn centre_of(m: &Indexed, f: usize) -> P {
    let t = m.tri(f);
    mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0)
}

/// A face's unit normal and centroid.
pub fn normal_and_centre(t: &[P; 3]) -> (P, P) {
    let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    let l = crate::geometry::support_reader::math::len(n).max(1e-30);
    (mul(n, 1.0 / l), mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn cube(lo: P, hi: P) -> Vec<[P; 3]> {
        box_tris(lo, hi)
    }

    #[test]
    fn a_cube_is_watertight_with_unit_volume() {
        let m = Indexed::weld(&cube([0.0; 3], [1.0; 3]));
        let c = check(&m, true);
        assert!(c.watertight(), "{c:?}");
        assert_eq!(c.shells, 1);
        assert!((c.volume - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_missing_face_and_a_flipped_face_are_found_and_the_flip_is_repaired() {
        let mut tris = cube([0.0; 3], [1.0; 3]);
        tris.pop();
        let c = check(&Indexed::weld(&tris), false);
        assert_eq!(c.boundary_edges, 3);
        let mut tris = cube([0.0; 3], [1.0; 3]);
        tris[0].swap(1, 2);
        let mut m = Indexed::weld(&tris);
        assert_eq!(check(&m, false).misoriented_edges, 3);
        assert_eq!(orient(&mut m), (1, 0));
        let c = check(&m, false);
        assert!(c.watertight() && (c.volume - 1.0).abs() < 1e-12, "{c:?}");
    }

    #[test]
    fn holes_are_filled_and_the_mesh_is_closed() {
        let mut tris = cube([0.0; 3], [1.0; 3]);
        tris.truncate(10);
        let mut m = Indexed::weld(&tris);
        let c = close_holes(&mut m);
        assert_eq!((c.holes_filled, c.boundary_left), (1, 0));
        let k = check(&m, true);
        assert!(k.watertight() && (k.volume - 1.0).abs() < 1e-12, "{k:?}");
    }

    #[test]
    fn finishing_drops_membranes_and_specks() {
        let mut tris = cube([0.0; 3], [1.0; 3]);
        let f = [[0.0, 0.0, 0.5], [1.0, 0.0, 0.5], [1.0, 1.0, 0.5]];
        tris.push(f);
        tris.push([f[0], f[2], f[1]]);
        tris.extend(cube([5.0; 3], [5.01; 3]));
        let (m, st) = finish(&Indexed::weld(&tris));
        assert_eq!((st.membrane_faces, st.specks), (2, 1));
        assert!(check(&m, true).watertight() && m.faces.len() == 12);
    }

    #[test]
    fn a_tiny_edge_collapses_and_the_mesh_stays_closed() {
        // A cube with one face split at a point a hair from a corner.
        let mut tris = cube([0.0; 3], [1.0; 3]);
        let t = tris.remove(0);
        let p = [t[0][0] + 1e-6, t[0][1] + 1e-6, t[0][2]];
        tris.extend([[t[0], t[1], p], [p, t[1], t[2]], [t[0], p, t[2]]]);
        let mut m = Indexed::weld(&tris);
        assert_eq!(collapse_short_edges(&mut m, 1e-4), 1);
        let c = check(&m, true);
        assert!(c.watertight() && (c.volume - 1.0).abs() < 1e-5, "{c:?}");
    }

    #[test]
    fn two_cubes_sharing_an_edge_come_apart_closed() {
        let mut tris = cube([0.0; 3], [1.0; 3]);
        tris.extend(cube([1.0, 1.0, 0.0], [2.0, 2.0, 1.0]));
        let mut m = Indexed::weld(&tris);
        assert_eq!(check(&m, false).non_manifold_edges, 1);
        let c = close_holes(&mut m);
        assert_eq!((c.corners_split, c.non_manifold_faces_removed, c.holes_filled), (2, 0, 0));
        let k = check(&m, false);
        assert!(k.watertight() && k.shells == 2 && (k.volume - 2.0).abs() < 1e-12, "{k:?}");
    }

    #[test]
    fn two_cubes_sharing_a_corner_are_pinched() {
        let mut tris = cube([0.0; 3], [1.0; 3]);
        tris.extend(cube([1.0; 3], [2.0; 3]));
        let c = check(&Indexed::weld(&tris), false);
        assert_eq!((c.shells, c.pinched_vertices, c.non_manifold_edges), (2, 1, 0));
    }
}
