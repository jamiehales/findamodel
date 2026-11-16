//! The fused reader: a support solid welded into one shell (the support side of a twin or welded
//! split, rods, braces, raft and tips all one solid) read into straight stretches and the joins
//! between them, for [`super::classify`] to route and lay out as it does any split solid.
//!
//! 1. **Slices.** The solid is cut every [`DZ`] mm. Each triangle crossing a plane gives one
//!    segment, chained into loops by mesh edge (exact, no float matching); outer loops are blobs,
//!    inner loops their holes.
//! 2. **Blob graph.** Blobs in consecutive slices that overlap are linked: the solid's Reeb graph of
//!    height.
//! 3. **Walks.** Each contact facing down or sideways seeds a walk in the blob just under its loop.
//!    The walk follows the rod's core down (a mean-shift centre within the rod's radius, so a brace
//!    or branch welded on one side does not pull it), at a fork taking the blob that holds the
//!    core's predicted place. It ends on the ground (the raft band, or a raft-sized blob), at a
//!    contact facing up (a strut's lower tip on the model), or at a dead end.
//! 4. **Tree.** Walks ending on one ground blob form a trie up from it. At each fork the child that
//!    best continues the line below is the trunk; the others are branches hanging from it there.
//! 5. **Widths** are measured at the core: twice the median distance from the core to the blob's
//!    edge over [`WIDTH_RAYS`] directions, which a weld on one side does not inflate.
//! 6. **Braces** come only from the solid's own leftover blobs: those no walk owns, joined into
//!    components, whose attachments to two supports are paired by the way they leave; and level
//!    bars, blobs two supports share over a short run of height, above the root zone (touching
//!    cone feet are not bars). Every brace's centre line must lie inside the solid
//!    ([`MATERIAL_SHARE`] of its samples inside a blob of their slice): none crosses empty space.
//!
//! Every walk runs on its own and every map is walked in key order, so the reading is the same on
//! every run and every thread count.

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::Serialize;

use super::math::{P, add, cross, dist, dot, len, mul, sub};
use super::pieces::{Section, SectionKind, Segment};

/// Slice spacing (mm). The walk's windows are counted in slices and tuned for it.
pub const DZ: f64 = 0.1;
/// A closed solid no straight split reads is read here when it holds more triangles than this ...
pub const MIN_TRIANGLES: usize = 20_000;
/// ... or at least this many contacts.
pub const MIN_CONTACTS: usize = 6;
/// A walk's line is judged over this many slices.
const LINE_WINDOW: usize = 20;
/// The core is the blob's centre within this share of the rod's radius of the predicted centre.
const CORE_R: f64 = 1.15;
/// A blob at least this large (mm2) within [`GROUND_REACH_MM`] above the raft band is ground.
const RAFT_AREA_MM2: f64 = 30.0;
const GROUND_REACH_MM: f64 = 6.0;
/// Directions the core width is measured in.
pub const WIDTH_RAYS: usize = 16;
/// The socket is where the width first reaches this share of the shaft's.
const SOCKET_SHARE: f64 = 0.85;
/// Bends kept on a support's line (as the classifier's `STRAIGHT_MM`).
const STRAIGHT_MM: f64 = 0.12;
/// A level bar is a shared run of blobs no taller than this; a longer one is two walks on one rod.
const LEVEL_MAX_RUN_MM: f64 = 1.2;
/// No level bar within this of the raft band: touching cone feet merge blobs there.
pub const ROOT_ZONE_MM: f64 = 2.5;
/// A brace's attachments pair when both leave towards the other within this score (sum of one less
/// the cosines).
const PAIR_SCORE: f64 = 0.6;
/// Ends still open after that pair within this score, where the solid runs between them.
const LOOSE_PAIR_SCORE: f64 = 1.5;
/// Share of a brace's centre-line samples that must lie inside the solid.
pub const MATERIAL_SHARE: f64 = 0.9;
/// A walk's dead end within this (plus the contact's radius) of a contact facing up ends there.
const FOOT_REACH_MM: f64 = 1.0;
/// A contact whose way out of the model rises more than this is a lower tip (facing up).
const UP_DZ: f64 = 0.3;

/// A contact for the fused reader: centre, width and the way out of the model along the support.
#[derive(Debug, Clone, Copy)]
pub struct Seed {
    pub at: P,
    pub d: f64,
    pub dir: P,
}

/// What a fused reading found, for logs and checks.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Stats {
    pub triangles: usize,
    pub slices: usize,
    pub blobs: usize,
    pub broken_loops: usize,
    pub seeds: usize,
    pub unseeded: usize,
    pub ground: usize,
    pub struts: usize,
    pub dead: usize,
    pub trunks: usize,
    pub branches: usize,
    pub braces: usize,
    pub level_bars: usize,
    pub root_zone_bars: usize,
    pub refused_braces: usize,
    pub posts: usize,
    /// Brace ends left unpaired, in components joining two or more supports.
    pub dangling: usize,
    /// Leftover components joining one support only (weld fillets, stubs): no brace.
    pub stubs: usize,
    pub ms: f64,
}

/// The reading: straight stretches, the joins between them (stretch, stretch, where), and stats.
#[derive(Debug, Clone, Default)]
pub struct Reading {
    pub segments: Vec<Segment>,
    pub links: Vec<(usize, usize, P)>,
    pub stats: Stats,
}

// ---------------------------------------------------------------------------------------------
// Slices

struct Mesh {
    pos: Vec<P>,
    tris: Vec<[u32; 3]>,
    /// Per triangle, the edge id of edge i (corner i to corner i + 1).
    tedge: Vec<[u32; 3]>,
}

impl Mesh {
    fn new(tv: &[[P; 3]]) -> Mesh {
        let mut vid: FxHashMap<[i64; 3], u32> = FxHashMap::default();
        let mut pos = Vec::new();
        let mut tris = Vec::with_capacity(tv.len());
        for t in tv {
            let ids = t.map(|v| {
                let k = v.map(|x| (x * 1e5).round() as i64);
                *vid.entry(k).or_insert_with(|| {
                    pos.push(v);
                    pos.len() as u32 - 1
                })
            });
            if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
                tris.push(ids);
            }
        }
        let mut eid: FxHashMap<(u32, u32), u32> = FxHashMap::default();
        let tedge = tris
            .iter()
            .map(|t| {
                let mut e = [0u32; 3];
                for (i, x) in e.iter_mut().enumerate() {
                    let (a, b) = (t[i], t[(i + 1) % 3]);
                    let n = eid.len() as u32;
                    *x = *eid.entry((a.min(b), a.max(b))).or_insert(n);
                }
                e
            })
            .collect();
        Mesh { pos, tris, tedge }
    }
}

#[derive(Clone)]
struct Blob {
    area: f64,
    c: [f64; 2],
    lo: [f64; 2],
    hi: [f64; 2],
    /// Outer loop, then hole loops.
    loops: Vec<Vec<[f64; 2]>>,
}

impl Blob {
    fn holds(&self, x: f64, y: f64) -> bool {
        x >= self.lo[0]
            && x <= self.hi[0]
            && y >= self.lo[1]
            && y <= self.hi[1]
            && inside(&self.loops[0], x, y)
            && !self.loops[1..].iter().any(|h| inside(h, x, y))
    }

    /// Distance from `(x, y)` along `(dx, dy)` to the blob's edge (its nearest crossing).
    fn ray(&self, x: f64, y: f64, dx: f64, dy: f64) -> f64 {
        let mut best = f64::INFINITY;
        for l in &self.loops {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
                let den = dx * ey - dy * ex;
                if den.abs() < 1e-15 {
                    continue;
                }
                let (wx, wy) = (a[0] - x, a[1] - y);
                let t = (wx * ey - wy * ex) / den;
                let u = (wx * dy - wy * dx) / den;
                if t >= 0.0 && (0.0..=1.0).contains(&u) {
                    best = best.min(t);
                }
            }
        }
        best
    }
}

