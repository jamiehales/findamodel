// Copied from the prototype's touch tool (scratchpad t52_merge/touch_tool as of merge r7: sphere-tip recognition, a sphere
// tip sized by the larger of its ball's and its cone's loop, and
// TOUCH_PEN) with only its entry point changed, and the maps it iterates (winding-number cells, loop chaining) ordered
// so that every run gives the same answer (the prototype's hash order did not); the algorithm and tolerances are the
// prototype's.
//
// Touch rule: the original mesh's own contacts, by geometry alone, and the built mesh's patches.
//
// Every support surface triangle's vertices are marked inside or outside the model by the generalised
// winding number. The inside parts of each support shell are grouped into connected patches, split
// where an intersection loop crosses them. A patch bounded by one loop is a touch, by two or more a
// pass-through, and a shell inside with no loop is buried. Loops on a surface that does not separate
// inside from outside (a model's inner shell, a stray sheet) are not boundaries and are left out.
//
// Usage: support-reader touch orig <work dir> <id>            one JSON line: contacts, pass-throughs, buried, flags
//        touch built <work dir> <id> <truth.json>  one JSON line: built tips and patches
//        touch panel <work dir> <id> <which> <x> <y> <z> <r>  triangles near a point, by call
//        touch selftest
#![allow(
    dead_code,
    unused_mut,
    unused_variables,
    unused_assignments,
    clippy::type_complexity,
    clippy::too_many_arguments
)]
use rayon::prelude::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub type P = [f64; 3];
pub const JOIN_GAP: f64 = 1e-3;
pub const BEHIND_REACH: f64 = 5.0;

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
pub fn unit(a: P) -> P {
    mul(a, 1.0 / len(a).max(1e-12))
}
pub fn lerp(a: P, b: P, t: f64) -> P {
    add(a, mul(sub(b, a), t))
}
pub fn seg_dist(p: P, a: P, b: P) -> f64 {
    let d = sub(b, a);
    let t = (dot(sub(p, a), d) / dot(d, d).max(1e-18)).clamp(0.0, 1.0);
    dist(p, lerp(a, b, t))
}

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

pub fn read_stl(path: &Path) -> Vec<[P; 3]> {
    let b = std::fs::read(path).unwrap_or_default();
    if b.len() < 84 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(b[80..84].try_into().unwrap()) as usize;
    let f = |at: usize| f64::from(f32::from_le_bytes(b[at..at + 4].try_into().unwrap()));
    (0..n)
        .map(|i| {
            let base = 84 + i * 50 + 12;
            [0, 1, 2].map(|v| [f(base + v * 12), f(base + v * 12 + 4), f(base + v * 12 + 8)])
        })
        .collect()
}

/// The model: triangles in columns, for the area, inside and depth queries.
pub struct Model {
    tris: Vec<[P; 3]>,
    ids: Vec<[u32; 3]>,
    cols: HashMap<(i32, i32), Vec<u32>>,
    open_edges: usize,
    nonmanifold_edges: usize,
}

pub const COL: f64 = 2.0;
pub fn col(v: f64) -> i32 {
    (v / COL).floor() as i32
}

