//! Port of synthetic/geom.py: shapes of revolution, gridded boxes, spheres, a BSP CSG union with
//! T-junction repair, weld and closed-mesh stats, and STL io. Every operation follows the Python's
//! order of evaluation so that the results match it bit for bit where numpy allows.

use crate::vec::{V3, v3};
use std::collections::HashMap;

pub type Tri = [V3; 3];

pub const EPS: f64 = 1e-6;

/// geom.frame: the unit axis and two unit vectors square to it.
pub fn frame(axis: V3) -> (V3, V3, V3) {
    let axis = axis.unit();
    let helper = if axis[2].abs() < 0.9 { v3(0.0, 0.0, 1.0) } else { v3(1.0, 0.0, 0.0) };
    let u = axis.cross(helper).unit();
    (axis, u, axis.cross(u))
}

fn ring(c: V3, u: V3, v: V3, r: f64, n: usize) -> Vec<V3> {
    (0..n)
        .map(|k| {
            let ang = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            c + (u * ang.cos() + v * ang.sin()) * r
        })
        .collect()
}

/// geom.revolve_polys: planar polygons of a closed shape of revolution; profile (t, r), r = 0 a pole.
pub fn revolve_polys(a: V3, axis: V3, profile: &[(f64, f64)], n: usize) -> Vec<Vec<V3>> {
    let (axis, u, v) = frame(axis);
    let rings: Vec<Vec<V3>> = profile
        .iter()
        .map(|&(t, r)| {
            let c = a + axis * t;
            if r <= 1e-9 { vec![c] } else { ring(c, u, v, r, n) }
        })
        .collect();
    let mut polys = Vec::new();
    if rings[0].len() > 1 {
        polys.push(rings[0].iter().rev().copied().collect());
    }
    if rings[rings.len() - 1].len() > 1 {
        polys.push(rings[rings.len() - 1].clone());
    }
    for w in rings.windows(2) {
        let (lo, hi) = (&w[0], &w[1]);
        if lo.len() == 1 && hi.len() == 1 {
            continue;
        }
        if lo.len() == 1 {
            polys.extend((0..n).map(|k| vec![lo[0], hi[(k + 1) % n], hi[k]]));
        } else if hi.len() == 1 {
            polys.extend((0..n).map(|k| vec![lo[k], lo[(k + 1) % n], hi[0]]));
        } else {
            polys.extend((0..n).map(|k| vec![lo[k], lo[(k + 1) % n], hi[(k + 1) % n], hi[k]]));
        }
    }
    polys
}

/// geom.revolve_tris: triangles of the same shape, flat end caps as fans from a centre vertex.
pub fn revolve_tris(a: V3, axis: V3, profile: &[(f64, f64)], n: usize) -> Vec<Tri> {
    let (axis, u, v) = frame(axis);
    let rings: Vec<(V3, Option<Vec<V3>>)> = profile
        .iter()
        .map(|&(t, r)| {
            let c = a + axis * t;
            if r <= 1e-9 { (c, None) } else { (c, Some(ring(c, u, v, r, n))) }
        })
        .collect();
    let mut tris = Vec::new();
    if let (c0, Some(r0)) = &rings[0] {
        tris.extend((0..n).map(|k| [*c0, r0[(k + 1) % n], r0[k]]));
    }
    if let (c1, Some(r1)) = &rings[rings.len() - 1] {
        tris.extend((0..n).map(|k| [*c1, r1[k], r1[(k + 1) % n]]));
    }
    for w in rings.windows(2) {
        let ((cl, lo), (ch, hi)) = (&w[0], &w[1]);
        match (lo, hi) {
            (None, None) => {}
            (None, Some(hi)) => tris.extend((0..n).map(|k| [*cl, hi[(k + 1) % n], hi[k]])),
            (Some(lo), None) => tris.extend((0..n).map(|k| [lo[k], lo[(k + 1) % n], *ch])),
            (Some(lo), Some(hi)) => {
                for k in 0..n {
                    let j = (k + 1) % n;
                    tris.push([lo[k], lo[j], hi[j]]);
                    tris.push([lo[k], hi[j], hi[k]]);
                }
            }
        }
    }
    tris
}

