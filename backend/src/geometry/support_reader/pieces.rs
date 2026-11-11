//! Stage 1 and 2 of the reader: the pieces of the support mesh and how they connect.
//!
//! 1. Every closed solid of the support mesh (split across manifold edges; at an edge with more
//!    than two triangles each triangle pairs with the one bounding the same wedge) is fitted as a
//!    solid of revolution (rod, cone and ball sections along one straight axis) where it is one,
//!    as a ball or a bar (a prism of any cross-section), else kept as an unread piece with its
//!    extent (principal axis), ends and width. A solid that is no single straight solid (a welded
//!    union of rods, a tip bent at a mitre) is split into its straight stretches where it has them
//!    ([`Piece::segments`]). Nothing is dropped.
//! 2. Connections: pieces whose solids overlap or abut, measured on the raw mesh by rays along the
//!    three axes on a jittered lattice ([`RAY_PITCH_MM`]), each piece's volume inside the model,
//!    near misses (a gap under [`NEAR_MM`] with nothing between), and which pieces each contact
//!    sits in.

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::Serialize;

#[cfg(test)]
use super::math::dist;
use super::math::{P, add, centroid, cross, dot, eig3, find, len, mul, ray_hits, solve3, sub, unit};
use super::union_split::{self, Rod};

/// Vertices within 0.05 micron are one.
const WELD: f64 = 20000.0;
/// The contact holder's parity rays: generic directions, so no ray runs along a polygon's edges.
const RAYS: [P; 3] = [[0.0137, 0.0071, 0.99988], [-0.0113, 0.0169, 0.99979], [0.0191, -0.0127, -0.99974]];
/// Solids ending and starting this close along a ray touch.
const ABUT_MM: f64 = 0.02;
/// A gap this small with nothing between is listed as a near miss.
const NEAR_MM: f64 = 0.3;
/// Rays per tile side.
const TILE: i64 = 128;
/// Spacing of the connection rays. Fine enough for a 0.1 mm hair support to meet a dozen rays
/// along each axis.
pub const RAY_PITCH_MM: f64 = 0.025;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PieceKind {
    /// A solid of revolution about one straight axis.
    Axi,
    Ball,
    /// A prism of any cross-section, read as a rod of the same length and cross-section area.
    Bar,
    Unread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionKind {
    Rod,
    Cone,
    Ball,
}

/// One stretch of a piece's profile along its axis, measured from its `a` end.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Section {
    pub k: SectionKind,
    pub t0: f64,
    pub t1: f64,
    pub r0: f64,
    pub r1: f64,
    /// A ball's centre along the axis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tc: Option<f64>,
    /// A ball's radius, when it is read from a profile run.
    #[serde(rename = "R", skip_serializing_if = "Option::is_none")]
    pub big_r: Option<f64>,
}

impl Section {
    fn new(k: SectionKind, t0: f64, t1: f64, r0: f64, r1: f64) -> Self {
        Section { k, t0, t1, r0, r1, tc: None, big_r: None }
    }
}

/// One straight stretch of a piece that is no single straight solid.
#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub a: P,
    pub b: P,
    pub width: f64,
    pub sections: Vec<Section>,
    pub volume: f64,
    pub tris: usize,
    pub lo: P,
    pub hi: P,
}

