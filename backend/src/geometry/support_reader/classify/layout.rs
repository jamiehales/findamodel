//! Stage 6b: the layout. Each route becomes a support (trunk, branch or strut) with its tip, line,
//! shaft and root widths; posts, legs and braces follow. Strong tapers are written in steps,
//! shafts beside the model narrowed, and the result converted to the writer's
//! [`SupportLayout`].

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::super::math::{P, add, closest_on_tri, dist, dot, mul, pairwise_sum, sub, unit_div};
use super::super::pieces::SectionKind;
use super::rest::Host;
use super::tips::{above_root, cut_from, point_along, polyline_len, simplify};
use super::{
    End, Exception, NARROW_CLEAR_MM, NARROW_INSIDE_MM, NARROW_MAX_MM, NARROW_MIN_D, Owner, ROOT_TOP, Reader, Role,
    SOCKET_CLEAR_MM, SOCKET_MOVE_MM, SOCKET_SQUARE_COS, STEP_GAIN2, STEP_GAIN3, STEP_MIN_MM, STEP_SAMPLE_MM,
    STEP_SNAP_MM, Step, seg_dist,
};
use crate::geometry::Vec3;
use crate::geometry::support_layout::{
    BallTip, BraceEnd, BraceLayout, ContactTip, KickstandLayout, PostLayout, SupportBase, SupportLayout, SupportPath,
};

/// A tip as the layout holds it.
#[derive(Debug, Clone)]
pub(crate) struct LTip {
    pub contact: P,
    /// Away from the model, as the touch rule read it (the writer takes the surface normal from
    /// the model's faces).
    pub normal: P,
    pub socket: P,
    pub contact_d: f64,
    pub body_d: f64,
    pub ball_d: Option<f64>,
    pub cone_end_d: Option<f64>,
    pub ball_depth: Option<f64>,
    pub rounded: bool,
    /// The original ball's centre, for [`Reader::ball_seats`].
    pub ball_c: Option<P>,
}

#[derive(Debug, Clone)]
pub(crate) enum LBase {
    Plate { xy: [f64; 2], root_d: f64 },
    Parent { parent: usize, at: P },
    Lower(LTip),
}

