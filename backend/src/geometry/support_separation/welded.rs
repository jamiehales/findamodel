//! Supports welded into the model: one closed shell holding both the model and its supports, as
//! netfabb-style exports union them. [`super::classify`] tries this when the mesh is a single
//! closed shell (where the rules by component find nothing to split).
//!
//! The split cuts along the mesh's own edges, so it is a labelling of the original triangles, the
//! per-triangle support mask every other split gives too ([`classify`]):
//!
//! 1. Coplanar triangles are joined into facets ([`PLANE_TOL_MM`]), and rings of facets around an
//!    axis (at least [`MIN_RING`] distinct normals, radius [`MIN_RING_RADIUS_MM`] to
//!    [`MAX_RING_RADIUS_MM`]) are the supports' rods, cones and balls. Coarse narrow facets and
//!    everything below the raft are support too.
//! 2. Small regions left between support (under [`FILL_SHARE`] of the model's surface) and facets
//!    mostly bounded by support are filled in; support regions that do not reach the plate go back
//!    to the model (rivets, pins).
//! 3. The split is kept only when the model stands above the raft, has at least [`MIN_LOOPS`]
//!    contacts (seam loops), none wider than [`MAX_LOOP_MM`], and the supports have a footprint on
//!    the plate of at least [`MIN_FOOTPRINT_MM2`].
//!
//! Both sides of such a split are open along the seams. The mask is all that is stored and sent
//! (the viewer shows the open sides as they are); code that needs closed pieces (the support
//! reader, the watertight repair) rebuilds the caps from the mask and the mesh with
//! [`closed_split`].

use rustc_hash::FxHashMap;

use super::Classification;
use crate::geometry::Triangle;
use crate::geometry::support_reader::math::{P, add, cross, dot, len, mul, solve3, sub, unit};

/// Tolerance on a facet's angle to a ring's axis (`n.d`), as the reader's union split.
const TOL_S: f64 = 2e-3;
/// Tolerance on a facet's offset from a ring's axis (mm).
const TOL_K: f64 = 2e-3;
/// A triangle joins a facet when its corners are this close to the facet's plane (mm).
const PLANE_TOL_MM: f64 = 1e-3;
/// Fewest distinct facet normals for a ring.
pub const MIN_RING: usize = 5;
/// Largest ring radius (mm) read as a support's.
pub const MAX_RING_RADIUS_MM: f64 = 3.0;
/// Smallest ring radius (mm) read as a support's: finer rings are sculpt detail or noise.
pub const MIN_RING_RADIUS_MM: f64 = 0.1;
/// Facets with more neighbours than this are not tried as a ring's middle facet (a large flat
/// face bordered by many sculpt triangles), which keeps the search linear.
const MAX_FACET_NEIGHBOURS: usize = 12;

/// The mesh welded: corners as f64 and vertex ids.
pub struct Mesh {
    pub tv: Vec<[P; 3]>,
    pub ids: Vec<[u32; 3]>,
    pub pos: Vec<P>,
}

impl Mesh {
    pub fn new(triangles: &[Triangle]) -> Self {
        let mut seen: FxHashMap<[i64; 3], u32> = FxHashMap::default();
        let mut tv = Vec::with_capacity(triangles.len());
        let mut ids = Vec::with_capacity(triangles.len());
        let mut pos = Vec::new();
        for t in triangles {
            let c = [t.v0, t.v1, t.v2].map(|v| [f64::from(v.x), f64::from(v.y), f64::from(v.z)]);
            let id = c.map(|v| {
                let k = v.map(|x| (x * 1e5).round() as i64);
                *seen.entry(k).or_insert_with(|| {
                    pos.push(v);
                    (pos.len() - 1) as u32
                })
            });
            tv.push(c);
            ids.push(id);
        }
        Mesh { tv, ids, pos }
    }

    /// Triangles on each undirected edge.
    pub fn edges(&self) -> FxHashMap<(u32, u32), Vec<u32>> {
        let mut em: FxHashMap<(u32, u32), Vec<u32>> = FxHashMap::default();
        em.reserve(self.ids.len() * 3 / 2);
        for (i, t) in self.ids.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                em.entry((a.min(b), a.max(b))).or_default().push(i as u32);
            }
        }
        em
    }
}

pub struct Facet {
    pub tris: Vec<u32>,
    pub n: P,
    pub h: f64,
    pub area: f64,
    pub c: P,
}

/// Coplanar triangles joined across edges, with each triangle's facet and the facets' neighbours
/// (shared edges as vertex pairs).
pub struct Facets {
    pub facets: Vec<Facet>,
    pub of: Vec<u32>,
    pub nb: FxHashMap<(u32, u32), Vec<(u32, u32)>>,
    pub adj: Vec<Vec<u32>>,
}