struct Slices {
    z0: f64,
    z: Vec<f64>,
    blobs: Vec<Vec<Blob>>,
    broken: usize,
}

impl Slices {
    fn slice_of(&self, z: f64) -> Option<usize> {
        let k = ((z - self.z0) / DZ - 0.5).round();
        (k >= 0.0 && (k as usize) < self.z.len()).then_some(k as usize)
    }

    /// Whether `p` lies inside the solid (inside a blob of the nearest slice).
    fn solid_at(&self, p: P) -> bool {
        self.slice_of(p[2]).is_some_and(|k| self.blobs[k].iter().any(|b| b.holds(p[0], p[1])))
    }
}

fn shoelace(l: &[[f64; 2]]) -> (f64, [f64; 2]) {
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    let o = l[0];
    for i in 0..l.len() {
        let p = [l[i][0] - o[0], l[i][1] - o[1]];
        let q = l[(i + 1) % l.len()];
        let q = [q[0] - o[0], q[1] - o[1]];
        let cr = p[0] * q[1] - q[0] * p[1];
        a += cr;
        cx += (p[0] + q[0]) * cr;
        cy += (p[1] + q[1]) * cr;
    }
    let a2 = a / 2.0;
    if a2.abs() < 1e-14 {
        return (0.0, o);
    }
    (a2, [o[0] + cx / (6.0 * a2), o[1] + cy / (6.0 * a2)])
}

fn inside(l: &[[f64; 2]], x: f64, y: f64) -> bool {
    let mut c = false;
    let n = l.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (l[i][0], l[i][1]);
        let (xj, yj) = (l[j][0], l[j][1]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            c = !c;
        }
        j = i;
    }
    c
}

/// The blobs of one plane at height `h`, from the triangles `cand` (indices) that may cross it.
fn slice_one(m: &Mesh, cand: &[u32], h: f64) -> (Vec<Blob>, usize) {
    // Segments: (edge going down, edge going up, the point on the first).
    let mut segs: Vec<(u32, u32, [f64; 2])> = Vec::new();
    for &ti in cand {
        let t = m.tris[ti as usize];
        let zs = t.map(|v| m.pos[v as usize][2]);
        let lo = zs[0].min(zs[1]).min(zs[2]);
        let hi = zs[0].max(zs[1]).max(zs[2]);
        if !(lo < h && h <= hi) {
            continue;
        }
        let above = zs.map(|z| z >= h);
        let (mut down, mut up) = (None, None);
        for i in 0..3 {
            let j = (i + 1) % 3;
            if above[i] != above[j] {
                let (a, b) = (m.pos[t[i] as usize], m.pos[t[j] as usize]);
                let f = (h - a[2]) / (b[2] - a[2]);
                let p = [a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])];
                if above[i] {
                    down = Some((m.tedge[ti as usize][i], p));
                } else {
                    up = Some((m.tedge[ti as usize][i], p));
                }
            }
        }
        if let (Some(d), Some(u)) = (down, up) {
            segs.push((d.0, u.0, d.1));
        }
    }
    // Chained by edge, each loop started at its segment with the smallest edge id, so loops and
    // their sums come out the same whatever order the segments came in.
    segs.sort_unstable_by_key(|s| (s.0, s.1));
    let by_start: FxHashMap<u32, usize> = segs.iter().enumerate().map(|(i, s)| (s.0, i)).collect();
    let mut used = vec![false; segs.len()];
    let mut loops: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut broken = 0;
    for i in 0..segs.len() {
        if used[i] {
            continue;
        }
        let mut l = Vec::new();
        let mut cur = i;
        let mut ok = false;
        loop {
            used[cur] = true;
            l.push(segs[cur].2);
            let e = segs[cur].1;
            if e == segs[i].0 {
                ok = true;
                break;
            }
            match by_start.get(&e) {
                Some(&q) if !used[q] => cur = q,
                _ => break,
            }
        }
        if ok && l.len() >= 3 {
            loops.push(l);
        } else {
            broken += 1;
        }
    }
    let mut outers: Vec<Blob> = Vec::new();
    let mut holes: Vec<(f64, Vec<[f64; 2]>)> = Vec::new();
    for l in loops {
        let (a, c) = shoelace(&l);
        if a > 0.0 {
            let mut lo = [f64::MAX; 2];
            let mut hi = [f64::MIN; 2];
            for p in &l {
                for d in 0..2 {
                    lo[d] = lo[d].min(p[d]);
                    hi[d] = hi[d].max(p[d]);
                }
            }
            outers.push(Blob { area: a, c, lo, hi, loops: vec![l] });
        } else if a < 0.0 {
            holes.push((a, l));
        }
    }
    for (a, l) in holes {
        let (x, y) = (l[0][0], l[0][1]);
        let mut best: Option<usize> = None;
        for (i, o) in outers.iter().enumerate() {
            if x >= o.lo[0]
                && x <= o.hi[0]
                && y >= o.lo[1]
                && y <= o.hi[1]
                && inside(&o.loops[0], x, y)
                && best.is_none_or(|b| outers[b].area > o.area)
            {
                best = Some(i);
            }
        }
        if let Some(b) = best {
            outers[b].area += a;
            outers[b].loops.push(l);
        }
    }
    outers.retain(|o| o.area > 1e-6);
    // In a fixed order: by centre.
    outers.sort_by(|a, b| a.c[0].total_cmp(&b.c[0]).then(a.c[1].total_cmp(&b.c[1])));
    (outers, broken)
}

fn slice(m: &Mesh) -> Slices {
    let zmin = m.pos.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
    let zmax = m.pos.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    let z0 = zmin;
    let nk = (((zmax - zmin) / DZ).floor() as usize).max(1);
    let plane = |k: usize| z0 + (k as f64 + 0.5) * DZ;
    // Per triangle the first and last plane it crosses (some corner below, some at or above).
    let mut order: Vec<(u32, u32, u32)> = m
        .tris
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let lo = t.iter().map(|&v| m.pos[v as usize][2]).fold(f64::MAX, f64::min);
            let hi = t.iter().map(|&v| m.pos[v as usize][2]).fold(f64::MIN, f64::max);
            let k0 = (((lo - z0) / DZ - 0.5).floor() as i64 + 1).max(0);
            let k1 = (((hi - z0) / DZ - 0.5).floor() as i64).min(nk as i64 - 1);
            (k0 <= k1).then_some((k0 as u32, k1 as u32, i as u32))
        })
        .collect();
    order.par_sort_unstable();
    const SLAB: usize = 16;
    let starts: Vec<u32> = order.iter().map(|o| o.0).collect();
    let res: Vec<(Vec<Vec<Blob>>, usize)> = (0..nk.div_ceil(SLAB))
        .into_par_iter()
        .map(|s| {
            let (ka, kb) = (s * SLAB, ((s + 1) * SLAB).min(nk));
            let end = starts.partition_point(|&k| (k as usize) < kb);
            let cand: Vec<u32> = order[..end].iter().filter(|o| o.1 as usize >= ka).map(|o| o.2).collect();
            let mut broken = 0;
            let out = (ka..kb)
                .map(|k| {
                    let (b, n) = slice_one(m, &cand, plane(k));
                    broken += n;
                    b
                })
                .collect();
            (out, broken)
        })
        .collect();
    let mut blobs = Vec::with_capacity(nk);
    let mut broken = 0;
    for (b, n) in res {
        blobs.extend(b);
        broken += n;
    }
    Slices { z0, z: (0..nk).map(plane).collect(), blobs, broken }
}

/// Whether two blobs of consecutive slices overlap.
fn overlap(a: &Blob, b: &Blob) -> bool {
    if a.hi[0] < b.lo[0] || b.hi[0] < a.lo[0] || a.hi[1] < b.lo[1] || b.hi[1] < a.lo[1] {
        return false;
    }
    if b.holds(a.c[0], a.c[1]) || a.holds(b.c[0], b.c[1]) {
        return true;
    }
    let (s, l) = if a.loops[0].len() <= b.loops[0].len() { (a, b) } else { (b, a) };
    s.loops[0].iter().any(|p| l.holds(p[0], p[1]))
        || l.loops[0].iter().step_by((l.loops[0].len() / 64).max(1)).any(|p| s.holds(p[0], p[1]))
}