#[derive(Debug, Clone, Serialize)]
pub struct Piece {
    pub id: usize,
    /// Triangle count (the triangles themselves are in [`Pieces::tri_piece`]).
    pub tris: usize,
    pub open_edges: usize,
    pub volume: f64,
    /// Volume measured by the connection rays.
    pub ray_volume: f64,
    pub area: f64,
    pub lo: P,
    pub hi: P,
    pub kind: PieceKind,
    /// Axis ends (lowest first for a solid of revolution).
    pub a: P,
    pub b: P,
    pub width: f64,
    pub sections: Vec<Section>,
    /// Share of the area whose normals miss the fitted axis.
    pub azi_bad: f64,
    /// Why an unread piece is unread.
    pub why: &'static str,
    pub segments: Vec<Segment>,
    /// Volume inside the model.
    pub in_model: f64,
    pub in_model_at: Option<P>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionKind {
    Overlap,
    Abut,
    Near,
}

#[derive(Debug, Clone, Serialize)]
pub struct Connection {
    pub a: usize,
    pub b: usize,
    pub kind: ConnectionKind,
    pub at: P,
    pub overlap: f64,
    pub abut_rays: f64,
    pub gap: Option<f64>,
}

/// A piece a contact lies in (`inside`), or whose surface is within reach of it, with the
/// distance to its nearest vertex or face centre.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ContactPiece(pub usize, pub bool, pub f64);

#[derive(Debug, Clone, Serialize)]
pub struct Pieces {
    pub pitch: f64,
    pub stl_tris: usize,
    pub dropped_degenerate: usize,
    pub dropped_repeated: usize,
    pub nonmanifold_edges: usize,
    pub pieces: Vec<Piece>,
    pub connections: Vec<Connection>,
    /// Per contact: up to six pieces, those it lies in first, then by distance.
    pub contact_pieces: Vec<Vec<ContactPiece>>,
    /// Per support triangle (as given): its piece, or `u32::MAX` when dropped.
    #[serde(skip)]
    pub tri_piece: Vec<u32>,
}

struct Fit {
    open_edges: usize,
    volume: f64,
    area: f64,
    kind: PieceKind,
    a: P,
    b: P,
    width: f64,
    sections: Vec<Section>,
    azi_bad: f64,
    why: &'static str,
}

/// The fit of one solid (`tv`, its triangles' corners), from its open edges, volume and area.
fn fit(tv: &[[P; 3]], f: &mut Fit, hint: Option<&Rod>) {
    let mut cen = [0.0; 3];
    let mut nn = [[0.0; 3]; 3];
    let mut cov = [[0.0; 3]; 3];
    let mut fa = Vec::with_capacity(tv.len());
    for t in tv {
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        let a = 0.5 * len(n);
        let c = centroid(t);
        cen = add(cen, mul(c, a));
        fa.push((unit(n), a, c));
    }
    let area = f.area.max(1e-30);
    cen = mul(cen, 1.0 / area);
    for (n, a, c) in &fa {
        let d = sub(*c, cen);
        for i in 0..3 {
            for j in 0..3 {
                nn[i][j] += a * n[i] * n[j];
                cov[i][j] += a * d[i] * d[j];
            }
        }
    }
    let (cv, ce) = eig3(cov);
    // The unread fallback first: the principal axis through the surface centroid.
    let ext = |d: P, c: P| -> (f64, f64) {
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for t in tv {
            for v in t {
                let x = dot(sub(*v, c), d);
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        (lo, hi)
    };
    let radial = |d: P, c: P, v: P| -> f64 {
        let w = sub(v, c);
        len(sub(w, mul(d, dot(w, d))))
    };
    {
        let d = ce[2];
        let (lo, hi) = ext(d, cen);
        f.a = add(cen, mul(d, lo));
        f.b = add(cen, mul(d, hi));
        let l = (hi - lo).max(1e-9);
        let mut rs: Vec<f64> = tv.iter().flat_map(|t| t.iter().map(|v| radial(d, cen, *v))).collect();
        rs.sort_by(f64::total_cmp);
        let med = rs[rs.len() / 2];
        f.width = if f.open_edges == 0 && f.volume > 0.0 {
            2.0 * (f.volume / (std::f64::consts::PI * l)).sqrt()
        } else {
            2.0 * med
        };
    }
    if f.open_edges == 0 && f.volume <= 0.0 {
        f.why = "inverted";
        return;
    }
    if tv.len() < 6 {
        f.why = "sheet";
        return;
    }
    // A ball: every vertex the same distance from the centroid ...
    {
        let rs: Vec<f64> = tv.iter().flat_map(|t| t.iter().map(|v| len(sub(*v, cen)))).collect();
        let mean = rs.iter().sum::<f64>() / rs.len() as f64;
        let dev = rs.iter().map(|r| (r - mean).abs()).fold(0.0, f64::max);
        // ... and the vertices lie all round it (a low-polygon rod's two end rings are equidistant too).
        let round =
            cv[0] > 0.4 * cv[2] && f.open_edges == 0 && (f.volume / (4.19 * mean * mean * mean) - 1.0).abs() < 0.3;
        if tv.len() >= 20 && round && dev <= (0.03 * mean).max(0.004) {
            f.kind = PieceKind::Ball;
            f.a = cen;
            f.b = cen;
            f.width = 2.0 * mean;
            let mut s = Section::new(SectionKind::Ball, -mean, mean, mean, mean);
            s.tc = Some(0.0);
            f.sections = vec![s];
            return;
        }
    }
    // A solid of revolution: every face normal's line meets one axis. Directions tried: the
    // eigenvectors of the normals' and of the positions' covariance; the axis position solved
    // from the faces.
    let (_, ne) = eig3(nn);
    let mut best: Option<(f64, P, P)> = None;
    let cands: Vec<P> = match hint {
        Some(h) => vec![h.d],
        None => vec![ne[0], ne[2], ne[1], ce[2], ce[0]],
    };
    for d in cands {
        let d = unit(d);
        let c = match hint {
            Some(h) => Some(h.c),
            None => axis_through(&fa, d, cen),
        };
        let Some(c) = c else { continue };
        // Share of the area whose normal misses the axis by more than 0.3 of the face's radius
        // (a square rod's triangles miss by 0.26).
        let mut bad = 0.0;
        for (n, a, fc) in &fa {
            let w = sub(*fc, c);
            let rad = sub(w, mul(d, dot(w, d)));
            let r = len(rad);
            if r < 1e-6 {
                continue;
            }
            let miss = dot(*n, cross(d, rad)).abs() / r;
            if miss > 0.3 {
                bad += a;
            }
        }
        let bad = bad / area;
        if best.as_ref().is_none_or(|b| bad < b.0) {
            best = Some((bad, d, c));
        }
    }
    let Some((bad, mut d, c)) = best else {
        f.why = "no axis";
        return;
    };
    f.azi_bad = bad;
    if hint.is_some() && bad > 0.10 {
        f.why = "split rod off its axis";
        return;
    }
    if bad > 0.10 {
        // A bar: a prism of any cross-section (every face along the axis or across it). Read as a
        // rod of the same length and cross-section area.
        let mut pick: Option<(f64, P)> = None;
        for d in [ne[0], ne[1], ne[2], ce[2]] {
            let d = unit(d);
            let side: f64 = fa.iter().filter(|(n, _, _)| dot(*n, d).abs() <= 0.05).map(|f| f.1).sum();
            // Sides along the axis make up most of the surface; the ends may be cut at any angle
            // (mitred bars).
            if side / area < 0.55 {
                continue;
            }
            let (lo, hi) = ext(d, cen);
            if pick.as_ref().is_none_or(|b| hi - lo > b.0) {
                pick = Some((hi - lo, d));
            }
        }
        if let Some((l, mut d)) = pick
            && l > 1e-6
            && f.open_edges == 0
        {
            if d[2] < 0.0 {
                d = mul(d, -1.0);
            }
            let (lo, hi) = ext(d, cen);
            let r = (f.volume / (std::f64::consts::PI * l)).sqrt();
            f.kind = PieceKind::Bar;
            f.a = add(cen, mul(d, lo));
            f.b = add(cen, mul(d, hi));
            f.width = 2.0 * r;
            f.sections = vec![Section::new(SectionKind::Rod, 0.0, l, r, r)];
            return;
        }
        f.why = if f.open_edges > 0 { "open, not a solid of revolution" } else { "not a solid of revolution" };
        return;
    }
    if d[2] < 0.0 {
        d = mul(d, -1.0);
    }
    // A welded rod's profile from its facets' planes, as points at those radii.
    let (t0, t1, rmax, secs) = match hint {
        Some(h) => {
            let u = unit(if d[0].abs() < 0.9 { cross(d, [1.0, 0.0, 0.0]) } else { cross(d, [0.0, 1.0, 0.0]) });
            let pts: Vec<P> = h.prof.iter().map(|&(t, r)| add(add(h.c, mul(h.d, t)), mul(u, r))).collect();
            profile_pts(&pts, d, c)
        }
        None => profile_secs(tv, d, c),
    };
    f.kind = PieceKind::Axi;
    f.a = add(c, mul(d, t0));
    f.b = add(c, mul(d, t1));
    f.width = 2.0 * rmax;
    f.sections = secs;
}

/// The profile of a solid of revolution about the axis (c, d): (t0, t1, largest radius, sections).
fn profile_secs(tv: &[[P; 3]], d: P, c: P) -> (f64, f64, f64, Vec<Section>) {
    let vs: Vec<P> = tv.iter().flat_map(|t| t.iter().copied()).collect();
    profile_pts(&vs, d, c)
}

/// Douglas-Peucker simplification of a profile, appending the kept points after the first.
fn douglas_peucker(pts: &[(f64, f64)], tol: f64, out: &mut Vec<(f64, f64)>) {
    if pts.len() <= 2 {
        out.push(pts[pts.len() - 1]);
        return;
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = (dx * dx + dy * dy).sqrt().max(1e-12);
    let (mut k, mut far) = (0, 0.0);
    for (i, q) in pts.iter().enumerate().skip(1).take(pts.len() - 2) {
        let dist = ((q.0 - a.0) * dy - (q.1 - a.1) * dx).abs() / l;
        if dist > far {
            far = dist;
            k = i;
        }
    }
    if far <= tol {
        out.push(b);
    } else {
        douglas_peucker(&pts[..=k], tol, out);
        douglas_peucker(&pts[k..], tol, out);
    }
}

fn profile_pts(vs: &[P], d: P, c: P) -> (f64, f64, f64, Vec<Section>) {
    let radial = |v: P| -> f64 {
        let w = sub(v, c);
        len(sub(w, mul(d, dot(w, d))))
    };
    // The profile: the largest radius at each height along the axis.
    let mut pts: Vec<(f64, f64)> = vs.iter().map(|v| (dot(sub(*v, c), d), radial(*v))).collect();
    pts.sort_by(|x, y| x.0.total_cmp(&y.0));
    // Bins 0.004 mm long, each at its largest radius; a bin holding two clearly different radii
    // (not counting points on the axis) is a step (a flat shoulder, e.g. a wide pad under a post
    // in one shell): both kept, in the order that continues the radius before it, so the next
    // stretch does not taper from the wide one.
    let mut bins: Vec<(f64, usize, usize)> = Vec::new();
    let mut start = pts[0].0;
    for (k, &(t, _)) in pts.iter().enumerate() {
        if bins.is_empty() || t - start > 0.004 {
            bins.push((t, k, k + 1));
            start = t;
        } else {
            bins.last_mut().expect("a bin exists").2 = k + 1;
        }
    }
    let mut prof: Vec<(f64, f64)> = Vec::new();
    for &(t, k0, k1) in &bins {
        let hi = pts[k0..k1].iter().map(|q| q.1).fold(0.0, f64::max);
        // The smallest radius of a point clearly off the axis (cap centres sit on it).
        let lo = pts[k0..k1].iter().filter(|q| q.1 > 0.02).map(|q| q.1).fold(hi, f64::min);
        if let Some(&(_, prev)) = prof.last()
            && lo < 0.6 * hi
            && hi - lo > 0.05
        {
            if (prev - hi).abs() <= (prev - lo).abs() {
                prof.push((t, hi));
                prof.push((t + 1e-4, lo));
            } else {
                prof.push((t, lo));
                prof.push((t + 1e-4, hi));
            }
        } else {
            prof.push((t, hi));
        }
    }
    let (t0, t1) = (pts[0].0, pts[pts.len() - 1].0);
    let rmax = prof.iter().map(|x| x.1).fold(0.0, f64::max);
    // Ball runs: at least five profile points on a circle centred on the axis, spanning 0.8 of
    // its radius.
    let mut secs: Vec<Section> = Vec::new();
    let mut runs: Vec<(usize, usize, f64, f64)> = Vec::new();
    let mut i = 0;
    while i + 4 < prof.len() {
        let (a, b) = (prof[i], prof[i + 2]);
        let mut got = None;
        if (b.0 - a.0).abs() > 1e-6 {
            let tc = ((b.0 * b.0 + b.1 * b.1) - (a.0 * a.0 + a.1 * a.1)) / (2.0 * (b.0 - a.0));
            let rr = ((a.0 - tc).powi(2) + a.1 * a.1).sqrt();
            let tol = (0.02 * rr).max(0.004);
            if rr < 20.0 {
                let mut j = i;
                while j + 1 < prof.len()
                    && (((prof[j + 1].0 - tc).powi(2) + prof[j + 1].1.powi(2)).sqrt() - rr).abs() <= tol
                {
                    j += 1;
                }
                // A cone's points also fit a huge circle badly; a rod's never do. Near-straight
                // runs are no ball.
                let straight = {
                    let (x0, x1) = (prof[i], prof[j]);
                    let mid = prof[(i + j) / 2];
                    let lin = x0.1 + (x1.1 - x0.1) * (mid.0 - x0.0) / (x1.0 - x0.0).max(1e-9);
                    (mid.1 - lin).abs() < 0.05 * rr
                };
                if j - i >= 4 && prof[j].0 - prof[i].0 >= 0.8 * rr && !straight {
                    got = Some((i, j, tc, rr));
                }
            }
        }
        if let Some(g) = got {
            runs.push(g);
            i = g.1;
        } else {
            i += 1;
        }
    }
    // Between ball runs: straight stretches of the profile, each a rod or a cone.
    let tol = (0.02 * rmax).max(0.006);
    let mut at = 0;
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for r in &runs {
        if r.0 > at {
            spans.push((at, r.0));
        }
        at = r.1;
    }
    if at < prof.len() - 1 {
        spans.push((at, prof.len() - 1));
    }
    for (s, e) in spans {
        let mut line = vec![prof[s]];
        douglas_peucker(&prof[s..=e], tol, &mut line);
        for w in line.windows(2) {
            let (x, y) = (w[0], w[1]);
            if y.0 - x.0 < 0.01 {
                continue; // a step (cap or shoulder)
            }
            let k =
                if (x.1 - y.1).abs() <= (0.05 * x.1.max(y.1)).max(0.01) { SectionKind::Rod } else { SectionKind::Cone };
            secs.push(Section::new(k, x.0 - t0, y.0 - t0, x.1, y.1));
        }
    }
    for r in &runs {
        let mut s = Section::new(SectionKind::Ball, prof[r.0].0 - t0, prof[r.1].0 - t0, prof[r.0].1, prof[r.1].1);
        s.tc = Some(r.2 - t0);
        s.big_r = Some(r.3);
        secs.push(s);
    }
    secs.sort_by(|x, y| x.t0.total_cmp(&y.t0));
    if secs.is_empty() {
        // Shorter than a step along its axis: a disc, kept as a rod of its own height.
        secs.push(Section::new(SectionKind::Rod, 0.0, t1 - t0, rmax, rmax));
    }
    (t0, t1, rmax, secs)
}

/// The volume a profile describes (a ball section at 0.8 of its cylinder).
fn rev_volume(secs: &[Section]) -> f64 {
    secs.iter()
        .map(|x| {
            if x.k == SectionKind::Ball {
                let r = x.big_r.unwrap_or(0.0).max(x.r0.max(x.r1));
                std::f64::consts::PI * (x.t1 - x.t0) * r * r * 0.8
            } else {
                std::f64::consts::PI * (x.t1 - x.t0) / 3.0 * (x.r0 * x.r0 + x.r0 * x.r1 + x.r1 * x.r1)
            }
        })
        .sum()
}

/// The axis position for direction d: every face normal's line meets it (least squares),
/// pinned along d at `cen`.
fn axis_through(fa: &[(P, f64, P)], d: P, cen: P) -> Option<P> {
    let mut m = [[0.0; 3]; 3];
    let mut rhs = [0.0; 3];
    for (n, a, c) in fa {
        let q = cross(*n, d);
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += a * q[i] * q[j];
            }
            rhs[i] += a * q[i] * dot(q, *c);
        }
    }
    let tr = (m[0][0] + m[1][1] + m[2][2]).max(1e-12);
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] += tr * d[i] * d[j];
        }
        rhs[i] += tr * d[i] * dot(d, cen);
    }
    solve3(m, rhs)
}