#[derive(Debug, Clone)]
pub(crate) struct LSupport {
    pub tip: LTip,
    pub base: LBase,
    /// Bottom to top.
    pub joints: Vec<P>,
    pub shaft_d: f64,
    pub seg_d: Option<Vec<f64>>,
    pub stick: bool,
    pub twig: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct LPost {
    pub foot: P,
    pub joints: Vec<P>,
    pub top: P,
    pub root_d: f64,
    pub d: f64,
    pub seg_d: Option<Vec<f64>>,
}

#[derive(Debug, Clone)]
pub(crate) struct LKick {
    pub support: usize,
    pub knot: P,
    pub joints: Vec<P>,
    pub foot: P,
    pub root_d: f64,
    pub d: f64,
    pub seg_d: Option<Vec<f64>>,
}

/// A brace end: (support or post, kickstand, point).
pub(crate) type LEnd = (usize, Option<usize>, P);

#[derive(Debug, Clone)]
pub(crate) struct LBrace {
    pub lower: LEnd,
    pub upper: LEnd,
    pub d: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Lay {
    pub supports: Vec<LSupport>,
    pub posts: Vec<LPost>,
    pub braces: Vec<LBrace>,
    pub kicks: Vec<LKick>,
}

/// What a stepped shaft was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Src {
    Support(usize),
    Post(usize),
    Kick(usize),
}

/// Python's `round` (half to even) to an integer.
fn round_half_even(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// Number of `acc` values at or below `x` (`np.searchsorted(acc, x, side="right")`).
fn searchsorted_right(acc: &[f64], x: f64) -> usize {
    acc.iter().take_while(|&&v| v <= x).count()
}

/// Overlap of discs of squared radius `a` (per sample) with steps of squared radius `d2`.
fn step_iou(a: &[f64], d2: &[f64]) -> f64 {
    let mn: Vec<f64> = a.iter().zip(d2).map(|(x, y)| x.min(*y)).collect();
    let mx: Vec<f64> = a.iter().zip(d2).map(|(x, y)| x.max(*y)).collect();
    pairwise_sum(&mn) / pairwise_sum(&mx).max(1e-12)
}

/// The k-1 cuts (sample indices) that split the radius profile `r` into k constant-width steps
/// with the least squared area error (each step at its mean area), cuts near a snap moved onto it;
/// each step's radius; and the ideal overlap.
fn step_cuts(r: &[f64], k: usize, snaps: &[usize]) -> (Vec<usize>, Vec<f64>, f64) {
    let n = r.len();
    let a: Vec<f64> = r.iter().map(|x| x * x).collect();
    let mut c1 = vec![0.0];
    let mut c2 = vec![0.0];
    for x in &a {
        c1.push(c1[c1.len() - 1] + x);
        c2.push(c2[c2.len() - 1] + x * x);
    }
    let cost = |i: usize, j: usize| (c2[j] - c2[i]) - (c1[j] - c1[i]).powi(2) / ((j - i).max(1) as f64);
    let mut cuts: Vec<usize> = Vec::new();
    if k == 2 && n >= 4 {
        let mut best = (2, f64::INFINITY);
        for i in 2..n - 1 {
            let v = cost(0, i) + cost(i, n);
            if v < best.1 {
                best = (i, v);
            }
        }
        cuts = vec![best.0];
    } else if k >= 3 && n >= 6 {
        let (mut best, mut v3) = (None, 1e30);
        let step = (n / 150).max(1);
        let mut i = 2;
        while i < n - 3 {
            let mut j = i + 2;
            while j < n - 1 {
                let v = cost(0, i) + cost(i, j) + cost(j, n);
                if v < v3 {
                    best = Some(vec![i, j]);
                    v3 = v;
                }
                j += step;
            }
            i += step;
        }
        cuts = best.unwrap_or_default();
    }
    let mut out = Vec::new();
    for &c in &cuts {
        let near: Vec<usize> = snaps
            .iter()
            .copied()
            .filter(|&q| 0 < q && q < n && (q as f64 - c as f64).abs() * STEP_SAMPLE_MM <= STEP_SNAP_MM)
            .collect();
        let mut pick = c;
        let mut bd = f64::INFINITY;
        for q in near {
            let d = (q as f64 - c as f64).abs();
            if d < bd {
                bd = d;
                pick = q;
            }
        }
        out.push(pick);
    }
    out.sort_unstable();
    out.dedup();
    let mut edges = vec![0];
    edges.extend(&out);
    edges.push(n);
    let radii: Vec<f64> =
        edges.windows(2).map(|w| (pairwise_sum(&a[w[0]..w[1]]) / (w[1] - w[0]) as f64).sqrt()).collect();
    let mut d2 = Vec::with_capacity(n);
    for (w, rr) in edges.windows(2).zip(&radii) {
        d2.extend(std::iter::repeat_n(rr * rr, w[1] - w[0]));
    }
    let iou = step_iou(&a, &d2);
    (out, radii, iou)
}

/// How many constant-width steps a taper section with radius profile `r` earns (1 to 3).
fn steps_wanted(r: &[f64]) -> usize {
    if (r.len() as f64) * STEP_SAMPLE_MM < STEP_MIN_MM || r.len() < 6 {
        return 1;
    }
    let (i1, i2, i3) = (step_cuts(r, 1, &[]).2, step_cuts(r, 2, &[]).2, step_cuts(r, 3, &[]).2);
    if i2 - i1 < STEP_GAIN2 {
        return 1;
    }
    if i3 - i2 >= STEP_GAIN3 { 3 } else { 2 }
}

impl Reader<'_> {
    fn export_exception(&mut self, piece: Option<usize>, contact: Option<usize>, role: Option<Role>, at: P, why: &str) {
        self.exceptions.push(Exception { kind: "export", at, why: why.into(), contact, piece, role });
    }

    pub(super) fn export(&mut self) -> Lay {
        let mut lay = Lay::default();
        let mut lines: BTreeMap<Src, Vec<P>> = BTreeMap::new();
        let mut step_src: Vec<(Src, Vec<Step>)> = Vec::new();
        let mut report: Vec<(usize, String)> = Vec::new();
        let mut index: BTreeMap<usize, usize> = BTreeMap::new();
        for si in 0..self.supports.len() {
            let s = self.supports[si].clone();
            let ci = s.contact;
            let pts = self.line_of(&s.path);
            if pts.is_empty() {
                report.push((ci, "empty route".into()));
                continue;
            }
            let (mut tip, mut dist_) = self.tip_of(ci, &s.path, &pts);
            self.supports[si].tip_len = dist_;
            let mut line = cut_from(&pts, dist_);
            line[0] = tip.socket;
            let sd = self.shaft_d(&s.path, dist_).unwrap_or(tip.body_d);
            // DragonFruit puts a joint ball (the shaft's width + 0.1 mm) on the socket, so a socket
            // nearer the surface than that ball's radius pokes the ball into the model (only tips
            // may enter it). Such a socket moves on down the route until the ball clears the
            // surface by SOCKET_CLEAR_MM, only where the shaft leaves the tip square to the surface
            // (a shaft at a slant clears it with its own side, as the original's does), and by at
            // most SOCKET_MOVE_MM.
            if s.role != Role::Micro {
                let (nrm, con) = (tip.normal, tip.contact);
                let need = (sd + 0.1) / 2.0 + SOCKET_CLEAR_MM;
                let total = polyline_len(&pts);
                let mut d2 = dist_;
                let sock = point_along(&pts, dist_);
                let u = sub(point_along(&pts, (dist_ + 0.3).min(total)), sock);
                let square = super::super::math::len(u) > 1e-9 && dot(unit_div(u), nrm) >= SOCKET_SQUARE_COS;
                let lift = |d: f64| dot(sub(point_along(&pts, d), con), nrm);
                while square && lift(d2) < need && d2 + 0.05 < (0.5 * total).min(dist_ + SOCKET_MOVE_MM + 0.05) {
                    d2 += 0.05;
                }
                if d2 > dist_ && lift(d2) >= need - 1e-6 {
                    dist_ = d2;
                    tip.socket = point_along(&pts, dist_);
                    self.supports[si].tip_len = dist_;
                    line = cut_from(&pts, dist_);
                    line[0] = tip.socket;
                }
            }
            let base = match s.end {
                End::Plate => {
                    // The shaft starts at the root's top; what the route does below it is the root.
                    let above: Vec<P> = line.iter().copied().filter(|p| p[2] > ROOT_TOP + 0.05).collect();
                    let mut last = line[line.len() - 1];
                    if !above.is_empty() && above.len() < line.len() {
                        let (a, b) = (above[above.len() - 1], line[above.len()]);
                        let w = (a[2] - ROOT_TOP) / (a[2] - b[2]).max(1e-9);
                        last = add(a, mul(sub(b, a), w));
                    }
                    let xy = [last[0], last[1]];
                    let root_d = self.root_d(s.path[s.path.len() - 1].0, sd, Owner::Support(si));
                    line = if above.is_empty() { vec![line[0]] } else { above };
                    line.push([xy[0], xy[1], ROOT_TOP]);
                    LBase::Plate { xy, root_d }
                }
                End::Branch(o, at) => match o {
                    Owner::Support(o) if index.contains_key(&o) => LBase::Parent { parent: index[&o], at },
                    _ => {
                        report.push((ci, format!("branch onto {o:?}, which was not written")));
                        continue;
                    }
                },
                End::Foot(c2) => {
                    let rpath: Vec<Step> = s.path.iter().rev().map(|&(q, t0, t1)| (q, t1, t0)).collect();
                    let rpts = self.line_of(&rpath);
                    let (mut low, mut ldist) = self.tip_of(c2, &rpath, &rpts);
                    let total = polyline_len(&pts);
                    if dist_ + ldist > total - 0.05 {
                        // Too short for two tips: each takes its share.
                        let share = total / (dist_ + ldist).max(1e-9) * 0.45;
                        dist_ *= share;
                        ldist *= share;
                        tip.socket = point_along(&pts, dist_);
                        low.socket = point_along(&rpts, ldist);
                        line = cut_from(&pts, dist_);
                    }
                    let keep = total - dist_ - ldist;
                    let mut out = vec![line[0]];
                    let mut acc = 0.0;
                    for w in line.windows(2) {
                        let l = dist(w[0], w[1]);
                        if acc + l >= keep {
                            break;
                        }
                        out.push(w[1]);
                        acc += l;
                    }
                    out.push(low.socket);
                    line = out;
                    LBase::Lower(low)
                }
            };
            let line = simplify(&line);
            let k = lay.supports.len();
            index.insert(si, k);
            lines.insert(Src::Support(k), line.clone());
            let (stick, twig) = if matches!(base, LBase::Lower(_)) && s.role == Role::Strut {
                self.strut_kind(&s.path)
            } else {
                (false, false)
            };
            lay.supports.push(LSupport {
                tip,
                base,
                joints: line.iter().skip(1).take(line.len().saturating_sub(2)).rev().copied().collect(),
                shaft_d: sd,
                seg_d: None,
                stick,
                twig,
            });
            step_src.push((Src::Support(k), s.path.clone()));
        }
        let n_sup = lay.supports.len();
        let mut pindex: BTreeMap<usize, usize> = BTreeMap::new();
        let mut kindex: BTreeMap<usize, usize> = BTreeMap::new();
        for k in 0..self.posts.len() {
            let chain = self.posts[k].clone();
            let pts = simplify(&self.line_of(&chain));
            let q0 = chain[0].0;
            if pts[pts.len() - 1][2] <= ROOT_TOP + 0.2 {
                for &(q, _, _) in &chain {
                    self.export_exception(
                        Some(q),
                        None,
                        Some(Role::Post),
                        pts[pts.len() - 1],
                        "post wholly under the root top (2 mm): DragonFruit's root replaces it",
                    );
                }
                continue;
            }
            let d = self.shaft_d(&chain, 0.0).unwrap_or(2.0 * self.p[q0].r);
            let (foot, above) = above_root(&pts);
            let i = lay.posts.len();
            pindex.insert(k, i);
            lay.posts.push(LPost {
                foot,
                joints: above[..above.len() - 1].to_vec(),
                top: above[above.len() - 1],
                root_d: self.root_d(q0, d, Owner::Post(k)),
                d,
                seg_d: None,
            });
            step_src.push((Src::Post(i), chain));
            let mut l = vec![foot];
            l.extend(above);
            lines.insert(Src::Post(i), l);
        }
        let host_index = |o: Owner, pindex: &BTreeMap<usize, usize>| -> Option<usize> {
            match o {
                Owner::Post(k) => pindex.get(&k).map(|&i| n_sup + i),
                Owner::Kick(_) => None,
                Owner::Support(s) => index.get(&s).copied(),
            }
        };
        for k in 0..self.kicks.len() {
            let chain = self.kicks[k].chain.clone();
            let pts = simplify(&self.line_of(&chain));
            let q0 = chain[0].0;
            let h = host_index(self.kicks[k].host, &pindex);
            if h.is_none() || pts[pts.len() - 1][2] <= ROOT_TOP + 0.2 {
                let why = if h.is_some() {
                    "leg under the root top (2 mm)"
                } else {
                    "leg leaning on a support that was not written, or on another leg"
                };
                for &(q, _, _) in &chain {
                    self.export_exception(Some(q), None, Some(Role::Leg), pts[pts.len() - 1], why);
                }
                continue;
            }
            let d = self.shaft_d(&chain, 0.0).unwrap_or(2.0 * self.p[q0].r);
            let (foot, above) = above_root(&pts);
            let i = lay.kicks.len();
            kindex.insert(k, i);
            lay.kicks.push(LKick {
                support: h.expect("checked"),
                knot: self.kicks[k].knot,
                joints: above[..above.len() - 1].to_vec(),
                foot,
                root_d: self.root_d(q0, d, Owner::Kick(k)),
                d,
                seg_d: None,
            });
            step_src.push((Src::Kick(i), chain));
        }
        for bi in 0..self.braces.len() {
            let (lower, upper, bd, piece) =
                (self.braces[bi].lower, self.braces[bi].upper, self.braces[bi].d, self.braces[bi].piece);
            let mut ends: Vec<LEnd> = Vec::new();
            for h in [lower, upper] {
                let h: Host = h;
                if let Owner::Kick(k) = h.1 {
                    if let Some(&i) = kindex.get(&k) {
                        ends.push((lay.kicks[i].support, Some(i), h.2));
                    }
                    continue;
                }
                if let Some(i) = host_index(h.1, &pindex) {
                    ends.push((i, None, h.2));
                }
            }
            let at = self.p[piece].mid();
            if ends.len() < 2 {
                self.export_exception(
                    Some(piece),
                    None,
                    Some(Role::Brace),
                    at,
                    "brace with an end on a support that was not written",
                );
                continue;
            }
            // A brace end at a branch's own knot that lies on the parent's shaft line (within the
            // parent's radius, and within 1 mm of the knot) goes on the parent: the same point, on
            // the shaft that carries both. Not where that would put both ends on one support.
            for (e, o) in [(0, 1), (1, 0)] {
                let k = ends[e].0;
                let x = ends[e].2;
                if k < n_sup
                    && let LBase::Parent { parent, at } = lay.supports[k].base
                    && dist(x, at) <= 1.0
                {
                    let (pl, pd) = if parent < n_sup {
                        (lines.get(&Src::Support(parent)), lay.supports[parent].shaft_d)
                    } else {
                        (lines.get(&Src::Post(parent - n_sup)), lay.posts[parent - n_sup].d)
                    };
                    if let Some(pl) = pl
                        && parent != ends[o].0
                        && pl.windows(2).map(|w| seg_dist(w[0], w[1], x).0).fold(f64::INFINITY, f64::min)
                            <= pd / 2.0 + 0.05
                    {
                        ends[e].0 = parent;
                    }
                }
            }
            if ends[0].0 == ends[1].0 && ends[0].1 == ends[1].1 {
                self.export_exception(Some(piece), None, Some(Role::Brace), at, "brace with both ends on one support");
                continue;
            }
            if ends[0].2[2] > ends[1].2[2] {
                ends.swap(0, 1);
            }
            lay.braces.push(LBrace { lower: ends[0], upper: ends[1], d: bd });
        }
        for (ci, why) in report {
            self.export_exception(None, Some(ci), None, self.c[ci].at, &why);
        }
        self.step_widths(&mut lay, &step_src);
        self.narrow_widths(&mut lay);
        lay
    }

    /// The written line of a support, post or leg, bottom to top.
    fn written_line(lay: &Lay, src: Src) -> Option<Vec<P>> {
        let mut v = Vec::new();
        match src {
            Src::Support(i) => {
                let s = &lay.supports[i];
                v.push(match &s.base {
                    LBase::Plate { xy, .. } => [xy[0], xy[1], ROOT_TOP],
                    LBase::Parent { at, .. } => *at,
                    LBase::Lower(_) => return None,
                });
                v.extend(s.joints.iter().copied());
                v.push(s.tip.socket);
            }
            Src::Post(i) => {
                let s = &lay.posts[i];
                v.push(s.foot);
                v.extend(s.joints.iter().copied());
                v.push(s.top);
            }
            Src::Kick(i) => {
                let s = &lay.kicks[i];
                v.push(s.foot);
                v.extend(s.joints.iter().copied());
                v.push(s.knot);
            }
        }
        Some(v)
    }

    /// A strongly tapered trunk, branch, post or leg is written as up to three constant-width
    /// segments (bottom to top, with a joint added at each new step). See [`STEP_GAIN2`].
    fn step_widths(&mut self, lay: &mut Lay, src: &[(Src, Vec<Step>)]) {
        let n_sup = lay.supports.len();
        // Points on each support or post where something else hangs.
        let mut knots: BTreeMap<usize, Vec<P>> = BTreeMap::new();
        for s in &lay.supports {
            if let LBase::Parent { parent, at } = s.base {
                knots.entry(parent).or_default().push(at);
            }
        }
        for b in &lay.braces {
            for e in [b.lower, b.upper] {
                if e.1.is_none() {
                    knots.entry(e.0).or_default().push(e.2);
                }
            }
        }
        for k in &lay.kicks {
            knots.entry(k.support).or_default().push(k.knot);
        }
        self.steps = 0;
        let mut ordered: Vec<&(Src, Vec<Step>)> = src.iter().collect();
        ordered.sort_by_key(|x| match x.0 {
            Src::Support(i) => (0, i),
            Src::Post(i) => (1, i),
            Src::Kick(i) => (2, i),
        });
        for (srck, path) in ordered {
            let Some(line) = Self::written_line(lay, *srck) else { continue };
            let own = match *srck {
                Src::Support(i) => Some(i),
                Src::Post(i) => Some(n_sup + i),
                Src::Kick(_) => None,
            };
            let seg_len: Vec<f64> = line.windows(2).map(|w| dist(w[0], w[1])).collect();
            let total: f64 = seg_len.iter().sum();
            if total < STEP_MIN_MM {
                continue;
            }
            let n = 6usize.max(round_half_even(total / STEP_SAMPLE_MM) as usize);
            let ss: Vec<f64> = (0..n).map(|i| (i as f64 + 0.5) * total / n as f64).collect();
            let mut acc = vec![0.0];
            for l in &seg_len {
                acc.push(acc[acc.len() - 1] + l);
            }
            let at_s = |x: f64| {
                let k = (searchsorted_right(&acc, x).saturating_sub(1)).min(seg_len.len() - 1);
                let w = (x - acc[k]) / seg_len[k].max(1e-9);
                add(line[k], mul(sub(line[k + 1], line[k]), w))
            };
            let pts: Vec<P> = ss.iter().map(|&x| at_s(x)).collect();
            let r: Vec<Option<f64>> = pts.iter().map(|&x| self.radius_on(path, x)).collect();
            if r.iter().any(|v| v.is_none()) {
                continue;
            }
            let r: Vec<f64> = r.into_iter().map(|v| v.expect("checked")).collect();
            let nearest = |e: P| -> usize { super::prepare::argmin(pts.iter().map(|p| dist(*p, e))) };
            // The taper sections of the route on this shaft: how many steps the strongest earns.
            let mut want = 1;
            for &(q, _, _) in path {
                let pq = &self.p[q];
                for sec in &pq.secs {
                    if sec.k != SectionKind::Cone {
                        continue;
                    }
                    let ends = [pq.at(sec.t0), pq.at(sec.t1)];
                    let idx = [nearest(ends[0]), nearest(ends[1])];
                    let off = dist(pts[idx[0]], ends[0]).min(dist(pts[idx[1]], ends[1]));
                    if off > sec.r0.max(sec.r1) + STEP_SNAP_MM {
                        continue; // a cone of the route not on the written shaft (the tip's, the root's)
                    }
                    let (i0, i1) = (idx[0].min(idx[1]), idx[0].max(idx[1]));
                    if i1 - i0 >= 6 {
                        want = want.max(steps_wanted(&r[i0..=i1]));
                    }
                }
            }
            if want == 1 {
                continue;
            }
            let mut snaps: Vec<usize> =
                (1..line.len() - 1).map(|k| round_half_even(acc[k] / total * n as f64) as usize).collect();
            let mut exact: BTreeMap<usize, f64> = BTreeMap::new();
            for &x in own.and_then(|o| knots.get(&o)).map(|v| v.as_slice()).unwrap_or(&[]) {
                let q = nearest(x);
                snaps.push(q);
                let (lo, hi) = (pts[q.saturating_sub(1)], pts[(q + 1).min(n - 1)]);
                let dir = sub(hi, lo);
                exact.insert(
                    q,
                    ss[q] + dot(sub(x, pts[q]), super::super::math::div(dir, super::super::math::len(dir).max(1e-9))),
                );
            }
            let (cuts, radii, _) = step_cuts(&r, want, &snaps);
            if cuts.is_empty() {
                continue;
            }
            // The cuts as points along the line; joints already there are reused, others added.
            let inner = &acc[1..acc.len() - 1];
            let cut_s: Vec<f64> = cuts
                .iter()
                .map(|c| exact.get(c).copied().unwrap_or(*c as f64 * total / n as f64))
                .map(|c| inner.iter().copied().find(|x| (x - c).abs() <= STEP_SAMPLE_MM).unwrap_or(c))
                .collect();
            let mut bounds: Vec<f64> = inner.iter().copied().chain(cut_s.iter().copied()).collect();
            bounds.sort_by(f64::total_cmp);
            bounds.dedup();
            let mut new_line = vec![line[0]];
            for b in bounds.iter().copied().chain([total]) {
                let k = (searchsorted_right(&acc, b - 1e-6).saturating_sub(1)).min(seg_len.len() - 1);
                let p = add(line[k], mul(sub(line[k + 1], line[k]), (b - acc[k]) / seg_len[k].max(1e-9)));
                if b < total && dist(p, new_line[new_line.len() - 1]) < 1e-6 {
                    continue;
                }
                new_line.push(p);
            }
            // Width of each written segment: its step's.
            let mut edges = vec![0.0];
            edges.extend(&cut_s);
            edges.push(total);
            let mut seg_s = vec![0.0];
            for w in new_line.windows(2) {
                seg_s.push(seg_s[seg_s.len() - 1] + dist(w[0], w[1]));
            }
            let seg_d: Vec<f64> = seg_s
                .windows(2)
                .map(|w| {
                    let m = (w[0] + w[1]) / 2.0;
                    let k = (searchsorted_right(&edges, m).saturating_sub(1)).min(radii.len() - 1);
                    2.0 * radii[k]
                })
                .collect();
            let joints: Vec<P> = new_line[1..new_line.len() - 1].to_vec();
            match *srck {
                Src::Support(i) => {
                    lay.supports[i].joints = joints;
                    lay.supports[i].seg_d = Some(seg_d);
                }
                Src::Post(i) => {
                    lay.posts[i].joints = joints;
                    lay.posts[i].seg_d = Some(seg_d);
                }
                Src::Kick(i) => {
                    lay.kicks[i].joints = joints;
                    lay.kicks[i].seg_d = Some(seg_d);
                }
            }
            self.steps += cuts.len();
        }
    }

    /// DragonFruit builds round shafts, so a polygon shaft whose flat runs close to a model face
    /// builds into it (by the polygon's sagitta). A trunk, branch, post or leg segment thicker than
    /// [`NARROW_MIN_D`] whose round build would come within [`NARROW_CLEAR_MM`] of the model is
    /// written narrower, just enough to clear it by that, by at most [`NARROW_MAX_MM`]. DragonFruit
    /// has one width per segment, so the whole segment narrows (narrowing only the near stretch
    /// would need a step, one more joint). The last 0.3 mm plus radius under a socket (its joint
    /// ball and tip) and everything below the root top are not measured.
    fn narrow_widths(&mut self, lay: &mut Lay) {
        self.narrowed = 0;
        if self.model.is_empty() {
            return;
        }
        let mut items: Vec<(Src, bool, f64)> = Vec::new();
        for (i, s) in lay.supports.iter().enumerate() {
            if !matches!(s.base, LBase::Lower(_)) {
                items.push((Src::Support(i), true, s.shaft_d));
            }
        }
        items.extend(lay.posts.iter().enumerate().map(|(i, p)| (Src::Post(i), false, p.d)));
        items.extend(lay.kicks.iter().enumerate().map(|(i, k)| (Src::Kick(i), false, k.d)));
        for (src, has_tip, d0) in items {
            let line = Self::written_line(lay, src).expect("not a strut");
            let seg_d0 = match src {
                Src::Support(i) => lay.supports[i].seg_d.clone(),
                Src::Post(i) => lay.posts[i].seg_d.clone(),
                Src::Kick(i) => lay.kicks[i].seg_d.clone(),
            };
            let mut segd = seg_d0.unwrap_or_else(|| vec![d0; line.len() - 1]);
            if segd.iter().copied().fold(f64::MIN, f64::max) <= NARROW_MIN_D + 0.01 {
                continue;
            }
            let mut changed = false;
            for k in 0..line.len() - 1 {
                let (a, b) = (line[k], line[k + 1]);
                let d = segd[k];
                let l = dist(a, b);
                if d <= NARROW_MIN_D + 0.01 || l < 1e-6 {
                    continue;
                }
                let r = d / 2.0;
                let u = super::super::math::div(sub(b, a), l);
                let skip_top = if has_tip && k == line.len() - 2 { r + 0.3 } else { 0.0 };
                let lo = [0, 1, 2].map(|i| a[i].min(b[i]) - r - 0.2);
                let hi = [0, 1, 2].map(|i| a[i].max(b[i]) + r + 0.2);
                let sel = self.model_boxes().query(lo, hi);
                if sel.is_empty() {
                    continue;
                }
                let mut near = 1e9f64;
                // np.arange(0.05, l - skip_top, 0.1)
                let count = ((l - skip_top - 0.05) / 0.1).ceil().max(0.0) as usize;
                for i in 0..count {
                    let p = add(a, mul(u, 0.05 + i as f64 * 0.1));
                    if p[2] >= ROOT_TOP + 0.3 {
                        for &i in &sel {
                            near = near.min(dist(p, closest_on_tri(p, &self.model[i as usize])));
                        }
                    }
                }
                // Only a shaft beside the model (its read polygon clears it): one already running
                // into the model is another matter, left as it is.
                if near - r < NARROW_CLEAR_MM && near - r > -NARROW_INSIDE_MM {
                    let nd = (2.0 * (near - NARROW_CLEAR_MM)).max(d - NARROW_MAX_MM);
                    if nd < d - 1e-4 {
                        segd[k] = nd;
                        changed = true;
                        self.narrowed += 1;
                    }
                }
            }
            if changed {
                let single = segd.len() == 1;
                match src {
                    Src::Support(i) => {
                        if single {
                            lay.supports[i].shaft_d = segd[0];
                        }
                        lay.supports[i].seg_d = Some(segd);
                    }
                    Src::Post(i) => {
                        if single {
                            lay.posts[i].d = segd[0];
                        }
                        lay.posts[i].seg_d = Some(segd);
                    }
                    Src::Kick(i) => {
                        if single {
                            lay.kicks[i].d = segd[0];
                        }
                        lay.kicks[i].seg_d = Some(segd);
                    }
                }
            }
        }
    }
}

/// Model faces near a point, for the surface normal under a contact. In f32, as the writer's
/// inputs are: the written cone's placement is solved iteratively and short tips are sensitive to
/// the normal's last bits.
struct Surface {
    tris: Vec<[Vec3; 3]>,
    cell: f32,
    grid: rustc_hash::FxHashMap<(i32, i32, i32), Vec<usize>>,
}

impl Surface {
    /// The model's faces in the grid cells that [`Surface::normal_at`] will look in for the
    /// `queries` (point, radius); the other cells are left empty.
    fn new(model: &[[P; 3]], queries: &[(Vec3, f32)]) -> Self {
        let cell = 0.5f32;
        let tris: Vec<[Vec3; 3]> = model.iter().map(|t| t.map(v3)).collect();
        let key = |p: Vec3| ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32, (p.z / cell).floor() as i32);
        let mut grid: rustc_hash::FxHashMap<(i32, i32, i32), Vec<usize>> = Default::default();
        let (mut need_lo, mut need_hi) = ((i32::MAX, i32::MAX, i32::MAX), (i32::MIN, i32::MIN, i32::MIN));
        for &(p, radius) in queries {
            let (a, b) = Self::query_cells(p, radius, cell);
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    for z in a.2..=b.2 {
                        grid.entry((x, y, z)).or_default();
                    }
                }
            }
            need_lo = (need_lo.0.min(a.0), need_lo.1.min(a.1), need_lo.2.min(a.2));
            need_hi = (need_hi.0.max(b.0), need_hi.1.max(b.1), need_hi.2.max(b.2));
        }
        for (i, t) in tris.iter().enumerate() {
            let lo = Vec3::new(
                t[0].x.min(t[1].x).min(t[2].x),
                t[0].y.min(t[1].y).min(t[2].y),
                t[0].z.min(t[1].z).min(t[2].z),
            );
            let hi = Vec3::new(
                t[0].x.max(t[1].x).max(t[2].x),
                t[0].y.max(t[1].y).max(t[2].y),
                t[0].z.max(t[1].z).max(t[2].z),
            );
            let (a, b) = (key(lo), key(hi));
            if (b.0 - a.0 + 1) * (b.1 - a.1 + 1) * (b.2 - a.2 + 1) > 4096 {
                continue;
            }
            if b.0 < need_lo.0
                || b.1 < need_lo.1
                || b.2 < need_lo.2
                || a.0 > need_hi.0
                || a.1 > need_hi.1
                || a.2 > need_hi.2
            {
                continue;
            }
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    for z in a.2..=b.2 {
                        if let Some(list) = grid.get_mut(&(x, y, z)) {
                            list.push(i);
                        }
                    }
                }
            }
        }
        Surface { tris, cell, grid }
    }

    /// The grid cells (lowest, highest) holding every face within `radius` of `p`.
    fn query_cells(p: Vec3, radius: f32, cell: f32) -> ((i32, i32, i32), (i32, i32, i32)) {
        let key = |p: Vec3| ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32, (p.z / cell).floor() as i32);
        let r = Vec3::new(radius, radius, radius);
        (key(p - r), key(p + r))
    }

    /// The closest point of a triangle to `p` (Ericson, Real-Time Collision Detection 5.1.5).
    fn closest(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
        let (a, b, c) = (t[0], t[1], t[2]);
        let (ab, ac, ap) = (b - a, c - a, p - a);
        let (d1, d2) = (ab.dot(ap), ac.dot(ap));
        if d1 <= 0.0 && d2 <= 0.0 {
            return a;
        }
        let bp = p - b;
        let (d3, d4) = (ab.dot(bp), ac.dot(bp));
        if d3 >= 0.0 && d4 <= d3 {
            return b;
        }
        let vc = d1 * d4 - d3 * d2;
        if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            return a + ab * (d1 / (d1 - d3));
        }
        let cp = p - c;
        let (d5, d6) = (ab.dot(cp), ac.dot(cp));
        if d6 >= 0.0 && d5 <= d6 {
            return c;
        }
        let vb = d5 * d2 - d1 * d6;
        if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            return a + ac * (d2 / (d2 - d6));
        }
        let va = d3 * d6 - d5 * d4;
        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
            return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
        }
        let den = 1.0 / (va + vb + vc);
        a + ab * (vb * den) + ac * (vc * den)
    }

    /// The model's outward normal at a contact: the area-weighted face normal of the faces within
    /// `radius` that face the support's socket side. A support reaching into an open body touches
    /// the back of its faces (none turned toward it): the surface it presses on is still that
    /// face, its normal turned toward the support.
    fn normal_at(&self, p: Vec3, radius: f32, toward: Vec3) -> Option<Vec3> {
        let (a, b) = Self::query_cells(p, radius, self.cell);
        let mut seen = rustc_hash::FxHashSet::default();
        let (mut sum, mut back) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0));
        for x in a.0..=b.0 {
            for y in a.1..=b.1 {
                for z in a.2..=b.2 {
                    for &i in self.grid.get(&(x, y, z)).map(|v| v.as_slice()).unwrap_or(&[]) {
                        if !seen.insert(i) {
                            continue;
                        }
                        let t = &self.tris[i];
                        if (Self::closest(p, t) - p).length() > radius {
                            continue;
                        }
                        let n2 = (t[1] - t[0]).cross(t[2] - t[0]);
                        if n2.dot(toward) <= 0.0 {
                            back = back - n2;
                            continue;
                        }
                        sum = sum + n2;
                    }
                }
            }
        }
        if sum.length() <= 1e-9 {
            sum = back;
        }
        (sum.length() > 1e-9).then(|| sum.normalized())
    }
}