pub fn facets(m: &Mesh, em: &FxHashMap<(u32, u32), Vec<u32>>) -> Facets {
    let nt = m.tv.len();
    let mut tn = Vec::with_capacity(nt);
    let mut ta = Vec::with_capacity(nt);
    let mut tc = Vec::with_capacity(nt);
    for t in &m.tv {
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        ta.push(0.5 * len(n));
        tn.push(if len(n) > 0.0 { unit(n) } else { [0.0; 3] });
        tc.push(mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0));
    }
    // Grown from the largest triangles, whose planes are the most exact: a neighbour joins when
    // its corners lie on the seed's plane.
    let mut tadj: Vec<[u32; 3]> = vec![[u32::MAX; 3]; nt];
    for (&(a, b), l) in em {
        if l.len() == 2 {
            for (x, y) in [(l[0], l[1]), (l[1], l[0])] {
                let t = m.ids[x as usize];
                let k = (0..3).find(|&k| {
                    let (p, q) = (t[k], t[(k + 1) % 3]);
                    (p.min(q), p.max(q)) == (a, b)
                });
                if let Some(k) = k {
                    tadj[x as usize][k] = y;
                }
            }
        }
    }
    let mut order: Vec<u32> = (0..nt as u32).collect();
    order.sort_by(|&x, &y| ta[y as usize].total_cmp(&ta[x as usize]).then(x.cmp(&y)));
    let mut of = vec![u32::MAX; nt];
    let mut facets: Vec<Facet> = Vec::new();
    let mut stack = Vec::new();
    for &seed in &order {
        if of[seed as usize] != u32::MAX {
            continue;
        }
        let id = facets.len() as u32;
        let (n, h) = (tn[seed as usize], dot(tn[seed as usize], tc[seed as usize]));
        let mut tris = vec![seed];
        of[seed as usize] = id;
        if ta[seed as usize] > 1e-12 {
            stack.push(seed);
            while let Some(t) = stack.pop() {
                for &o in &tadj[t as usize] {
                    if o == u32::MAX || of[o as usize] != u32::MAX {
                        continue;
                    }
                    if m.tv[o as usize].iter().all(|v| (dot(n, *v) - h).abs() < PLANE_TOL_MM) {
                        of[o as usize] = id;
                        tris.push(o);
                        stack.push(o);
                    }
                }
            }
        }
        facets.push(Facet { tris, n: [0.0; 3], h: 0.0, area: 0.0, c: [0.0; 3] });
    }
    for f in facets.iter_mut() {
        let (mut n, mut c, mut a) = ([0.0; 3], [0.0; 3], 0.0);
        for &i in &f.tris {
            let i = i as usize;
            n = add(n, mul(tn[i], ta[i]));
            c = add(c, mul(tc[i], ta[i]));
            a += ta[i];
        }
        f.n = if len(n) > 0.0 { unit(n) } else { [0.0; 3] };
        f.c = if a > 0.0 { mul(c, 1.0 / a) } else { tc[f.tris[0] as usize] };
        f.area = a;
        f.h = dot(f.n, f.c);
    }
    let mut nb: FxHashMap<(u32, u32), Vec<(u32, u32)>> = FxHashMap::default();
    for (&(a, b), l) in em {
        if l.len() != 2 {
            continue;
        }
        let (fa, fb) = (of[l[0] as usize], of[l[1] as usize]);
        if fa != fb {
            nb.entry((fa.min(fb), fa.max(fb))).or_default().push((a, b));
        }
    }
    let mut adj: Vec<Vec<u32>> = vec![vec![]; facets.len()];
    for &(a, b) in nb.keys() {
        adj[a as usize].push(b);
        adj[b as usize].push(a);
    }
    for l in adj.iter_mut() {
        l.sort_unstable();
    }
    Facets { facets, of, nb, adj }
}

/// A ring of facets around an axis: a rod's, a cone's or a band of a ball's.
pub struct Ring {
    pub c: P,
    pub d: P,
    pub s: f64,
    pub k: f64,
    pub facets: Vec<u32>,
    pub radius: f64,
    pub distinct: usize,
}