type Stretch = (P, P, f64, Vec<Section>, Vec<usize>);

/// A closed solid that is no single straight solid (a tip bent at a mitre, a leg or tip unioned
/// onto its shaft, a lattice welded into one shell) split into its straight stretches. The sides
/// of a tessellated rod or cone are long flat strips (coplanar triangles) whose long edges run
/// along its axis; strips sharing such a long edge, and lying along each other, are one stretch. A
/// crease across the strips (a mitre ring, the loop where a leg meets a shaft) ends a stretch.
/// Each stretch is then fitted on its own (axis from its faces, profile, rod and cone sections).
/// Caps, balls and the bits round a junction are left to the stretches they join. Returns the
/// stretches (with the triangles each was fitted on), or nothing when the solid does not split
/// into two or more that cover most of its sides.
fn split_straight(tv: &[[P; 3]]) -> Vec<Stretch> {
    let nt = tv.len();
    let mut vid: FxHashMap<[u64; 3], usize> = FxHashMap::default();
    let tri: Vec<[usize; 3]> = tv
        .iter()
        .map(|t| {
            t.map(|v| {
                let k = v.map(|x| x.to_bits());
                let n = vid.len();
                *vid.entry(k).or_insert(n)
            })
        })
        .collect();
    let fa: Vec<(P, f64, P)> = tv
        .iter()
        .map(|t| {
            let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            (unit(n), 0.5 * len(n), centroid(t))
        })
        .collect();
    let total: f64 = fa.iter().map(|f| f.1).sum();
    let mut edges: FxHashMap<(usize, usize), Vec<usize>> = FxHashMap::default();
    for (k, t) in tri.iter().enumerate() {
        for i in 0..3 {
            let (x, y) = (t[i], t[(i + 1) % 3]);
            edges.entry((x.min(y), x.max(y))).or_default().push(k);
        }
    }
    let mut edge_list: Vec<(&(usize, usize), &Vec<usize>)> = edges.iter().collect();
    edge_list.sort_unstable_by_key(|e| *e.0);
    let mut par: Vec<u32> = (0..nt as u32).collect();
    for (_, l) in &edge_list {
        if l.len() == 2 && fa[l[0]].1 > 0.0 && fa[l[1]].1 > 0.0 && dot(fa[l[0]].0, fa[l[1]].0) > 0.99995 {
            let (x, y) = (find(&mut par, l[0] as u32), find(&mut par, l[1] as u32));
            par[x as usize] = y;
        }
    }
    // Flat facets: their long direction (principal axis of their corners) where they are long
    // and thin.
    let mut fac: FxHashMap<u32, Vec<usize>> = FxHashMap::default();
    for k in 0..nt {
        let r = find(&mut par, k as u32);
        fac.entry(r).or_default().push(k);
    }
    let mut fdir: FxHashMap<u32, P> = FxHashMap::default();
    for (&r, ks) in &fac {
        let mut vs: Vec<usize> = ks.iter().flat_map(|&k| tri[k]).collect();
        vs.sort_unstable();
        vs.dedup();
        let pos: Vec<P> = vs
            .iter()
            .map(|&v| {
                let k = ks.iter().find(|&&k| tri[k].contains(&v)).expect("a facet vertex is on one of its triangles");
                tv[*k][tri[*k].iter().position(|&x| x == v).expect("vertex of the triangle")]
            })
            .collect();
        let c = mul(pos.iter().fold([0.0; 3], |a, b| add(a, *b)), 1.0 / pos.len() as f64);
        let mut cov = [[0.0; 3]; 3];
        for q in &pos {
            let d = sub(*q, c);
            for i in 0..3 {
                for j in 0..3 {
                    cov[i][j] += d[i] * d[j];
                }
            }
        }
        let (ev, evec) = eig3(cov);
        if ev[2] > 9.0 * ev[1].max(1e-12) {
            fdir.insert(r, evec[2]);
        }
    }
    // Strips into stretches across their long shared edges.
    let mut sp: FxHashMap<u32, u32> = fdir.keys().map(|&k| (k, k)).collect();
    fn f2(p: &mut FxHashMap<u32, u32>, mut x: u32) -> u32 {
        while p[&x] != x {
            let y = p[&p[&x]];
            p.insert(x, y);
            x = y;
        }
        x
    }
    let cos8 = 8.0f64.to_radians().cos();
    let cos25 = 25.0f64.to_radians().cos();
    // Generator edges (along the strips) and a facet on them.
    let mut gens: Vec<(usize, usize, u32)> = Vec::new();
    let mut vpos: FxHashMap<usize, P> = FxHashMap::default();
    for (k, t) in tri.iter().enumerate() {
        for i in 0..3 {
            vpos.insert(t[i], tv[k][i]);
        }
    }
    for &(&(x, y), l) in &edge_list {
        if l.len() != 2 {
            continue;
        }
        let (fa_, fb_) = (find(&mut par, l[0] as u32), find(&mut par, l[1] as u32));
        if fa_ == fb_ {
            continue;
        }
        let (Some(&ga), Some(&gb)) = (fdir.get(&fa_), fdir.get(&fb_)) else { continue };
        let e = unit(sub(vpos[&y], vpos[&x]));
        if dot(ga, gb).abs() > cos8 && dot(e, ga).abs() > cos25 {
            let (p, q) = (f2(&mut sp, fa_), f2(&mut sp, fb_));
            sp.insert(p, q);
            gens.push((x, y, fa_));
            gens.push((x, y, fb_));
        }
    }
    let mut groups: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    let mut keys: Vec<u32> = sp.keys().copied().collect();
    keys.sort_unstable();
    for k in keys {
        let r = f2(&mut sp, k);
        groups.entry(r).or_default().push(k);
    }
    // The corners of the strips (ends of their long shared edges) give the profile: points inside
    // a flat facet (the loop where a leg was unioned on) sit nearer the axis than the polygon's
    // corners.
    let tri_fac: Vec<u32> = (0..nt as u32).map(|k| find(&mut par, k)).collect();
    let mut fac_gv: FxHashMap<u32, Vec<usize>> = FxHashMap::default();
    for &(x, y, f) in &gens {
        fac_gv.entry(f).or_default().extend([x, y]);
    }
    let fit_group = |ks: &[usize]| -> Option<Stretch> {
        let sub_fa: Vec<(P, f64, P)> = ks.iter().map(|&k| fa[k]).collect();
        let area: f64 = sub_fa.iter().map(|f| f.1).sum();
        if area < 1e-9 {
            return None;
        }
        // Direction: the area-weighted long direction of the strips, refined by the normals
        // (least normal along it).
        let mut nn = [[0.0; 3]; 3];
        for (n, a, _) in &sub_fa {
            for i in 0..3 {
                for j in 0..3 {
                    nn[i][j] += a * n[i] * n[j];
                }
            }
        }
        let (_, ne) = eig3(nn);
        let mut d = unit(ne[0]);
        if d[2] < 0.0 {
            d = mul(d, -1.0);
        }
        let cen = mul(sub_fa.iter().fold([0.0; 3], |s, f| add(s, mul(f.2, f.1))), 1.0 / area);
        let c = axis_through(&sub_fa, d, cen)?;
        let mut fs: Vec<u32> = ks.iter().map(|&k| tri_fac[k]).collect();
        fs.sort_unstable();
        fs.dedup();
        let mut vs: Vec<usize> = fs.iter().flat_map(|f| fac_gv.get(f).cloned().unwrap_or_default()).collect();
        vs.sort_unstable();
        vs.dedup();
        let (t0, t1, rmax, secs) = if vs.len() >= 6 {
            let pts: Vec<P> = vs.iter().map(|v| vpos[v]).collect();
            profile_pts(&pts, d, c)
        } else {
            let sub_tv: Vec<[P; 3]> = ks.iter().map(|&k| tv[k]).collect();
            profile_secs(&sub_tv, d, c)
        };
        if t1 - t0 < 1e-6 {
            return None;
        }
        Some((add(c, mul(d, t0)), add(c, mul(d, t1)), 2.0 * rmax, secs, ks.to_vec()))
    };
    let mut segs: Vec<Stretch> = Vec::new();
    let mut group_list: Vec<&Vec<u32>> = groups.values().collect();
    group_list.sort_unstable_by_key(|g| g.iter().min().copied());
    for fs in group_list {
        let ks: Vec<usize> = fs.iter().flat_map(|f| fac[f].iter().copied()).collect();
        let area: f64 = ks.iter().map(|&k| fa[k].1).sum();
        if fs.len() < 3 || area < 0.001 * total {
            continue;
        }
        if let Some(sg) = fit_group(&ks) {
            segs.push(sg);
        }
    }
    let mut covered: f64 = segs.iter().map(|s| s.4.iter().map(|&k| fa[k].1).sum::<f64>()).sum();
    // Stretches on one line (a shaft whose strips the union cut in two) are one.
    loop {
        let mut merged = false;
        'outer: for i in 0..segs.len() {
            for j in i + 1..segs.len() {
                let (a0, b0, w0) = (segs[i].0, segs[i].1, segs[i].2);
                let (a1, b1, w1) = (segs[j].0, segs[j].1, segs[j].2);
                let (d0, d1) = (unit(sub(b0, a0)), unit(sub(b1, a1)));
                if dot(d0, d1).abs() < 5.0f64.to_radians().cos() {
                    continue;
                }
                let off = |p: P| {
                    let w = sub(p, a0);
                    len(sub(w, mul(d0, dot(w, d0))))
                };
                let r = 0.5 * w0.max(w1);
                if off(a1) > 0.3 * r + 0.05 || off(b1) > 0.3 * r + 0.05 {
                    continue;
                }
                let (s0, s1) = (dot(sub(a1, a0), d0), dot(sub(b1, a0), d0));
                let (lo1, hi1) = (s0.min(s1), s0.max(s1));
                let l0 = len(sub(b0, a0));
                let gap = (lo1 - l0).max(-hi1).max(0.0);
                if gap > 2.0 * r + 0.5 {
                    continue;
                }
                let mut ks = segs[i].4.clone();
                ks.extend(segs[j].4.iter().copied());
                if let Some(sg) = fit_group(&ks) {
                    segs[i] = sg;
                    segs.remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            break;
        }
    }
    covered = covered.min(total);
    if segs.len() < 2 || covered < 0.5 * total {
        return Vec::new();
    }
    // A fixed order (lowest end first).
    segs.sort_by(|x, y| {
        let k = |s: &Stretch| (s.0[2].min(s.1[2]), s.0[0], s.0[1]);
        let (a, b) = (k(x), k(y));
        a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2))
    });
    segs
}