fn v3(p: P) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// Where [`contact_tip`] asks the surface for its normal: the contact and the radius around it.
fn normal_query(t: &LTip) -> (Vec3, f32) {
    // The writer works in f32, as the prototype's export did.
    (v3(t.contact), (t.contact_d as f32 / 2.0).max(0.15))
}

fn contact_tip(t: &LTip, surface: &Surface) -> ContactTip {
    let (contact, radius) = normal_query(t);
    let d = t.contact_d as f32;
    let socket = v3(t.socket);
    let normal = surface.normal_at(contact, radius, socket - contact).unwrap_or_else(|| v3(t.normal));
    ContactTip {
        contact,
        surface_normal: normal,
        from_outside: true,
        socket,
        contact_diameter_mm: d,
        body_diameter_mm: t.body_d as f32,
        ball: t.ball_d.map(|b| BallTip {
            diameter_mm: b as f32,
            cone_end_mm: t.cone_end_d.unwrap_or(0.0) as f32,
            depth_mm: t.ball_depth.map(|x| x as f32),
        }),
    }
}

/// The layout as the writer takes it.
pub(super) fn to_support_layout(lay: &Lay, model: &[[P; 3]]) -> SupportLayout {
    let mut queries = Vec::new();
    for s in &lay.supports {
        queries.push(normal_query(&s.tip));
        if let LBase::Lower(low) = &s.base {
            queries.push(normal_query(low));
        }
    }
    let surface = Surface::new(model, &queries);
    let mut out = SupportLayout::default();
    for (i, s) in lay.supports.iter().enumerate() {
        let base = match &s.base {
            LBase::Plate { xy, root_d } => {
                SupportBase::Plate { x: xy[0] as f32, y: xy[1] as f32, diameter_mm: *root_d as f32 }
            }
            LBase::Parent { parent, at } => SupportBase::Shaft { parent: *parent, point: v3(*at) },
            LBase::Lower(low) => SupportBase::Model(contact_tip(low, &surface)),
        };
        out.supports.push(SupportPath {
            tip: contact_tip(&s.tip, &surface),
            base,
            joints: s.joints.iter().copied().map(v3).collect(),
            shaft_diameter_mm: s.shaft_d as f32,
        });
        if let Some(d) = &s.seg_d {
            out.segment_diameters.insert(i, d.iter().map(|&x| x as f32).collect());
        }
        if s.stick {
            out.sticks.insert(i);
        }
        if s.twig {
            out.twigs.insert(i);
        }
    }
    let n = lay.supports.len();
    for (i, p) in lay.posts.iter().enumerate() {
        out.posts.push(PostLayout {
            foot: v3(p.foot),
            joints: p.joints.iter().copied().map(v3).collect(),
            top: v3(p.top),
            root_diameter_mm: p.root_d as f32,
            diameter_mm: p.d as f32,
        });
        if let Some(d) = &p.seg_d {
            out.segment_diameters.insert(n + i, d.iter().map(|&x| x as f32).collect());
        }
    }
    for b in &lay.braces {
        let end = |e: &LEnd| BraceEnd { support: e.0, kickstand: e.1, point: v3(e.2) };
        out.braces.push(BraceLayout { lower: end(&b.lower), upper: end(&b.upper), diameter_mm: b.d as f32 });
    }
    for (i, k) in lay.kicks.iter().enumerate() {
        out.kickstands.push(KickstandLayout {
            support: k.support,
            knot: v3(k.knot),
            joints: k.joints.iter().copied().map(v3).collect(),
            foot: v3(k.foot),
            root_diameter_mm: k.root_d as f32,
            diameter_mm: k.d as f32,
        });
        if let Some(d) = &k.seg_d {
            out.kickstand_segment_diameters.insert(i, d.iter().map(|&x| x as f32).collect());
        }
    }
    out
}