pub fn ball_profile(r: f64, lat: usize) -> Vec<(f64, f64)> {
    let pi = std::f64::consts::PI;
    (0..=lat)
        .map(|k| (r * (1.0 - (pi * k as f64 / lat as f64).cos()), r * (pi * k as f64 / lat as f64).sin()))
        .collect()
}

/// Python's round() of a float to an int (half to even).
pub fn py_round(x: f64) -> i64 {
    x.round_ties_even() as i64
}

pub fn grid_face(o: V3, e1: V3, e2: V3, h: f64, flip: bool) -> Vec<Tri> {
    let n1 = py_round(e1.norm() / h).max(1) as usize;
    let n2 = py_round(e2.norm() / h).max(1) as usize;
    let mut tris = Vec::with_capacity(n1 * n2 * 2);
    let (f1, f2) = (n1 as f64, n2 as f64);
    for i in 0..n1 {
        for j in 0..n2 {
            let (i0, i1, j0, j1) = (i as f64 / f1, (i + 1) as f64 / f1, j as f64 / f2, (j + 1) as f64 / f2);
            let p = o + e1 * i0 + e2 * j0;
            let q = o + e1 * i1 + e2 * j0;
            let r = o + e1 * i1 + e2 * j1;
            let s = o + e1 * i0 + e2 * j1;
            if flip {
                tris.push([p, s, r]);
                tris.push([p, r, q]);
            } else {
                tris.push([p, q, r]);
                tris.push([p, r, s]);
            }
        }
    }
    tris
}

/// geom.grid_box: a closed box, outward normals, faces gridded at about h; skip names faces left out.
pub fn grid_box(lo: V3, hi: V3, h: f64, skip: &[&str]) -> Vec<Tri> {
    let d = hi - lo;
    let (ex, ey, ez) = (v3(d[0], 0.0, 0.0), v3(0.0, d[1], 0.0), v3(0.0, 0.0, d[2]));
    let mut tris = Vec::new();
    let has = |s: &str| skip.contains(&s);
    if !has("bottom") {
        tris.extend(grid_face(lo, ex, ey, h, true));
    }
    if !has("top") {
        tris.extend(grid_face(lo + ez, ex, ey, h, false));
    }
    if !has("front") {
        tris.extend(grid_face(lo, ex, ez, h, false));
    }
    if !has("back") {
        tris.extend(grid_face(lo + ey, ex, ez, h, true));
    }
    if !has("left") {
        tris.extend(grid_face(lo, ey, ez, h, false));
    }
    if !has("right") {
        tris.extend(grid_face(lo + ex, ey, ez, h, true));
    }
    tris
}

pub fn sphere_mesh(c: V3, r: f64, lon: usize, lat: usize) -> Vec<Tri> {
    revolve_tris(c - v3(0.0, 0.0, r), v3(0.0, 0.0, 1.0), &ball_profile(r, lat), lon)
}

/// geom.ray_first_hit: the nearest hit of a ray, (t, point, unit normal facing the ray's origin).
pub fn ray_first_hit(tris: &[Tri], origin: V3, direction: V3) -> Option<(f64, V3, V3)> {
    let o = origin;
    let d = direction.unit();
    let mut best: Option<(usize, f64)> = None;
    for (i, tr) in tris.iter().enumerate() {
        let e1 = tr[1] - tr[0];
        let e2 = tr[2] - tr[0];
        let h = d.cross(e2);
        // (e1 * h).sum(1): a left-to-right sum of three terms.
        let a = e1[0] * h[0] + e1[1] * h[1] + e1[2] * h[2];
        let ok = a.abs() > 1e-12;
        let f = if ok { 1.0 / a } else { 0.0 };
        let s = o - tr[0];
        let u = f * (s[0] * h[0] + s[1] * h[1] + s[2] * h[2]);
        let q = s.cross(e1);
        let v = f * (q[0] * d[0] + q[1] * d[1] + q[2] * d[2]);
        let t = f * (q[0] * e2[0] + q[1] * e2[1] + q[2] * e2[2]);
        let good = ok && u >= -1e-9 && v >= -1e-9 && u + v <= 1.0 + 1e-9 && t > 1e-9;
        if good && best.is_none_or(|(_, bt)| t < bt) {
            best = Some((i, t));
        }
    }
    let (i, t) = best?;
    let tr = tris[i];
    let mut n = (tr[1] - tr[0]).cross(tr[2] - tr[0]).unit();
    if n.dot(d) > 0.0 {
        n = -n;
    }
    Some((t, o + d * t, n))
}