#[derive(Clone, Copy)]
struct Ev {
    ray: u32,
    z: f32,
    d: i8,
    /// `u32::MAX` is the model.
    piece: u32,
}

fn jit(i: i64, j: i64) -> (f64, f64) {
    let mut h = (i as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ (j as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 32;
    (((h & 0xFFFFFF) as f64 + 0.5) / 16777216.0, (((h >> 24) & 0xFFFFFF) as f64 + 0.5) / 16777216.0)
}

fn edge(ax: f64, ay: f64, bx: f64, by: f64, px: f64, py: f64) -> f64 {
    if (ax, ay) <= (bx, by) {
        (bx - ax) * (py - ay) - (by - ay) * (px - ax)
    } else {
        -((ax - bx) * (py - by) - (ay - by) * (px - bx))
    }
}

#[allow(clippy::too_many_arguments)]
fn raster(t: &[P; 3], tx: i64, ty: i64, pitch: f64, piece: u32, mask: Option<&[bool]>, out: &mut Vec<Ev>) {
    let area = (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0]);
    if area == 0.0 {
        return;
    }
    let x0 = t.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
    let x1 = t.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
    let y0 = t.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
    let y1 = t.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
    let i0 = ((x0 / pitch).floor() as i64 - 1).max(tx * TILE);
    let i1 = ((x1 / pitch).floor() as i64).min(tx * TILE + TILE - 1);
    let j0 = ((y0 / pitch).floor() as i64 - 1).max(ty * TILE);
    let j1 = ((y1 / pitch).floor() as i64).min(ty * TILE + TILE - 1);
    let s = if area > 0.0 { 1.0 } else { -1.0 };
    let d: i8 = if area > 0.0 { -1 } else { 1 };
    let in0 = ((t[0][0], t[0][1]) <= (t[1][0], t[1][1])) == (s > 0.0);
    let in1 = ((t[1][0], t[1][1]) <= (t[2][0], t[2][1])) == (s > 0.0);
    let in2 = ((t[2][0], t[2][1]) <= (t[0][0], t[0][1])) == (s > 0.0);
    for j in j0..=j1 {
        for i in i0..=i1 {
            let ray = ((j - ty * TILE) * TILE + (i - tx * TILE)) as usize;
            if let Some(m) = mask
                && !m[ray]
            {
                continue;
            }
            let (jx, jy) = jit(i, j);
            let (px, py) = ((i as f64 + jx) * pitch, (j as f64 + jy) * pitch);
            let e0 = edge(t[0][0], t[0][1], t[1][0], t[1][1], px, py) * s;
            let e1 = edge(t[1][0], t[1][1], t[2][0], t[2][1], px, py) * s;
            let e2 = edge(t[2][0], t[2][1], t[0][0], t[0][1], px, py) * s;
            if e0 < 0.0 || e1 < 0.0 || e2 < 0.0 {
                continue;
            }
            if (e0 == 0.0 && !in0) || (e1 == 0.0 && !in1) || (e2 == 0.0 && !in2) {
                continue;
            }
            let sum = e0 + e1 + e2;
            if sum <= 0.0 {
                continue;
            }
            let z = (e1 * t[0][2] + e2 * t[1][2] + e0 * t[2][2]) / sum;
            out.push(Ev { ray: ray as u32, z: z as f32, d, piece });
        }
    }
}

fn tile_index(tris: &[[P; 3]], pitch: f64) -> FxHashMap<(i64, i64), Vec<u32>> {
    let mut m: FxHashMap<(i64, i64), Vec<u32>> = FxHashMap::default();
    let w = TILE as f64 * pitch;
    for (i, t) in tris.iter().enumerate() {
        let x0 = t.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
        let x1 = t.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
        let y0 = t.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        let y1 = t.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        for tx in ((x0 - pitch) / w - 0.01).floor() as i64..=((x1 + pitch) / w + 0.01).floor() as i64 {
            for ty in ((y0 - pitch) / w - 0.01).floor() as i64..=((y1 + pitch) / w + 0.01).floor() as i64 {
                m.entry((tx, ty)).or_default().push(i as u32);
            }
        }
    }
    m
}

#[derive(Default, Clone)]
struct Pair {
    ov: f64,
    ovc: P,
    abut: f64,
    abc: P,
    gap: f64,
    gapc: P,
    has_gap: bool,
}

/// Per tile: the pairs' measures and each piece's (volume, volume inside the model, its moment).
type TileTotals = (FxHashMap<(u32, u32), Pair>, FxHashMap<u32, (f64, f64, P)>);

fn canon(ids: [u32; 3]) -> [u32; 3] {
    let m = *ids.iter().min().expect("three ids");
    if ids[0] == m {
        ids
    } else if ids[1] == m {
        [ids[1], ids[2], ids[0]]
    } else {
        [ids[2], ids[0], ids[1]]
    }
}

/// Reads the pieces of `support` (Z-up) against `model`, with the contacts (point, width) whose
/// pieces to find, measuring connections with rays `pitch` apart.
pub fn read(support: &[[P; 3]], model: &[[P; 3]], contacts: &[(P, f64)], pitch: f64) -> Pieces {
    // Weld, and drop degenerate and repeated triangles.
    let mut vid: FxHashMap<[i64; 3], u32> = FxHashMap::default();
    let mut verts: Vec<P> = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut src: Vec<u32> = Vec::new();
    let mut seen: rustc_hash::FxHashSet<[u32; 3]> = rustc_hash::FxHashSet::default();
    let mut dropped = (0usize, 0usize);
    for (k, t) in support.iter().enumerate() {
        let ids = t.map(|v| {
            let key = v.map(|x| (x * WELD).round() as i64);
            *vid.entry(key).or_insert_with(|| {
                verts.push(v);
                verts.len() as u32 - 1
            })
        });
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
            dropped.0 += 1;
            continue;
        }
        if !seen.insert(canon(ids)) {
            dropped.1 += 1;
            continue;
        }
        tris.push(ids);
        src.push(k as u32);
    }
    // A face lying on its own reversed twin (a joined rod's split end caps lying on each other) is
    // no surface. Twins are kept for pairing; a closed solid made of nothing else (the zero-volume
    // sheet left between two joined rods once their sides pair as one) is dropped below.
    let mut twin = vec![false; tris.len()];
    let mut twin_pairs: Vec<(usize, usize)> = Vec::new();
    {
        let keyset: FxHashMap<[u32; 3], usize> = tris.iter().enumerate().map(|(k, t)| (canon(*t), k)).collect();
        for (k, t) in tris.iter().enumerate() {
            if let Some(&j) = keyset.get(&canon([t[0], t[2], t[1]]))
                && j != k
            {
                twin[k] = true;
                twin[j] = true;
                if k < j {
                    twin_pairs.push((k, j));
                }
            }
        }
    }
    // Closed solids.
    // Per edge: its triangles, each with its direction along the edge and its third corner.
    type EdgeFaces = Vec<(u32, bool, u32)>;
    let mut edges: FxHashMap<(u32, u32), EdgeFaces> = FxHashMap::default();
    for (k, t) in tris.iter().enumerate() {
        for i in 0..3 {
            let (x, y, z) = (t[i], t[(i + 1) % 3], t[(i + 2) % 3]);
            edges.entry((x.min(y), x.max(y))).or_default().push((k as u32, x < y, z));
        }
    }
    let mut par: Vec<u32> = (0..tris.len() as u32).collect();
    let mut open_of = vec![0u32; tris.len()];
    let mut nonmanifold = 0usize;
    let pi = std::f64::consts::PI;
    let two_pi = 2.0 * pi;
    let flat = |x: f64, y: f64| ((y - x).rem_euclid(two_pi) - pi).abs() < 1e-4;
    let ang = |a: f64, b: f64| {
        let d = (a - b).rem_euclid(two_pi);
        d.min(two_pi - d)
    };
    for (&(a, b), l) in &edges {
        if l.len() == 1 {
            open_of[l[0].0 as usize] += 1;
            continue;
        }
        if l.len() == 2 {
            let (x, y) = (find(&mut par, l[0].0), find(&mut par, l[1].0));
            par[x as usize] = y;
            continue;
        }
        nonmanifold += 1;
        let (pa, pb) = (verts[a as usize], verts[b as usize]);
        let ax = unit(sub(pb, pa));
        let r0 = sub(verts[l[0].2 as usize], pa);
        let rf = unit(sub(r0, mul(ax, dot(r0, ax))));
        let o = cross(ax, rf);
        let mut items: Vec<(f64, u8, u32, bool)> = l
            .iter()
            .map(|&(k, s, z)| {
                let w = sub(verts[z as usize], pa);
                let mut th = dot(w, o).atan2(dot(w, rf));
                if (th + pi).abs() < 1e-6 {
                    th = pi;
                }
                ((th * 1e5).round() / 1e5, if s { 0 } else { 1 }, k, s)
            })
            .collect();
        items.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        // Flat continuations first. Two faces of opposite orientation lying in one plane through
        // the edge (half a turn apart) are one flat sheet (an end cap, a facet); each is paired
        // with its unique flat partner before the stack pairs the rest, so solids that overlap at
        // a shared edge (joined braces ending on one axis point) are not merged by the
        // interleaved order of their faces. A twin sheet (two joined rods' end caps lying on each
        // other) whose border this edge is leaves the edge to the stack: the rods on either side
        // stay two solids, as with separate shells.
        let tw: Vec<usize> = (0..items.len()).filter(|&i| twin[items[i].2 as usize]).collect();
        let border = !tw.is_empty()
            && tw.iter().all(|&i| {
                !(0..items.len()).any(|j| j != i && items[j].3 != items[i].3 && flat(items[i].0, items[j].0))
            });
        // Where two solids meet flush along a face that ends at this edge (two faces lying on each
        // other with opposite winding, and nothing half a turn round from them) a straight run
        // across the edge belongs to two solids and the stack's pairing is right (rods cut into
        // sections whose caps weld, even when the caps are triangulated differently).
        let flush_dirs: Vec<f64> = (0..items.len())
            .flat_map(|a| ((a + 1)..items.len()).map(move |b| (a, b)))
            .filter(|&(a, b)| items[a].3 != items[b].3 && ang(items[a].0, items[b].0) < 1e-4)
            .map(|(a, _)| items[a].0)
            .collect();
        let flush = flush_dirs.iter().any(|&t| !flush_dirs.iter().any(|&u| (ang(t, u) - pi).abs() < 1e-4));
        if !border && !flush {
            let cands: Vec<Vec<usize>> = (0..items.len())
                .map(|i| {
                    (0..items.len())
                        .filter(|&j| j != i && items[j].3 != items[i].3 && flat(items[i].0, items[j].0))
                        .collect()
                })
                .collect();
            let mut used = vec![false; items.len()];
            for i in 0..items.len() {
                if used[i] || cands[i].len() != 1 {
                    continue;
                }
                let j = cands[i][0];
                if used[j] || cands[j].len() != 1 || cands[j][0] != i {
                    continue;
                }
                used[i] = true;
                used[j] = true;
                let (x, y) = (find(&mut par, items[i].2), find(&mut par, items[j].2));
                par[x as usize] = y;
            }
            items = items.iter().zip(&used).filter(|(_, u)| !**u).map(|(x, _)| *x).collect();
        }
        let mut st: Vec<u32> = Vec::new();
        for round in 0..2 {
            for &(_, _, k, s) in &items {
                if !s {
                    if round == 0 || !st.is_empty() {
                        st.push(k);
                    }
                } else if let Some(&top) = st.last()
                    && top != k
                {
                    st.pop();
                    let (x, y) = (find(&mut par, top), find(&mut par, k));
                    par[x as usize] = y;
                }
            }
        }
    }
    let mut groups: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for k in 0..tris.len() as u32 {
        let r = find(&mut par, k);
        groups.entry(r).or_default().push(k);
    }
    let mut glist: Vec<Vec<u32>> = groups.into_values().collect();
    let mut skip_tri = vec![false; tris.len()];
    glist.retain(|g| {
        if g.iter().all(|&k| twin[k as usize]) {
            for &k in g {
                skip_tri[k as usize] = true;
            }
            false
        } else {
            true
        }
    });
    glist.sort_by_key(|g| g[0]);
    let tv = |k: u32| tris[k as usize].map(|v| verts[v as usize]);
    let mut pieces: Vec<(Vec<u32>, Piece)> = glist
        .into_par_iter()
        .enumerate()
        .map(|(id, g)| {
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            let (mut vol, mut area, mut open) = (0.0, 0.0, 0usize);
            let tvs: Vec<[P; 3]> = g.iter().map(|&k| tv(k)).collect();
            let o = tvs[0][0];
            for (i, t) in tvs.iter().enumerate() {
                for v in t {
                    for c in 0..3 {
                        lo[c] = lo[c].min(v[c]);
                        hi[c] = hi[c].max(v[c]);
                    }
                }
                vol += dot(sub(t[0], o), cross(sub(t[1], o), sub(t[2], o))) / 6.0;
                area += 0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0])));
                open += open_of[g[i] as usize] as usize;
            }
            let mut f = Fit {
                open_edges: open,
                volume: vol,
                area,
                kind: PieceKind::Unread,
                a: lo,
                b: hi,
                width: 0.0,
                sections: vec![],
                azi_bad: 1.0,
                why: "",
            };
            fit(&tvs, &mut f, None);
            // A closed solid read as no single straight solid (unread, a bar of far too many faces,
            // or a solid of revolution whose profile holds three times its volume or a third of
            // it: an axis far off) is split into its straight stretches where it has them.
            let rv = rev_volume(&f.sections);
            let bogus = f.kind == PieceKind::Axi && f.volume > 0.0 && (rv > 3.0 * f.volume || rv < f.volume / 3.0);
            let mut segments = Vec::new();
            if f.open_edges == 0
                && f.volume > 0.0
                && (f.kind == PieceKind::Unread || (f.kind == PieceKind::Bar && g.len() > 64) || bogus)
            {
                // A welded union of regular rods first (rods from their facets' planes), else the
                // straight stretches.
                let ids: Vec<[u32; 3]> = g.iter().map(|&k| tris[k as usize]).collect();
                let rods: Vec<Stretch> = union_split::split_union(&tvs, &ids)
                    .and_then(|rs| {
                        let mut v = Vec::new();
                        for rod in rs {
                            let gtv: Vec<[P; 3]> = rod.tris.iter().map(|&i| tvs[i]).collect();
                            let area: f64 =
                                gtv.iter().map(|t| 0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0])))).sum();
                            let mut q = Fit {
                                open_edges: 1,
                                volume: 0.0,
                                area,
                                kind: PieceKind::Unread,
                                a: [0.0; 3],
                                b: [0.0; 3],
                                width: 0.0,
                                sections: vec![],
                                azi_bad: 1.0,
                                why: "",
                            };
                            fit(&gtv, &mut q, Some(&rod));
                            if q.kind != PieceKind::Axi {
                                return None;
                            }
                            v.push((q.a, q.b, q.width, q.sections, rod.tris));
                        }
                        Some(v)
                    })
                    .unwrap_or_else(|| split_straight(&tvs));
                segments = rods
                    .into_iter()
                    .map(|(a, b, w, secs, ks)| {
                        let v = rev_volume(&secs);
                        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                        for &k in &ks {
                            for q in &tvs[k] {
                                for c in 0..3 {
                                    lo[c] = lo[c].min(q[c]);
                                    hi[c] = hi[c].max(q[c]);
                                }
                            }
                        }
                        Segment { a, b, width: w, sections: secs, volume: v, tris: ks.len(), lo, hi }
                    })
                    .collect();
            }
            let piece = Piece {
                id,
                tris: g.len(),
                open_edges: f.open_edges,
                volume: f.volume,
                ray_volume: 0.0,
                area: f.area,
                lo,
                hi,
                kind: f.kind,
                a: f.a,
                b: f.b,
                width: f.width,
                sections: f.sections,
                azi_bad: f.azi_bad,
                why: f.why,
                segments,
                in_model: 0.0,
                in_model_at: None,
            };
            (g, piece)
        })
        .collect();
    let np = pieces.len();
    let mut tri_piece = vec![u32::MAX; support.len()];
    let mut piece_of = vec![0u32; tris.len()];
    for (i, (g, _)) in pieces.iter().enumerate() {
        for &k in g {
            tri_piece[src[k as usize] as usize] = i as u32;
            piece_of[k as usize] = i as u32;
        }
    }

    // Connections on the raw mesh.
    let stris: Vec<[P; 3]> = (0..tris.len() as u32).map(tv).collect();
    let mut pairs: FxHashMap<(u32, u32), Pair> = FxHashMap::default();
    let mut vol = vec![0.0; np];
    let mut inm = vec![0.0; np];
    let mut inmc = vec![[0.0; 3]; np];
    for perm in [[0usize, 1, 2], [1, 2, 0], [2, 0, 1]] {
        let pm = |t: &[P; 3]| t.map(|v| [v[perm[0]], v[perm[1]], v[perm[2]]]);
        let st: Vec<[P; 3]> = stris.iter().map(pm).collect();
        let mt: Vec<[P; 3]> = model.iter().map(pm).collect();
        let (sti, mti) = (tile_index(&st, pitch), tile_index(&mt, pitch));
        let inv = [0, 1, 2].map(|k| perm.iter().position(|&x| x == k).expect("a permutation"));
        let col = pitch * pitch;
        let mut keys: Vec<(i64, i64)> = sti.keys().copied().collect();
        keys.sort_unstable();
        let part: Vec<TileTotals> = keys
            .par_iter()
            .map(|&(tx, ty)| {
                let mut tp: FxHashMap<(u32, u32), Pair> = FxHashMap::default();
                let mut tvol: FxHashMap<u32, (f64, f64, P)> = FxHashMap::default();
                let mut ev: Vec<Ev> = Vec::new();
                for &ti in &sti[&(tx, ty)] {
                    if skip_tri[ti as usize] {
                        continue;
                    }
                    raster(&st[ti as usize], tx, ty, pitch, piece_of[ti as usize], None, &mut ev);
                }
                if ev.is_empty() {
                    return (tp, tvol);
                }
                let mut mask = vec![false; (TILE * TILE) as usize];
                for e in &ev {
                    mask[e.ray as usize] = true;
                }
                if let Some(l) = mti.get(&(tx, ty)) {
                    for &ti in l {
                        raster(&mt[ti as usize], tx, ty, pitch, u32::MAX, Some(&mask), &mut ev);
                    }
                }
                // At one height, exits before entries: two solids meeting flush (a rod standing on
                // the raft's top) then abut.
                ev.sort_by(|a, b| a.ray.cmp(&b.ray).then(a.z.total_cmp(&b.z)).then(a.d.cmp(&b.d)));
                let mut k = 0;
                let mut open: Vec<(u32, i32)> = Vec::new();
                let mut sums: Vec<(u32, i32)> = Vec::new();
                while k < ev.len() {
                    let ray = ev[k].ray;
                    let mut e = k;
                    while e < ev.len() && ev[e].ray == ray {
                        e += 1;
                    }
                    // Pieces (or the model) that do not close on this ray leak: their crossings are
                    // left out.
                    sums.clear();
                    for x in &ev[k..e] {
                        if let Some(s) = sums.iter_mut().find(|s| s.0 == x.piece) {
                            s.1 += x.d as i32;
                        } else {
                            sums.push((x.piece, x.d as i32));
                        }
                    }
                    let leaky = |p: u32| sums.iter().any(|s| s.0 == p && s.1 != 0);
                    let (gi, gj) = (ray as i64 % TILE + tx * TILE, ray as i64 / TILE + ty * TILE);
                    let (jx, jy) = jit(gi, gj);
                    let (px, py) = ((gi as f64 + jx) * pitch, (gj as f64 + jy) * pitch);
                    let real = |z: f64| {
                        let q = [px, py, z];
                        [q[inv[0]], q[inv[1]], q[inv[2]]]
                    };
                    open.clear();
                    let mut wm = 0i32;
                    // The piece that closed last, with nothing open since.
                    let mut last_close: Option<(u32, f64)> = None;
                    for q in k..e {
                        let x = ev[q];
                        if leaky(x.piece) {
                            continue;
                        }
                        let z = x.z as f64;
                        if x.piece == u32::MAX {
                            wm += x.d as i32;
                        } else {
                            let was_open = open.iter().any(|o| o.0 == x.piece);
                            if let Some(o) = open.iter_mut().find(|o| o.0 == x.piece) {
                                o.1 += x.d as i32;
                            } else {
                                open.push((x.piece, x.d as i32));
                            }
                            open.retain(|o| o.1 != 0);
                            let is_open = open.iter().any(|o| o.0 == x.piece);
                            if !was_open
                                && is_open
                                && let Some((p0, z0)) = last_close
                                && p0 != x.piece
                                && !open.iter().any(|o| o.0 == p0)
                            {
                                let gap = z - z0;
                                if gap <= NEAR_MM {
                                    let key = (p0.min(x.piece), p0.max(x.piece));
                                    let pr = tp.entry(key).or_default();
                                    let at = real(0.5 * (z + z0));
                                    if gap <= ABUT_MM {
                                        pr.abut += 1.0;
                                        pr.abc = add(pr.abc, at);
                                    } else if !pr.has_gap || gap < pr.gap {
                                        pr.gap = gap;
                                        pr.gapc = at;
                                        pr.has_gap = true;
                                    }
                                }
                            }
                            if was_open && !is_open && open.is_empty() {
                                last_close = Some((x.piece, z));
                            }
                        }
                        // The stretch up to the next crossing.
                        let mut nq = q + 1;
                        while nq < e && leaky(ev[nq].piece) {
                            nq += 1;
                        }
                        if nq >= e {
                            break;
                        }
                        let z1 = ev[nq].z as f64;
                        if z1 <= z || open.is_empty() {
                            continue;
                        }
                        let l = (z1 - z) * col;
                        let mid = real(0.5 * (z + z1));
                        for o in &open {
                            let v = tvol.entry(o.0).or_insert((0.0, 0.0, [0.0; 3]));
                            v.0 += l;
                            if wm != 0 {
                                v.1 += l;
                                v.2 = add(v.2, mul(mid, l));
                            }
                        }
                        for i in 0..open.len() {
                            for j in i + 1..open.len() {
                                let key = (open[i].0.min(open[j].0), open[i].0.max(open[j].0));
                                let pr = tp.entry(key).or_default();
                                pr.ov += l;
                                pr.ovc = add(pr.ovc, mul(mid, l));
                            }
                        }
                    }
                    k = e;
                }
                (tp, tvol)
            })
            .collect();
        for (tp, tvol) in part {
            for (k, v) in tp {
                let pr = pairs.entry(k).or_default();
                pr.ov += v.ov;
                pr.ovc = add(pr.ovc, v.ovc);
                pr.abut += v.abut;
                pr.abc = add(pr.abc, v.abc);
                if v.has_gap && (!pr.has_gap || v.gap < pr.gap) {
                    pr.gap = v.gap;
                    pr.gapc = v.gapc;
                    pr.has_gap = true;
                }
            }
            for (p, v) in tvol {
                vol[p as usize] += v.0 / 3.0;
                inm[p as usize] += v.1 / 3.0;
                inmc[p as usize] = add(inmc[p as usize], mul(v.2, 1.0 / 3.0));
            }
        }
    }
    // Two solids whose faces lie on each other (joined rods' end caps) abut there, even where
    // other solids overlap the spot so that no ray sees the abutment.
    for &(k, j) in &twin_pairs {
        if skip_tri[k] || skip_tri[j] {
            continue;
        }
        let (a, b) = (piece_of[k], piece_of[j]);
        if a == b {
            continue;
        }
        let c = centroid(&tv(k as u32));
        // Inside a joint ball the two meet through the ball, as they do with separate shells.
        if pieces.iter().any(|(_, p)| p.kind == PieceKind::Ball && len(sub(c, p.a)) < 0.5 * p.width) {
            continue;
        }
        let pr = pairs.entry((a.min(b), a.max(b))).or_default();
        pr.abut += 1.0;
        pr.abc = add(pr.abc, c);
    }
    // Contacts: the pieces each contact point lies in (parity along three slightly tilted rays,
    // two up and one down, by majority: a single +z ray can run exactly along the edges of a
    // polygonal solid), else the nearest pieces' surfaces.
    let contact_pieces: Vec<Vec<ContactPiece>> = contacts
        .par_iter()
        .map(|&(at, d)| {
            let rad = d * 0.5;
            let reach = rad * 1.5 + 0.3;
            let mut hits: Vec<ContactPiece> = Vec::new();
            for (i, (g, p)) in pieces.iter().enumerate() {
                if (0..3).any(|k| at[k] < p.lo[k] - reach || at[k] > p.hi[k] + reach) {
                    continue;
                }
                let mut cnt = [0usize; 3];
                let mut best = f64::MAX;
                for &k in g {
                    let t = tv(k);
                    for (j, dir) in RAYS.iter().enumerate() {
                        if ray_hits(at, *dir, &t) {
                            cnt[j] += 1;
                        }
                    }
                    for v in &t {
                        best = best.min(len(sub(*v, at)));
                    }
                    best = best.min(len(sub(centroid(&t), at)));
                }
                let inside = cnt.iter().filter(|c| *c % 2 == 1).count() >= 2;
                if inside || best <= reach {
                    hits.push(ContactPiece(i, inside, best));
                }
            }
            hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.total_cmp(&b.2)));
            hits.truncate(6);
            hits
        })
        .collect();
    for (i, (_, p)) in pieces.iter_mut().enumerate() {
        p.ray_volume = vol[i];
        p.in_model = inm[i];
        p.in_model_at = (inm[i] > 0.0).then(|| mul(inmc[i], 1.0 / inm[i]));
    }
    let mut keys: Vec<(u32, u32)> = pairs.keys().copied().collect();
    keys.sort_unstable();
    let mut connections = Vec::new();
    for k in keys {
        let v = &pairs[&k];
        let ov = v.ov / 3.0;
        let (kind, at) = if ov > 1e-7 {
            (ConnectionKind::Overlap, mul(v.ovc, 1.0 / v.ov))
        } else if v.abut > 0.0 {
            (ConnectionKind::Abut, mul(v.abc, 1.0 / v.abut))
        } else if v.has_gap {
            (ConnectionKind::Near, v.gapc)
        } else {
            continue;
        };
        connections.push(Connection {
            a: k.0 as usize,
            b: k.1 as usize,
            kind,
            at,
            overlap: ov,
            abut_rays: v.abut,
            gap: v.has_gap.then_some(v.gap),
        });
    }
    Pieces {
        pitch,
        stl_tris: support.len(),
        dropped_degenerate: dropped.0,
        dropped_repeated: dropped.1,
        nonmanifold_edges: nonmanifold,
        pieces: pieces.into_iter().map(|(_, p)| p).collect(),
        connections,
        contact_pieces,
        tri_piece,
    }
}