impl Model {
    fn new(tris: Vec<[P; 3]>) -> Self {
        let mut cols: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
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
        let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &ids {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *uses.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        let open_edges = uses.values().filter(|&&n| n == 1).count();
        let nonmanifold_edges = uses.values().filter(|&&n| n > 2).count();
        Self { tris, ids, cols, open_edges, nonmanifold_edges }
    }

    /// Winding number of a vertical ray.
    fn inside(&self, p: P) -> bool {
        let Some(list) = self.cols.get(&(col(p[0]), col(p[1]))) else { return false };
        let mut w = 0;
        for &i in list {
            let [a, b, c] = self.tris[i as usize];
            let d = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if d.abs() < 1e-14 {
                continue;
            }
            let u = ((p[0] - a[0]) * (c[1] - a[1]) - (p[1] - a[1]) * (c[0] - a[0])) / d;
            let v = ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])) / d;
            if u < 0.0 || v < 0.0 || u + v >= 1.0 {
                continue;
            }
            if a[2] + (b[2] - a[2]) * u + (c[2] - a[2]) * v > p[2] {
                w += if d > 0.0 { 1 } else { -1 };
            }
        }
        w != 0
    }

    /// Inside by any test that can see it on an imperfect body: the winding number of a vertical ray,
    /// an odd count of its crossings (a reversed shell overlapping another), or lying behind the
    /// nearest triangle (an open body, whose ray can miss).
    fn inside_any(&self, p: P) -> bool {
        if self.inside(p) {
            return true;
        }
        let mut crossings = 0;
        if let Some(list) = self.cols.get(&(col(p[0]), col(p[1]))) {
            for &i in list {
                let [a, b, c] = self.tris[i as usize];
                let d = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                if d.abs() < 1e-14 {
                    continue;
                }
                let u = ((p[0] - a[0]) * (c[1] - a[1]) - (p[1] - a[1]) * (c[0] - a[0])) / d;
                let v = ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])) / d;
                if u >= 0.0 && v >= 0.0 && u + v < 1.0 && a[2] + (b[2] - a[2]) * u + (c[2] - a[2]) * v > p[2] {
                    crossings += 1;
                }
            }
        }
        if crossings % 2 == 1 {
            return true;
        }
        let r = [BEHIND_REACH; 3];
        let mut best: Option<(f64, f64)> = None;
        for &i in &self.near(sub(p, r), add(p, r)) {
            let t = &self.tris[i as usize];
            let q = closest_on_tri(p, t);
            let d = dist(p, q);
            if d <= BEHIND_REACH && best.is_none_or(|b| d < b.0) {
                best = Some((d, dot(sub(p, q), cross(sub(t[1], t[0]), sub(t[2], t[0])))));
            }
        }
        best.is_some_and(|b| b.1 < 0.0)
    }

    /// Triangles whose box meets the box.
    fn near(&self, lo: P, hi: P) -> Vec<u32> {
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
    fn depth(&self, p: P) -> f64 {
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

/// One closed primitive.
#[derive(Clone)]
pub struct Part {
    tris: Vec<[P; 3]>,
    lo: P,
    hi: P,
    kind: &'static str,
}

impl Part {
    fn new(tris: Vec<[P; 3]>, kind: &'static str) -> Self {
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for v in tris.iter().flatten() {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        Part { tris, lo, hi, kind }
    }

    fn sphere_like(&self) -> Option<(P, f64)> {
        let mut verts: Vec<P> = self.tris.iter().flatten().copied().collect();
        verts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        verts.dedup();
        if verts.len() < 12 {
            return None;
        }
        let c = mul(add(self.lo, self.hi), 0.5);
        let ds: Vec<f64> = verts.iter().map(|v| dist(*v, c)).collect();
        let (lo, hi) = ds.iter().fold((f64::MAX, f64::MIN), |(l, h), d| (l.min(*d), h.max(*d)));
        let ext: Vec<f64> = (0..3).map(|k| self.hi[k] - self.lo[k]).collect();
        let (emin, emax) = ext.iter().fold((f64::MAX, f64::MIN), |(l, h), e| (l.min(*e), h.max(*e)));
        (hi - lo <= 0.08 * hi + 0.01 && emin >= 0.8 * emax).then_some((c, hi))
    }

    fn edges(&self) -> Vec<(P, P)> {
        let mut seen = BTreeMap::new();
        for t in &self.tris {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let key = |p: P| p.map(|x| (x * 1e5) as i64);
                let (ka, kb) = (key(a), key(b));
                seen.entry(if ka < kb { (ka, kb) } else { (kb, ka) }).or_insert((a, b));
            }
        }
        seen.into_values().collect()
    }
}

/// One curve where a part crosses the model's surface.
#[derive(Debug, Clone)]
pub struct Loop {
    normal: P,
    area: f64,
    centre: P,
    perimeter: f64,
    closed: bool,
    /// Closed only by joining ends across a crack in the body.
    joined: bool,
    points: usize,
}

/// A vertex id per position, for keys shared between neighbouring triangles.
pub fn weld_ids(tris: &[[P; 3]]) -> (Vec<[u32; 3]>, Vec<P>) {
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
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

/// Where a pair of triangles cross: the segment's two ends, each with the edge it lies on.
#[allow(clippy::type_complexity)]
pub fn cross_segment(
    t: &[P; 3],
    ti: &[u32; 3],
    u: &[P; 3],
    ui: &[u32; 3],
) -> Option<((P, (bool, u32, u32)), (P, (bool, u32, u32)))> {
    let plane = |x: &[P; 3]| {
        let n = cross(sub(x[1], x[0]), sub(x[2], x[0]));
        (n, dot(n, x[0]))
    };
    let (nu, ou) = plane(u);
    let (nt, ot) = plane(t);
    if len(nu) < 1e-14 || len(nt) < 1e-14 {
        return None;
    }
    // A vertex exactly on the plane counts as on its positive side, the same for every pair: it keeps
    // neighbouring triangles' answers consistent, and a part lying on the surface does not cross it.
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

/// The curves where a part crosses the body.
pub fn loops_of(part: &Part, model: &Model) -> Vec<Loop> {
    let (ids, _) = weld_ids(&part.tris);
    type Key = (bool, u32, u32, u32);
    let mut segments: Vec<(Key, P, Key, P)> = Vec::new();
    let candidates = model.near(part.lo, part.hi);
    for (ti, t) in part.tris.iter().enumerate() {
        let lo: P = [0, 1, 2].map(|k| t.iter().map(|v| v[k]).fold(f64::MAX, f64::min));
        let hi: P = [0, 1, 2].map(|k| t.iter().map(|v| v[k]).fold(f64::MIN, f64::max));
        for &b in &candidates {
            let u = &model.tris[b as usize];
            if (0..3).any(|k| {
                u.iter().map(|v| v[k]).fold(f64::MIN, f64::max) < lo[k]
                    || u.iter().map(|v| v[k]).fold(f64::MAX, f64::min) > hi[k]
            }) {
                continue;
            }
            if let Some(((pa, fa), (pb, fb))) = cross_segment(t, &ids[ti], u, &model.ids[b as usize]) {
                // An end on the part's edge is named with the body triangle, and the reverse.
                let name = |f: (bool, u32, u32)| (f.0, f.1, f.2, if f.0 { ti as u32 } else { b });
                segments.push((name(fa), pa, name(fb), pb));
            }
        }
    }
    chain(&segments)
}

/// Chains segments through their named ends.
pub fn chain<K: Ord + Copy>(segments: &[(K, P, K, P)]) -> Vec<Loop> {
    let mut at: BTreeMap<K, Vec<u32>> = BTreeMap::new();
    for (i, s) in segments.iter().enumerate() {
        at.entry(s.0).or_default().push(i as u32);
        at.entry(s.2).or_default().push(i as u32);
    }
    let mut used = vec![false; segments.len()];
    let mut walks: Vec<(Vec<P>, bool)> = Vec::new();
    let walk = |start: K, used: &mut Vec<bool>, at: &BTreeMap<K, Vec<u32>>| -> Option<(Vec<P>, bool)> {
        let mut node = start;
        let mut points = Vec::new();
        loop {
            let next = at[&node].iter().copied().find(|&i| !used[i as usize]);
            let Some(i) = next else { break };
            used[i as usize] = true;
            let s = &segments[i as usize];
            let (from, to, target) = if s.0 == node { (s.1, s.3, s.2) } else { (s.3, s.1, s.0) };
            if points.is_empty() {
                points.push(from);
            }
            points.push(to);
            node = target;
            if node == start {
                return Some((points, true));
            }
        }
        (!points.is_empty()).then_some((points, false))
    };
    // Open chains from their ends first, so what remains is closed.
    let ends: Vec<K> = at.iter().filter(|(_, l)| l.len() % 2 == 1).map(|(k, _)| *k).collect();
    for start in ends {
        if let Some(w) = walk(start, &mut used, &at) {
            walks.push(w);
        }
    }
    for i in 0..segments.len() {
        if !used[i] {
            if let Some(w) = walk(segments[i].0, &mut used, &at) {
                walks.push(w);
            }
        }
    }
    // Numeric cracks in a body: join open chains whose ends meet.
    let mut joined_any = vec![false; walks.len()];
    let mut i = 0;
    while i < walks.len() {
        if walks[i].1 {
            i += 1;
            continue;
        }
        let (first, last) = (walks[i].0[0], *walks[i].0.last().unwrap());
        if dist(first, last) <= JOIN_GAP {
            walks[i].1 = true;
            joined_any[i] = true;
            continue;
        }
        let mut merged = false;
        for j in 0..walks.len() {
            if j == i || walks[j].1 {
                continue;
            }
            let (f2, l2) = (walks[j].0[0], *walks[j].0.last().unwrap());
            let tail: Option<Vec<P>> = if dist(last, f2) <= JOIN_GAP {
                Some(walks[j].0.clone())
            } else if dist(last, l2) <= JOIN_GAP {
                Some(walks[j].0.iter().rev().copied().collect())
            } else {
                None
            };
            if let Some(tail) = tail {
                walks[i].0.extend(tail);
                joined_any[i] = true;
                walks.remove(j);
                joined_any.remove(j);
                if j < i {
                    i -= 1;
                }
                merged = true;
                break;
            }
        }
        if !merged {
            i += 1;
        }
    }
    walks
        .iter()
        .zip(&joined_any)
        .map(|((points, closed), joined)| {
            let mut vector = [0.0; 3];
            let mut perimeter = 0.0;
            let mut centre = [0.0; 3];
            let origin = points[0];
            for k in 0..points.len() {
                let (a, b) = (points[k], points[(k + 1) % points.len()]);
                vector = add(vector, cross(sub(a, origin), sub(b, origin)));
                if k + 1 < points.len() || *closed {
                    let l = dist(a, b);
                    perimeter += l;
                    centre = add(centre, mul(add(a, b), l / 2.0));
                }
            }
            Loop {
                area: len(vector) / 2.0,
                normal: unit(vector),
                centre: mul(centre, 1.0 / f64::max(perimeter, 1e-12)),
                perimeter,
                closed: *closed,
                joined: *joined,
                points: points.len(),
            }
        })
        .collect()
}

pub fn shells_at(tris: &[[P; 3]], scale: f64) -> Vec<Vec<[P; 3]>> {
    let key = |p: P| p.map(|x| (x * scale).round() as i64);
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let mut parent: Vec<u32> = (0..tris.len() as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            let up = p[p[x as usize] as usize];
            p[x as usize] = up;
            x = up;
        }
        x
    }
    for (i, t) in tris.iter().enumerate() {
        for v in t {
            let other = *ids.entry(key(*v)).or_insert(i as u32);
            let (a, b) = (find(&mut parent, i as u32), find(&mut parent, other));
            parent[a as usize] = b;
        }
    }
    let mut groups: HashMap<u32, usize> = HashMap::new();
    let mut out: Vec<Vec<[P; 3]>> = Vec::new();
    for (i, t) in tris.iter().enumerate() {
        let r = find(&mut parent, i as u32);
        let slot = *groups.entry(r).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[slot].push(*t);
    }
    out
}

pub fn v3(v: &Value) -> Option<P> {
    Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?])
}

/// A cone shell in the built mesh: a frustum running from the tip's contact to its socket.
#[derive(Clone)]
pub struct BuiltTip {
    pos: P,
    socket: P,
    radius: f64,
    contact_d: f64,
    disk: bool,
    normal: P,
}

pub fn built_tips(entity: &Value) -> Vec<BuiltTip> {
    let info = &entity["info"];
    let mut out = Vec::new();
    for key in ["cone", "a", "b"] {
        let c = &info[key];
        let Some(pos) = v3(&c["pos"]) else { continue };
        if let Some(socket) = v3(&c["socket"]) {
            let (cd, bd) = (c["contact_d"].as_f64().unwrap_or(0.1), c["body_d"].as_f64().unwrap_or(0.1));
            let normal = v3(&c["surfaceNormal"]).unwrap_or([0.0, 0.0, 1.0]);
            out.push(BuiltTip { pos, socket, radius: cd.max(bd) / 2.0, contact_d: cd, disk: false, normal });
        } else {
            let cd = c["contact_d"].as_f64().unwrap_or(0.1);
            let standoff = c["standoff"].as_f64().unwrap_or(0.1);
            let normal = v3(&c["surfaceNormal"]).unwrap_or([0.0, 0.0, 1.0]);
            out.push(BuiltTip {
                pos,
                socket: add(pos, mul(normal, standoff + cd)),
                radius: cd / 2.0,
                contact_d: cd,
                disk: true,
                normal,
            });
        }
    }
    out
}

/// A built cone with its base run on into the model by `by`, as the slicer's tip offset does.
pub fn with_penetration(part: &Part, tip: &BuiltTip, normal: P, by: f64) -> Part {
    let moved = part
        .tris
        .iter()
        .map(|t| t.map(|v| if dist(v, tip.pos) <= tip.contact_d / 2.0 + 0.01 { sub(v, mul(normal, by)) } else { v }))
        .collect();
    Part::new(moved, "tip")
}

pub fn dedupe(tris: &[[P; 3]]) -> Vec<[P; 3]> {
    let mut seen = std::collections::HashSet::new();
    tris.iter()
        .filter(|t| {
            let keys = t.map(|v| v.map(|x| (x * 1e5).round() as i64));
            let least = (0..3).min_by_key(|&i| keys[i]).unwrap();
            seen.insert([keys[least], keys[(least + 1) % 3], keys[(least + 2) % 3]])
        })
        .copied()
        .collect()
}

fn built_parts(dir: &Path, id: &str) -> (Vec<Part>, Vec<BuiltTip>, BTreeMap<&'static str, usize>) {
    let tris = read_stl(&dir.join(format!("{id}.df_support.stl")));
    let labels: Vec<i32> = std::fs::read(dir.join(format!("{id}.df_labels.bin")))
        .unwrap_or_default()
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let df: Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{id}.df.json"))).unwrap()).unwrap();
    let entities = df["entities"].as_array().cloned().unwrap_or_default();
    let mut by: BTreeMap<i32, Vec<[P; 3]>> = BTreeMap::new();
    for (t, l) in tris.iter().zip(&labels) {
        by.entry(*l).or_default().push(*t);
    }
    let mut parts = Vec::new();
    let mut tips = Vec::new();
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (label, group) in by {
        let entity = if label >= 0 { entities.get(label as usize) } else { None };
        let ty = entity.and_then(|e| e["type"].as_str()).unwrap_or("?");
        let own: Vec<BuiltTip> = entity.map(built_tips).unwrap_or_default();
        tips.extend(own.iter().cloned());
        let has_cone = !own.is_empty();
        // DragonFruit's primitives touch each other to about 1e-4 (a cone's wide ring on its joint's
        // sphere), and share their own vertices exactly.
        for shell in shells_at(&group, 1e6) {
            let part_probe = Part::new(shell, "?");
            let verts: Vec<P> = part_probe.tris.iter().flatten().copied().collect();
            let is_tip = own.iter().any(|t| {
                if t.disk {
                    verts.iter().all(|v| dist(*v, t.pos) <= t.contact_d + dist(t.pos, t.socket) + 0.1)
                } else {
                    // The disk stands along the surface normal and the cone runs from there to the socket,
                    // which on a slanted surface is not the line from the contact to the socket.
                    let start = add(t.pos, mul(t.normal, (t.contact_d / 2.0).max(0.1)));
                    // The slicer's tip offset runs the cone's base on below the contact, along the normal.
                    let below = sub(t.pos, mul(t.normal, 0.3));
                    verts
                        .iter()
                        .all(|v| seg_dist(*v, below, start).min(seg_dist(*v, start, t.socket)) <= t.radius + 0.02)
                        && verts.iter().map(|v| dist(*v, t.pos)).fold(f64::MAX, f64::min) <= t.radius + 0.35
                }
            });
            // merge r7: a sphere tip (cball): a ball whose centre lies within its radius + 0.15 mm of a tip's contact,
            // nearer the contact than the socket (a joint ball sits at the socket).
            let is_sphere_tip = !is_tip
                && part_probe.sphere_like().is_some_and(|(c, r)| {
                    own.iter().any(|t| !t.disk && dist(c, t.pos) <= r + 0.15 && dist(c, t.pos) < dist(c, t.socket))
                });
            let kind: &'static str = if ty == "root" {
                "root"
            } else if is_tip || is_sphere_tip {
                "tip"
            } else if ty == "braces" {
                "brace"
            } else if part_probe.sphere_like().is_some() {
                "joint"
            } else {
                match ty {
                    "kickstands" => "kickstand",
                    "trunks" if !has_cone => "post",
                    "trunks" | "branches" | "twigs" | "sticks" => "shaft",
                    _ => "other",
                }
            };
            *counts.entry(kind).or_insert(0) += 1;
            parts.push(Part { kind, ..part_probe });
        }
    }
    (parts, tips, counts)
}