// ---------------------------------------------------------------------------------------------
// Blob graph

struct Graph {
    off: Vec<usize>,
    node_k: Vec<u32>,
    up: Vec<Vec<u32>>,
    down: Vec<Vec<u32>>,
}

impl Graph {
    fn blob<'a>(&self, s: &'a Slices, n: u32) -> &'a Blob {
        let k = self.node_k[n as usize] as usize;
        &s.blobs[k][n as usize - self.off[k]]
    }

    fn z(&self, s: &Slices, n: u32) -> f64 {
        s.z[self.node_k[n as usize] as usize]
    }

    fn pos(&self, s: &Slices, n: u32) -> P {
        let b = self.blob(s, n);
        [b.c[0], b.c[1], self.z(s, n)]
    }
}

fn link(s: &Slices) -> Graph {
    let nk = s.blobs.len();
    let mut off = Vec::with_capacity(nk + 1);
    let mut t = 0;
    for b in &s.blobs {
        off.push(t);
        t += b.len();
    }
    off.push(t);
    let mut node_k = vec![0u32; t];
    for k in 0..nk {
        for x in &mut node_k[off[k]..off[k + 1]] {
            *x = k as u32;
        }
    }
    let pairs: Vec<Vec<(u32, u32)>> = (0..nk.saturating_sub(1))
        .into_par_iter()
        .map(|k| {
            let (a, b) = (&s.blobs[k], &s.blobs[k + 1]);
            let key = |x: f64| x.floor() as i64;
            let mut grid: FxHashMap<(i64, i64), Vec<u32>> = FxHashMap::default();
            for (j, bb) in b.iter().enumerate() {
                for gx in key(bb.lo[0])..=key(bb.hi[0]) {
                    for gy in key(bb.lo[1])..=key(bb.hi[1]) {
                        grid.entry((gx, gy)).or_default().push(j as u32);
                    }
                }
            }
            let mut out = Vec::new();
            let mut seen: Vec<u32> = Vec::new();
            for (i, ba) in a.iter().enumerate() {
                seen.clear();
                for gx in key(ba.lo[0])..=key(ba.hi[0]) {
                    for gy in key(ba.lo[1])..=key(ba.hi[1]) {
                        if let Some(l) = grid.get(&(gx, gy)) {
                            seen.extend(l);
                        }
                    }
                }
                seen.sort_unstable();
                seen.dedup();
                for &j in &seen {
                    if overlap(ba, &b[j as usize]) {
                        out.push(((off[k] + i) as u32, (off[k + 1] + j as usize) as u32));
                    }
                }
            }
            out
        })
        .collect();
    let mut up = vec![Vec::new(); t];
    let mut down = vec![Vec::new(); t];
    for p in pairs {
        for (a, b) in p {
            up[a as usize].push(b);
            down[b as usize].push(a);
        }
    }
    Graph { off, node_k, up, down }
}

// ---------------------------------------------------------------------------------------------
// Walks

#[derive(Debug, Clone, Copy, PartialEq)]
enum End {
    Ground,
    /// On the lower tip (facing up) of this contact: a strut.
    Foot(usize),
    Dead,
}

struct Walk {
    nodes: Vec<u32>,
    /// The rod's core at each node.
    core: Vec<P>,
    end: End,
}

struct Ground {
    z: f64,
}

impl Ground {
    fn holds(&self, g: &Graph, s: &Slices, n: u32) -> bool {
        let z = g.z(s, n);
        z <= self.z || (z < self.z + GROUND_REACH_MM && g.blob(s, n).area > RAFT_AREA_MM2)
    }
}

/// The blob a seed's walk starts in: the one at or nearest a point just out of the loop along the
/// support, in the slice under it.
fn start_node(g: &Graph, s: &Slices, seed: &Seed) -> Option<u32> {
    let p = add(seed.at, mul(seed.dir, 0.15 + seed.d / 4.0));
    let k = s.slice_of(p[2] - DZ)?;
    let mut best: Option<(f64, u32)> = None;
    let reach = 0.4 + seed.d / 2.0;
    for (i, b) in s.blobs[k].iter().enumerate() {
        if p[0] < b.lo[0] - reach || p[0] > b.hi[0] + reach || p[1] < b.lo[1] - reach || p[1] > b.hi[1] + reach {
            continue;
        }
        let d = if b.holds(p[0], p[1]) {
            0.0
        } else {
            b.loops[0].iter().map(|q| ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt()).fold(f64::MAX, f64::min)
        };
        if d < reach && best.is_none_or(|bb| d < bb.0) {
            best = Some((d, (g.off[k] + i) as u32));
        }
    }
    best.map(|b| b.1)
}

/// Mean-shift centre of the blob within `r` of `at`.
fn core(b: &Blob, at: [f64; 2], r: f64) -> [f64; 2] {
    let mut c = at;
    for _ in 0..3 {
        let st = r / 5.0;
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0usize);
        for i in -5..=5 {
            for j in -5..=5 {
                let (dx, dy) = (i as f64 * st, j as f64 * st);
                if dx * dx + dy * dy > r * r {
                    continue;
                }
                let (x, y) = (c[0] + dx, c[1] + dy);
                if b.holds(x, y) {
                    sx += x;
                    sy += y;
                    n += 1;
                }
            }
        }
        if n == 0 {
            return b.loops[0]
                .iter()
                .copied()
                .min_by(|a, d| {
                    ((a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2))
                        .total_cmp(&((d[0] - c[0]).powi(2) + (d[1] - c[1]).powi(2)))
                })
                .unwrap_or(b.c);
        }
        let nc = [sx / n as f64, sy / n as f64];
        let moved = ((nc[0] - c[0]).powi(2) + (nc[1] - c[1]).powi(2)).sqrt();
        c = nc;
        if moved < st * 0.2 {
            break;
        }
    }
    c
}

/// The rod's width at its core `c` in blob `b`: twice the median distance to the blob's edge over
/// [`WIDTH_RAYS`] directions, less the stretch of the cut of a rod leaning `tan` (horizontal per
/// vertical).
fn core_width(b: &Blob, c: [f64; 2], tan: f64) -> f64 {
    let mut rs: Vec<f64> = (0..WIDTH_RAYS)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / WIDTH_RAYS as f64;
            b.ray(c[0], c[1], a.cos(), a.sin())
        })
        .filter(|r| r.is_finite())
        .collect();
    if rs.is_empty() {
        return 2.0 * (b.area / std::f64::consts::PI).sqrt();
    }
    rs.sort_by(f64::total_cmp);
    let cos = 1.0 / (1.0 + tan * tan).sqrt();
    2.0 * rs[rs.len() / 2] * cos.sqrt()
}