/// Finds the rings of facets: three facets in a row give an axis, and the facets around it that
/// fit it, joined across edges in planes through the axis, are its ring.
pub fn rings(m: &Mesh, f: &Facets) -> Vec<Ring> {
    let nf = f.facets.len();
    let fs = &f.facets;
    let meridian = |c: P, d: P, a: u32, b: u32| -> bool {
        let key = (a.min(b), a.max(b));
        f.nb[&key].iter().all(|&(x, y)| {
            let (p, q) = (m.pos[x as usize], m.pos[y as usize]);
            let l = len(sub(q, p));
            l < 1e-9 || (dot(cross(sub(p, c), sub(q, c)), d) / l).abs() < 2e-3
        })
    };
    let fits = |q: &Facet, c: P, d: P, s: f64, k: f64| -> bool {
        if (dot(q.n, d) - s).abs() > TOL_S || (dot(q.n, c) - q.h - k).abs() > TOL_K {
            return false;
        }
        let u = sub(q.n, mul(d, s));
        let w = sub(q.c, c);
        let w = sub(w, mul(d, dot(w, d)));
        len(u) > 1e-3 && dot(u, w) > 0.0
    };
    let mut in_ring = vec![false; nf];
    let mut out = Vec::new();
    for g in 0..nf {
        if in_ring[g] || fs[g].area < 1e-9 || f.adj[g].len() > MAX_FACET_NEIGHBOURS {
            continue;
        }
        let ng = &f.adj[g];
        'pairs: for x in 0..ng.len() {
            for y in x + 1..ng.len() {
                let (fa, fh) = (ng[x] as usize, ng[y] as usize);
                if in_ring[fa] || in_ring[fh] {
                    continue;
                }
                let (a, b) = (sub(fs[g].n, fs[fa].n), sub(fs[fh].n, fs[fa].n));
                if len(a) < 1e-2 || len(b) < 1e-2 || len(sub(fs[fh].n, fs[g].n)) < 1e-2 {
                    continue;
                }
                let dd = cross(a, b);
                if len(dd) < 1e-5 {
                    continue;
                }
                let d = unit(dd);
                let rhs = [fs[g].h - fs[fa].h, fs[fh].h - fs[fa].h, dot(d, fs[g].c)];
                let Some(c) = solve3([a, b, d], rhs) else { continue };
                let s = dot(fs[fa].n, d);
                let k = dot(fs[fa].n, c) - fs[fa].h;
                if ![fa, g, fh].iter().all(|&q| fits(&fs[q], c, d, s, k)) {
                    continue;
                }
                if !meridian(c, d, fa as u32, g as u32) || !meridian(c, d, fh as u32, g as u32) {
                    continue;
                }
                // Grow the ring across meridian edges.
                let mut mem = vec![fa as u32, g as u32, fh as u32];
                let mut stack = mem.clone();
                let mut seen: rustc_hash::FxHashSet<u32> = mem.iter().copied().collect();
                while let Some(q) = stack.pop() {
                    for &o in &f.adj[q as usize] {
                        if seen.contains(&o) || in_ring[o as usize] {
                            continue;
                        }
                        seen.insert(o);
                        if fits(&fs[o as usize], c, d, s, k) && meridian(c, d, q, o) {
                            mem.push(o);
                            stack.push(o);
                        }
                    }
                }
                let mut ns: Vec<P> = Vec::new();
                for &q in &mem {
                    if !ns.iter().any(|n| dot(*n, fs[q as usize].n) > 0.999) {
                        ns.push(fs[q as usize].n);
                    }
                }
                if ns.len() < MIN_RING {
                    continue;
                }
                // Radius: the farthest corner from the axis.
                let mut r = 0.0f64;
                for &q in &mem {
                    for &i in &fs[q as usize].tris {
                        for v in &m.tv[i as usize] {
                            let w = sub(*v, c);
                            r = r.max(len(sub(w, mul(d, dot(w, d)))));
                        }
                    }
                }
                if !(MIN_RING_RADIUS_MM..=MAX_RING_RADIUS_MM).contains(&r) {
                    continue;
                }
                for &q in &mem {
                    in_ring[q as usize] = true;
                }
                out.push(Ring { c, d, s, k, facets: mem, radius: r, distinct: ns.len() });
                break 'pairs;
            }
        }
    }
    out
}

/// Guards and measures of a welded split.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub ring_triangles: usize,
    pub support_triangles: usize,
    pub loops: usize,
    pub largest_loop_mm: f64,
    pub model_bottom_mm: f64,
    pub footprint_mm2: f64,
    pub verdict: &'static str,
    /// The cross-section at the raft's top the pre-check read ([`plausible`]).
    pub slice: Slice,
}

/// A seam loop: directed edges (a, b) as the model's triangles run them.
pub type Loop = Vec<(u32, u32)>;

/// Largest seam loop (mm across) read as a support's contact.
pub const MAX_LOOP_MM: f64 = 3.0;
/// Fewest contacts for welded supports.
pub const MIN_LOOPS: usize = 3;
/// Least support footprint on the plate (mm²): pads or a raft, not a spike's point.
pub const MIN_FOOTPRINT_MM2: f64 = 2.0;
/// Non-ring regions under this share of the model's surface are support (joints, slivers, caps,
/// the raft).
const FILL_SHARE: f64 = 0.05;
/// How far a support's closing cone runs into the model (mm).
pub const TIP_DEPTH_MM: f64 = 0.1;
/// How far the support side's seam corners are moved into the model (mm).
pub const SEAM_SHIFT_MM: f64 = 0.02;
/// A facet at least this large (mm²) with this share of its edge on support is support.
const ABSORB_MIN_AREA_MM2: f64 = 0.01;
const ABSORB_SHARE: f64 = 0.75;
/// ... or a facet with an edge this many median edges long (a sliver of a rod face).
const ABSORB_EDGE_MEDIANS: f64 = 5.0;
/// A facet this large (mm²) with an edge this many median edges long is a support face.
const COARSE_MIN_AREA_MM2: f64 = 0.05;
const COARSE_EDGE_MEDIANS: f64 = 10.0;
/// ... and no wider than a support rod (mm): a model's wide flat face is not one.
const COARSE_MAX_WIDTH_MM: f64 = 1.5;

fn tri_area(t: &[P; 3]) -> f64 {
    0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0])))
}

/// The mesh's cross-section at the raft's top, for the pre-check ([`plausible`]).
#[derive(Debug, Clone, Default)]
pub struct Slice {
    /// Triangles crossing the raft's top.
    pub crossing: usize,
    /// Cross-sections: the crossing triangles joined across the edges that cross.
    pub sections: usize,
    /// The widest cross-section's extent across the plate (mm).
    pub widest_mm: f64,
    /// The most triangles one cross-section crosses.
    pub most_triangles: usize,
    /// Surface lying on the plate (mm²): triangles wholly within [`FOOTPRINT_MM`] of it.
    pub footprint_mm2: f64,
}

