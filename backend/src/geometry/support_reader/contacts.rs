//! The touch rule: where the original supports hold the model, by geometry alone.
//!
//! Every support shell's vertices are marked inside or outside the model by the generalised
//! winding number. The inside parts of each shell are grouped into connected patches, split where
//! an intersection loop crosses them. A patch bounded by one loop is a touch (a tip, whatever its
//! shape), by two or more a pass-through, and a shell inside with no loop is buried. Loops on a
//! surface that does not separate inside from outside (a model's inner shell, a stray sheet) are
//! not boundaries and are left out. The owner's rulings (2026-11-10): a tip through a thin wall is
//! a tip, its contact the entry loop; a tip resting flush (no loop, the model within
//! [`FLUSH_GAP`]) is a tip; a side graze (one elongated loop with the shell going on past both of
//! its long ends) and any other multi-loop patch is a clip, not a contact. Overlapping touches are
//! one contact, sized by the union of their loops. A contact sits at its loops' area-weighted
//! centre.

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::math::{P, add, centroid, closest_on_tri, cross, dist, dot, len, lerp, mul, seg_dist, sub, unit};

/// Open loop ends this close are joined (numeric cracks in a body).
const JOIN_GAP: f64 = 1e-3;
/// A loop smaller than this in area is float noise.
const MIN_LOOP_AREA: f64 = 1e-6;
/// The two sides of a loop's model face are tried this far off it.
const SIDE_STEP: f64 = 0.005;
/// A tip resting flush: a shell with no loop this near the model.
const FLUSH_GAP: f64 = 0.03;
/// A loop this much longer than round (perimeter squared over 4 pi area) is elongated.
const ELONGATED: f64 = 2.5;
/// A pass-through's two loops this close, each this small, with the support ending this soon past
/// the far one: a tip through a thin wall.
const THIN_WALL_SPACING: f64 = 2.0;
const THIN_WALL_LOOP_D: f64 = 1.5;
const THIN_WALL_END: f64 = 3.0;
/// Two loops of one patch this close in centre and width are one loop on a doubled surface.
const COINCIDENT_MM: f64 = 0.03;
/// Shells reaching no higher than this above the raft's top are raft and are not judged.
pub const RAFT_BAND_MARGIN_MM: f64 = 0.5;
/// Winding number cells: exact within, dipoles beyond [`FAR`] cell radii.
const FINE: f64 = 2.0;
const COARSE: f64 = 16.0;
const FAR: f64 = 3.0;
/// The model's column grid for box queries.
const COL: f64 = 2.0;

fn col(v: f64) -> i32 {
    (v / COL).floor() as i32
}

fn area_vec(t: &[P; 3]) -> P {
    mul(cross(sub(t[1], t[0]), sub(t[2], t[0])), 0.5)
}

/// The solid angle a triangle subtends at a point (Van Oosterom and Strackee).
fn solid_angle(t: &[P; 3], p: P) -> f64 {
    let (a, b, c) = (sub(t[0], p), sub(t[1], p), sub(t[2], p));
    let (la, lb, lc) = (len(a), len(b), len(c));
    let num = dot(a, cross(b, c));
    let den = la * lb * lc + dot(a, b) * lc + dot(b, c) * la + dot(c, a) * lb;
    2.0 * num.atan2(den)
}

struct Cell {
    ids: Vec<u32>,
    c: P,
    n: P,
    r: f64,
}

/// The generalised winding number of a triangle soup: exact near, dipoles of cells far away.
pub(super) struct Wind {
    tris: Vec<[P; 3]>,
    fine: Vec<Cell>,
    coarse: Vec<(Cell, Vec<usize>)>,
    lo: P,
    hi: P,
}