fn walk(g: &Graph, s: &Slices, ground: &Ground, start: u32, feet: &[(usize, Seed)]) -> Walk {
    let mut nodes = vec![start];
    let b0 = g.blob(s, start);
    let mut core_xy = b0.c;
    let mut cores = vec![[b0.c[0], b0.c[1], g.z(s, start)]];
    let mut r_run = (b0.area / std::f64::consts::PI).sqrt();
    let mut cur = start;
    loop {
        if ground.holds(g, s, cur) {
            return Walk { nodes, core: cores, end: End::Ground };
        }
        let d = &g.down[cur as usize];
        if d.is_empty() {
            let here = *cores.last().expect("a walk has a node");
            let foot = feet
                .iter()
                .filter(|(_, f)| dist(f.at, here) < FOOT_REACH_MM + f.d / 2.0)
                .min_by(|a, b| dist(a.1.at, here).total_cmp(&dist(b.1.at, here)));
            let end = foot.map_or(End::Dead, |f| End::Foot(f.0));
            return Walk { nodes, core: cores, end };
        }
        let n = cores.len();
        let back = cores[n - 1 - (n - 1).min(LINE_WINDOW)];
        let steps = (n - 1).clamp(1, LINE_WINDOW) as f64;
        let dir = [(core_xy[0] - back[0]) / steps, (core_xy[1] - back[1]) / steps];
        let pred = [core_xy[0] + dir[0], core_xy[1] + dir[1]];
        let r_est = r_run.max(0.08);
        let next = if d.len() == 1 {
            d[0]
        } else {
            let mut best = (f64::MAX, d[0]);
            for &c in d {
                let b = g.blob(s, c);
                let sc = |q: [f64; 2]| {
                    if b.holds(q[0], q[1]) {
                        0.0
                    } else {
                        let cc = core(b, q, 2.0 * r_est);
                        ((cc[0] - q[0]).powi(2) + (cc[1] - q[1]).powi(2)).sqrt()
                    }
                };
                // Along the line first; straight down a little behind.
                let e = sc(pred).min(sc(core_xy) + 0.5 * r_est);
                if e < best.0 || (e == best.0 && b.area > g.blob(s, best.1).area) {
                    best = (e, c);
                }
            }
            best.1
        };
        let b = g.blob(s, next);
        let guess = if b.holds(pred[0], pred[1]) { pred } else { core(b, pred, 2.0 * r_est) };
        core_xy = core(b, guess, CORE_R * r_est);
        // The rod's radius: grows down a tip cone, but not into a fused joint.
        let r_new = (b.area / std::f64::consts::PI).sqrt();
        if r_new < 1.3 * r_run || (r_run < 0.3 && r_new < 2.0 * r_run) {
            r_run = 0.8 * r_run + 0.2 * r_new;
        }
        nodes.push(next);
        cores.push([core_xy[0], core_xy[1], g.z(s, next)]);
        cur = next;
        if nodes.len() > 100_000 {
            return Walk { nodes, core: cores, end: End::Dead };
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tree

/// One support: its seed's walk from the tip down to its base (the ground or foot node, or the
/// attach node on its parent, inclusive).
struct Sup {
    seed: usize,
    nodes: Vec<u32>,
    /// Parent support and the attach node.
    parent: Option<(usize, u32)>,
    end: End,
}

struct TNode {
    blob: u32,
    parent: Option<usize>,
    children: Vec<usize>,
    starts: Vec<usize>,
    routes: usize,
}

fn cosang(a: P, b: P) -> f64 {
    let (la, lb) = (len(a), len(b));
    if la == 0.0 || lb == 0.0 { 0.0 } else { dot(a, b) / (la * lb) }
}

/// The trunk/branch tree from the walks: walks ending at one node are a trie up from it; at each
/// fork the child that best continues the line below is the trunk, the others hang from it.
fn tree(g: &Graph, s: &Slices, walks: &[Option<Walk>], root_z: f64) -> Vec<Sup> {
    let mut groups: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
    for (i, w) in walks.iter().enumerate() {
        if let Some(w) = w {
            groups.entry(*w.nodes.last().expect("a walk has a node")).or_default().push(i);
        }
    }
    let mut sups = Vec::new();
    for (key, members) in groups {
        let mut t: Vec<TNode> = vec![TNode { blob: key, parent: None, children: vec![], starts: vec![], routes: 0 }];
        let mut child_of: FxHashMap<(usize, u32), usize> = FxHashMap::default();
        for &r in &members {
            let w = walks[r].as_ref().expect("grouped walks exist");
            let mut cur = 0usize;
            t[0].routes += 1;
            for &b in w.nodes.iter().rev().skip(1) {
                let nx = match child_of.get(&(cur, b)) {
                    Some(&x) => x,
                    None => {
                        t.push(TNode { blob: b, parent: Some(cur), children: vec![], starts: vec![], routes: 0 });
                        let x = t.len() - 1;
                        t[cur].children.push(x);
                        child_of.insert((cur, b), x);
                        x
                    }
                };
                cur = nx;
                t[cur].routes += 1;
            }
            t[cur].starts.push(r);
        }
        let end = walks[members[0]].as_ref().expect("grouped walks exist").end;
        let up_dir = |x: usize| -> P {
            let mut q = x;
            for _ in 0..LINE_WINDOW {
                match t[q].children.iter().max_by_key(|&&c| (t[c].routes, std::cmp::Reverse(c))) {
                    Some(&c) => q = c,
                    None => break,
                }
            }
            sub(g.pos(s, t[q].blob), g.pos(s, t[x].blob))
        };
        let down_dir = |x: usize| -> P {
            let mut q = x;
            for _ in 0..LINE_WINDOW {
                match t[q].parent {
                    Some(pp) => q = pp,
                    None => break,
                }
            }
            sub(g.pos(s, t[x].blob), g.pos(s, t[q].blob))
        };
        let mut stack: Vec<(usize, Option<(usize, u32)>)> = vec![(0, None)];
        while let Some((x0, par)) = stack.pop() {
            let mut path = vec![x0];
            let mut x = x0;
            let mut pushed = Vec::new();
            let seed;
            loop {
                // A tip whose walk starts on this node (a duplicate seed on one blob is dropped).
                if t[x].children.is_empty() {
                    seed = t[x].starts[0];
                    break;
                }
                let below = down_dir(x);
                let mut best = (f64::MIN, t[x].children[0]);
                for &c in &t[x].children {
                    let sc = cosang(up_dir(c), below) + 0.001 * t[c].routes as f64;
                    if sc > best.0 {
                        best = (sc, c);
                    }
                }
                // In the root zone the walks meet where cone feet touch, not at a fork: each goes on
                // to the ground as a trunk of its own.
                let root = g.z(s, t[x].blob) < root_z;
                for &c in &t[x].children {
                    if c != best.1 {
                        pushed.push((c, t[x].blob, root));
                    }
                }
                x = best.1;
                path.push(x);
            }
            let sid = sups.len();
            for (c, at, root) in pushed {
                stack.push((c, if root { None } else { Some((sid, at)) }));
            }
            let mut nodes: Vec<u32> = path.iter().rev().map(|&q| t[q].blob).collect();
            if let Some((_, at)) = par {
                nodes.push(at);
            }
            sups.push(Sup { seed, nodes, parent: par, end: if par.is_some() { End::Ground } else { end } });
        }
    }
    sups
}

// ---------------------------------------------------------------------------------------------
// Lines, widths and the rest

fn douglas_peucker(pts: &[P], tol: f64) -> Vec<P> {
    fn rec(p: &[P], tol: f64, keep: &mut [bool], a: usize, b: usize) {
        if b <= a + 1 {
            return;
        }
        let d = sub(p[b], p[a]);
        let ld = len(d).max(1e-12);
        let mut best = (0.0, a);
        for (i, q) in p.iter().enumerate().take(b).skip(a + 1) {
            let e = len(cross(sub(*q, p[a]), d)) / ld;
            if e > best.0 {
                best = (e, i);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            rec(p, tol, keep, a, best.1);
            rec(p, tol, keep, best.1, b);
        }
    }
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    rec(pts, tol, &mut keep, 0, pts.len() - 1);
    pts.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect()
}

/// A support as laid out: its line from the contact down, the shaft width and the tip's widths.
struct Line {
    seed: usize,
    /// Contact, socket, then the shaft's bends down to its base.
    pts: Vec<P>,
    shaft_d: f64,
    parent: Option<(usize, P)>,
    end: End,
}

/// The share of samples (every 0.1 mm) along `a` to `b` that lie inside the solid.
fn material_share(s: &Slices, a: P, b: P) -> f64 {
    let n = ((dist(a, b) / 0.1).ceil() as usize).max(2);
    let inside = (0..=n).filter(|&i| s.solid_at(add(a, mul(sub(b, a), i as f64 / n as f64)))).count();
    inside as f64 / (n + 1) as f64
}

/// A brace read: its ends (on two supports' lines), the supports and width.
struct Brace {
    a: P,
    b: P,
    sa: usize,
    sb: usize,
    d: f64,
}

/// The nearest point to `q` on support `l`'s shaft line at about `q`'s height.
fn on_line(l: &Line, q: P) -> P {
    let mut best = (f64::MAX, l.pts[1.min(l.pts.len() - 1)]);
    for w in l.pts[1..].windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = sub(b, a);
        let t = (dot(sub(q, a), ab) / dot(ab, ab).max(1e-12)).clamp(0.0, 1.0);
        let p = add(a, mul(ab, t));
        let d = dist(p, q);
        if d < best.0 {
            best = (d, p);
        }
    }
    best.1
}

/// Reads a fused support solid (`tv`, closed) held up at `seeds` (the contacts on or near it),
/// standing on the plate with its raft band up to `ground_z`.
pub fn read(tv: &[[P; 3]], seeds: &[Seed], ground_z: f64) -> Reading {
    let started = std::time::Instant::now();
    let m = Mesh::new(tv);
    let mut stats = Stats { triangles: m.tris.len(), ..Default::default() };
    if m.tris.is_empty() {
        return Reading { stats, ..Default::default() };
    }
    let s = slice(&m);
    let g = link(&s);
    stats.slices = s.z.len();
    stats.blobs = g.node_k.len();
    stats.broken_loops = s.broken;
    let ground = Ground { z: ground_z };
    let feet: Vec<(usize, Seed)> = seeds.iter().copied().enumerate().filter(|(_, c)| c.dir[2] > UP_DZ).collect();
    let starts: Vec<Option<u32>> =
        seeds.iter().map(|c| if c.dir[2] > UP_DZ { None } else { start_node(&g, &s, c) }).collect();
    stats.seeds = starts.iter().flatten().count();
    stats.unseeded = seeds.len() - stats.seeds - feet.len();
    let walks: Vec<Option<Walk>> = starts.par_iter().map(|st| st.map(|n| walk(&g, &s, &ground, n, &feet))).collect();
    for w in walks.iter().flatten() {
        match w.end {
            End::Ground => stats.ground += 1,
            End::Foot(_) => stats.struts += 1,
            End::Dead => stats.dead += 1,
        }
    }
    // Dead ends are left out: a walk that ends nowhere is no support the writer can build.
    let kept: Vec<Option<Walk>> = walks.into_iter().map(|w| w.filter(|w| !matches!(w.end, End::Dead))).collect();
    let sups = tree(&g, &s, &kept, ground_z + ROOT_ZONE_MM);

    // Each support's line, on its own walk's cores (a shared blob holds several rods).
    let lines: Vec<Line> = sups
        .iter()
        .map(|sp| {
            let w = kept[sp.seed].as_ref().expect("a support has a walk");
            let n = sp.nodes.len();
            let pts: Vec<P> = sp
                .nodes
                .iter()
                .enumerate()
                .map(|(i, &q)| if w.nodes.get(i) == Some(&q) { w.core[i] } else { g.pos(&s, q) })
                .collect();
            let ws: Vec<f64> = (0..n)
                .map(|i| {
                    let (a, b) = (i.saturating_sub(5), (i + 5).min(n - 1));
                    let d = sub(pts[b], pts[a]);
                    let tan = if d[2].abs() > 1e-9 { (d[0] * d[0] + d[1] * d[1]).sqrt() / d[2].abs() } else { 0.0 };
                    core_width(g.blob(&s, sp.nodes[i]), [pts[i][0], pts[i][1]], tan)
                })
                .collect();
            let own = if sp.parent.is_some() { n.saturating_sub(1) } else { n }.max(1);
            let mut rr = ws[..own].to_vec();
            rr.sort_by(f64::total_cmp);
            let shaft_d = rr[rr.len() / 2];
            let sock = (0..n).find(|&i| ws[i] >= SOCKET_SHARE * shaft_d).unwrap_or(0).min(n - 1);
            let mut shaft = douglas_peucker(&pts[sock..], STRAIGHT_MM);
            let parent = sp.parent.map(|(p, at)| (p, *shaft.last().unwrap_or(&g.pos(&s, at))));
            match sp.end {
                End::Ground if sp.parent.is_none() => {
                    let last = *shaft.last().expect("a shaft has a point");
                    shaft.push([last[0], last[1], 0.0]);
                }
                End::Foot(f) => shaft.push(seeds[f].at),
                _ => {}
            }
            let mut line = vec![seeds[sp.seed].at];
            line.extend(shaft);
            Line { seed: sp.seed, pts: line, shaft_d, parent, end: sp.end }
        })
        .collect();
    for l in &lines {
        match (l.parent, l.end) {
            (Some(_), _) => stats.branches += 1,
            (None, End::Ground) => stats.trunks += 1,
            _ => {}
        }
    }

    // The rest: blobs no support owns.
    let n = g.node_k.len();
    let mut owner: Vec<u32> = vec![u32::MAX; n];
    let mut multi: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
    for (i, sp) in sups.iter().enumerate() {
        let own = if sp.parent.is_some() { &sp.nodes[..sp.nodes.len() - 1] } else { &sp.nodes[..] };
        for &q in own {
            if owner[q as usize] == u32::MAX {
                owner[q as usize] = i as u32;
            } else if owner[q as usize] as usize != i {
                multi.entry(q).or_default().push(i);
            }
        }
    }
    let mut braces: Vec<Brace> = Vec::new();
    // Level bars: blobs two supports' own stretches share over a short run of height, above the
    // root zone.
    let mut zones: std::collections::BTreeMap<(usize, usize), Vec<f64>> = Default::default();
    for (&q, others) in &multi {
        let a = owner[q as usize] as usize;
        let z = g.z(&s, q);
        for &b in others {
            if a != b {
                zones.entry((a.min(b), a.max(b))).or_default().push(z);
            }
        }
    }
    for ((sa, sb), mut zs) in zones {
        zs.sort_by(f64::total_cmp);
        let mut runs: Vec<(f64, f64)> = Vec::new();
        for z in zs {
            match runs.last_mut() {
                Some(r) if z - r.1 < 1.5 => r.1 = z,
                _ => runs.push((z, z)),
            }
        }
        for (z0, z1) in runs {
            if z1 - z0 > LEVEL_MAX_RUN_MM {
                continue;
            }
            let zm = (z0 + z1) / 2.0;
            if zm < ground_z + ROOT_ZONE_MM {
                stats.root_zone_bars += 1;
                continue;
            }
            let (Some(pa), Some(pb)) = (at_height(&lines[sa], zm), at_height(&lines[sb], zm)) else { continue };
            if dist(pa, pb) < 0.5 * (lines[sa].shaft_d + lines[sb].shaft_d) {
                continue;
            }
            if material_share(&s, pa, pb) < MATERIAL_SHARE {
                stats.refused_braces += 1;
                continue;
            }
            let d = (z1 - z0 + DZ).max(0.3).min(lines[sa].shaft_d).min(lines[sb].shaft_d);
            braces.push(Brace { a: pa, b: pb, sa, sb, d });
            stats.level_bars += 1;
        }
    }
    // Free components.
    let free = |q: u32| owner[q as usize] == u32::MAX && !ground.holds(&g, &s, q);
    let mut comp = vec![u32::MAX; n];
    let mut ncomp = 0u32;
    let mut posts: Vec<(Vec<P>, f64)> = Vec::new();
    for st in 0..n as u32 {
        if !free(st) || comp[st as usize] != u32::MAX {
            continue;
        }
        let mut q = vec![st];
        comp[st as usize] = ncomp;
        let mut members = vec![];
        let mut att: Vec<(u32, u32)> = vec![];
        let mut grounded = false;
        while let Some(x) = q.pop() {
            members.push(x);
            for &y in g.up[x as usize].iter().chain(&g.down[x as usize]) {
                if free(y) {
                    if comp[y as usize] == u32::MAX {
                        comp[y as usize] = ncomp;
                        q.push(y);
                    }
                } else if owner[y as usize] != u32::MAX {
                    att.push((x, y));
                } else {
                    grounded = true;
                }
            }
        }
        ncomp += 1;
        members.sort_unstable();
        att.sort_unstable();
        att.dedup();
        if members.len() < 3 && att.len() <= 1 && !grounded {
            continue;
        }
        let mut ws: Vec<f64> = members
            .iter()
            .map(|&m| {
                let b = g.blob(&s, m);
                core_width(b, core(b, b.c, (b.area / std::f64::consts::PI).sqrt()), 1.0)
            })
            .collect();
        ws.sort_by(f64::total_cmp);
        let d = ws[ws.len() / 2];
        // Attachments, one per support within 1 mm of height.
        let mut groups: Vec<(usize, u32, u32)> = Vec::new();
        att.sort_by_key(|a| (owner[a.1 as usize], g.node_k[a.1 as usize], a.0));
        for &(f, o) in &att {
            let sp = owner[o as usize] as usize;
            let z = g.z(&s, o);
            if groups.iter().any(|gg| gg.0 == sp && (g.z(&s, gg.2) - z).abs() < 1.0) {
                continue;
            }
            groups.push((sp, f, o));
        }
        if grounded {
            let zs: Vec<f64> = members.iter().map(|&m| g.z(&s, m)).collect();
            let zspan = zs.iter().copied().fold(f64::MIN, f64::max) - zs.iter().copied().fold(f64::MAX, f64::min);
            if zspan > 1.0 && groups.is_empty() {
                let mut pts: Vec<(f64, P)> = members.iter().map(|&m| (g.z(&s, m), g.pos(&s, m))).collect();
                pts.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut line: Vec<P> = vec![[pts[0].1[0], pts[0].1[1], 0.0]];
                line.extend(douglas_peucker(&pts.iter().map(|x| x.1).collect::<Vec<_>>(), STRAIGHT_MM));
                posts.push((line, d));
            }
            continue;
        }
        // Each attachment's way out: free links followed away from the support.
        let leave = |f: u32, o: u32| -> P {
            let goup = g.node_k[o as usize] < g.node_k[f as usize];
            let mut x = f;
            for _ in 0..15 {
                let l = if goup { &g.up[x as usize] } else { &g.down[x as usize] };
                match l.iter().find(|&&y| comp[y as usize] == comp[f as usize]) {
                    Some(&y) => x = y,
                    None => break,
                }
            }
            sub(g.pos(&s, x), g.pos(&s, f))
        };
        // Where each attachment leaves its support: the centre of the free blob next to it, on the
        // brace's axis (its own section, clear of the weld).
        let feet: Vec<P> = groups.iter().map(|gg| g.pos(&s, gg.1)).collect();
        let ends: Vec<P> = groups.iter().map(|gg| on_line(&lines[gg.0], cores_at(&g, &s, &kept, &sups, gg))).collect();
        let dirs: Vec<P> = groups.iter().map(|gg| leave(gg.1, gg.2)).collect();
        let mut used = vec![false; groups.len()];
        let mut cands: Vec<(f64, usize, usize)> = Vec::new();
        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                if groups[i].0 == groups[j].0 {
                    continue;
                }
                let sc =
                    (1.0 - cosang(dirs[i], sub(ends[j], ends[i]))) + (1.0 - cosang(dirs[j], sub(ends[i], ends[j])));
                cands.push((sc, i, j));
            }
        }
        cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        // Pairs leaving towards each other first; then, for ends still open, any pair the solid
        // runs between (the material check is what keeps a brace off empty space).
        let mut order: Vec<(f64, usize, usize)> = cands.iter().copied().filter(|c| c.0 <= PAIR_SCORE).collect();
        order.extend(cands.iter().copied().filter(|c| c.0 > PAIR_SCORE && c.0 <= LOOSE_PAIR_SCORE));
        let mut refused: Vec<(usize, usize)> = Vec::new();
        for (_, i, j) in order {
            if used[i] || used[j] || refused.contains(&(i, j)) {
                continue;
            }
            // The brace's axis through its two free ends, met with each support's line.
            let axis = sub(feet[j], feet[i]);
            let (a, b) = if len(axis) > 1e-6 {
                (meet(&lines[groups[i].0], feet[i], axis), meet(&lines[groups[j].0], feet[i], axis))
            } else {
                (ends[i], ends[j])
            };
            // Only where the solid runs between them: no brace across empty space.
            if material_share(&s, a, b) < MATERIAL_SHARE {
                refused.push((i, j));
                continue;
            }
            used[i] = true;
            used[j] = true;
            // No wider than the rods it joins (a component in a dense crown is measured wide).
            let d = d.min(lines[groups[i].0].shaft_d).min(lines[groups[j].0].shaft_d);
            braces.push(Brace { a, b, sa: groups[i].0, sb: groups[j].0, d });
        }
        stats.refused_braces += refused.iter().filter(|&&(i, j)| !used[i] && !used[j]).count();
        // An end left open where the component joins two or more supports (a stub off one support,
        // a weld fillet, is no brace end).
        let supports: std::collections::BTreeSet<usize> = groups.iter().map(|gg| gg.0).collect();
        if supports.len() >= 2 {
            stats.dangling += used.iter().filter(|u| !**u).count();
        } else {
            stats.stubs += 1;
        }
    }
    stats.braces = braces.len();
    stats.posts = posts.len();

    // Segments and their joins.
    let mut out = Reading::default();
    let mut seg_of_line: Vec<Vec<usize>> = Vec::with_capacity(lines.len());
    for l in &lines {
        let mut ids = Vec::new();
        let cd = seeds[l.seed].d;
        for (k, w) in l.pts.windows(2).enumerate() {
            let (top, bottom) = (w[0], w[1]);
            if dist(top, bottom) < 1e-6 {
                continue;
            }
            let (r_top, r_bottom) =
                if k == 0 { (cd / 2.0, l.shaft_d / 2.0) } else { (l.shaft_d / 2.0, l.shaft_d / 2.0) };
            let kind = if k == 0 { SectionKind::Cone } else { SectionKind::Rod };
            let id = push_segment(&mut out.segments, bottom, top, r_bottom, r_top, kind);
            if let Some(&prev) = ids.last() {
                out.links.push((prev, id, top));
            }
            ids.push(id);
        }
        seg_of_line.push(ids);
    }
    for (i, l) in lines.iter().enumerate() {
        if let (Some((p, at)), Some(&last)) = (l.parent, seg_of_line[i].last())
            && let Some(host) = nearest_segment(&out.segments, &seg_of_line[p], at)
        {
            out.links.push((last, host, at));
        }
    }
    for b in &braces {
        let (lo, hi) = if b.a[2] <= b.b[2] { (b.a, b.b) } else { (b.b, b.a) };
        let id = push_segment(&mut out.segments, lo, hi, b.d / 2.0, b.d / 2.0, SectionKind::Rod);
        for (sp, at) in [(b.sa, b.a), (b.sb, b.b)] {
            if let Some(host) = nearest_segment(&out.segments, &seg_of_line[sp], at) {
                out.links.push((id, host, at));
            }
        }
    }
    for (line, d) in &posts {
        let mut prev: Option<usize> = None;
        for w in line.windows(2) {
            if dist(w[0], w[1]) < 1e-6 {
                continue;
            }
            let id = push_segment(&mut out.segments, w[0], w[1], d / 2.0, d / 2.0, SectionKind::Rod);
            if let Some(p) = prev {
                out.links.push((p, id, w[0]));
            }
            prev = Some(id);
        }
    }
    stats.ms = started.elapsed().as_secs_f64() * 1000.0;
    out.stats = stats;
    out
}

/// The core of the support the attachment `gg` (support, free node, owned node) is on, at its
/// owned node.
fn cores_at(g: &Graph, s: &Slices, walks: &[Option<Walk>], sups: &[Sup], gg: &(usize, u32, u32)) -> P {
    let sp = &sups[gg.0];
    if let Some(w) = walks[sp.seed].as_ref()
        && let Some(i) = w.nodes.iter().position(|&q| q == gg.2)
    {
        return w.core[i];
    }
    g.pos(s, gg.2)
}

/// The point of support line `l`'s shaft nearest the line through `p0` along `d`.
fn meet(l: &Line, p0: P, d: P) -> P {
    let du = super::math::unit(d);
    let mut best = (f64::MAX, l.pts[l.pts.len() - 1]);
    for w in l.pts[1..].windows(2) {
        let (a, b) = (w[0], w[1]);
        let e = sub(b, a);
        // Closest points of the segment a + s e (s in 0..1) and the line p0 + t du.
        let (ee, ed) = (dot(e, e), dot(e, du));
        let r = sub(a, p0);
        let den = ee - ed * ed;
        let sp = if den.abs() < 1e-12 { 0.0 } else { ((ed * dot(r, du) - dot(r, e)) / den).clamp(0.0, 1.0) };
        let q = add(a, mul(e, sp));
        let t = dot(sub(q, p0), du);
        let dd = dist(q, add(p0, mul(du, t)));
        if dd < best.0 {
            best = (dd, q);
        }
    }
    best.1
}

/// Where support line `l` crosses height `z` on its shaft.
fn at_height(l: &Line, z: f64) -> Option<P> {
    l.pts[1..].windows(2).find_map(|w| {
        let (a, b) = (w[0], w[1]);
        if (a[2] - z) * (b[2] - z) > 0.0 || (a[2] - b[2]).abs() < 1e-9 {
            return None;
        }
        let f = (z - a[2]) / (b[2] - a[2]);
        Some(add(a, mul(sub(b, a), f)))
    })
}

fn nearest_segment(segs: &[Segment], ids: &[usize], at: P) -> Option<usize> {
    ids.iter().copied().min_by(|&x, &y| {
        let d = |i: usize| super::math::seg_dist(at, segs[i].a, segs[i].b);
        d(x).total_cmp(&d(y)).then(x.cmp(&y))
    })
}

/// Adds a straight stretch from `a` (radius `ra`) to `b` (radius `rb`): a rod, or a cone.
fn push_segment(segs: &mut Vec<Segment>, a: P, b: P, ra: f64, rb: f64, kind: SectionKind) -> usize {
    let l = dist(a, b);
    let r = ra.max(rb);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in [a, b] {
        for c in 0..3 {
            lo[c] = lo[c].min(p[c] - r);
            hi[c] = hi[c].max(p[c] + r);
        }
    }
    let kind = if kind == SectionKind::Cone && (ra - rb).abs() < 1e-6 { SectionKind::Rod } else { kind };
    let sections = vec![Section { k: kind, t0: 0.0, t1: l, r0: ra, r1: rb, tc: None, big_r: None }];
    let volume = std::f64::consts::PI * l * (ra * ra + ra * rb + rb * rb) / 3.0;
    segs.push(Segment { a, b, width: 2.0 * r, sections, volume, tris: 0, lo, hi });
    segs.len() - 1
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A closed mesh of the union of capsules (each: ends, radius), by marching tetrahedra on a
    /// grid of `h` mm over their signed distance field. Every corner is shared through its grid
    /// edge, so the mesh is closed and manifold, as a boolean union of supports is.
    pub(crate) fn capsules(caps: &[(P, P, f64)], h: f64) -> Vec<[P; 3]> {
        let sdf = |p: P| {
            caps.iter().map(|&(a, b, r)| super::super::math::seg_dist(p, a, b) - r).fold(f64::INFINITY, f64::min)
        };
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for &(a, b, r) in caps {
            for c in 0..3 {
                lo[c] = lo[c].min(a[c].min(b[c]) - r - 2.0 * h);
                hi[c] = hi[c].max(a[c].max(b[c]) + r + 2.0 * h);
            }
        }
        let n = [0, 1, 2].map(|c| ((hi[c] - lo[c]) / h).ceil() as usize + 1);
        let at = |i: usize, j: usize, k: usize| [lo[0] + i as f64 * h, lo[1] + j as f64 * h, lo[2] + k as f64 * h];
        let idx = |i: usize, j: usize, k: usize| (i * n[1] + j) * n[2] + k;
        let vals: Vec<f64> = (0..n[0] * n[1] * n[2])
            .into_par_iter()
            .map(|q| {
                let (i, j, k) = (q / (n[1] * n[2]), (q / n[2]) % n[1], q % n[2]);
                let v = sdf(at(i, j, k));
                // Kept off zero, so every crossing has one corner on each side and no cut point lies
                // on a grid corner (where cuts of several edges would weld into one).
                let floor = 0.01 * h;
                if v.abs() < floor { if v < 0.0 { -floor } else { floor } } else { v }
            })
            .collect();
        // The six tetrahedra of a cube sharing its main diagonal (corners as bits x, y, z).
        const TETS: [[usize; 4]; 6] =
            [[0, 1, 3, 7], [0, 3, 2, 7], [0, 2, 6, 7], [0, 6, 4, 7], [0, 4, 5, 7], [0, 5, 1, 7]];
        let mut tris = Vec::new();
        for i in 0..n[0] - 1 {
            for j in 0..n[1] - 1 {
                for k in 0..n[2] - 1 {
                    let corner = |c: usize| (i + (c & 1), j + ((c >> 1) & 1), k + ((c >> 2) & 1));
                    let cs: [(usize, usize, usize); 8] = std::array::from_fn(corner);
                    if cs.iter().all(|&(a, b, c)| vals[idx(a, b, c)] > 0.0)
                        || cs.iter().all(|&(a, b, c)| vals[idx(a, b, c)] < 0.0)
                    {
                        continue;
                    }
                    for t in TETS {
                        let v = t.map(|c| cs[c]);
                        let p = v.map(|(a, b, c)| at(a, b, c));
                        let f = v.map(|(a, b, c)| vals[idx(a, b, c)]);
                        // From the edge's lower grid corner, so both cells sharing it cut it alike.
                        let cut = |x: usize, y: usize| {
                            let (x, y) = if v[x] < v[y] { (x, y) } else { (y, x) };
                            let w = f[x] / (f[x] - f[y]);
                            add(p[x], mul(sub(p[y], p[x]), w))
                        };
                        let ins: Vec<usize> = (0..4).filter(|&q| f[q] < 0.0).collect();
                        let outs: Vec<usize> = (0..4).filter(|&q| f[q] >= 0.0).collect();
                        // Facing out: from the corners inside towards those outside.
                        let mean =
                            |q: &[usize]| mul(q.iter().fold([0.0; 3], |acc, &i| add(acc, p[i])), 1.0 / q.len() as f64);
                        let out_dir = sub(mean(&outs), mean(&ins));
                        let mut emit = |a: P, b: P, c: P| {
                            if dot(cross(sub(b, a), sub(c, a)), out_dir) >= 0.0 {
                                tris.push([a, b, c])
                            } else {
                                tris.push([a, c, b])
                            }
                        };
                        match ins.len() {
                            1 => {
                                let x = ins[0];
                                emit(cut(x, outs[0]), cut(x, outs[1]), cut(x, outs[2]));
                            }
                            3 => {
                                let y = outs[0];
                                emit(cut(ins[0], y), cut(ins[1], y), cut(ins[2], y));
                            }
                            2 => {
                                let (a, b, c, d) = (
                                    cut(ins[0], outs[0]),
                                    cut(ins[0], outs[1]),
                                    cut(ins[1], outs[0]),
                                    cut(ins[1], outs[1]),
                                );
                                emit(a, b, d);
                                emit(a, d, c);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        tris
    }

    fn down(at: P, d: f64) -> Seed {
        Seed { at, d, dir: [0.0, 0.0, -1.0] }
    }

    /// Two trunks 6 mm apart, welded by a diagonal brace, with a branch off the first; tips at the
    /// top.
    fn braced() -> (Vec<[P; 3]>, Vec<Seed>) {
        let caps = vec![
            ([0.0, 0.0, -0.5], [0.0, 0.0, 14.0], 0.5),
            ([6.0, 0.0, -0.5], [6.0, 0.0, 14.0], 0.5),
            // The brace, from low on the first to high on the second.
            ([0.0, 0.0, 6.0], [6.0, 0.0, 10.0], 0.3),
            // The branch, leaving the first trunk at 8 mm.
            ([0.0, 0.0, 8.0], [-3.0, 0.0, 12.0], 0.4),
        ];
        let seeds = vec![down([0.0, 0.0, 14.5], 0.6), down([6.0, 0.0, 14.5], 0.6), down([-3.0, 0.0, 12.5], 0.5)];
        (capsules(&caps, 0.1), seeds)
    }

    #[test]
    fn capsule_meshes_are_closed() {
        let (tv, _) = braced();
        let m = Mesh::new(&tv);
        let mut count: FxHashMap<u32, u32> = FxHashMap::default();
        for e in &m.tedge {
            for &x in e {
                *count.entry(x).or_default() += 1;
            }
        }
        assert!(count.values().all(|&c| c == 2), "every edge has two faces");
    }

    #[test]
    fn reads_trunks_a_branch_and_a_brace() {
        let (tv, seeds) = braced();
        let r = read(&tv, &seeds, 3.5);
        let st = &r.stats;
        assert_eq!((st.seeds, st.ground, st.dead), (3, 3, 0), "{st:?}");
        assert_eq!((st.trunks, st.branches), (2, 1), "{st:?}");
        assert_eq!(st.braces, 1, "{st:?}");
        // The brace runs between the trunks, from about 6 mm up on one to about 10 on the other.
        let b = r.segments.last().expect("the brace is last");
        let (lo, hi) = (b.a, b.b);
        assert!((lo[0] - 0.0).abs() < 0.4 && (lo[2] - 6.0).abs() < 1.0, "{lo:?}");
        assert!((hi[0] - 6.0).abs() < 0.4 && (hi[2] - 10.0).abs() < 1.0, "{hi:?}");
        // Shafts as wide as the rods.
        let shaft = &r.segments[1];
        assert!((shaft.width - 1.0).abs() < 0.15, "{}", shaft.width);
    }

    #[test]
    fn reads_an_x_brace_and_no_brace_where_none_runs() {
        // Two trunks with an X between them, and a third with nothing joining it.
        let caps = vec![
            ([0.0, 0.0, -0.5], [0.0, 0.0, 14.0], 0.5),
            ([6.0, 0.0, -0.5], [6.0, 0.0, 14.0], 0.5),
            ([12.0, 0.0, -0.5], [12.0, 0.0, 14.0], 0.5),
            ([0.0, 0.0, 5.0], [6.0, 0.0, 11.0], 0.3),
            ([6.0, 0.0, 5.0], [0.0, 0.0, 11.0], 0.3),
        ];
        let tv = capsules(&caps, 0.1);
        let seeds: Vec<Seed> = [0.0, 6.0, 12.0].iter().map(|&x| down([x, 0.0, 14.5], 0.6)).collect();
        let r = read(&tv, &seeds, 3.5);
        assert_eq!(r.stats.trunks, 3, "{:?}", r.stats);
        let braces = &r.segments[r.segments.len() - r.stats.braces..];
        assert_eq!(braces.len(), 2, "{:?}", r.stats);
        for b in braces {
            // Each arm of the X joins the first two trunks, rising 6 mm over 6 mm.
            let (x0, x1) = (b.a[0].min(b.b[0]), b.a[0].max(b.b[0]));
            assert!(x0 < 0.6 && x1 > 5.4 && x1 < 6.6, "{b:?}");
            assert!((b.b[2] - b.a[2] - 6.0).abs() < 1.2, "{b:?}");
        }
        // Nothing reaches the third trunk.
        assert!(r.segments.iter().all(|s| s.sections[0].r0 > 0.4 || s.a[0].max(s.b[0]) < 7.0), "a brace at the third");
    }

    #[test]
    fn touching_feet_in_the_root_zone_are_no_bar() {
        // Two trunks whose cone feet touch just above the raft band.
        let caps = vec![
            ([0.0, 0.0, -0.5], [0.0, 0.0, 12.0], 0.4),
            ([1.6, 0.0, -0.5], [1.6, 0.0, 12.0], 0.4),
            ([0.0, 0.0, 3.6], [1.6, 0.0, 3.6], 0.5),
        ];
        let tv = capsules(&caps, 0.08);
        let seeds = vec![down([0.0, 0.0, 12.4], 0.5), down([1.6, 0.0, 12.4], 0.5)];
        let r = read(&tv, &seeds, 3.5);
        assert_eq!(r.stats.trunks, 2, "{:?}", r.stats);
        assert_eq!(r.stats.level_bars, 0, "{:?}", r.stats);
    }

    #[test]
    fn a_dead_end_on_a_contact_facing_up_is_a_strut() {
        // A rod from a tip down onto the model (a contact facing up at its foot).
        let caps = vec![([0.0, 0.0, 6.0], [2.0, 0.0, 14.0], 0.4)];
        let tv = capsules(&caps, 0.08);
        let seeds = vec![down([2.0, 0.0, 14.4], 0.5), Seed { at: [0.0, 0.0, 5.6], d: 0.5, dir: [0.0, 0.0, 1.0] }];
        let r = read(&tv, &seeds, 3.5);
        assert_eq!((r.stats.struts, r.stats.dead), (1, 0), "{:?}", r.stats);
        let last = r.segments.iter().map(|s| s.a[2].min(s.b[2])).fold(f64::MAX, f64::min);
        assert!((last - 5.6).abs() < 0.3, "the strut ends on the foot: {last}");
    }

    #[test]
    fn a_fused_solid_is_read_through_pieces_and_classified() {
        use super::super::contacts::Contact;
        let (tv, seeds) = braced();
        let pieces = super::super::pieces::read_seeded(&tv, &[], &seeds, 3.5, super::super::pieces::RAY_PITCH_MM);
        assert_eq!(pieces.fused.len(), 1, "the gate reads it fused too");
        let pieces = pieces.with_fused().expect("a fused reading");
        let contacts: Vec<Contact> = seeds
            .iter()
            .map(|s| Contact {
                at: s.at,
                area: std::f64::consts::PI * s.d * s.d / 4.0,
                d: s.d,
                dir: s.dir,
                n: 1,
                depth: 0.2,
                open: false,
                thin_wall: false,
                flush: false,
                shells: vec![0],
            })
            .collect();
        let c = super::super::classify::classify(&pieces, &contacts, &[]);
        let l = &c.layout;
        use crate::geometry::support_layout::SupportBase;
        let plate = l.supports.iter().filter(|s| matches!(s.base, SupportBase::Plate { .. })).count();
        let branch = l.supports.iter().filter(|s| matches!(s.base, SupportBase::Shaft { .. })).count();
        assert_eq!((plate, branch), (2, 1), "{:?}", c.supports);
        assert_eq!(l.braces.len(), 1, "{:?}", c.exceptions);
    }

    #[test]
    fn the_reading_is_the_same_on_every_thread_count() {
        let (tv, seeds) = braced();
        let on = |n: usize| {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(n).build().unwrap();
            pool.install(|| read(&tv, &seeds, 3.5))
        };
        let (a, b) = (on(1), on(4));
        let key = |r: &Reading| format!("{:?} {:?}", r.segments, r.links);
        assert_eq!(key(&a), key(&b));
        assert_eq!(Stats { ms: 0.0, ..a.stats }, Stats { ms: 0.0, ..b.stats });
    }
}