/// A face this close to the plate (mm) is on it, as the footprint guard of [`split`] measures it.
const FOOTPRINT_MM: f64 = 0.05;
/// Widest cross-section at the raft's top (mm) that welded supports show: a rod is at most
/// [`MAX_RING_RADIUS_MM`] in radius, and the guard models' sections through a model standing on
/// the plate (a base, feet) are mostly wider. The Hussars' widest is 3.9 mm (raft 2 or 3 mm).
const PRECHECK_MAX_WIDTH_MM: f64 = 8.0;
/// Most triangles one cross-section at the raft's top crosses for welded supports: a rod's ring
/// is a few dozen triangles, a sculpt standing on the plate crosses hundreds. The Hussars' most
/// is 213.
const PRECHECK_MAX_TRIANGLES: usize = 400;

/// Cuts the mesh at the raft's top (Y up): one pass over the triangles, hashing only the edges
/// that cross it.
pub fn slice(triangles: &[Triangle], raft_height_mm: f32) -> Slice {
    let raft = f64::from(raft_height_mm);
    let p = |v: crate::geometry::Vec3| [f64::from(v.x), f64::from(v.y), f64::from(v.z)];
    let mut s = Slice::default();
    // Per crossing edge (welded corner keys): the first crossing triangle on it.
    let mut edges: FxHashMap<([i64; 3], [i64; 3]), u32> = FxHashMap::default();
    let mut parent: Vec<u32> = Vec::new();
    // Per crossing triangle: where its crossing edges meet the raft's top (x, z).
    let mut points: Vec<[[f64; 2]; 2]> = Vec::new();
    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    }
    for t in triangles {
        let c = [t.v0, t.v1, t.v2];
        let y = c.map(|v| f64::from(v.y));
        if y.iter().all(|&y| y < FOOTPRINT_MM) {
            s.footprint_mm2 += tri_area(&c.map(p));
        }
        let below = y.map(|y| y <= raft);
        if below.iter().all(|&b| b) || below.iter().all(|&b| !b) {
            continue;
        }
        let id = parent.len() as u32;
        parent.push(id);
        let mut at = [[0.0; 2]; 2];
        let mut n = 0;
        for k in 0..3 {
            let (a, b) = (k, (k + 1) % 3);
            if below[a] == below[b] {
                continue;
            }
            let (pa, pb) = (p(c[a]), p(c[b]));
            let f = (raft - pa[1]) / (pb[1] - pa[1]);
            if n < 2 {
                at[n] = [pa[0] + f * (pb[0] - pa[0]), pa[2] + f * (pb[2] - pa[2])];
                n += 1;
            }
            let (ka, kb) = (corner_key(c[a]), corner_key(c[b]));
            let key = if ka <= kb { (ka, kb) } else { (kb, ka) };
            let other = *edges.entry(key).or_insert(id);
            let (ra, rb) = (find(&mut parent, id), find(&mut parent, other));
            if ra != rb {
                parent[rb as usize] = ra;
            }
        }
        points.push(at);
    }
    s.crossing = parent.len();
    let mut sections: FxHashMap<u32, (usize, [f64; 4])> = FxHashMap::default();
    for (i, at) in points.iter().enumerate() {
        let root = find(&mut parent, i as u32);
        let e = sections.entry(root).or_insert((0, [f64::MAX, f64::MAX, f64::MIN, f64::MIN]));
        e.0 += 1;
        for q in at {
            e.1 = [e.1[0].min(q[0]), e.1[1].min(q[1]), e.1[2].max(q[0]), e.1[3].max(q[1])];
        }
    }
    s.sections = sections.len();
    for (n, b) in sections.values() {
        s.most_triangles = s.most_triangles.max(*n);
        s.widest_mm = s.widest_mm.max((b[2] - b[0]).hypot(b[3] - b[1]));
    }
    s
}

/// The pre-check: whether the mesh could hold welded supports, from its cross-section at the
/// raft's top alone (see [`slice`]), so most models exit before the costly split. Exact:
/// supports reach the plate and the model stands above the raft, so something crosses the raft's
/// top, and everything on the plate is support, so the footprint guard of [`split`] holds for
/// all of it. Empirical (with margins over the Hussars and checked on the guard sets): every
/// cross-section is as narrow and as coarse as a support's.
pub fn plausible(s: &Slice) -> Result<(), &'static str> {
    if s.crossing == 0 {
        Err("nothing crosses the raft")
    } else if s.footprint_mm2 < MIN_FOOTPRINT_MM2 {
        Err("no footprint")
    } else if s.widest_mm > PRECHECK_MAX_WIDTH_MM {
        Err("wide section at the raft")
    } else if s.most_triangles > PRECHECK_MAX_TRIANGLES {
        Err("sculpted section at the raft")
    } else {
        Ok(())
    }
}