fn tip_json(t: &LTip) -> Value {
    let mut v = json!({"contact": t.contact, "normal": t.normal, "socket": t.socket, "contact_d": t.contact_d, "body_d": t.body_d});
    if let Some(b) = t.ball_d {
        v["ball_d"] = json!(b);
    }
    if let Some(b) = t.cone_end_d {
        v["cone_end_d"] = json!(b);
    }
    if let Some(b) = t.ball_depth {
        v["ball_depth"] = json!(b);
    }
    if t.rounded {
        v["rounded"] = json!(true);
    }
    v
}

/// The layout in the prototype's JSON shape (the synthetic suite's stage 2 reads it).
pub(super) fn to_json(lay: &Lay) -> Value {
    let supports: Vec<Value> = lay
        .supports
        .iter()
        .map(|s| {
            let base = match &s.base {
                LBase::Plate { xy, root_d } => json!({"plate": xy, "root_d": root_d}),
                LBase::Parent { parent, at } => json!({"parent": parent, "at": at}),
                LBase::Lower(low) => json!({"lower": tip_json(low)}),
            };
            let mut v = json!({"tip": tip_json(&s.tip), "base": base, "joints": s.joints, "shaft_d": s.shaft_d});
            if let LBase::Lower(low) = &s.base {
                v["lower"] = tip_json(low);
            }
            if let Some(d) = &s.seg_d {
                v["seg_d"] = json!(d);
            }
            if s.stick {
                v["stick"] = json!(true);
            }
            if s.twig {
                v["twig"] = json!(true);
            }
            v
        })
        .collect();
    let posts: Vec<Value> = lay
        .posts
        .iter()
        .map(|p| {
            let mut v = json!({"foot": p.foot, "joints": p.joints, "top": p.top, "root_d": p.root_d, "d": p.d});
            if let Some(d) = &p.seg_d {
                v["seg_d"] = json!(d);
            }
            v
        })
        .collect();
    let braces: Vec<Value> = lay.braces.iter().map(|b| json!({"lower": [b.lower.0, b.lower.1, b.lower.2], "upper": [b.upper.0, b.upper.1, b.upper.2], "d": b.d})).collect();
    let kicks: Vec<Value> = lay
        .kicks
        .iter()
        .map(|k| {
            let mut v = json!({"support": k.support, "knot": k.knot, "joints": k.joints, "foot": k.foot, "root_d": k.root_d, "d": k.d});
            if let Some(d) = &k.seg_d {
                v["seg_d"] = json!(d);
            }
            v
        })
        .collect();
    json!({"supports": supports, "posts": posts, "braces": braces, "kickstands": kicks})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_normals_from_the_cells_asked_for_equal_those_of_the_whole_grid() {
        use super::super::super::contacts::tests::{ball, frustum, rand01};
        let mut model = ball([0.0, 0.0, 10.0], 8.0);
        model.extend(frustum([0.0, 0.0, 0.0], [4.0, 0.0, 10.0], 1.0, 0.5, 24));
        let mut state = 5u64;
        let queries: Vec<(Vec3, f32)> = (0..300)
            .map(|_| {
                let p =
                    [-10.0 + 20.0 * rand01(&mut state), -10.0 + 20.0 * rand01(&mut state), 20.0 * rand01(&mut state)];
                (v3(p), (0.1 + 1.5 * rand01(&mut state)) as f32)
            })
            .collect();
        let asked = Surface::new(&model, &queries);
        let whole = Surface::new(&model, &[(Vec3::new(0.0, 0.0, 10.0), 40.0)]);
        let mut found = 0;
        for &(p, r) in &queries {
            let toward = Vec3::new(0.3, -0.2, 1.0);
            let (a, b) = (asked.normal_at(p, r, toward), whole.normal_at(p, r, toward));
            assert_eq!(a.map(|n| [n.x, n.y, n.z].map(f32::to_bits)), b.map(|n| [n.x, n.y, n.z].map(f32::to_bits)));
            found += usize::from(a.is_some());
        }
        assert!(found > 10, "{found}");
    }

    fn taper(r0: f64, r1: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| r0 + (r1 - r0) * (i as f64 + 0.5) / n as f64).collect()
    }

    #[test]
    fn a_taper_earns_steps_by_its_ratio() {
        // 40 samples is 8 mm, past STEP_MIN_MM. 1.2:1 stays one width, 1.6:1 earns a second, 2.5:1
        // a third.
        assert_eq!(steps_wanted(&taper(0.6, 0.5, 40)), 1);
        assert_eq!(steps_wanted(&taper(0.8, 0.5, 40)), 2);
        assert_eq!(steps_wanted(&taper(1.25, 0.5, 40)), 3);
        // Too short a taper earns nothing.
        assert_eq!(steps_wanted(&taper(1.25, 0.5, 10)), 1);
    }

    #[test]
    fn step_cuts_split_at_the_least_area_error_and_snap_to_joints() {
        let r = taper(1.0, 0.5, 40);
        let (cuts, radii, iou) = step_cuts(&r, 2, &[]);
        assert_eq!(cuts.len(), 1);
        assert!((cuts[0] as i64 - 20).abs() <= 2, "{cuts:?}");
        assert!(radii[0] > radii[1]);
        assert!(iou > step_cuts(&r, 1, &[]).2);
        // A joint within STEP_SNAP_MM (7 samples) takes the cut.
        let (snapped, _, _) = step_cuts(&r, 2, &[25]);
        assert_eq!(snapped, vec![25]);
        let (far, _, _) = step_cuts(&r, 2, &[39]);
        assert_ne!(far, vec![39]);
    }

    #[test]
    fn numpy_rounding_and_search_helpers() {
        assert_eq!(round_half_even(2.5), 2);
        assert_eq!(round_half_even(3.5), 4);
        assert_eq!(searchsorted_right(&[0.0, 1.0, 2.0], 1.0), 2);
        assert_eq!(searchsorted_right(&[0.0, 1.0, 2.0], -1.0), 0);
    }
}