// ---- BSP CSG ----
#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub n: V3,
    pub w: f64,
}

impl Plane {
    pub fn of(pts: &[V3]) -> Option<Plane> {
        let m = pts.len();
        for i in 0..m {
            let n = (pts[(i + 1) % m] - pts[i]).cross(pts[(i + 2) % m] - pts[i]);
            let ln = n.norm();
            if ln > 1e-10 {
                let n = n / ln;
                return Some(Plane { n, w: n.dot(pts[i]) });
            }
        }
        None
    }
    fn flip(self) -> Plane {
        Plane { n: -self.n, w: -self.w }
    }
}

#[derive(Clone, Debug)]
pub struct Poly {
    pub v: Vec<V3>,
    pub plane: Plane,
}

impl Poly {
    fn flip(&self) -> Poly {
        Poly { v: self.v.iter().rev().copied().collect(), plane: self.plane.flip() }
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Where each output of split goes. In build, the coplanar lists are both the node's own list.
enum Dest<'a> {
    /// (coplanar front, coplanar back, front, back)
    Four(&'a mut Vec<Poly>, &'a mut Vec<Poly>, &'a mut Vec<Poly>, &'a mut Vec<Poly>),
    /// (coplanar both, front, back)
    Three(&'a mut Vec<Poly>, &'a mut Vec<Poly>, &'a mut Vec<Poly>),
}

fn split(plane: &Plane, poly: Poly, dest: &mut Dest) {
    let (n, w) = (plane.n, plane.w);
    let types: Vec<u8> = poly
        .v
        .iter()
        .map(|p| {
            let d = n.dot(*p) - w;
            if d < -EPS {
                BACK
            } else if d > EPS {
                FRONT
            } else {
                COPLANAR
            }
        })
        .collect();
    let kind = types.iter().fold(0u8, |k, t| k | t);
    type Outs<'b> = (&'b mut Vec<Poly>, Option<&'b mut Vec<Poly>>, &'b mut Vec<Poly>, &'b mut Vec<Poly>);
    let (cf, cb, f, b): Outs = match dest {
        Dest::Four(cf, cb, f, b) => (cf, Some(cb), f, b),
        Dest::Three(c, f, b) => (c, None, f, b),
    };
    match kind {
        COPLANAR => {
            if n.dot(poly.plane.n) > 0.0 {
                cf.push(poly);
            } else {
                match cb {
                    Some(cb) => cb.push(poly),
                    None => cf.push(poly),
                }
            }
        }
        FRONT => f.push(poly),
        BACK => b.push(poly),
        _ => {
            let m = poly.v.len();
            let (mut ff, mut bb) = (Vec::new(), Vec::new());
            for i in 0..m {
                let j = (i + 1) % m;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (poly.v[i], poly.v[j]);
                if ti != BACK {
                    ff.push(vi);
                }
                if ti != FRONT {
                    bb.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (w - n.dot(vi)) / n.dot(vj - vi);
                    let x = vi + (vj - vi) * t;
                    ff.push(x);
                    bb.push(x);
                }
            }
            if ff.len() >= 3 {
                f.push(Poly { v: ff, plane: poly.plane });
            }
            if bb.len() >= 3 {
                b.push(Poly { v: bb, plane: poly.plane });
            }
        }
    }
}

#[derive(Default)]
pub struct Node {
    plane: Option<Plane>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polys: Vec<Poly>,
}

impl Node {
    pub fn new(polys: Vec<Poly>) -> Node {
        let mut n = Node::default();
        if !polys.is_empty() {
            n.build(polys);
        }
        n
    }

    fn invert(&mut self) {
        self.polys = self.polys.iter().map(Poly::flip).collect();
        if let Some(p) = self.plane {
            self.plane = Some(p.flip());
        }
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    fn clip_polys(&self, polys: Vec<Poly>) -> Vec<Poly> {
        let Some(plane) = self.plane else { return polys };
        let (mut f, mut b) = (Vec::new(), Vec::new());
        for p in polys {
            // split(plane, p, f, b, f, b): coplanar front to f, coplanar back to b.
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            split(&plane, p, &mut Dest::Four(&mut cf, &mut cb, &mut f, &mut b));
            // Coplanar outputs and split outputs never both happen for one polygon, and a polygon gives at
            // most one coplanar output, so appending afterwards keeps Python's order.
            f.extend(cf);
            b.extend(cb);
        }
        if let Some(front) = &self.front {
            f = front.clip_polys(f);
        }
        let b = match &self.back {
            Some(back) => back.clip_polys(b),
            None => Vec::new(),
        };
        f.extend(b);
        f
    }

    fn clip_to(&mut self, other: &Node) {
        self.polys = other.clip_polys(std::mem::take(&mut self.polys));
        if let Some(f) = &mut self.front {
            f.clip_to(other);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(other);
        }
    }

    fn all(&self) -> Vec<Poly> {
        let mut out = self.polys.clone();
        if let Some(f) = &self.front {
            out.extend(f.all());
        }
        if let Some(b) = &self.back {
            out.extend(b.all());
        }
        out
    }

    fn build(&mut self, polys: Vec<Poly>) {
        if polys.is_empty() {
            return;
        }
        if self.plane.is_none() {
            self.plane = Some(polys[0].plane);
        }
        let plane = self.plane.unwrap();
        let (mut f, mut b) = (Vec::new(), Vec::new());
        for p in polys {
            split(&plane, p, &mut Dest::Three(&mut self.polys, &mut f, &mut b));
        }
        if !f.is_empty() {
            self.front.get_or_insert_with(Box::default).build(f);
        }
        if !b.is_empty() {
            self.back.get_or_insert_with(Box::default).build(b);
        }
    }
}

fn bounds(polys: &[Poly]) -> (V3, V3) {
    let mut lo = V3([f64::INFINITY; 3]);
    let mut hi = V3([f64::NEG_INFINITY; 3]);
    for p in polys {
        for q in &p.v {
            lo = lo.min(*q);
            hi = hi.max(*q);
        }
    }
    (lo, hi)
}

pub fn csg_union(a: Vec<Poly>, b: Vec<Poly>) -> Vec<Poly> {
    let mut na = Node::new(a);
    let mut nb = Node::new(b);
    na.clip_to(&nb);
    nb.clip_to(&na);
    nb.invert();
    nb.clip_to(&na);
    nb.invert();
    na.build(nb.all());
    na.all()
}

/// geom.union_polys: a true union of closed solids given as planar polygon lists. Each solid joins
/// the first one it overlaps.
pub fn union_polys(solids: Vec<Vec<Vec<V3>>>) -> Vec<Poly> {
    let mut groups: Vec<Vec<Poly>> = solids
        .into_iter()
        .map(|s| s.into_iter().filter_map(|v| Plane::of(&v).map(|plane| Poly { v, plane })).collect())
        .collect();
    let mut changed = true;
    while changed && groups.len() > 1 {
        changed = false;
        'outer: for i in 0..groups.len() {
            let (li, hi) = bounds(&groups[i]);
            for j in i + 1..groups.len() {
                let (lj, hj) = bounds(&groups[j]);
                if (0..3).all(|k| li[k] <= hj[k] + 1e-6) && (0..3).all(|k| lj[k] <= hi[k] + 1e-6) {
                    let gj = groups.remove(j);
                    let gi = std::mem::take(&mut groups[i]);
                    groups[i] = csg_union(gi, gj);
                    changed = true;
                    break 'outer;
                }
            }
        }
    }
    groups.into_iter().flatten().collect()
}

pub fn polys_to_tris(polys: &[Poly]) -> Vec<Tri> {
    let mut tris = Vec::new();
    for p in polys {
        for k in 1..p.v.len().saturating_sub(1) {
            tris.push([p.v[0], p.v[k], p.v[k + 1]]);
        }
    }
    tris
}

pub fn weld_index(tris: &[Tri], tol: f64) -> (Vec<V3>, Vec<[usize; 3]>) {
    let mut keys: HashMap<[i64; 3], usize> = HashMap::new();
    let mut verts = Vec::new();
    let mut idx = Vec::with_capacity(tris.len());
    for t in tris {
        let mut row = [0usize; 3];
        for k in 0..3 {
            let key = t[k].0.map(|x| (x / tol).round_ties_even() as i64);
            let id = *keys.entry(key).or_insert_with(|| {
                verts.push(t[k]);
                verts.len() - 1
            });
            row[k] = id;
        }
        idx.push(row);
    }
    (verts, idx)
}

/// geom.repair_t_junctions: welds and splits every edge at the vertices lying on it.
pub fn repair_t_junctions(tris: &[Tri]) -> (Vec<Tri>, bool) {
    let (verts, idx) = weld_index(tris, 1e-5);
    // Vertices sorted by x, to find those inside an edge's box quickly; the first (lowest index)
    // vertex that qualifies is taken, as in the Python's ascending scan.
    let mut by_x: Vec<usize> = (0..verts.len()).collect();
    by_x.sort_by(|&a, &b| verts[a][0].total_cmp(&verts[b][0]).then(a.cmp(&b)));
    let xs: Vec<f64> = by_x.iter().map(|&i| verts[i][0]).collect();
    let on_edge = |a: usize, b: usize| -> Option<usize> {
        let (pa, pb) = (verts[a], verts[b]);
        let d = pb - pa;
        let ll = d.dot(d);
        if ll < 1e-14 {
            return None;
        }
        let lo = pa.min(pb) - V3([1e-5; 3]);
        let hi = pa.max(pb) + V3([1e-5; 3]);
        let start = xs.partition_point(|&x| x < lo[0]);
        let mut best: Option<usize> = None;
        for &i in &by_x[start..] {
            let q = verts[i];
            if q[0] > hi[0] {
                break;
            }
            if !(0..3).all(|k| q[k] >= lo[k] && q[k] <= hi[k]) || i == a || i == b {
                continue;
            }
            if best.is_some_and(|bi| bi < i) {
                continue;
            }
            let t = (q - pa).dot(d) / ll;
            if 1e-6 < t && t < 1.0 - 1e-6 && (pa + d * t - q).norm() < 2e-5 {
                best = Some(i);
            }
        }
        best
    };
    let mut out = Vec::new();
    let mut stack: Vec<[usize; 3]> = idx.into_iter().filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]).collect();
    let mut guard = 0;
    while !stack.is_empty() && guard < 200000 {
        guard += 1;
        let [a, b, c] = stack.pop().unwrap();
        let area = (verts[b] - verts[a]).cross(verts[c] - verts[a]).norm();
        if area < 1e-10 {
            continue;
        }
        let mut done = true;
        for (x, y, z) in [(a, b, c), (b, c, a), (c, a, b)] {
            if let Some(m) = on_edge(x, y) {
                stack.push([x, m, z]);
                stack.push([m, y, z]);
                done = false;
                break;
            }
        }
        if done {
            out.push([a, b, c]);
        }
    }
    (out.iter().map(|t| t.map(|i| verts[i])).collect(), stack.is_empty())
}