#[cfg(test)]
mod tests {
    use super::super::contacts::tests::{ball, boxm, frustum};
    use super::*;

    fn read1(support: &[[P; 3]], contacts: &[(P, f64)]) -> Pieces {
        read(support, &boxm([-10.0, -10.0, 10.0], [10.0, 10.0, 15.0]), contacts, RAY_PITCH_MM)
    }

    #[test]
    fn a_cone_reads_as_a_solid_of_revolution_with_its_taper() {
        let p = read1(&frustum([0.0, 0.0, 2.0], [0.0, 0.0, 6.0], 0.5, 0.2, 24), &[]);
        assert_eq!(p.pieces.len(), 1);
        let x = &p.pieces[0];
        assert_eq!(x.kind, PieceKind::Axi);
        assert!(x.a[2] < x.b[2], "the axis runs up");
        assert!(dist(x.a, [0.0, 0.0, 2.0]) < 1e-6 && dist(x.b, [0.0, 0.0, 6.0]) < 1e-6);
        assert!(x.sections.iter().any(|s| s.k == SectionKind::Cone));
        assert!((x.width - 1.0).abs() < 0.01, "{}", x.width);
        assert!((x.ray_volume - x.volume).abs() < 0.05 * x.volume, "{} {}", x.ray_volume, x.volume);
    }