/// Splits one closed shell with welded supports (Y up, base at 0). Returns the support mask (None
/// when a guard fails) and the report.
pub fn split(triangles: &[Triangle], raft_height_mm: f32) -> (Option<Vec<bool>>, Report) {
    let slice = slice(triangles, raft_height_mm);
    if let Err(verdict) = plausible(&slice) {
        return (None, Report { slice, verdict, ..Report::default() });
    }
    let m = Mesh::new(triangles);
    let em = m.edges();
    let f = facets(&m, &em);
    let rs = rings(&m, &f);
    let nt = m.tv.len();
    let mut rep = Report { slice, ..Report::default() };
    let raft = f64::from(raft_height_mm);
    let mut sup = vec![false; nt];
    for r in &rs {
        for &q in &r.facets {
            for &i in &f.facets[q as usize].tris {
                sup[i as usize] = true;
            }
        }
    }
    rep.ring_triangles = sup.iter().filter(|s| **s).count();
    let widths: Vec<f64> = f.facets.iter().map(|fc| facet_width(&m, fc)).collect();
    // Coarse facets: far larger than the sculpt's triangles.
    {
        let edge_len = |t: &[P; 3]| (0..3).map(|k| len(sub(t[k], t[(k + 1) % 3]))).fold(0.0, f64::max);
        let mut lens: Vec<f64> = m.tv.iter().map(|t| len(sub(t[0], t[1]))).collect();
        let mid = lens.len() / 2;
        let median = if lens.is_empty() { 0.0 } else { *lens.select_nth_unstable_by(mid, f64::total_cmp).1 };
        for (q, fc) in f.facets.iter().enumerate() {
            let longest = fc.tris.iter().map(|&i| edge_len(&m.tv[i as usize])).fold(0.0, f64::max);
            if fc.area >= COARSE_MIN_AREA_MM2
                && longest >= COARSE_EDGE_MEDIANS * median
                && widths[q] <= COARSE_MAX_WIDTH_MM
            {
                for &i in &fc.tris {
                    sup[i as usize] = true;
                }
            }
        }
    }
    // The raft: everything below its top.
    for (s, t) in sup.iter_mut().zip(&m.tv) {
        if t.iter().all(|v| v[1] <= raft) {
            *s = true;
        }
    }
    let mut tadj: Vec<Vec<u32>> = vec![Vec::new(); nt];
    for l in em.values() {
        if l.len() == 2 {
            tadj[l[0] as usize].push(l[1]);
            tadj[l[1] as usize].push(l[0]);
        }
    }
    let regions = |mask: &[bool], want: bool| -> (Vec<u32>, Vec<f64>) {
        let mut id = vec![u32::MAX; nt];
        let mut area = Vec::new();
        let mut stack = Vec::new();
        for s0 in 0..nt {
            if mask[s0] != want || id[s0] != u32::MAX {
                continue;
            }
            let k = area.len() as u32;
            let mut a = 0.0;
            id[s0] = k;
            stack.push(s0 as u32);
            while let Some(t) = stack.pop() {
                a += tri_area(&m.tv[t as usize]);
                for &o in &tadj[t as usize] {
                    if mask[o as usize] == want && id[o as usize] == u32::MAX {
                        id[o as usize] = k;
                        stack.push(o);
                    }
                }
            }
            area.push(a);
        }
        (id, area)
    };
    let (mid, marea) = regions(&sup, false);
    let Some(body) = (0..marea.len()).max_by(|&x, &y| marea[x].total_cmp(&marea[y])) else {
        rep.verdict = "all rings";
        return (None, rep);
    };
    for t in 0..nt {
        if !sup[t] && mid[t] as usize != body && marea[mid[t] as usize] < FILL_SHARE * marea[body] {
            sup[t] = true;
        }
    }
    // Facets mostly bounded by support are support: rod faces the rings missed.
    let fs = &f.facets;
    let edge_len = |t: &[P; 3]| (0..3).map(|k| len(sub(t[k], t[(k + 1) % 3]))).fold(0.0, f64::max);
    let mut lens: Vec<f64> = m.tv.iter().map(|t| len(sub(t[0], t[1]))).collect();
    let mid = lens.len() / 2;
    let median_edge = if lens.is_empty() { 0.0 } else { *lens.select_nth_unstable_by(mid, f64::total_cmp).1 };
    let longest: Vec<f64> =
        fs.iter().map(|fc| fc.tris.iter().map(|&i| edge_len(&m.tv[i as usize])).fold(0.0, f64::max)).collect();
    for _ in 0..8 {
        let mut changed = false;
        for (q, fc) in fs.iter().enumerate() {
            let s0 = sup[fc.tris[0] as usize];
            if s0
                || widths[q] > COARSE_MAX_WIDTH_MM
                || (fc.area < ABSORB_MIN_AREA_MM2 && longest[q] < ABSORB_EDGE_MEDIANS * median_edge)
            {
                continue;
            }
            let (mut on, mut all) = (0.0, 0.0);
            for &o in &f.adj[q] {
                let key = ((q as u32).min(o), (q as u32).max(o));
                let l: f64 = f.nb[&key].iter().map(|&(x, y)| len(sub(m.pos[x as usize], m.pos[y as usize]))).sum();
                all += l;
                if sup[fs[o as usize].tris[0] as usize] {
                    on += l;
                }
            }
            if all > 0.0 && on >= ABSORB_SHARE * all {
                for &i in &fc.tris {
                    sup[i as usize] = true;
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Support regions that do not reach the plate are the sculpt's (rivets, pipes).
    let (sid, sarea) = regions(&sup, true);
    let mut grounded = vec![false; sarea.len()];
    for t in 0..nt {
        if sup[t] && m.tv[t].iter().all(|v| v[1] <= raft) {
            grounded[sid[t] as usize] = true;
        }
    }
    for t in 0..nt {
        if sup[t] && !grounded[sid[t] as usize] {
            sup[t] = false;
        }
    }
    rep.support_triangles = sup.iter().filter(|s| **s).count();
    for (tv, &s) in m.tv.iter().zip(&sup) {
        if s && tv.iter().all(|v| v[1] < 0.05) {
            rep.footprint_mm2 += tri_area(tv);
        }
    }
    rep.model_bottom_mm =
        (0..nt).filter(|&t| !sup[t]).flat_map(|t| m.tv[t].iter().map(|v| v[1])).fold(f64::MAX, f64::min);
    let loops: Vec<Loop> = seam_loops(&m, &em, &sup).into_iter().map(|(lp, _)| lp).collect();
    rep.loops = loops.len();
    for lp in &loops {
        let pts: Vec<P> = lp.iter().map(|e| m.pos[e.0 as usize]).collect();
        let mut w = 0.0f64;
        for p in &pts {
            for q in &pts {
                w = w.max(len(sub(*p, *q)));
            }
        }
        rep.largest_loop_mm = rep.largest_loop_mm.max(w);
    }
    rep.verdict = if rep.support_triangles == 0 {
        "no supports"
    } else if rep.model_bottom_mm <= raft {
        "model on the plate"
    } else if rep.loops < MIN_LOOPS {
        "too few contacts"
    } else if rep.largest_loop_mm > MAX_LOOP_MM {
        "contact too large"
    } else if rep.footprint_mm2 < MIN_FOOTPRINT_MM2 {
        "no footprint"
    } else {
        "split"
    };
    let mask = (rep.verdict == "split").then_some(sup);
    (mask, rep)
}

/// The seam loops of a labelling: the model side's directed edges (a, b), as its triangles run
/// them, where the triangle across the edge is support, chained into loops, each with whether it
/// closes. Built in triangle order and started from the lowest vertex id, so the same mesh and
/// mask always give the same loops.
fn seam_loops(m: &Mesh, em: &FxHashMap<(u32, u32), Vec<u32>>, sup: &[bool]) -> Vec<(Loop, bool)> {
    let mut next: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for (t, ids) in m.ids.iter().enumerate() {
        if sup[t] {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (ids[k], ids[(k + 1) % 3]);
            if a == b {
                continue;
            }
            let Some(l) = em.get(&(a.min(b), a.max(b))) else { continue };
            if l.len() == 2 && l.iter().any(|&o| o as usize != t && sup[o as usize]) {
                next.entry(a).or_default().push(b);
            }
        }
    }
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    let mut loops = Vec::new();
    for s0 in starts {
        while let Some(b) = next.get_mut(&s0).and_then(Vec::pop) {
            let mut lp = vec![(s0, b)];
            let mut cur = b;
            let mut closed = true;
            while cur != s0 {
                let Some(n) = next.get_mut(&cur).and_then(Vec::pop) else {
                    closed = false;
                    break;
                };
                lp.push((cur, n));
                cur = n;
            }
            loops.push((lp, closed));
        }
    }
    loops
}

/// The welding key of a corner, as [`Mesh::new`] welds it.
fn corner_key(v: crate::geometry::Vec3) -> [i64; 3] {
    [v.x, v.y, v.z].map(|x| (f64::from(x) * 1e5).round() as i64)
}

/// Whether any model triangle shares a corner with a support triangle: only then can the mask cut
/// across edges. Splits by component never do, so they skip the welding.
fn shares_corners(triangles: &[Triangle], is_support: &[bool]) -> bool {
    let supports = is_support.iter().filter(|s| **s).count();
    // The smaller side is hashed, the other probed.
    let hashed = supports * 2 <= is_support.len();
    let mut seen = rustc_hash::FxHashSet::default();
    for (t, &s) in triangles.iter().zip(is_support) {
        if s == hashed {
            seen.extend([t.v0, t.v1, t.v2].map(corner_key));
        }
    }
    !seen.is_empty()
        && triangles
            .iter()
            .zip(is_support)
            .any(|(t, &s)| s != hashed && [t.v0, t.v1, t.v2].iter().any(|v| seen.contains(&corner_key(*v))))
}

/// Splits `triangles` (Y up) by `is_support` into (model, supports), as [`super::split`] does,
/// and closes both sides where the mask cuts a welded shell along its edges ([`split`]). Each
/// closed seam loop gets a flat fan about its centre on the model side, and on the support side a
/// cone [`TIP_DEPTH_MM`] into the model, with the support's seam corners moved
/// [`SEAM_SHIFT_MM`] into the model, so the support reader finds each touch there. The caps are
/// rebuilt from the mask and the mesh alone and come out the same every time. The original
/// triangles keep their order (support triangles at the seam with their corners moved), and the
/// caps follow them. A mask that cuts no edge (a split by component) gives exactly
/// [`super::split`]'s pieces.
pub fn closed_split(triangles: &[Triangle], is_support: &[bool]) -> (Vec<Triangle>, Vec<Triangle>) {
    let (mut model, supports) = super::split(triangles, is_support);
    if !shares_corners(triangles, is_support) {
        return (model, supports);
    }
    let m = Mesh::new(triangles);
    let em = m.edges();
    let loops: Vec<Loop> = seam_loops(&m, &em, is_support).into_iter().filter_map(|(lp, c)| c.then_some(lp)).collect();
    if loops.is_empty() {
        return (model, supports);
    }
    use crate::geometry::Vec3;
    let v3 = |p: P| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let tri = |a: P, b: P, c: P| {
        let n = cross(sub(b, a), sub(c, a));
        let n = if len(n) > 0.0 { unit(n) } else { [0.0; 3] };
        Triangle::new(v3(a), v3(b), v3(c), v3(n))
    };
    // Each loop's centre and the direction into the model (its support cap's outward normal).
    let mut caps = Vec::with_capacity(loops.len());
    let mut shift: FxHashMap<u32, P> = FxHashMap::default();
    for lp in &loops {
        let c = mul(lp.iter().fold([0.0; 3], |c, e| add(c, m.pos[e.0 as usize])), 1.0 / lp.len() as f64);
        let mut into = [0.0; 3];
        for &(a, b) in lp {
            into = add(into, cross(sub(m.pos[a as usize], c), sub(m.pos[b as usize], c)));
        }
        let into = if len(into) > 0.0 { unit(into) } else { [0.0; 3] };
        for e in lp {
            shift.insert(e.0, into);
        }
        caps.push((c, into));
    }
    let at = |v: u32| -> P {
        match shift.get(&v) {
            Some(d) => add(m.pos[v as usize], mul(*d, SEAM_SHIFT_MM)),
            None => m.pos[v as usize],
        }
    };
    let mut closed_supports = Vec::with_capacity(supports.len() + loops.iter().map(Vec::len).sum::<usize>());
    for ((t, ids), _) in triangles.iter().zip(&m.ids).zip(is_support).filter(|(_, s)| **s) {
        if ids.iter().any(|v| shift.contains_key(v)) {
            closed_supports.push(tri(at(ids[0]), at(ids[1]), at(ids[2])));
        } else {
            closed_supports.push(*t);
        }
    }
    for (lp, &(c, into)) in loops.iter().zip(&caps) {
        let tip = add(c, mul(into, TIP_DEPTH_MM));
        for &(a, b) in lp {
            let (pa, pb) = (m.pos[a as usize], m.pos[b as usize]);
            model.push(tri(pb, pa, c));
            closed_supports.push(tri(at(a), at(b), tip));
        }
    }
    (model, closed_supports)
}

/// Labels a welded single shell ([`split`]) as a [`Classification`], or None when its guards
/// fail. The split is confident: its guards held on every model of the guard sets. It is not
/// `generated` (that is for many separate generated parts), so a split not checked at indexing is
/// still rebuilt to check it, on the closed pieces ([`closed_split`]).
pub fn classify(triangles: &[Triangle], raft_height_mm: f32) -> Option<Classification> {
    let (mask, _) = split(triangles, raft_height_mm);
    mask.map(|is_support| Classification { is_support, confident: true, generated: false, welded: true })
}

/// A facet's in-plane width: its corners' extent across the direction to its farthest corner.
fn facet_width(m: &Mesh, fc: &Facet) -> f64 {
    let pts: Vec<P> = fc.tris.iter().flat_map(|&i| m.tv[i as usize]).collect();
    let c = mul(pts.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / pts.len() as f64);
    let far = pts.iter().copied().max_by(|a, b| len(sub(*a, c)).total_cmp(&len(sub(*b, c)))).unwrap_or(c);
    let u = sub(far, c);
    if len(u) == 0.0 {
        return 0.0;
    }
    let v = cross(fc.n, unit(u));
    let ys: Vec<f64> = pts.iter().map(|p| dot(sub(*p, c), v)).collect();
    ys.iter().copied().fold(f64::MIN, f64::max) - ys.iter().copied().fold(f64::MAX, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Vec3, swap_up_axis};
    use manifold_csg::Manifold;

    const RAFT: f32 = 2.0;

    /// A Manifold's triangles, turned from Z up to the loader's Y up.
    fn triangles(m: &Manifold) -> Vec<Triangle> {
        let (vp, np, idx) = m.to_mesh_f64();
        let v = |i: u64| {
            let k = i as usize * np;
            swap_up_axis(Vec3::new(vp[k] as f32, vp[k + 1] as f32, vp[k + 2] as f32))
        };
        idx.chunks(3)
            .map(|t| {
                let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
                Triangle::new(a, b, c, (b - a).cross(c - a).normalized())
            })
            .collect()
    }

    /// A finely meshed ball (the sculpt) of radius 3 centred 6 mm up.
    fn ball() -> Manifold {
        Manifold::sphere(3.0, 160).translate(0.0, 0.0, 6.0)
    }

    /// Eight-sided rods from the raft into the ball's underside, on a 1 mm raft slab.
    fn supports() -> Manifold {
        let mut m = Manifold::cube(12.0, 12.0, 1.0, false).translate(-6.0, -6.0, 0.0);
        for (x, y) in [(0.0, 0.0), (1.2, 0.0), (-1.2, 0.0), (0.0, 1.2), (0.0, -1.2)] {
            m = m.union(&Manifold::cylinder(4.2, 0.4, 0.2, 8, false).translate(x, y, 0.5));
        }
        m
    }

    /// Every edge of `side` is used once in each direction: closed and consistently wound.
    fn assert_closed(side: &[Triangle]) {
        let m = Mesh::new(side);
        let mut directed: FxHashMap<(u32, u32), u32> = FxHashMap::default();
        for ids in &m.ids {
            for k in 0..3 {
                *directed.entry((ids[k], ids[(k + 1) % 3])).or_default() += 1;
            }
        }
        for (&(a, b), &n) in &directed {
            assert_eq!(n, 1, "edge {a}-{b} used {n} times one way");
            assert_eq!(directed.get(&(b, a)), Some(&1), "edge {a}-{b} is open");
        }
    }

    #[test]
    fn welded_rods_are_cut_from_the_ball_at_their_tips() {
        let tris = triangles(&ball().union(&supports()));
        let (mask, rep) = split(&tris, RAFT);
        let sup = mask.unwrap_or_else(|| panic!("{rep:?}"));
        assert_eq!(rep.loops, 5, "{rep:?}");
        assert!(rep.largest_loop_mm < 1.0, "{rep:?}");
        // Every model triangle lies on the ball; the cut takes none of it.
        let m = Mesh::new(&tris);
        let centre = [0.0, 6.0, 0.0];
        for (t, &s) in m.tv.iter().zip(&sup) {
            let r = len(sub(mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0), centre));
            if !s {
                assert!((r - 3.0).abs() < 0.05, "model triangle off the ball at {r}");
            }
        }
        let ball_area: f64 = m.tv.iter().zip(&sup).filter(|(_, s)| !**s).map(|(t, _)| tri_area(t)).sum();
        let sphere = 4.0 * std::f64::consts::PI * 9.0;
        assert!(ball_area > 0.97 * sphere && ball_area < sphere, "{ball_area}");
    }

    #[test]
    fn a_welded_mask_rebuilds_into_closed_pieces_the_same_every_time() {
        let tris = triangles(&ball().union(&supports()));
        let c = super::super::classify(&tris, RAFT).expect("a welded split");
        assert!(c.welded && c.confident && !c.generated);
        // The plain split is open along the seams.
        let (open_model, open_supports) = super::super::split(&tris, &c.is_support);
        assert!(Mesh::new(&open_model).edges().values().any(|l| l.len() == 1));
        let (model, supports) = closed_split(&tris, &c.is_support);
        assert_closed(&model);
        assert_closed(&supports);
        // The original triangles come first, unchanged on the model side, then one cap per contact.
        assert_eq!(&model[..open_model.len()], &open_model[..]);
        assert_eq!(supports.len() - open_supports.len(), model.len() - open_model.len());
        // Each support cap's tip is inside the ball, just past its surface.
        let centre = Vec3::new(0.0, 6.0, 0.0);
        let tips: Vec<f32> = supports[open_supports.len()..].iter().map(|t| (t.v2 - centre).length()).collect();
        assert!(tips.iter().all(|r| (2.85..3.0).contains(r)), "{tips:?}");
        // Rebuilt from the mask alone, bit for bit.
        assert_eq!(closed_split(&tris, &c.is_support), (model, supports));
    }

    #[test]
    fn a_split_by_component_is_left_as_it_is() {
        use crate::geometry::support_separation::tests::cuboid;
        let mut tris = cuboid(Vec3::new(-2.0, 3.0, -2.0), Vec3::new(2.0, 7.0, 2.0), 4);
        let body = tris.len();
        tris.extend(cuboid(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 3.0, 1.0), 1));
        let mask: Vec<bool> = (0..tris.len()).map(|i| i >= body).collect();
        assert_eq!(closed_split(&tris, &mask), super::super::split(&tris, &mask));
    }

    #[test]
    fn rivets_on_the_model_stay_with_it() {
        let mut body = ball();
        for z in [5.0, 6.0, 7.0] {
            body = body.union(&Manifold::cylinder(0.8, 0.3, 0.3, 8, false).translate(2.8, 0.0, z));
        }
        let tris = triangles(&body.union(&supports()));
        let (mask, rep) = split(&tris, RAFT);
        let sup = mask.unwrap_or_else(|| panic!("{rep:?}"));
        for (t, &s) in tris.iter().zip(&sup) {
            // Rivet faces stand out of the ball's side, above the raft.
            if t.v0.x < -3.05 && t.v0.y > RAFT {
                assert!(!s, "a rivet triangle was cut as support");
            }
        }
    }

    #[test]
    fn a_figure_on_a_base_is_not_cut() {
        let base = Manifold::cylinder(3.0, 4.0, 4.0, 64, false);
        let tris = triangles(&Manifold::sphere(3.0, 160).translate(0.0, 0.0, 5.5).union(&base));
        let (mask, rep) = split(&tris, RAFT);
        assert!(mask.is_none(), "{rep:?}");
        // The pre-check turns it away: the base is wide where it crosses the raft's top.
        assert_eq!(rep.verdict, "wide section at the raft", "{rep:?}");
    }

    #[test]
    fn a_ball_on_a_spike_is_not_cut() {
        // A model standing on a cone of its own: grounded, but no footprint.
        let spike = Manifold::cylinder(3.5, 0.0, 0.6, 8, false);
        let tris = triangles(&ball().union(&spike));
        let (mask, rep) = split(&tris, RAFT);
        assert!(mask.is_none(), "{rep:?}");
        assert_eq!(rep.verdict, "no footprint", "{rep:?}");
    }

    #[test]
    fn the_pre_check_reads_the_rods_where_they_cross_the_raft() {
        let s = slice(&triangles(&ball().union(&supports())), RAFT);
        // Five rods, each one narrow section, on a 144 mm² slab.
        assert_eq!(s.sections, 5, "{s:?}");
        assert!(s.widest_mm < 1.0 && s.most_triangles <= 64, "{s:?}");
        assert!((s.footprint_mm2 - 144.0).abs() < 1.0, "{s:?}");
        assert_eq!(plausible(&s), Ok(()));
        // A ball floating above the raft crosses nothing.
        assert_eq!(
            plausible(&slice(&triangles(&ball().translate(0.0, 0.0, 3.0)), RAFT)),
            Err("nothing crosses the raft")
        );
    }
}