include!("touch.rs");
/// A shell's closed solids, as the reader splits them: paired at edges with more than two triangles
/// by winding and angle.
pub fn solids(tris: &[[P; 3]]) -> Vec<Vec<[P; 3]>> {
    let key = |p: P| p.map(|x| (x * 1e4).round() as i64);
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
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
    let mut edges: HashMap<(u32, u32), Vec<(u32, bool, u32)>> = HashMap::new();
    for (k, &[a, b, c]) in idx.iter().enumerate() {
        for (x, y, z) in [(a, b, c), (b, c, a), (c, a, b)] {
            edges.entry((x.min(y), x.max(y))).or_default().push((k as u32, x < y, z));
        }
    }
    let mut parent: Vec<u32> = (0..idx.len() as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            let up = p[p[x as usize] as usize];
            p[x as usize] = up;
            x = up;
        }
        x
    }
    for (&(a, b), list) in &edges {
        if list.len() == 2 {
            let (x, y) = (find(&mut parent, list[0].0), find(&mut parent, list[1].0));
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
                    let open: u32 = stack.pop().unwrap();
                    let (x, y) = (find(&mut parent, open), find(&mut parent, k));
                    parent[x as usize] = y;
                }
            }
        }
    }
    let mut groups: HashMap<u32, usize> = HashMap::new();
    let mut out: Vec<Vec<[P; 3]>> = Vec::new();
    for i in 0..idx.len() {
        let r = find(&mut parent, i as u32);
        let slot = *groups.entry(r).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[slot].push(tris[i]);
    }
    out
}