    #[test]
    fn a_sphere_reads_as_a_ball_and_a_flat_plank_as_a_bar() {
        let p = read1(&ball([0.0, 0.0, 5.0], 0.4), &[]);
        assert_eq!(p.pieces[0].kind, PieceKind::Ball);
        assert!((p.pieces[0].width - 0.8).abs() < 0.02);
        // A square rod still reads as one of revolution (its faces miss the axis by 0.26 of their
        // radius); a flat bar does not, and reads as a bar of its length and section area.
        let p = read1(&boxm([0.0, 0.0, 2.0], [0.4, 0.4, 6.0]), &[]);
        assert_eq!(p.pieces[0].kind, PieceKind::Axi);
        let p = read1(&boxm([0.0, 0.0, 2.0], [0.3, 1.0, 9.0]), &[]);
        assert_eq!(p.pieces[0].kind, PieceKind::Bar);
        let r = (0.3 * 1.0 / std::f64::consts::PI).sqrt();
        assert!((p.pieces[0].width - 2.0 * r).abs() < 0.01, "{}", p.pieces[0].width);
    }

    #[test]
    fn overlapping_solids_connect_and_a_gap_is_a_near_miss() {
        let mut s = frustum([0.0, 0.0, 0.0], [0.0, 0.0, 5.0], 0.4, 0.4, 16);
        s.extend(frustum([0.0, 0.0, 4.8], [3.0, 0.0, 6.0], 0.2, 0.2, 16));
        s.extend(frustum([0.0, 0.0, 5.2], [0.0, 0.0, 8.0], 0.4, 0.4, 16));
        let p = read1(&s, &[]);
        assert_eq!(p.pieces.len(), 3);
        let kinds: Vec<(usize, usize, ConnectionKind)> = p.connections.iter().map(|c| (c.a, c.b, c.kind)).collect();
        assert!(kinds.contains(&(0, 1, ConnectionKind::Overlap)), "{kinds:?}");
        assert!(kinds.contains(&(0, 2, ConnectionKind::Near)), "{kinds:?}");
        let near = p.connections.iter().find(|c| (c.a, c.b) == (0, 2)).unwrap();
        assert!((near.gap.unwrap() - 0.2).abs() < 0.01);
    }