impl Wind {
    pub(super) fn new(tris: Vec<[P; 3]>) -> Self {
        let key = |p: P, s: f64| p.map(|x| (x / s).floor() as i32);
        let mut by: BTreeMap<[i32; 3], Vec<u32>> = BTreeMap::new();
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for (i, t) in tris.iter().enumerate() {
            by.entry(key(centroid(t), FINE)).or_default().push(i as u32);
            for v in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        let mut fine = Vec::new();
        for (_, ids) in by {
            let (mut c, mut w, mut n) = ([0.0; 3], 0.0, [0.0; 3]);
            for &i in &ids {
                let t = &tris[i as usize];
                let a = area_vec(t);
                let m = len(a).max(1e-12);
                c = add(c, mul(add(add(t[0], t[1]), t[2]), m / 3.0));
                w += m;
                n = add(n, a);
            }
            let c = mul(c, 1.0 / w);
            let r = ids.iter().flat_map(|&i| tris[i as usize]).map(|v| dist(v, c)).fold(0.0, f64::max);
            fine.push(Cell { ids, c, n, r });
        }
        let mut groups: BTreeMap<[i32; 3], Vec<usize>> = BTreeMap::new();
        for (k, f) in fine.iter().enumerate() {
            groups.entry(key(f.c, COARSE)).or_default().push(k);
        }
        let coarse = groups
            .into_values()
            .map(|kids| {
                let (mut c, mut w, mut n) = ([0.0; 3], 0.0, [0.0; 3]);
                for &k in &kids {
                    let m = (fine[k].ids.len() as f64).max(1.0);
                    c = add(c, mul(fine[k].c, m));
                    w += m;
                    n = add(n, fine[k].n);
                }
                let c = mul(c, 1.0 / w);
                let r = kids.iter().map(|&k| dist(fine[k].c, c) + fine[k].r).fold(0.0, f64::max);
                (Cell { ids: Vec::new(), c, n, r }, kids)
            })
            .collect();
        Wind { tris, fine, coarse, lo, hi }
    }

    pub(super) fn w(&self, p: P) -> f64 {
        if (0..3).any(|k| p[k] < self.lo[k] - 0.01 || p[k] > self.hi[k] + 0.01) {
            return 0.0;
        }
        let dipole = |cell: &Cell| {
            let d = sub(cell.c, p);
            let l = len(d);
            dot(cell.n, d) / (l * l * l)
        };
        let mut s = 0.0;
        for (cc, kids) in &self.coarse {
            if dist(p, cc.c) > FAR * cc.r + 1e-9 {
                s += dipole(cc);
                continue;
            }
            for &k in kids {
                let f = &self.fine[k];
                if dist(p, f.c) > FAR * f.r + 1e-9 {
                    s += dipole(f);
                } else {
                    for &i in &f.ids {
                        s += solid_angle(&self.tris[i as usize], p);
                    }
                }
            }
        }
        s / (4.0 * std::f64::consts::PI)
    }

    pub(super) fn inside(&self, p: P) -> bool {
        self.w(p) > 0.5
    }
}

/// The model: triangles in columns, for box and depth queries.
pub(super) struct Model {
    pub(super) tris: Vec<[P; 3]>,
    ids: Vec<[u32; 3]>,
    cols: FxHashMap<(i32, i32), Vec<u32>>,
}

impl Model {
    pub(super) fn new(tris: Vec<[P; 3]>) -> Self {
        let mut cols: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
        for (i, t) in tris.iter().enumerate() {
            let lo = [0, 1].map(|k| t.iter().map(|v| v[k]).fold(f64::MAX, f64::min));
            let hi = [0, 1].map(|k| t.iter().map(|v| v[k]).fold(f64::MIN, f64::max));
            for x in col(lo[0])..=col(hi[0]) {
                for y in col(lo[1])..=col(hi[1]) {
                    cols.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        let (ids, _) = weld_ids(&tris);
        Self { tris, ids, cols }
    }

    /// Triangles whose box meets the box.
    pub(super) fn near(&self, lo: P, hi: P) -> Vec<u32> {
        let mut out = Vec::new();
        for x in col(lo[0])..=col(hi[0]) {
            for y in col(lo[1])..=col(hi[1]) {
                if let Some(list) = self.cols.get(&(x, y)) {
                    out.extend_from_slice(list);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out.retain(|&i| {
            let t = &self.tris[i as usize];
            (0..3).all(|k| {
                t.iter().map(|v| v[k]).fold(f64::MIN, f64::max) >= lo[k]
                    && t.iter().map(|v| v[k]).fold(f64::MAX, f64::min) <= hi[k]
            })
        });
        out
    }

    /// Distance to the surface, searching outward.
    pub(super) fn depth(&self, p: P) -> f64 {
        for r in [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0] {
            let d = [r, r, r];
            let best = self
                .near(sub(p, d), add(p, d))
                .iter()
                .map(|&i| dist(p, closest_on_tri(p, &self.tris[i as usize])))
                .fold(f64::MAX, f64::min);
            if best <= r {
                return best;
            }
        }
        32.0
    }
}

/// A vertex id per position, for keys shared between neighbouring triangles.
fn weld_ids(tris: &[[P; 3]]) -> (Vec<[u32; 3]>, Vec<P>) {
    let mut ids: FxHashMap<[i64; 3], u32> = FxHashMap::default();
    let mut points = Vec::new();
    let idx = tris
        .iter()
        .map(|t| {
            t.map(|v| {
                *ids.entry(v.map(|x| (x * 1e5).round() as i64)).or_insert_with(|| {
                    points.push(v);
                    points.len() as u32 - 1
                })
            })
        })
        .collect();
    (idx, points)
}

type End = (P, (bool, u32, u32));

/// Where a pair of triangles cross: the segment's two ends, each with the edge it lies on.
fn cross_segment(t: &[P; 3], ti: &[u32; 3], u: &[P; 3], ui: &[u32; 3]) -> Option<(End, End)> {
    let plane = |x: &[P; 3]| {
        let n = cross(sub(x[1], x[0]), sub(x[2], x[0]));
        (n, dot(n, x[0]))
    };
    let (nu, ou) = plane(u);
    let (nt, ot) = plane(t);
    if len(nu) < 1e-14 || len(nt) < 1e-14 {
        return None;
    }
    // A vertex exactly on the plane counts as on its positive side, the same for every pair: it
    // keeps neighbouring triangles' answers consistent, and a part lying on the surface does not
    // cross it.
    let dt = t.map(|v| dot(nu, v) - ou >= 0.0);
    let du = u.map(|v| dot(nt, v) - ot >= 0.0);
    if dt[0] == dt[1] && dt[1] == dt[2] || du[0] == du[1] && du[1] == du[2] {
        return None;
    }
    let line = cross(nt, nu);
    if len(line) < 1e-16 {
        return None;
    }
    let crossings = |x: &[P; 3], ids: &[u32; 3], sides: &[bool; 3], n: P, o: f64, other: bool| {
        let mut out = Vec::with_capacity(2);
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            if sides[i] != sides[j] {
                let (a, b) = if ids[i] < ids[j] { (x[i], x[j]) } else { (x[j], x[i]) };
                let (da, db) = (dot(n, a) - o, dot(n, b) - o);
                let at = lerp(a, b, da / (da - db));
                out.push((dot(line, at), at, (other, ids[i].min(ids[j]), ids[i].max(ids[j]))));
            }
        }
        out
    };
    let mut a = crossings(t, ti, &dt, nu, ou, false);
    let mut b = crossings(u, ui, &du, nt, ot, true);
    if a.len() != 2 || b.len() != 2 {
        return None;
    }
    a.sort_by(|x, y| x.0.total_cmp(&y.0));
    b.sort_by(|x, y| x.0.total_cmp(&y.0));
    if a[1].0 <= b[0].0 || b[1].0 <= a[0].0 {
        return None;
    }
    let start = if a[0].0 >= b[0].0 { a[0] } else { b[0] };
    let end = if a[1].0 <= b[1].0 { a[1] } else { b[1] };
    Some(((start.1, start.2), (end.1, end.2)))
}

/// The closed solids of a shell (split across manifold edges, each triangle at a non-manifold
/// edge paired with the one bounding the same wedge).
fn solids(tris: &[[P; 3]]) -> Vec<Vec<[P; 3]>> {
    let key = |p: P| p.map(|x| (x * 1e4).round() as i64);
    let mut ids: FxHashMap<[i64; 3], u32> = FxHashMap::default();
    let mut points: Vec<P> = Vec::new();
    let idx: Vec<[u32; 3]> = tris
        .iter()
        .map(|t| {
            t.map(|v| {
                *ids.entry(key(v)).or_insert_with(|| {
                    points.push(v);
                    points.len() as u32 - 1
                })
            })
        })
        .collect();
    // Per edge: its triangles, each with its direction along the edge and its third corner.
    type EdgeFaces = Vec<(u32, bool, u32)>;
    let mut edges: FxHashMap<(u32, u32), EdgeFaces> = FxHashMap::default();
    for (k, &[a, b, c]) in idx.iter().enumerate() {
        for (x, y, z) in [(a, b, c), (b, c, a), (c, a, b)] {
            edges.entry((x.min(y), x.max(y))).or_default().push((k as u32, x < y, z));
        }
    }
    let mut parent: Vec<u32> = (0..idx.len() as u32).collect();
    for (&(a, b), list) in &edges {
        if list.len() == 2 {
            let (x, y) = (super::math::find(&mut parent, list[0].0), super::math::find(&mut parent, list[1].0));
            parent[x as usize] = y;
        } else if list.len() > 2 {
            let o = points[a as usize];
            let ax = unit(sub(points[b as usize], o));
            let r0 = sub(points[list[0].2 as usize], o);
            let reference = unit(sub(r0, mul(ax, dot(r0, ax))));
            let other = cross(ax, reference);
            let mut items: Vec<(f64, u8, u32, bool)> = list
                .iter()
                .map(|&(k, fwd, z)| {
                    let w = sub(points[z as usize], o);
                    let mut th = dot(w, other).atan2(dot(w, reference));
                    if (th + std::f64::consts::PI).abs() < 1e-6 {
                        th = std::f64::consts::PI;
                    }
                    ((th * 1e5).round() / 1e5, u8::from(!fwd), k, fwd)
                })
                .collect();
            items.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)).then(p.2.cmp(&q.2)));
            let mut stack = Vec::new();
            for &(_, _, k, fwd) in items.iter().chain(items.iter()) {
                if !fwd {
                    stack.push(k);
                } else if stack.last().is_some_and(|&top| top != k) {
                    let open: u32 = stack.pop().expect("a face on the stack");
                    let (x, y) = (super::math::find(&mut parent, open), super::math::find(&mut parent, k));
                    parent[x as usize] = y;
                }
            }
        }
    }
    let mut groups: FxHashMap<u32, usize> = FxHashMap::default();
    let mut out: Vec<Vec<[P; 3]>> = Vec::new();
    for (i, t) in tris.iter().enumerate() {
        let r = super::math::find(&mut parent, i as u32);
        let slot = *groups.entry(r).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[slot].push(*t);
    }
    out
}

/// The shells of a mesh (triangles joined through shared vertices), with repeated triangles
/// dropped.
pub(super) fn shells_of(tris: &[[P; 3]]) -> Vec<Vec<[P; 3]>> {
    let key = |p: P| p.map(|x| (x * 1e4).round() as i64);
    let mut ids: FxHashMap<[i64; 3], u32> = FxHashMap::default();
    let mut parent: Vec<u32> = (0..tris.len() as u32).collect();
    for (i, t) in tris.iter().enumerate() {
        for v in t {
            let other = *ids.entry(key(*v)).or_insert(i as u32);
            let (a, b) = (super::math::find(&mut parent, i as u32), super::math::find(&mut parent, other));
            parent[a as usize] = b;
        }
    }
    let mut groups: FxHashMap<u32, usize> = FxHashMap::default();
    let mut out: Vec<Vec<[P; 3]>> = Vec::new();
    for (i, t) in tris.iter().enumerate() {
        let r = super::math::find(&mut parent, i as u32);
        let slot = *groups.entry(r).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[slot].push(*t);
    }
    out.into_iter()
        .map(|s| {
            let mut seen = FxHashSet::default();
            s.into_iter()
                .filter(|t| {
                    let keys = t.map(|v| v.map(|x| (x * 1e5).round() as i64));
                    let least = (0..3).min_by_key(|&i| keys[i]).expect("three corners");
                    seen.insert([keys[least], keys[(least + 1) % 3], keys[(least + 2) % 3]])
                })
                .collect()
        })
        .collect()
}

type Key = (bool, u32, u32, u32);

struct Seg {
    a: Key,
    pa: P,
    b: Key,
    pb: P,
    /// The support triangle and the model triangle it lies on.
    ti: u32,
    mi: u32,
}

fn segments_of(tris: &[[P; 3]], ids: &[[u32; 3]], lo: P, hi: P, model: &Model) -> Vec<Seg> {
    let mut out = Vec::new();
    let candidates = model.near(lo, hi);
    for (ti, t) in tris.iter().enumerate() {
        let tlo: P = [0, 1, 2].map(|k| t.iter().map(|v| v[k]).fold(f64::MAX, f64::min));
        let thi: P = [0, 1, 2].map(|k| t.iter().map(|v| v[k]).fold(f64::MIN, f64::max));
        for &b in &candidates {
            let u = &model.tris[b as usize];
            if (0..3).any(|k| {
                u.iter().map(|v| v[k]).fold(f64::MIN, f64::max) < tlo[k]
                    || u.iter().map(|v| v[k]).fold(f64::MAX, f64::min) > thi[k]
            }) {
                continue;
            }
            if let Some(((pa, fa), (pb, fb))) = cross_segment(t, &ids[ti], u, &model.ids[b as usize]) {
                let name = |f: (bool, u32, u32)| (f.0, f.1, f.2, if f.0 { ti as u32 } else { b });
                out.push(Seg { a: name(fa), pa, b: name(fb), pb, ti: ti as u32, mi: b });
            }
        }
    }
    out
}

/// One loop: its points, whether it closed, and its segments.
struct Walk {
    points: Vec<P>,
    closed: bool,
    segs: Vec<usize>,
}

fn walks_of(segments: &[Seg]) -> Vec<Walk> {
    let mut at: BTreeMap<Key, Vec<u32>> = BTreeMap::new();
    for (i, s) in segments.iter().enumerate() {
        at.entry(s.a).or_default().push(i as u32);
        at.entry(s.b).or_default().push(i as u32);
    }
    let mut used = vec![false; segments.len()];
    let walk = |start: Key, used: &mut Vec<bool>| -> Option<Walk> {
        let mut node = start;
        let (mut points, mut segs) = (Vec::new(), Vec::new());
        while let Some(i) = at[&node].iter().copied().find(|&i| !used[i as usize]) {
            used[i as usize] = true;
            segs.push(i as usize);
            let s = &segments[i as usize];
            let (from, to, target) = if s.a == node { (s.pa, s.pb, s.b) } else { (s.pb, s.pa, s.a) };
            if points.is_empty() {
                points.push(from);
            }
            points.push(to);
            node = target;
            if node == start {
                return Some(Walk { points, closed: true, segs });
            }
        }
        (!points.is_empty()).then_some(Walk { points, closed: false, segs })
    };
    let mut walks = Vec::new();
    let ends: Vec<Key> = at.iter().filter(|(_, l)| l.len() % 2 == 1).map(|(k, _)| *k).collect();
    for start in ends {
        if let Some(w) = walk(start, &mut used) {
            walks.push(w);
        }
    }
    for i in 0..segments.len() {
        if !used[i]
            && let Some(w) = walk(segments[i].a, &mut used)
        {
            walks.push(w);
        }
    }
    // Numeric cracks: join open chains whose ends meet.
    let mut i = 0;
    while i < walks.len() {
        if walks[i].closed {
            i += 1;
            continue;
        }
        let (first, last) = (walks[i].points[0], *walks[i].points.last().expect("a walk has points"));
        if dist(first, last) <= JOIN_GAP {
            walks[i].closed = true;
            continue;
        }
        let mut merged = false;
        for j in 0..walks.len() {
            if j == i || walks[j].closed {
                continue;
            }
            let (f2, l2) = (walks[j].points[0], *walks[j].points.last().expect("a walk has points"));
            let rev = if dist(last, f2) <= JOIN_GAP {
                Some(false)
            } else if dist(last, l2) <= JOIN_GAP {
                Some(true)
            } else {
                None
            };
            if let Some(rev) = rev {
                let other = walks.remove(j);
                let k = if j < i { i - 1 } else { i };
                let mut pts = other.points;
                if rev {
                    pts.reverse();
                }
                walks[k].points.extend(pts);
                walks[k].segs.extend(other.segs);
                i = k;
                merged = true;
                break;
            }
        }
        if !merged {
            i += 1;
        }
    }
    walks
}

/// Area, unit normal, centre and perimeter of a loop's points.
fn loop_stats(points: &[P], closed: bool) -> (f64, P, P, f64) {
    let (mut vector, mut perimeter, mut centre) = ([0.0; 3], 0.0, [0.0; 3]);
    let origin = points[0];
    for k in 0..points.len() {
        let (a, b) = (points[k], points[(k + 1) % points.len()]);
        vector = add(vector, cross(sub(a, origin), sub(b, origin)));
        if k + 1 < points.len() || closed {
            let l = dist(a, b);
            perimeter += l;
            centre = add(centre, mul(add(a, b), l / 2.0));
        }
    }
    (len(vector) / 2.0, unit(vector), mul(centre, 1.0 / perimeter.max(1e-12)), perimeter)
}

/// The width of a circle of this area.
pub fn eq_d(area: f64) -> f64 {
    2.0 * (area / std::f64::consts::PI).sqrt()
}

struct LoopOut {
    points: Vec<P>,
    area: f64,
    normal: P,
    centre: P,
    perimeter: f64,
    closed: bool,
}

/// One inside patch of a shell.
struct Patch {
    loops: Vec<usize>,
    inside_verts: Vec<P>,
}

struct ShellCall {
    patches: Vec<Patch>,
    loops: Vec<LoopOut>,
    buried: bool,
    lo: P,
    hi: P,
    verts: Vec<P>,
    inside: Vec<bool>,
}

fn call_shell(tris: Vec<[P; 3]>, model: &Model, wind: &Wind) -> ShellCall {
    let (ids, verts) = weld_ids(&tris);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for v in &verts {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let inside: Vec<bool> = verts.iter().map(|v| wind.inside(*v)).collect();
    let segments = segments_of(&tris, &ids, lo, hi, model);
    let walks = walks_of(&segments);
    // The support's own solid: where overlapping primitives were joined into one shell, a
    // primitive's surface inside another is not the support's surface, and its loops are not where
    // it meets the model. Each closed solid of the shell on its own, so primitives wound either
    // way still count.
    let own: Vec<Wind> = if walks.is_empty() { Vec::new() } else { solids(&tris).into_iter().map(Wind::new).collect() };
    let in_own = |p: P| own.iter().any(|w| w.w(p).abs() > 0.5);
    // A loop is a boundary when the model face it lies on has inside on one side and outside on
    // the other.
    let mut boundary = vec![false; walks.len()];
    for (k, w) in walks.iter().enumerate() {
        let picks: Vec<usize> = (0..3).map(|j| w.segs[j * w.segs.len() / 3]).collect();
        let mut votes = 0;
        for &s in &picks {
            let seg = &segments[s];
            let m = mul(add(seg.pa, seg.pb), 0.5);
            let n = unit(area_vec(&model.tris[seg.mi as usize]));
            let (a, b) = (wind.inside(add(m, mul(n, SIDE_STEP))), wind.inside(sub(m, mul(n, SIDE_STEP))));
            let ns = unit(area_vec(&tris[seg.ti as usize]));
            let (c, d) = (in_own(add(m, mul(ns, SIDE_STEP))), in_own(sub(m, mul(ns, SIDE_STEP))));
            if a != b && c != d {
                votes += 1;
            }
        }
        boundary[k] = votes * 2 > picks.len();
    }
    // Pieces of the inside: inside vertices and the loops' ends, joined where the inside runs
    // between them. A triangle crossing the surface is split by its segments, so one triangle can
    // hold parts of two patches.
    let nv = verts.len();
    let mut item: FxHashMap<Key, usize> = FxHashMap::default();
    let mut points: Vec<P> = Vec::new();
    let mut on_edge: FxHashMap<(u32, u32), Vec<(f64, usize)>> = FxHashMap::default();
    for (k, w) in walks.iter().enumerate() {
        if !boundary[k] {
            continue;
        }
        for &s in &w.segs {
            let seg = &segments[s];
            for (key, at) in [(seg.a, seg.pa), (seg.b, seg.pb)] {
                if item.contains_key(&key) {
                    continue;
                }
                let id = nv + points.len();
                item.insert(key, id);
                points.push(at);
                // On a support edge: by its name, or, where it fell on a model edge too, by where
                // it is.
                let tri = ids[seg.ti as usize];
                let edges: Vec<(u32, u32)> = if !key.0 {
                    vec![(key.1, key.2)]
                } else {
                    [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])]
                        .into_iter()
                        .map(|(a, b)| (a.min(b), a.max(b)))
                        .filter(|&(a, b)| seg_dist(at, verts[a as usize], verts[b as usize]) < 1e-7)
                        .collect()
                };
                for (u, v) in edges {
                    let t = dot(sub(at, verts[u as usize]), sub(verts[v as usize], verts[u as usize]));
                    on_edge.entry((u, v)).or_default().push((t, id));
                }
            }
        }
    }
    let mut parent: Vec<usize> = (0..nv + points.len()).collect();
    fn root(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let join = |parent: &mut Vec<usize>, a: usize, b: usize| {
        let (x, y) = (root(parent, a), root(parent, b));
        parent[x] = y;
    };
    let mut seen_edges: FxHashSet<(u32, u32)> = FxHashSet::default();
    for id in &ids {
        for (a, b) in [(id[0], id[1]), (id[1], id[2]), (id[2], id[0])] {
            let (u, v) = (a.min(b), a.max(b));
            if !seen_edges.insert((u, v)) {
                continue;
            }
            // Along the edge from u to v: inside and outside alternate at each crossing.
            let mut cuts = on_edge.get(&(u, v)).cloned().unwrap_or_default();
            cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
            // One crossing named twice (on a model edge, from both its triangles) is one cut.
            let scale = dist(verts[u as usize], verts[v as usize]).powi(2).max(1e-12);
            let mut merged: Vec<(f64, usize)> = Vec::new();
            for c in cuts {
                match merged.last() {
                    Some(&(t, id)) if (c.0 - t).abs() <= 1e-9 * scale => join(&mut parent, id, c.1),
                    _ => merged.push(c),
                }
            }
            let mut chain: Vec<usize> = vec![u as usize];
            chain.extend(merged.iter().map(|c| c.1));
            chain.push(v as usize);
            let mut state = inside[u as usize];
            for w in chain.windows(2) {
                if state {
                    join(&mut parent, w[0], w[1]);
                }
                state = !state;
            }
        }
    }
    for (k, w) in walks.iter().enumerate() {
        if !boundary[k] {
            continue;
        }
        for &s in &w.segs {
            join(&mut parent, item[&segments[s].a], item[&segments[s].b]);
        }
    }
    let mut slot: FxHashMap<usize, usize> = FxHashMap::default();
    let mut patches: Vec<Patch> = Vec::new();
    let mut patch_at = |parent: &mut Vec<usize>, x: usize, patches: &mut Vec<Patch>| -> usize {
        let r = root(parent, x);
        *slot.entry(r).or_insert_with(|| {
            patches.push(Patch { loops: Vec::new(), inside_verts: Vec::new() });
            patches.len() - 1
        })
    };
    for v in 0..nv {
        if inside[v] {
            let p = patch_at(&mut parent, v, &mut patches);
            patches[p].inside_verts.push(verts[v]);
        }
    }
    let mut loops: Vec<LoopOut> = Vec::new();
    for (k, w) in walks.iter().enumerate() {
        let (area, normal, centre, perimeter) = loop_stats(&w.points, w.closed);
        if area < MIN_LOOP_AREA && w.closed {
            continue;
        }
        if !boundary[k] {
            continue;
        }
        let p = patch_at(&mut parent, item[&segments[w.segs[0]].a], &mut patches);
        // The same loop twice: a surface doubled in the model or the support (two coincident
        // sheets), not an entry and an exit.
        let twin = patches[p].loops.iter().any(|&o| {
            let x: &LoopOut = &loops[o];
            dist(x.centre, centre) <= COINCIDENT_MM && (eq_d(x.area) - eq_d(area)).abs() <= COINCIDENT_MM
        });
        if twin {
            continue;
        }
        patches[p].loops.push(loops.len());
        loops.push(LoopOut { points: w.points.clone(), area, normal, centre, perimeter, closed: w.closed });
    }
    let buried = loops.is_empty() && inside.iter().all(|&x| x);
    ShellCall { patches, loops, buried, lo, hi, verts, inside }
}

/// The deepest of up to `n` of the points, by distance to the surface.
fn depth_of(points: &[P], model: &Model, n: usize) -> f64 {
    let step = (points.len() / n.max(1)).max(1);
    points.iter().step_by(step).map(|p| model.depth(*p)).fold(0.0, f64::max)
}

/// The direction away from the model just outside a loop: from its centre to the shell's
/// outside vertices near it.
fn outward(shell: &ShellCall, l: &LoopOut, wind: &Wind) -> P {
    let r = eq_d(l.area) / 2.0;
    let reach = 2.0 * r + 0.5;
    let mut s = [0.0; 3];
    for (v, inside) in shell.verts.iter().zip(&shell.inside) {
        let d = dist(*v, l.centre);
        if !inside && d <= reach && d > 1e-9 {
            s = add(s, mul(sub(*v, l.centre), 1.0 / d));
        }
    }
    if len(s) > 1e-9 {
        return unit(s);
    }
    if wind.inside(add(l.centre, mul(l.normal, 0.05))) { mul(l.normal, -1.0) } else { l.normal }
}

fn elongation(l: &LoopOut) -> f64 {
    l.perimeter * l.perimeter / (4.0 * std::f64::consts::PI * l.area.max(1e-12))
}

/// A graze: one elongated loop, with the shell going on past both of its long ends near its plane.
fn grazes(shell: &ShellCall, l: &LoopOut) -> bool {
    if elongation(l) < ELONGATED || l.points.len() < 3 {
        return false;
    }
    // The loop's long direction.
    let mut axis = unit(sub(l.points[l.points.len() / 2], l.points[0]));
    for _ in 0..8 {
        let mut next = [0.0; 3];
        for p in &l.points {
            let d = sub(*p, l.centre);
            next = add(next, mul(d, dot(d, axis)));
        }
        if len(next) < 1e-12 {
            break;
        }
        axis = unit(next);
    }
    let half = l.points.iter().map(|p| dot(sub(*p, l.centre), axis).abs()).fold(0.0, f64::max);
    let (mut plus, mut minus) = (false, false);
    for (v, inside) in shell.verts.iter().zip(&shell.inside) {
        if *inside || dot(sub(*v, l.centre), l.normal).abs() > 0.5 + half {
            continue;
        }
        let t = dot(sub(*v, l.centre), axis);
        plus |= t > half + 0.2;
        minus |= t < -half - 0.2;
    }
    plus && minus
}

/// A tip through a thin wall: its entry loop (on the side the support goes on from) and its exit.
fn thin_wall_entry(shell: &ShellCall, patch: &Patch) -> (usize, usize) {
    let (ka, kb) = (patch.loops[0], patch.loops[1]);
    let (a, b) = (shell.loops[ka].centre, shell.loops[kb].centre);
    let spacing = dist(a, b);
    let axis = unit(sub(b, a));
    let (mut before, mut after) = (0.0f64, 0.0f64);
    for (v, inside) in shell.verts.iter().zip(&shell.inside) {
        if !*inside {
            let t = dot(sub(*v, a), axis);
            before = before.max(-t);
            after = after.max(t - spacing);
        }
    }
    if before >= after { (ka, kb) } else { (kb, ka) }
}

/// Whether a multi-loop patch is a tip through a thin wall (else a graze or a part through).
fn is_thin_wall_tip(shell: &ShellCall, patch: &Patch) -> bool {
    let ls: Vec<&LoopOut> = patch.loops.iter().map(|&k| &shell.loops[k]).collect();
    if ls.len() != 2 {
        return false;
    }
    if ls.iter().any(|l| elongation(l) >= ELONGATED) {
        let spacing = dist(ls[0].centre, ls[1].centre);
        if spacing < 0.5 * ls.iter().map(|l| l.perimeter).fold(0.0, f64::max) {
            return false; // a graze
        }
    }
    let (a, b) = (ls[0].centre, ls[1].centre);
    let spacing = dist(a, b);
    let small = ls.iter().all(|l| eq_d(l.area) <= THIN_WALL_LOOP_D);
    if spacing <= THIN_WALL_SPACING && small && spacing > 1e-6 {
        let axis = unit(sub(b, a));
        let (mut before, mut after) = (0.0f64, 0.0f64);
        for (v, inside) in shell.verts.iter().zip(&shell.inside) {
            if *inside {
                continue;
            }
            let t = dot(sub(*v, a), axis);
            before = before.max(-t);
            after = after.max(t - spacing);
        }
        return before.min(after) <= THIN_WALL_END;
    }
    false
}

/// Union area of loops, rasterised on their mean plane.
fn union_area(ls: &[&LoopOut], dirs: &[P]) -> f64 {
    let mut n = [0.0; 3];
    for d in dirs {
        n = add(n, *d);
    }
    let n = if len(n) < 1e-9 { ls[0].normal } else { unit(n) };
    let helper = if n[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let u = unit(cross(n, helper));
    let v = cross(n, u);
    let polys: Vec<Vec<(f64, f64)>> =
        ls.iter().map(|l| l.points.iter().map(|p| (dot(*p, u), dot(*p, v))).collect()).collect();
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in polys.iter().flatten() {
        x0 = x0.min(p.0);
        x1 = x1.max(p.0);
        y0 = y0.min(p.1);
        y1 = y1.max(p.1);
    }
    let step = (((x1 - x0) * (y1 - y0)) / 40000.0).sqrt().max(1e-4);
    let inside = |poly: &[(f64, f64)], x: f64, y: f64| {
        let mut c = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[j]);
            if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                c = !c;
            }
            j = i;
        }
        c
    };
    let mut count = 0usize;
    let mut y = y0 + step / 2.0;
    while y < y1 {
        let mut x = x0 + step / 2.0;
        while x < x1 {
            if polys.iter().any(|p| inside(p, x, y)) {
                count += 1;
            }
            x += step;
        }
        y += step;
    }
    count as f64 * step * step
}

/// One contact the original supports make on the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    /// The loops' area-weighted centre.
    pub at: P,
    pub area: f64,
    /// The width of a circle of the contact's area.
    pub d: f64,
    /// Away from the model along the support, just outside the loop.
    pub dir: P,
    /// Touch loops merged into it (0 for a flush tip).
    #[serde(default)]
    pub n: usize,
    #[serde(default)]
    pub depth: f64,
    #[serde(default)]
    pub open: bool,
    #[serde(default)]
    pub thin_wall: bool,
    #[serde(default)]
    pub flush: bool,
    /// The support shells it is on.
    #[serde(default)]
    pub shells: Vec<usize>,
}