/// geom.closed_stats: (edges used other than twice, edges wound inconsistently) after welding.
pub fn closed_stats(tris: &[Tri]) -> (usize, usize) {
    let (_, idx) = weld_index(tris, 1e-5);
    let mut uses: HashMap<(usize, usize), Vec<bool>> = HashMap::new();
    for t in &idx {
        for (x, y) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            uses.entry((x.min(y), x.max(y))).or_default().push(x < y);
        }
    }
    let bad = uses.values().filter(|u| u.len() != 2).count();
    let odd = uses.values().filter(|u| u.len() == 2 && u[0] == u[1]).count();
    (bad, odd)
}

/// geom.write_stl's bytes: binary STL, float32, normals computed in float32.
pub fn stl_bytes(tris: &[[[f32; 3]; 3]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(84 + tris.len() * 50);
    let mut head = b"synthetic".to_vec();
    head.resize(80, 0);
    out.extend_from_slice(&head);
    out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for t in tris {
        let e1 = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
        let e2 = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
        let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        let ln = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        for c in n {
            out.extend_from_slice(&(c / ln).to_le_bytes());
        }
        for p in t {
            for c in p {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    out
}

pub fn read_stl(bytes: &[u8]) -> Vec<[[f32; 3]; 3]> {
    let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    let f = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    (0..n)
        .map(|i| {
            let base = 84 + i * 50 + 12;
            [0, 1, 2].map(|k| [0, 1, 2].map(|c| f(base + k * 12 + c * 4)))
        })
        .collect()
}