    #[test]
    fn a_contact_lies_in_the_piece_that_holds_it_and_volume_inside_the_model_is_measured() {
        let s = frustum([0.0, 0.0, 8.0], [0.0, 0.0, 10.5], 0.4, 0.2, 16);
        let p = read1(&s, &[([0.0, 0.0, 10.0], 0.45)]);
        assert_eq!(p.contact_pieces.len(), 1);
        let h = p.contact_pieces[0][0];
        assert_eq!((h.0, h.1), (0, true));
        assert!(p.pieces[0].in_model > 0.0);
        assert!(p.pieces[0].in_model_at.unwrap()[2] > 10.0);
    }

    #[test]
    fn degenerate_and_repeated_triangles_are_dropped() {
        let mut s = frustum([0.0, 0.0, 0.0], [0.0, 0.0, 5.0], 0.4, 0.4, 8);
        let first = s[0];
        s.push(first);
        s.push([[1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]);
        let p = read1(&s, &[]);
        assert_eq!((p.dropped_degenerate, p.dropped_repeated), (1, 1));
        assert_eq!(p.tri_piece[s.len() - 1], u32::MAX);
    }

    #[test]
    fn a_rod_bent_at_a_mitre_splits_into_its_straight_stretches() {
        // Two rods meeting at a mitre, welded into one closed solid by sharing their cut ring.
        let n = 16;
        let ring = |c: P, k: usize, tilt: f64| {
            let t = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            let (x, y) = (0.4 * t.cos(), 0.4 * t.sin());
            [c[0] + x, c[1] + y, c[2] + x * tilt]
        };
        let (a, m, b) = ([0.0, 0.0, 0.0], [0.0, 0.0, 6.0], [6.0, 0.0, 12.0]);
        // The second rod runs at 45 degrees; its rings are perpendicular to its own axis, the
        // mitre ring halfway.
        let up = |k: usize| ring(a, k, 0.0);
        let mid = |k: usize| ring(m, k, -0.414_213_562);
        let d = super::super::math::unit(sub(b, m));
        let top = |k: usize| {
            let t = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            let u = [d[2], 0.0, -d[0]];
            add(b, add(mul(u, 0.4 * t.cos()), [0.0, 0.4 * t.sin(), 0.0]))
        };
        let mut s = Vec::new();
        for k in 0..n {
            let k1 = (k + 1) % n;
            s.push([up(k), up(k1), mid(k1)]);
            s.push([up(k), mid(k1), mid(k)]);
            s.push([mid(k), mid(k1), top(k1)]);
            s.push([mid(k), top(k1), top(k)]);
            s.push([a, up(k1), up(k)]);
            s.push([b, top(k), top(k1)]);
        }
        let p = read1(&s, &[]);
        assert_eq!(p.pieces.len(), 1);
        assert_eq!(p.pieces[0].segments.len(), 2, "{:?}", p.pieces[0].why);
    }
}