/// Every contact of `support` on `model` (both Z-up), ignoring shells that stay below `raft_band`.
pub fn read(model: &[[P; 3]], support: &[[P; 3]], raft_band: f64) -> Vec<Contact> {
    let model_index = Model::new(model.to_vec());
    let wind = Wind::new(model.to_vec());
    let shells = shells_of(support);
    let calls: Vec<ShellCall> = shells.into_par_iter().map(|s| call_shell(s, &model_index, &wind)).collect();
    // (shell, loop, direction, depth, graze)
    let mut touches: Vec<(usize, usize, P, f64, bool)> = Vec::new();
    let mut flush: Vec<(usize, P, f64, P)> = Vec::new();
    for (i, c) in calls.iter().enumerate() {
        if c.hi[2] <= raft_band || c.buried {
            continue;
        }
        if c.loops.is_empty() {
            // Flush: no loop, and the model right there.
            let near_gap = |v: &P| {
                let r = [FLUSH_GAP; 3];
                model_index
                    .near(sub(*v, r), add(*v, r))
                    .iter()
                    .map(|&t| dist(*v, closest_on_tri(*v, &model_index.tris[t as usize])))
                    .fold(f64::MAX, f64::min)
            };
            let gap = c.verts.iter().map(near_gap).fold(f64::MAX, f64::min);
            if gap <= FLUSH_GAP {
                // Its footprint: the shell's vertices within the tolerance of the model.
                let near: Vec<P> = c.verts.iter().copied().filter(|v| near_gap(v) <= FLUSH_GAP).collect();
                let cen = mul(near.iter().fold([0.0; 3], |s, v| add(s, *v)), 1.0 / near.len().max(1) as f64);
                let d = (2.0 * near.iter().map(|v| dist(*v, cen)).fold(0.0, f64::max)).max(0.05);
                let mid = mul(add(c.lo, c.hi), 0.5);
                flush.push((i, cen, d, unit(sub(mid, cen))));
            }
            continue;
        }
        for p in &c.patches {
            match p.loops.len() {
                0 => {}
                1 => {
                    let l = &c.loops[p.loops[0]];
                    touches.push((
                        i,
                        p.loops[0],
                        outward(c, l, &wind),
                        depth_of(&p.inside_verts, &model_index, 32),
                        grazes(c, l),
                    ));
                }
                _ if is_thin_wall_tip(c, p) => {
                    // A tip poking through a thin wall is a tip: its contact is the loop on the side
                    // its shaft comes from.
                    let (entry, _) = thin_wall_entry(c, p);
                    let l = &c.loops[entry];
                    touches.push((i, entry, outward(c, l, &wind), depth_of(&p.inside_verts, &model_index, 32), false));
                }
                _ => {}
            }
        }
    }
    let thin: FxHashSet<(usize, usize)> = calls
        .iter()
        .enumerate()
        .flat_map(|(i, c)| {
            c.patches
                .iter()
                .filter(|p| p.loops.len() > 1 && is_thin_wall_tip(c, p))
                .map(move |p| (i, thin_wall_entry(c, p).0))
                .collect::<Vec<_>>()
        })
        .collect();
    // Merge overlapping touches into one contact, sized by their union.
    let n = touches.len();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let lp = |t: &(usize, usize, P, f64, bool)| -> &LoopOut { &calls[t.0].loops[t.1] };
    for a in 0..n {
        for b in a + 1..n {
            let (la, lb) = (lp(&touches[a]), lp(&touches[b]));
            if dist(la.centre, lb.centre) < (eq_d(la.area) + eq_d(lb.area)) / 2.0 {
                let (x, y) = (super::math::find(&mut parent, a as u32), super::math::find(&mut parent, b as u32));
                parent[x as usize] = y;
            }
        }
    }
    let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for a in 0..n {
        groups.entry(super::math::find(&mut parent, a as u32)).or_default().push(a);
    }
    let mut contacts: Vec<Contact> = Vec::new();
    for g in groups.values() {
        // A side graze is clipping, not a tip.
        if g.len() == 1 && touches[g[0]].4 {
            continue;
        }
        let ls: Vec<&LoopOut> = g.iter().map(|&a| lp(&touches[a])).collect();
        let dirs: Vec<P> = g.iter().map(|&a| touches[a].2).collect();
        let area = if g.len() == 1 { ls[0].area } else { union_area(&ls, &dirs) };
        let w: f64 = ls.iter().map(|l| l.area).sum::<f64>().max(1e-12);
        let at = ls.iter().fold([0.0; 3], |s, l| add(s, mul(l.centre, l.area / w)));
        let dir = unit(dirs.iter().fold([0.0; 3], |s, d| add(s, *d)));
        contacts.push(Contact {
            at,
            area,
            d: eq_d(area),
            dir,
            n: g.len(),
            depth: g.iter().map(|&a| touches[a].3).fold(0.0, f64::max),
            open: ls.iter().any(|l| !l.closed),
            thin_wall: g.iter().any(|&a| thin.contains(&(touches[a].0, touches[a].1))),
            flush: false,
            shells: g.iter().map(|&a| touches[a].0).collect(),
        });
    }
    // A tip resting flush is a tip.
    for (shell, at, d, dir) in flush {
        if contacts.iter().any(|c| dist(c.at, at) < (c.d + d) / 2.0) {
            continue;
        }
        contacts.push(Contact {
            at,
            area: std::f64::consts::PI * d * d / 4.0,
            d,
            dir,
            n: 0,
            depth: 0.0,
            open: false,
            thin_wall: false,
            flush: true,
            shells: vec![shell],
        });
    }
    contacts
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A closed axis-aligned box.
    pub(crate) fn boxm(lo: P, hi: P) -> Vec<[P; 3]> {
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

    /// A closed frustum from `a` (radius `ra`) to `b` (radius `rb`), `n` sides.
    pub(crate) fn frustum(a: P, b: P, ra: f64, rb: f64, n: usize) -> Vec<[P; 3]> {
        let d = unit(sub(b, a));
        let u = unit(if d[0].abs() < 0.9 { cross(d, [1.0, 0.0, 0.0]) } else { cross(d, [0.0, 1.0, 0.0]) });
        let v = cross(d, u);
        let ring = |c: P, r: f64, k: usize| {
            let t = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            add(c, add(mul(u, r * t.cos()), mul(v, r * t.sin())))
        };
        let mut out = Vec::new();
        for k in 0..n {
            let (p0, p1, q0, q1) = (ring(a, ra, k), ring(a, ra, k + 1), ring(b, rb, k), ring(b, rb, k + 1));
            out.push([p0, p1, q1]);
            out.push([p0, q1, q0]);
            out.push([a, p1, p0]);
            out.push([b, q0, q1]);
        }
        out
    }

    /// A closed UV sphere.
    pub(crate) fn ball(c: P, r: f64) -> Vec<[P; 3]> {
        let (lon, lat) = (24usize, 12usize);
        let at = |i: usize, j: usize| {
            let th = std::f64::consts::PI * j as f64 / lat as f64;
            let ph = 2.0 * std::f64::consts::PI * i as f64 / lon as f64;
            [c[0] + r * th.sin() * ph.cos(), c[1] + r * th.sin() * ph.sin(), c[2] + r * th.cos()]
        };
        let mut out = Vec::new();
        for j in 0..lat {
            for i in 0..lon {
                let (a, b, cc, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                if j > 0 {
                    out.push([a, d, b]);
                }
                if j + 1 < lat {
                    out.push([b, d, cc]);
                }
            }
        }
        out
    }

    #[test]
    fn winding_number_is_one_inside_a_closed_box_and_zero_outside() {
        let w = Wind::new(boxm([0.0; 3], [10.0, 10.0, 10.0]));
        assert!((w.w([5.0, 5.0, 5.0]) - 1.0).abs() < 1e-6);
        assert!(w.w([15.0, 5.0, 5.0]).abs() < 1e-6);
    }

    #[test]
    fn a_cone_entering_the_model_is_one_contact_at_its_loop() {
        let model = boxm([-5.0, -5.0, 10.0], [5.0, 5.0, 15.0]);
        // A cone from z = 6 up into the model's underside at z = 10, 0.3 mm wide at its tip.
        let support = frustum([0.0, 0.0, 6.0], [0.0, 0.0, 10.2], 0.4, 0.15, 32);
        let c = read(&model, &support, 3.5);
        assert_eq!(c.len(), 1, "{c:?}");
        assert!(dist(c[0].at, [0.0, 0.0, 10.0]) < 1e-3, "{:?}", c[0].at);
        // The cone is 0.15 + 0.25 * 0.2 / 4.2 = 0.162 mm in radius where it crosses z = 10.
        let want = 2.0 * (0.15 + 0.25 * 0.2 / 4.2);
        assert!((c[0].d - want).abs() < 0.01, "{}", c[0].d);
        assert!(c[0].dir[2] < -0.9, "{:?}", c[0].dir);
    }

    #[test]
    fn a_rod_through_the_model_is_no_contact_and_one_touching_flush_is() {
        let model = boxm([-5.0, -5.0, 10.0], [5.0, 5.0, 15.0]);
        let through = frustum([0.0, 0.0, 6.0], [0.0, 0.0, 18.0], 0.5, 0.5, 16);
        assert!(read(&model, &through, 3.5).is_empty());
        let flush = frustum([0.0, 0.0, 6.0], [0.0, 0.0, 9.99], 0.5, 0.5, 16);
        let c = read(&model, &flush, 3.5);
        assert_eq!(c.len(), 1);
        assert!(c[0].flush);
    }

    #[test]
    fn shells_in_the_raft_band_are_not_judged() {
        let model = boxm([-5.0, -5.0, 2.0], [5.0, 5.0, 6.0]);
        let low = frustum([0.0, 0.0, 0.0], [0.0, 0.0, 2.2], 0.4, 0.15, 16);
        assert!(read(&model, &low, 3.5).is_empty());
        assert_eq!(read(&model, &low, 2.0).len(), 1);
    }
}
