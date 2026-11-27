//! Scores a reading of a real-structure case against its Lychee truth (`truth.json`).
//!
//! The rules are the Lychee scorer's (kept with the support work files), on one case instead of a
//! whole model: tips matched one to one by contact, routes covered both ways, shaft lines covered
//! both ways, braces matched by both ends. Lychee's vocabulary is mapped to the reader's only where
//! the two mean the same:
//! - trunk: a support standing on the plate (the reader's base "plate");
//! - branch, leaf (a mini support: one tapered rod with no cone) and reverse branch: a support
//!   hanging from another (base "parent");
//! - mesh to mesh: a support between two parts of the model (base "model", a strut or micro
//!   support, with a tip at each end);
//! - kickstand: the reader's leg (a DragonFruit kickstand), one leg per kickstand with the same
//!   knee ([`kickstand_legs`]): an upright pillar read as a post and its lean read as a brace is
//!   not the same thing (the owner's decision 1 of 2026-11-25);
//! - brace: a brace.
//!
//! Which of two supports sharing a shaft is the trunk and which the branch is the author's choice
//! in Lychee, so a route is compared as a line from the contact down: the same line under another
//! base passes. Lychee has no posts, so a reader post counts against the braces stage. Roots and rafts are
//! drawn differently by design: lines below `Z_ROOT` are left out of routes and shafts.
//!
//! Seven readings that differ from Lychee's in name only are accepted, each by a geometric rule
//! (the owner's decisions of 2026-11-25):
//! - a branch's shaft and a brace swapped at its socket ([`socket_swaps`]): the two rods leave
//!   the socket alike and nothing hangs on either, so the mesh cannot say which is which, and
//!   the reader hangs the tip from the brace's other host with the shaft as the brace;
//! - a brace into the root zone read as a leg ([`brace_legs`]): the brace's lower end is under
//!   `ROOT_TOP`, the leg's knot is within `BRACE_MM` of its upper end and the leg's line above the
//!   root zone lies on the brace;
//! - a branch from inside a trunk's foot read as a support with a foot of its own (routes): the
//!   branch joins a trunk under `ROOT_TOP` and the reader stands it on the plate; its line above
//!   the root zone is compared as any other;
//! - a brace through a pillar read as one brace per stretch ([`brace_chain`]): two or more reader
//!   braces lie end to end on the Lychee brace, from one of its ends to the other, and meet on
//!   the lines of Lychee supports;
//! - a brace lying wholly in the root zone ([`in_root_zone`]): both its ends are under `ROOT_TOP`,
//!   where routes and shafts are not compared either, so the braces stage leaves it out (the
//!   owner's decision on x11_brace_in_root_zone);
//! - a branch hanging from a kickstand read as a support on the kickstand's foot
//!   ([`stands_in_a_kickstand`]): the kickstand is a leg, which holds up supports and carries no
//!   branch, so the branch's support runs up the kickstand's pillar to where the branch leaves it;
//!   that bend is not counted against Lychee's joints;
//! - the post under a missed tip ([`posts_of_missed_tips`]): a post standing on the line of a
//!   Lychee trunk whose own tip the reader misses counts against the tips stage only.

use crate::measure::{P, Seg};
use crate::score::{Loop, seg_dist1, tip_loop};
use crate::vec::V3;
use serde_json::{Value, json};

/// A reader tip matches a truth tip when their contacts are this close (one to one, nearest first).
pub const TIP_MM: f64 = 0.3;
/// Truth contacts this close are one point: one contact, which one reader tip answers.
pub const SAME_CONTACT_MM: f64 = 0.05;
/// The reader's contact width over Lychee's must lie in this range. Lychee's is the nominal point
/// diameter; the reader sizes the loop the cone really makes, which is wider on a slope.
pub const SIZE_RATIO: (f64, f64) = (0.7, 1.5);
/// The reader's tip direction against Lychee's axis.
pub const DIR_DEG: f64 = 15.0;
/// The reader's tip direction is taken from the contact to the point this far along its support
/// (the socket, where that is further): a hair support's sockets sit a few hundredths of a
/// millimetre from its contacts, too near to give a direction.
pub const DIR_BASE_MM: f64 = 0.5;
/// The reader's cone body width over Lychee's, on tips with a cone.
pub const BODY_RATIO: (f64, f64) = (0.75, 1.33);
/// The reader's socket against Lychee's, on tips with a cone.
pub const SOCKET_MM: f64 = 1.0;
/// A point of a shaft line is covered when the other side has a line this close.
pub const SHAFT_MM: f64 = 0.3;
pub const SHAFT_RECALL: f64 = 0.9;
pub const SHAFT_PRECISION: f64 = 0.9;
/// The same for a tip's route down to the plate, a parent's foot or the model.
pub const ROUTE_MM: f64 = 0.5;
/// A route is right when this share of it is covered, both ways.
pub const ROUTE_SHARE: f64 = 0.9;
/// A brace matches when both its ends are this close to the other's ends (either order).
pub const BRACE_MM: f64 = 1.0;
/// A shaft and a brace leaving one socket may be read for each other when the brace ends this
/// close to the socket ...
pub const SWAP_AT_MM: f64 = 0.3;
/// ... their widths are within this share of the wider one's ...
pub const SWAP_WIDTH: f64 = 0.1;
/// ... and they turn off the tip cone's line within this many degrees of each other.
pub const SWAP_TURN_DEG: f64 = 5.0;
/// Lines below this height are left out of shafts and routes.
pub const Z_ROOT: f64 = 2.0;
/// The top of the reader's roots: a support, post or leg on the plate stands upright under this
/// height and its line starts here (`support_synthetic/nominal.rs` builds it so).
pub const ROOT_TOP: f64 = 2.0;
/// Sampling step along lines.
pub const STEP: f64 = 0.25;
/// Validate: a truth contact lies this close to the cut model's surface.
pub const CONTACT_ON_MODEL_MM: f64 = 0.15;
/// Validate: a truth line sample counts as inside the support mesh when it is inside it or this
/// close to its surface.
pub const LINE_IN_MESH_MM: f64 = 0.1;
/// Validate: this share of every truth line lies in the support mesh.
pub const LINE_SHARE: f64 = 0.9;
/// Validate: no stretch of a truth line longer than this is bare of support mesh, and no stretch
/// of support mesh larger than this is left unexplained by the truth.
pub const BARE_MM: f64 = 1.0;
/// Validate: the step a truth line is walked at for its share in the mesh and its bare stretches.
pub const BARE_STEP: f64 = 0.2;
/// Validate: a support mesh corner is explained when a truth primitive's surface is this close.
pub const MESH_EXPLAINED_MM: f64 = 0.3;
/// Validate: this share of the support mesh's corners (above the root zone) is explained.
pub const MESH_SHARE: f64 = 0.9;
/// Validate: this share of the corners of every connected piece of the support mesh is explained.
pub const PIECE_SHARE: f64 = 0.9;
/// Validate: the raft kept under a case stays under this height.
pub const RAFT_TOP_MM: f64 = 6.0;
/// Stage 4a: clip length allowed beyond what the original supports clip themselves.
pub const CLIP_MM: f64 = 0.05;
/// Stage 4b: a rebuilt loop's centre against the original's.
pub const LOOP_POS_MM: f64 = 0.15;
/// Stage 4b: a rebuilt loop's area against the original's.
pub const AREA_SHARE: f64 = 0.2;
/// Stage 4b: contact the originals don't have.
pub const EXTRA_MM: f64 = 0.3;

fn arr(x: &Value) -> V3 {
    V3([0, 1, 2].map(|k| x[k].as_f64().unwrap_or(f64::NAN)))
}
fn num(x: &Value) -> f64 {
    x.as_f64().unwrap_or(0.0)
}
fn list(x: &Value) -> &[Value] {
    x.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn pts(x: &Value) -> Vec<V3> {
    list(x).iter().map(arr).collect()
}

/// Points along a line, one per `step` (the middle of each stretch).
pub fn sample(poly: &[V3], step: f64) -> Vec<V3> {
    let mut out = Vec::new();
    for w in poly.windows(2) {
        let l = (w[1] - w[0]).norm();
        let n = ((l / step).ceil() as usize).max(1);
        out.extend((0..n).map(|i| w[0] + (w[1] - w[0]) * ((i as f64 + 0.5) / n as f64)));
    }
    out
}

fn dist_poly(p: V3, poly: &[V3]) -> f64 {
    if poly.len() == 1 {
        return (p - poly[0]).norm();
    }
    poly.windows(2).map(|w| seg_dist1(p, w[0], w[1])).fold(f64::INFINITY, f64::min)
}

fn dist_polys(p: V3, polys: &[Vec<V3>]) -> f64 {
    polys.iter().filter(|l| !l.is_empty()).map(|l| dist_poly(p, l)).fold(f64::INFINITY, f64::min)
}

fn above(points: Vec<V3>) -> Vec<V3> {
    points.into_iter().filter(|p| p[2] >= Z_ROOT).collect()
}

/// The samples of a line above the root zone.
fn b_pts(line: &[V3]) -> Vec<V3> {
    above(sample(line, STEP))
}

fn share(points: &[V3], polys: &[Vec<V3>], tol: f64) -> f64 {
    if points.is_empty() {
        return 1.0;
    }
    points.iter().filter(|&&p| dist_polys(p, polys) <= tol).count() as f64 / points.len() as f64
}

/// The part of a bottom-to-top line below the projection of `point` onto it, top first.
fn project_down(poly: &[V3], point: V3) -> Vec<V3> {
    let mut best = (f64::INFINITY, 0usize, poly[0]);
    for (i, w) in poly.windows(2).enumerate() {
        let ab = w[1] - w[0];
        let t = ((point - w[0]).dot(ab) / ab.dot(ab).max(1e-12)).clamp(0.0, 1.0);
        let q = w[0] + ab * t;
        let d = (point - q).norm();
        if d < best.0 {
            best = (d, i, q);
        }
    }
    let mut out = vec![best.2];
    out.extend(poly[..=best.1].iter().rev());
    out
}

fn angle_deg(u: V3, v: V3) -> Option<f64> {
    let (nu, nv) = (u.norm(), v.norm());
    (nu > 1e-9 && nv > 1e-9).then(|| (u.dot(v) / (nu * nv)).clamp(-1.0, 1.0).acos().to_degrees())
}

/// One-to-one nearest-first matching of two point sets within `tol`: for each of `a`, its `b`.
fn match_points(a: &[V3], b: &[V3], tol: f64) -> Vec<Option<usize>> {
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (i, &p) in a.iter().enumerate() {
        for (j, &q) in b.iter().enumerate() {
            let d = (p - q).norm();
            if d <= tol {
                pairs.push((d, i, j));
            }
        }
    }
    pairs.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let mut out = vec![None; a.len()];
    let mut used = vec![false; b.len()];
    for (_, i, j) in pairs {
        if out[i].is_none() && !used[j] {
            out[i] = Some(j);
            used[j] = true;
        }
    }
    out
}

/// One tip of the reader's layout.
pub struct ReaderTip {
    pub support: usize,
    /// Its upper tip, or the lower tip of a strut.
    pub lower: bool,
    pub contact: V3,
    pub socket: V3,
    pub contact_d: f64,
    pub body_d: f64,
}

/// The reader's layout (the suite's layout JSON, `reading::layout`) as lines.
pub struct Reader {
    pub tips: Vec<ReaderTip>,
    /// Bottom-to-top line of each support.
    pub polys: Vec<Vec<V3>>,
    pub post_polys: Vec<Vec<V3>>,
    pub kick_polys: Vec<Vec<V3>>,
    /// Joints per leg.
    pub kick_joints: Vec<usize>,
    /// "plate", "parent" or "model" per support.
    pub base: Vec<&'static str>,
    pub parent: Vec<Option<usize>>,
    pub joints: Vec<usize>,
    pub braces: Vec<(V3, V3)>,
}

impl Reader {
    pub fn new(lay: &Value) -> Reader {
        let mut r = Reader {
            tips: Vec::new(),
            polys: Vec::new(),
            post_polys: Vec::new(),
            kick_polys: Vec::new(),
            kick_joints: Vec::new(),
            base: Vec::new(),
            parent: Vec::new(),
            joints: Vec::new(),
            braces: Vec::new(),
        };
        for (i, s) in list(&lay["supports"]).iter().enumerate() {
            let tip = |t: &Value, lower: bool| ReaderTip {
                support: i,
                lower,
                contact: arr(&t["contact"]),
                socket: arr(&t["socket"]),
                contact_d: num(&t["contact_d"]),
                body_d: num(&t["body_d"]),
            };
            r.tips.push(tip(&s["tip"], false));
            let b = &s["base"];
            // A support on the plate stands upright in its root: its own line starts at the root's top.
            let (kind, line, parent) = if let Some(p) = b.get("plate") {
                let (x, y) = (num(&p[0]), num(&p[1]));
                ("plate", vec![V3([x, y, 0.0]), V3([x, y, ROOT_TOP])], None)
            } else if let Some(at) = b.get("at") {
                ("parent", vec![arr(at)], b["parent"].as_u64().map(|x| x as usize))
            } else {
                r.tips.push(tip(&s["lower"], true));
                ("model", vec![arr(&s["lower"]["socket"])], None)
            };
            let mut line = line;
            line.extend(pts(&s["joints"]));
            line.push(arr(&s["tip"]["socket"]));
            r.polys.push(line);
            r.base.push(kind);
            r.parent.push(parent);
            r.joints.push(list(&s["joints"]).len());
        }
        let grounded = |foot: V3| V3([foot[0], foot[1], 0.0]);
        for p in list(&lay["posts"]) {
            let foot = arr(&p["foot"]);
            let mut line = vec![grounded(foot), foot];
            line.extend(pts(&p["joints"]));
            line.push(arr(&p["top"]));
            r.post_polys.push(line);
        }
        for k in list(&lay["kickstands"]) {
            let foot = arr(&k["foot"]);
            let mut line = vec![grounded(foot), foot];
            line.extend(pts(&k["joints"]));
            line.push(arr(&k["knot"]));
            r.kick_polys.push(line);
            r.kick_joints.push(list(&k["joints"]).len());
        }
        for b in list(&lay["braces"]) {
            r.braces.push((arr(&b["lower"][2]), arr(&b["upper"][2])));
        }
        r
    }

    fn host_poly(&self, i: usize) -> Option<&Vec<V3>> {
        if i < self.polys.len() { self.polys.get(i) } else { self.post_polys.get(i - self.polys.len()) }
    }

    /// A tip's way from its socket down to the plate, or to the model.
    pub fn route(&self, tip: &ReaderTip) -> Vec<V3> {
        let i = tip.support;
        let mut out: Vec<V3> = self.polys[i].iter().rev().copied().collect();
        if self.base[i] == "model" {
            if tip.lower {
                out.reverse();
            }
            // On to the other contact, as the truth's route runs.
            out.extend(self.tips.iter().filter(|t| t.support == i && t.lower != tip.lower).map(|t| t.contact));
            return out;
        }
        let mut seen = vec![i];
        let mut cur = i;
        for _ in 0..40 {
            let Some(p) = (cur < self.parent.len()).then(|| self.parent[cur]).flatten() else { break };
            if seen.contains(&p) {
                break;
            }
            seen.push(p);
            let Some(hp) = self.host_poly(p) else { break };
            out.extend(project_down(hp, *out.last().unwrap()));
            cur = p;
        }
        out
    }

    /// A tip's direction: from its contact to the socket, or, where the socket is nearer than
    /// [`DIR_BASE_MM`], to the point that far along the support's own line (on a support between
    /// two parts of the model, as far as its other contact).
    pub fn tip_dir(&self, tip: &ReaderTip) -> V3 {
        let i = tip.support;
        let mut line: Vec<V3> = self.polys[i].iter().rev().copied().collect();
        if self.base[i] == "model" {
            if tip.lower {
                line.reverse();
            }
            line.extend(self.tips.iter().filter(|t| t.support == i && t.lower != tip.lower).map(|t| t.contact));
        }
        let mut left = DIR_BASE_MM;
        let mut at = tip.contact;
        for p in line {
            let step = (p - at).norm();
            if step >= left {
                return at + (p - at) * (left / step) - tip.contact;
            }
            left -= step;
            at = p;
        }
        at - tip.contact
    }

    pub fn shafts(&self) -> Vec<Vec<V3>> {
        self.polys.iter().chain(&self.post_polys).chain(&self.kick_polys).cloned().collect()
    }

    pub fn cones(&self) -> Vec<Vec<V3>> {
        self.tips.iter().map(|t| vec![t.socket, t.contact]).collect()
    }
}

/// The truth's lines.
pub struct Truth<'a> {
    pub t: &'a Value,
    pub tips: &'a [Value],
    pub supports: &'a [Value],
    pub braces: &'a [Value],
}

impl<'a> Truth<'a> {
    pub fn new(t: &'a Value) -> Truth<'a> {
        Truth { t, tips: list(&t["tips"]), supports: list(&t["supports"]), braces: list(&t["braces"]) }
    }
    pub fn shafts(&self) -> Vec<Vec<V3>> {
        self.supports.iter().map(|s| pts(&s["line"])).filter(|l| l.len() >= 2).collect()
    }
    pub fn cones(&self) -> Vec<Vec<V3>> {
        self.tips
            .iter()
            .filter(|t| num(&t["length"]) > 1e-6)
            .map(|t| vec![arr(&t["socket"]), arr(&t["contact"])])
            .collect()
    }
    pub fn support(&self, id: &str) -> Option<&'a Value> {
        self.supports.iter().find(|s| s["id"] == id)
    }
    /// Whether a support is a branch that joins a trunk under the root's top (inside its foot).
    pub fn joins_a_trunk_in_its_foot(&self, id: &str) -> bool {
        let Some(s) = self.support(id) else { return false };
        let host = s["host"].as_str().and_then(|h| self.support(h));
        s["base"] == "parent"
            && host.is_some_and(|h| h["role"] == "trunk")
            && s["attach"][2].as_f64().is_some_and(|z| z < ROOT_TOP)
    }
    fn name(tip: &Value) -> String {
        format!("{}{}", tip["support"].as_str().unwrap_or("?"), if tip["end"] == "base" { " (base end)" } else { "" })
    }
}

fn result(errors: Vec<String>, extra: Value) -> Value {
    let mut v = json!({"pass": errors.is_empty(), "errors": errors});
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    v
}

/// The layout stages: tips, tip shape, routes, shafts, braces and joints.
pub fn layout_stages(truth: &Value, lay: &Value) -> Value {
    let t = Truth::new(truth);
    let r = Reader::new(lay);
    let tc: Vec<V3> = t.tips.iter().map(|x| arr(&x["contact"])).collect();
    let rc: Vec<V3> = r.tips.iter().map(|x| x.contact).collect();
    let m = match_points(&tc, &rc, TIP_MM);
    // Two Lychee supports ending at one point are one contact (the touch rule): the reader has one
    // tip there, and the second support's tip counts as shared, not missing.
    let shared: Vec<bool> = (0..tc.len())
        .map(|k| {
            m[k].is_none()
                && (0..tc.len()).any(|j| j != k && m[j].is_some() && (tc[j] - tc[k]).norm() <= SAME_CONTACT_MM)
        })
        .collect();
    // Tips: found, missing, extra.
    let mut e_tips = Vec::new();
    for (k, tip) in t.tips.iter().enumerate() {
        if m[k].is_none() && !shared[k] {
            let near = rc.iter().map(|&c| (c - tc[k]).norm()).fold(f64::INFINITY, f64::min);
            e_tips.push(format!(
                "tip {} missing (nearest reader tip {})",
                Truth::name(tip),
                if near.is_finite() { format!("{near:.2} mm away") } else { "none".into() }
            ));
        }
    }
    let used: Vec<usize> = m.iter().flatten().copied().collect();
    let mut extra = 0;
    for (j, c) in rc.iter().enumerate() {
        if !used.contains(&j) {
            extra += 1;
            e_tips.push(format!("extra tip at [{:.2}, {:.2}, {:.2}]", c[0], c[1], c[2]));
        }
    }
    let found = used.len();
    let n_shared = shared.iter().filter(|x| **x).count();
    let pos: Vec<f64> = m.iter().enumerate().filter_map(|(k, j)| j.map(|j| (tc[k] - rc[j]).norm())).collect();
    let tips = result(
        e_tips,
        json!({"truth": tc.len(), "reader": rc.len(), "found": found, "shared": n_shared, "missing": tc.len() - found - n_shared, "extra": extra,
               "worst_pos": pos.iter().copied().fold(0.0, f64::max)}),
    );
    // Tip shape: contact width, direction, and on cone tips the body width and the socket.
    let mut e_shape = Vec::new();
    let mut shapes = Vec::new();
    for (k, tip) in t.tips.iter().enumerate() {
        let Some(j) = m[k] else { continue };
        let rt = &r.tips[j];
        let name = Truth::name(tip);
        let cd = num(&tip["contact_d"]);
        let size = if cd > 0.0 { Some(rt.contact_d / cd) } else { None };
        let dir = angle_deg(arr(&tip["axis"]), r.tip_dir(rt));
        let cone = num(&tip["length"]) > 1e-6;
        let body = (cone && num(&tip["body_d"]) > 0.0).then(|| rt.body_d / num(&tip["body_d"]));
        let socket = cone.then(|| (rt.socket - arr(&tip["socket"])).norm());
        if let Some(x) = size.filter(|x| *x < SIZE_RATIO.0 || *x > SIZE_RATIO.1) {
            e_shape.push(format!("tip {name}: contact {:.2} mm, Lychee {cd:.2} (ratio {x:.2})", rt.contact_d));
        }
        if let Some(a) = dir.filter(|a| *a > DIR_DEG) {
            e_shape.push(format!("tip {name}: direction {a:.0} degrees off"));
        }
        if body.is_some_and(|x| x < BODY_RATIO.0 || x > BODY_RATIO.1) {
            e_shape.push(format!("tip {name}: body {:.2} mm, Lychee {:.2}", rt.body_d, num(&tip["body_d"])));
        }
        if let Some(d) = socket.filter(|d| *d > SOCKET_MM) {
            e_shape.push(format!("tip {name}: socket {d:.2} mm off"));
        }
        shapes.push(json!({"tip": name, "size": size, "dir": dir, "body": body, "socket": socket}));
    }
    let shape = result(e_shape, json!({"tips": shapes}));
    // Routes: where each tip's support stands, and its line down.
    let mut e_routes = Vec::new();
    let (mut routes_ok, mut renamed, mut own_feet) = (0, 0, 0);
    // The (support, brace) pairs read with shaft and brace swapped at the socket.
    let may_swap = socket_swaps(&t);
    let mut swapped: Vec<(usize, usize)> = Vec::new();
    for (k, tip) in t.tips.iter().enumerate() {
        let Some(j) = m[k] else { continue };
        let rt = &r.tips[j];
        let name = Truth::name(tip);
        let want = tip["base"].as_str().unwrap_or("?");
        let got = r.base[rt.support];
        let tr = pts(&tip["route"]);
        let mut rr = vec![rt.contact];
        rr.extend(r.route(rt));
        let (a, b) = (above(sample(&tr, STEP)), above(sample(&rr, STEP)));
        let (fa, fb) = (share(&a, std::slice::from_ref(&rr), ROUTE_MM), share(&b, std::slice::from_ref(&tr), ROUTE_MM));
        // Too short to compare as lines (both under 1 mm above the root zone): only the base tells.
        let short = a.len() < 4 && b.len() < 4;
        let line_ok = short || (fa >= ROUTE_SHARE && fb >= ROUTE_SHARE);
        let covered = format!("{:.0}% of Lychee's line covered, {:.0}% of the reader's", 100.0 * fa, 100.0 * fb);
        // A branch that joins a trunk inside its foot (under the root's top) may stand on a foot of
        // its own: the two differ only inside the root zone, which is not compared.
        let own_foot = line_ok && got == "plate" && t.joins_a_trunk_in_its_foot(tip["support"].as_str().unwrap_or("?"));
        own_feet += usize::from(own_foot);
        // Shaft and brace swapped at the socket: the tip's line down the brace and on down the
        // brace's other host, compared as any other.
        let swap = (!line_ok && got == "parent")
            .then(|| {
                may_swap.iter().copied().find(|&(i, b)| {
                    if t.supports[i]["id"] != tip["support"] {
                        return false;
                    }
                    let (p, q) = (arr(&t.braces[b]["a"]), arr(&t.braces[b]["b"]));
                    let far = if (p - tr[1]).norm() <= (q - tr[1]).norm() { q } else { p };
                    let host = list(&t.braces[b]["hosts"]).iter().find(|h| **h != tip["support"]);
                    let down = host.and_then(|h| t.tips.iter().find(|x| x["support"] == *h && x["end"] != "base"));
                    let Some(down) = down else { return false };
                    let mut below: Vec<V3> = pts(&down["route"]);
                    below.reverse();
                    let mut alt = vec![tr[0], tr[1], far];
                    alt.extend(project_down(&below, far));
                    let a = above(sample(&alt, STEP));
                    share(&a, std::slice::from_ref(&rr), ROUTE_MM) >= ROUTE_SHARE
                        && share(&b_pts(&rr), std::slice::from_ref(&alt), ROUTE_MM) >= ROUTE_SHARE
                })
            })
            .flatten();
        if let Some(pair) = swap {
            swapped.push(pair);
            routes_ok += 1;
        } else if own_foot {
            routes_ok += 1;
        } else if want != got && (short || !line_ok) {
            e_routes.push(format!("tip {name}: stands on the {got}, Lychee on the {want} ({covered})"));
        } else if !line_ok {
            e_routes.push(format!("tip {name}: route differs ({covered})"));
        } else {
            // The same line down under another name: which of two supports sharing a shaft is the
            // trunk and which the branch is the author's choice in Lychee and the reader's in the
            // layout.
            renamed += usize::from(want != got);
            routes_ok += 1;
        }
    }
    let routes = result(
        e_routes,
        json!({"right": routes_ok, "of": found, "same_line_other_base": renamed, "own_foot": own_feet,
               "socket_swaps": swapped.len()}),
    );
    // Braces: both ends, one to one; then the readings that differ from Lychee's in name only.
    let (ts, rs) = (t.shafts(), r.shafts());
    let mut t_lines = ts.clone();
    t_lines.extend(t.cones());
    let mut r_lines = rs.clone();
    r_lines.extend(r.cones());
    let tb: Vec<(V3, V3)> = t.braces.iter().map(|b| (arr(&b["a"]), arr(&b["b"]))).collect();
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (i, (a, b)) in tb.iter().enumerate() {
        for (j, (c, d)) in r.braces.iter().enumerate() {
            let dd = ((*a - *c).norm().max((*b - *d).norm())).min((*a - *d).norm().max((*b - *c).norm()));
            if dd <= BRACE_MM {
                pairs.push((dd, i, j));
            }
        }
    }
    pairs.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let (mut bt, mut br) = (vec![false; tb.len()], vec![false; r.braces.len()]);
    for (_, i, j) in pairs {
        if !bt[i] && !br[j] {
            bt[i] = true;
            br[j] = true;
        }
    }
    let matched = bt.iter().filter(|x| **x).count();
    // Shaft and brace swapped at a socket: the brace is the reader's shaft, and a reader brace
    // along the Lychee shaft (both ends within `BRACE_MM` of the shaft's) is that shaft.
    let mut swap_lines: Vec<Vec<V3>> = Vec::new();
    for &(i, b) in &swapped {
        bt[b] = true;
        let line = pts(&t.supports[i]["line"]);
        let (lo, hi) = (line[0], line[line.len() - 1]);
        let hit = (0..r.braces.len()).find(|&j| {
            let (c, d) = r.braces[j];
            !br[j] && ((c - lo).norm().max((d - hi).norm())).min((c - hi).norm().max((d - lo).norm())) <= BRACE_MM
        });
        if let Some(j) = hit {
            br[j] = true;
            swap_lines.push(vec![r.braces[j].0, r.braces[j].1]);
        }
    }
    // A brace into the root zone read as a leg.
    let as_legs = brace_legs(&tb, &bt, &r.kick_polys);
    for &(i, _) in &as_legs {
        bt[i] = true;
    }
    // A brace through a pillar read as one brace per stretch.
    let mut split = 0;
    for i in 0..tb.len() {
        if bt[i] {
            continue;
        }
        if let Some(links) = brace_chain(tb[i], &r.braces, &br, &t) {
            bt[i] = true;
            split += 1;
            for j in links {
                br[j] = true;
            }
        }
    }
    // A brace lying wholly in the root zone is not compared.
    let mut under_root = 0;
    for i in 0..tb.len() {
        if !bt[i] && in_root_zone(tb[i]) {
            bt[i] = true;
            under_root += 1;
        }
    }
    // The posts under missed tips.
    let missed: Vec<&Value> = (0..tc.len()).filter(|&k| m[k].is_none() && !shared[k]).map(|k| &t.tips[k]).collect();
    let excused = posts_of_missed_tips(&missed, &t, &r.post_polys);
    // Shafts: lines covered both ways. A leg that stands for a Lychee brace lies on that brace.
    let mut t_under = t_lines.clone();
    t_under.extend(as_legs.iter().map(|&(i, _)| vec![tb[i].0, tb[i].1]));
    // Where shaft and brace are swapped, the reader's shaft lies on the Lychee brace and its
    // brace on the Lychee shaft.
    t_under.extend(swapped.iter().map(|&(_, b)| vec![tb[b].0, tb[b].1]));
    let mut r_over = r_lines.clone();
    r_over.extend(swap_lines);
    let tp: Vec<V3> = above(ts.iter().flat_map(|l| sample(l, STEP)).collect());
    let rp: Vec<V3> = above(rs.iter().flat_map(|l| sample(l, STEP)).collect());
    let (recall, precision) = (share(&tp, &r_over, SHAFT_MM), share(&rp, &t_under, SHAFT_MM));
    let mut e_shafts = Vec::new();
    if recall < SHAFT_RECALL {
        e_shafts.push(format!("{:.0}% of Lychee's shaft length has a reader shaft", 100.0 * recall));
    }
    if precision < SHAFT_PRECISION {
        e_shafts.push(format!("{:.0}% of the reader's shaft length is on a Lychee shaft", 100.0 * precision));
    }
    let shafts = result(e_shafts, json!({"recall": recall, "precision": precision}));
    let r_brace_lines: Vec<Vec<V3>> = r.braces.iter().map(|(a, b)| vec![*a, *b]).collect();
    let t_brace_lines: Vec<Vec<V3>> = tb.iter().map(|(a, b)| vec![*a, *b]).collect();
    let mut e_braces = Vec::new();
    for (i, b) in t.braces.iter().enumerate() {
        if bt[i] {
            continue;
        }
        let p = sample(&[tb[i].0, tb[i].1], STEP);
        let fate = if share(&p, &r_brace_lines, SHAFT_MM) >= 0.8 {
            "the reader has braces along it, with other ends"
        } else if share(&p, &r_lines, SHAFT_MM) >= 0.8 {
            "the reader reads a shaft there"
        } else {
            "the reader has nothing there"
        };
        e_braces.push(format!("brace {}: {fate}", b["id"].as_str().unwrap_or("?")));
    }
    for (j, (c, d)) in r.braces.iter().enumerate() {
        if br[j] {
            continue;
        }
        let p = sample(&[*c, *d], STEP);
        let fate = if share(&p, &t_brace_lines, SHAFT_MM) >= 0.8 {
            "Lychee has braces along it, with other ends"
        } else if share(&p, &t_lines, SHAFT_MM) >= 0.8 {
            "Lychee has a shaft there"
        } else {
            "Lychee has nothing there"
        };
        e_braces.push(format!("extra brace near [{:.1}, {:.1}, {:.1}]: {fate}", c[0], c[1], c[2]));
    }
    // Kickstands: each is one reader leg with the same knee.
    let kickstands: Vec<&Value> = t.supports.iter().filter(|s| s["role"] == "leg").collect();
    let free_legs: Vec<usize> = (0..r.kick_polys.len()).filter(|k| as_legs.iter().all(|&(_, l)| l != *k)).collect();
    let kick_lines: Vec<Vec<V3>> = kickstands.iter().map(|s| pts(&s["line"])).collect();
    let leg_of = kickstand_legs(&kick_lines, &r.kick_polys, &free_legs);
    let posts = r.post_polys.len() - excused;
    if posts > 0 {
        e_braces.push(format!("{posts} posts (Lychee has none)"));
    }
    for (i, s) in kickstands.iter().enumerate() {
        if leg_of[i].is_some() {
            continue;
        }
        let p = above(sample(&kick_lines[i], STEP));
        let on = |lines: &[Vec<V3>]| (100.0 * share(&p, lines, SHAFT_MM)).round();
        e_braces.push(format!(
            "kickstand {}: not read as one leg ({}% of its line is on reader legs, {}% on posts and supports, {}% on braces)",
            s["id"].as_str().unwrap_or("?"),
            on(&r.kick_polys),
            on(&r.polys.iter().chain(&r.post_polys).cloned().collect::<Vec<_>>()),
            on(&r_brace_lines)
        ));
    }
    let extra_legs = free_legs.iter().filter(|k| !leg_of.contains(&Some(**k))).count();
    if extra_legs > 0 {
        e_braces.push(format!("{extra_legs} legs Lychee has no kickstand for"));
    }
    let braces = result(
        e_braces,
        json!({"truth": tb.len(), "reader": r.braces.len(), "matched": matched, "as_legs": as_legs.len(), "split": split,
               "socket_swaps": swapped.len(),
               "posts": r.post_polys.len(), "posts_of_missed_tips": excused, "legs": r.kick_polys.len(),
               "under_root": under_root, "kickstands": kickstands.len(), "kickstands_read": leg_of.iter().flatten().count()}),
    );
    // Joints: no more bends than Lychee's support has (once per support).
    let mut e_joints = Vec::new();
    let mut on_kickstands = 0;
    let mut seen: Vec<(String, usize)> = Vec::new();
    for (k, tip) in t.tips.iter().enumerate() {
        let Some(j) = m[k] else { continue };
        let id = tip["support"].as_str().unwrap_or("?").to_string();
        let rs = r.tips[j].support;
        if seen.contains(&(id.clone(), rs)) {
            continue;
        }
        seen.push((id.clone(), rs));
        let mut want = t.support(&id).map(|s| num(&s["joints"]) as usize).unwrap_or(0);
        // A branch on a kickstand read as a support up the kickstand's pillar bends where the
        // branch leaves the pillar.
        let in_kickstand = stands_in_a_kickstand(&t, &id, r.base[rs], &r.polys[rs]);
        want += usize::from(in_kickstand);
        on_kickstands += usize::from(in_kickstand);
        if r.joints[rs] > want {
            e_joints.push(format!("support {id}: {} joints, Lychee {want}", r.joints[rs]));
        }
    }
    // A kickstand's leg has no more joints than the kickstand has bends (its one knee), counted as
    // a support's are.
    for (i, s) in kickstands.iter().enumerate() {
        let Some(k) = leg_of[i] else { continue };
        let want = num(&s["joints"]) as usize;
        if r.kick_joints[k] > want {
            e_joints.push(format!(
                "kickstand {}: {} joints, Lychee {want}",
                s["id"].as_str().unwrap_or("?"),
                r.kick_joints[k]
            ));
        }
    }
    let joints = result(e_joints, json!({"on_kickstands": on_kickstands}));
    json!({"tips": tips, "shape": shape, "routes": routes, "shafts": shafts, "braces": braces, "joints": joints})
}

/// Whether the reader's support (bottom-to-top `line`, standing on `base`) reads a Lychee branch
/// that hangs from a kickstand as a support standing in the kickstand's foot.
///
/// DragonFruit's kickstand holds up a support and carries no branch, so the reader stands the
/// branch's support on the plate and runs it up the kickstand's pillar. That is accepted when the
/// support bends within [`ROUTE_MM`] of where the branch leaves the kickstand and everything below
/// that bend lies within [`SHAFT_MM`] of the kickstand's line; the bend is then one joint more
/// than Lychee's branch has. A bend elsewhere, or a line that leaves the pillar, is not it.
pub fn stands_in_a_kickstand(t: &Truth, id: &str, base: &str, line: &[V3]) -> bool {
    let Some(s) = t.support(id) else { return false };
    let Some(host) = s["host"].as_str().and_then(|h| t.support(h)).filter(|h| h["role"] == "leg") else {
        return false;
    };
    if base != "plate" || !s["attach"].is_array() {
        return false;
    }
    let (attach, pillar) = (arr(&s["attach"]), pts(&host["line"]));
    // The line's inner points are its joints (the first two points are the upright root).
    let Some(k) = (2..line.len().saturating_sub(1)).find(|&k| (line[k] - attach).norm() <= ROUTE_MM) else {
        return false;
    };
    let below = above(sample(&line[..=k], STEP));
    share(&below, std::slice::from_ref(&pillar), SHAFT_MM) >= SHAFT_PRECISION
}

/// Whether a Lychee brace lies wholly in the root zone: both ends under [`ROOT_TOP`], where the
/// reader has roots and writes no brace, and where routes and shafts are left out as well. A
/// brace with one end at or above the root's top is compared as any other.
pub fn in_root_zone(brace: (V3, V3)) -> bool {
    brace.0[2] < ROOT_TOP && brace.1[2] < ROOT_TOP
}

/// The (support, brace) pairs of the truth whose shaft and brace may be read for each other: a
/// support hanging from a parent by one straight shaft, and a brace ending at its socket (within
/// [`SWAP_AT_MM`]), that the mesh cannot tell apart there:
/// - the same width (within [`SWAP_WIDTH`]);
/// - both sloping down from the socket (neither within 25 degrees of straight down nor level,
///   the reader's classes of ways);
/// - turning off the tip cone's line within [`SWAP_TURN_DEG`] of each other;
/// - nothing hanging on either: no other support hangs from the support, and no other brace
///   ends on its shaft or on the brace between their ends (within [`SHAFT_MM`], further than
///   [`SWAP_AT_MM`] from the ends).
///
/// Either way the build is the same two rods; only their names are swapped.
pub fn socket_swaps(t: &Truth) -> Vec<(usize, usize)> {
    let sloping = |u: V3| u[2] < -0.15 && u[2] > -0.906_307_787;
    let ends: Vec<(V3, V3)> = t.braces.iter().map(|b| (arr(&b["a"]), arr(&b["b"]))).collect();
    let mut out = Vec::new();
    for (i, s) in t.supports.iter().enumerate() {
        let line = pts(&s["line"]);
        if s["base"] != "parent" || line.len() != 2 {
            continue;
        }
        let (attach, socket) = (line[0], line[1]);
        let id = s["id"].as_str().unwrap_or("?");
        let Some(tip) = t.tips.iter().find(|x| x["support"] == id && x["end"] != "base") else { continue };
        let cone = socket - arr(&tip["contact"]);
        if t.supports.iter().any(|c| c["host"] == id) || cone.norm() < 1e-6 {
            continue;
        }
        // Whether another brace ends on the rod from `a` to `b` between its ends.
        let hung = |a: V3, b: V3, skip: usize| {
            ends.iter().enumerate().any(|(j, &(p, q))| {
                j != skip
                    && [p, q].iter().any(|&e| {
                        seg_dist1(e, a, b) <= SHAFT_MM && (e - a).norm() > SWAP_AT_MM && (e - b).norm() > SWAP_AT_MM
                    })
            })
        };
        for (k, &(p, q)) in ends.iter().enumerate() {
            let far = if (p - socket).norm() <= SWAP_AT_MM {
                q
            } else if (q - socket).norm() <= SWAP_AT_MM {
                p
            } else {
                continue;
            };
            let (ds, db) = (num(&s["d"]), num(&t.braces[k]["d"]));
            let (us, ub) = (attach - socket, far - socket);
            let (Some(ts), Some(tb)) = (angle_deg(cone, us), angle_deg(cone, ub)) else { continue };
            if (ds - db).abs() <= SWAP_WIDTH * ds.max(db)
                && sloping(us / us.norm())
                && sloping(ub / ub.norm())
                && (ts - tb).abs() <= SWAP_TURN_DEG
                && !hung(attach, socket, k)
                && !hung(far, socket, k)
            {
                out.push((i, k));
            }
        }
    }
    out
}

/// Lychee braces read as legs: (brace, leg) pairs, one to one, nearest knot first.
///
/// A Lychee brace whose lower end is under the root's top ([`ROOT_TOP`]) runs into its host's
/// foot, where the reader has roots and no braces. A reader leg stands for it when the leg's knot
/// is within [`BRACE_MM`] of the brace's upper end and at least [`SHAFT_PRECISION`] of the leg's
/// line above the root zone lies within [`SHAFT_MM`] of the brace. `matched` marks the braces that
/// already have a reader brace.
pub fn brace_legs(braces: &[(V3, V3)], matched: &[bool], legs: &[Vec<V3>]) -> Vec<(usize, usize)> {
    let mut cand: Vec<(f64, usize, usize)> = Vec::new();
    for (i, &(a, b)) in braces.iter().enumerate() {
        let (low, high) = if a[2] <= b[2] { (a, b) } else { (b, a) };
        if matched[i] || low[2] >= ROOT_TOP {
            continue;
        }
        for (k, line) in legs.iter().enumerate() {
            let Some(&knot) = line.last() else { continue };
            let d = (knot - high).norm();
            let p = above(sample(line, STEP));
            if d <= BRACE_MM && !p.is_empty() && share(&p, &[vec![low, high]], SHAFT_MM) >= SHAFT_PRECISION {
                cand.push((d, i, k));
            }
        }
    }
    cand.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (_, i, k) in cand {
        if out.iter().all(|&(bi, bk)| bi != i && bk != k) {
            out.push((i, k));
        }
    }
    out
}

/// For each Lychee kickstand, the reader leg that reads it (one to one, nearest knot first).
///
/// A Lychee kickstand is an upright pillar from the plate and a lean from its top onto another
/// support; DragonFruit's kickstand is the same thing, so the reader must read it as one leg whose
/// line bends at the same knee. A leg reads a kickstand when its knot is within [`BRACE_MM`] of
/// the kickstand's top, at least [`SHAFT_PRECISION`] of the leg's line above the root zone lies
/// within [`SHAFT_MM`] of the kickstand's line, and at least [`SHAFT_RECALL`] of the kickstand's
/// line lies within [`SHAFT_MM`] of the leg's: a post under a brace, or a leg that stops at the
/// knee, covers only a part. `free` lists the legs not already standing for a brace.
pub fn kickstand_legs(kickstands: &[Vec<V3>], legs: &[Vec<V3>], free: &[usize]) -> Vec<Option<usize>> {
    let mut cand: Vec<(f64, usize, usize)> = Vec::new();
    for (i, line) in kickstands.iter().enumerate() {
        let Some(&top) = line.last() else { continue };
        let want = above(sample(line, STEP));
        for &k in free {
            let Some(&knot) = legs[k].last() else { continue };
            let got = above(sample(&legs[k], STEP));
            let d = (knot - top).norm();
            if d <= BRACE_MM
                && !got.is_empty()
                && share(&got, std::slice::from_ref(line), SHAFT_MM) >= SHAFT_PRECISION
                && share(&want, std::slice::from_ref(&legs[k]), SHAFT_MM) >= SHAFT_RECALL
            {
                cand.push((d, i, k));
            }
        }
    }
    cand.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let mut out = vec![None; kickstands.len()];
    for (_, i, k) in cand {
        if out[i].is_none() && !out.contains(&Some(k)) {
            out[i] = Some(k);
        }
    }
    out
}

/// The reader braces that together stand for one Lychee brace passing through pillars.
///
/// Lychee joins two supports with one brace even when other supports stand in its way; the reader
/// reads one brace per neighbouring pair. The split is accepted when the unused reader braces
/// whose both ends lie within [`SHAFT_MM`] of the Lychee brace are two or more, run end to end
/// (each starts within [`SHAFT_MM`] of where the last one stopped), start and stop within
/// [`BRACE_MM`] of the Lychee brace's ends, and meet on Lychee supports (each meeting point within
/// [`SHAFT_MM`] of a support's line). `used` marks the reader braces already matched.
pub fn brace_chain(brace: (V3, V3), reader: &[(V3, V3)], used: &[bool], t: &Truth) -> Option<Vec<usize>> {
    let (a, b) = brace;
    let ab = b - a;
    let along = |p: V3| (p - a).dot(ab) / ab.dot(ab).max(1e-12);
    // (start, stop, brace) of each reader brace on the line, turned to run from `a` to `b`.
    let mut links: Vec<(V3, V3, usize)> = Vec::new();
    for (j, &(c, d)) in reader.iter().enumerate() {
        if !used[j] && seg_dist1(c, a, b) <= SHAFT_MM && seg_dist1(d, a, b) <= SHAFT_MM {
            links.push(if along(c) <= along(d) { (c, d, j) } else { (d, c, j) });
        }
    }
    if links.len() < 2 {
        return None;
    }
    links.sort_by(|x, y| along(x.0).partial_cmp(&along(y.0)).unwrap());
    let on_a_support = |p: V3| t.shafts().iter().any(|line| dist_poly(p, line) <= SHAFT_MM);
    let ends = (links[0].0 - a).norm() <= BRACE_MM && (links[links.len() - 1].1 - b).norm() <= BRACE_MM;
    let joined = links.windows(2).all(|w| (w[0].1 - w[1].0).norm() <= SHAFT_MM && on_a_support(w[0].1));
    (ends && joined).then(|| links.iter().map(|l| l.2).collect())
}

/// The number of reader posts that stand for Lychee trunks whose own tip the reader misses.
///
/// A trunk with no tip read is a post to the reader. The missing tip already fails the tips stage;
/// its post is left out of the braces stage when at least [`SHAFT_PRECISION`] of the post's line
/// above the root zone lies within [`SHAFT_MM`] of that trunk's line (one post per missed tip).
pub fn posts_of_missed_tips(missed: &[&Value], t: &Truth, posts: &[Vec<V3>]) -> usize {
    let mut excused = vec![false; posts.len()];
    for tip in missed {
        let Some(s) = tip["support"].as_str().and_then(|id| t.support(id)).filter(|s| s["role"] == "trunk") else {
            continue;
        };
        let line = pts(&s["line"]);
        let hit = (0..posts.len()).find(|&j| {
            let p = above(sample(&posts[j], STEP));
            !excused[j] && !p.is_empty() && share(&p, std::slice::from_ref(&line), SHAFT_MM) >= SHAFT_PRECISION
        });
        if let Some(j) = hit {
            excused[j] = true;
        }
    }
    excused.iter().filter(|x| **x).count()
}

// ---- geometry for validate and stage 4 ----

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P, b: P) -> P {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The winding number of a mesh round a point (the sum of its triangles' solid angles over 4 pi).
pub fn winding(p: V3, tris: &[[P; 3]]) -> f64 {
    let mut total = 0.0;
    for t in tris {
        let (a, b, c) = (sub(t[0], p.0), sub(t[1], p.0), sub(t[2], p.0));
        let (la, lb, lc) = (dot(a, a).sqrt(), dot(b, b).sqrt(), dot(c, c).sqrt());
        let det = dot(a, cross(b, c));
        let den = la * lb * lc + dot(a, b) * lc + dot(b, c) * la + dot(c, a) * lb;
        total += 2.0 * det.atan2(den);
    }
    total / (4.0 * std::f64::consts::PI)
}

/// The distance from a point to a mesh's surface.
pub fn surface_dist(p: V3, tris: &[[P; 3]]) -> f64 {
    tris.iter().map(|t| crate::measure::closest_sq(p.0, t)).fold(f64::INFINITY, f64::min).sqrt()
}

/// Signed distance to a round cone from `a` (radius `ra`) to `b` (radius `rb`), radius interpolated.
fn cone_gap(p: V3, a: V3, b: V3, ra: f64, rb: f64) -> f64 {
    let d = b - a;
    let t = ((p - a).dot(d) / d.dot(d).max(1e-12)).clamp(0.0, 1.0);
    (p - (a + d * t)).norm() - (ra + t * (rb - ra))
}

/// The truth's primitives as round cones: shaft stretches, the pieces of each foot (as measured
/// on the mesh: a plate's width is the width across its corners), tip cones and braces.
fn primitives(t: &Truth) -> Vec<Prim> {
    let mut out = Vec::new();
    for s in t.supports {
        let line = pts(&s["line"]);
        let d = num(&s["d"]).max(0.05);
        let d_tip = s.get("d_tip").and_then(Value::as_f64).unwrap_or(d);
        for w in line.windows(2) {
            out.push((w[0], w[1], d / 2.0, d_tip / 2.0));
        }
        let (x, y) = (num(&s["foot"][0]), num(&s["foot"][1]));
        for q in list(&s["foot_pieces"]) {
            let (z, w) = (&q["z"], &q["d"]);
            out.push((V3([x, y, num(&z[0])]), V3([x, y, num(&z[1])]), num(&w[0]) / 2.0, num(&w[1]) / 2.0));
        }
    }
    for tip in t.tips {
        if num(&tip["length"]) > 1e-6 {
            out.push((
                arr(&tip["socket"]),
                arr(&tip["contact"]),
                num(&tip["body_d"]) / 2.0,
                num(&tip["contact_d"]) / 2.0,
            ));
        }
    }
    for b in t.braces {
        let r = num(&b["d"]).max(0.05) / 2.0;
        out.push((arr(&b["a"]), arr(&b["b"]), r, r));
    }
    out
}

fn seg_ends(segs: &[Seg]) -> (Vec<V3>, Vec<V3>) {
    let f = |p: [f32; 3]| V3(p.map(f64::from));
    (segs.iter().map(|s| f(s.1)).collect(), segs.iter().map(|s| f(s.2)).collect())
}

fn mids_lens(a: &[V3], b: &[V3]) -> (Vec<V3>, Vec<f64>) {
    (a.iter().zip(b).map(|(&x, &y)| (x + y) / 2.0).collect(), a.iter().zip(b).map(|(&x, &y)| (x - y).norm()).collect())
}

/// The tip as `score::tip_loop` takes it: the contact, the surface normal there (the tip's axis
/// when the cut has no face near it) and the tip's direction.
fn loop_tip(tip: &Value) -> Value {
    let axis = arr(&tip["axis"]);
    let normal = if tip["normal"].is_array() { arr(&tip["normal"]) } else { axis };
    json!({"contact": tip["contact"], "normal": normal.0, "dir": axis.0, "contact_d": tip["contact_d"]})
}

/// Whether an intersection at `m` belongs to a tip: near a truth contact, or along a mini
/// support's rod end (a mini is a rod that may enter the model).
fn at_a_tip(m: V3, t: &Truth) -> bool {
    t.tips.iter().any(|tip| {
        let c = arr(&tip["contact"]);
        let reach = num(&tip["contact_d"]).max(num(&tip["body_d"]).min(1.0)) / 2.0 + 0.4;
        (m - c).norm() <= reach
    })
}

/// The truth's own loops and clip, measured on the original supports.
pub struct Validated {
    pub errors: Vec<String>,
    /// Per truth tip, the loop the original support makes on the model.
    pub loops: Vec<Option<Loop>>,
    pub touch_mids: Vec<V3>,
    /// What the original supports clip of the model apart from their tips.
    pub clip_mm: f64,
    pub numbers: Value,
}

/// A line walked at one sample per `step`: the share of its samples that pass `inside`, and the
/// longest stretch whose samples all fail it, with where that stretch starts.
pub fn walk(line: &[V3], step: f64, inside: &dyn Fn(V3) -> bool) -> (f64, f64, Option<V3>) {
    let (mut worst, mut at) = (0.0, None);
    let (mut run, mut start) = (0.0, None);
    let (mut all, mut good) = (0usize, 0usize);
    for w in line.windows(2) {
        let l = (w[1] - w[0]).norm();
        let n = ((l / step).ceil() as usize).max(1);
        for i in 0..n {
            let p = w[0] + (w[1] - w[0]) * ((i as f64 + 0.5) / n as f64);
            all += 1;
            if inside(p) {
                good += 1;
                (run, start) = (0.0, None);
                continue;
            }
            run += l / n as f64;
            start = start.or(Some(p));
            if run > worst {
                (worst, at) = (run, start);
            }
        }
    }
    (if all == 0 { 1.0 } else { good as f64 / all as f64 }, worst, at)
}

/// The connected pieces of a mesh: triangles joined through shared corners (the same coordinates),
/// as lists of triangle indices.
pub fn pieces(tris: &[[P; 3]]) -> Vec<Vec<usize>> {
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut parent: Vec<usize> = (0..tris.len()).collect();
    let mut first: std::collections::HashMap<[u64; 3], usize> = std::collections::HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for p in t {
            let j = *first.entry(p.map(f64::to_bits)).or_insert(i);
            let (a, b) = (find(&mut parent, i), find(&mut parent, j));
            parent[a.max(b)] = a.min(b);
        }
    }
    let mut out: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..tris.len() {
        let root = find(&mut parent, i);
        out.entry(root).or_default().push(i);
    }
    out.into_values().collect()
}

/// The stretches of a mesh that fail `explained`: its failing corners joined along the edges of
/// its triangles, as (size, middle) of each group's bounding box.
pub fn stray_stretches(tris: &[[P; 3]], explained: &dyn Fn(V3) -> bool) -> Vec<(f64, V3)> {
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    // The failing corners, one per place.
    let mut index: std::collections::HashMap<[u64; 3], usize> = std::collections::HashMap::new();
    let mut points: Vec<V3> = Vec::new();
    let mut corner = |p: &P| -> Option<usize> {
        let key = p.map(f64::to_bits);
        if let Some(&i) = index.get(&key) {
            return (i != usize::MAX).then_some(i);
        }
        let i = if explained(V3(*p)) { usize::MAX } else { points.len() };
        if i != usize::MAX {
            points.push(V3(*p));
        }
        index.insert(key, i);
        (i != usize::MAX).then_some(i)
    };
    let ids: Vec<[Option<usize>; 3]> = tris.iter().map(|t| [corner(&t[0]), corner(&t[1]), corner(&t[2])]).collect();
    let mut parent: Vec<usize> = (0..points.len()).collect();
    for t in &ids {
        for k in 0..3 {
            if let (Some(a), Some(b)) = (t[k], t[(k + 1) % 3]) {
                let (a, b) = (find(&mut parent, a), find(&mut parent, b));
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut boxes: std::collections::BTreeMap<usize, (V3, V3)> = std::collections::BTreeMap::new();
    for (i, &p) in points.iter().enumerate() {
        let root = find(&mut parent, i);
        let b = boxes.entry(root).or_insert((p, p));
        for k in 0..3 {
            b.0[k] = b.0[k].min(p[k]);
            b.1[k] = b.1[k].max(p[k]);
        }
    }
    boxes.into_values().map(|(lo, hi)| ((hi - lo).norm(), (lo + hi) / 2.0)).collect()
}

/// A primitive of the truth: a round cone from a point and radius to another.
pub type Prim = (V3, V3, f64, f64);

/// The element each primitive of [`primitives`] is part of: a support with its foot and its tip
/// cones, or a brace.
fn elements(t: &Truth) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, s) in t.supports.iter().enumerate() {
        let n = pts(&s["line"]).windows(2).count() + list(&s["foot_pieces"]).len();
        out.extend(std::iter::repeat_n(i, n));
    }
    let mut next = t.supports.len();
    for tip in t.tips.iter().filter(|tip| num(&tip["length"]) > 1e-6) {
        let own = t.supports.iter().position(|s| s["id"] == tip["support"]);
        out.push(own.unwrap_or(next));
        next += usize::from(own.is_none());
    }
    out.extend((0..t.braces.len()).map(|k| next + k));
    out
}

/// The shells of the support mesh (its connected pieces) with the elements that may call each
/// their own: `None` for any, or the elements with a primitive that explains at least
/// [`PIECE_SHARE`] of the shell's corners (as the explained share is taken: inside it or within
/// [`MESH_EXPLAINED_MM`] of its surface). A shell that is one element's rod is that element's
/// alone, however close it runs to another's line.
pub type Shell = (Vec<[P; 3]>, Option<Vec<usize>>);

pub fn shells(support: &[[P; 3]], prims: &[Prim], elements: &[usize]) -> Vec<Shell> {
    pieces(support)
        .into_iter()
        .map(|piece| {
            let tris: Vec<[P; 3]> = piece.iter().map(|&i| support[i]).collect();
            let corners: Vec<V3> = tris.iter().flat_map(|t| t.iter().map(|p| V3(*p))).collect();
            let owners: Vec<usize> = (0..prims.len())
                .filter(|&k| {
                    let (a, b, ra, rb) = prims[k];
                    let near = corners.iter().filter(|&&p| cone_gap(p, a, b, ra, rb) <= MESH_EXPLAINED_MM).count();
                    near as f64 >= PIECE_SHARE * corners.len() as f64
                })
                .map(|k| elements[k])
                .collect();
            (tris, (!owners.is_empty()).then_some(owners))
        })
        .collect()
}

/// Whether the point `p` on primitive `k`'s line lies in mesh of its own, given that it lies in
/// the support mesh. It does unless only shells that are other elements' alone hold it: a
/// neighbour's rod crossing or lying beside the line cannot stand in for a shell the case lacks
/// (b14_branch_on_strut). The shells that are not another's (the element's own, and welded or
/// open ones no element owns) hold it when their winding number round it is over a quarter (a rod
/// left open where its tip cone sits has a half at its mouth). Where the element's whole width
/// lies inside another primitive of the truth it has no surface of its own to show (a rod's end
/// buried in its host, a mini support rising out of a tip cone), and passes.
pub fn in_own_mesh(p: V3, shells: &[Shell], prims: &[Prim], elements: &[usize], k: usize) -> bool {
    let foreign = |s: &Shell| s.1.as_ref().is_some_and(|o| !o.contains(&elements[k]));
    if !shells.iter().any(|s| foreign(s) && winding(p, &s.0) > 0.5) {
        return true;
    }
    if shells.iter().filter(|s| !foreign(s)).map(|s| winding(p, &s.0)).sum::<f64>() > 0.25 {
        return true;
    }
    let (a, b, ra, rb) = prims[k];
    let d = b - a;
    let r = ra + (rb - ra) * ((p - a).dot(d) / d.dot(d).max(1e-12)).clamp(0.0, 1.0);
    prims.iter().enumerate().any(|(j, &(pa, pb, qa, qb))| {
        elements[j] != elements[k] && cone_gap(p, pa, pb, qa, qb) + r <= MESH_EXPLAINED_MM
    })
}

/// Validate: is the truth the truth of these meshes? Contacts on the cut model, lines inside the
/// support mesh with no stretch over [`BARE_MM`] bare of it, the support mesh explained by the
/// truth as a whole, in every connected piece and with no stretch over [`BARE_MM`] left over
/// (the raft apart), and a loop at every tip.
pub fn validate(truth: &Value, model: &[[P; 3]], support: &[[P; 3]], isect: &[Seg], touch: &[Seg]) -> Validated {
    let t = Truth::new(truth);
    let mut errors = Vec::new();
    if model.is_empty() && !t.tips.is_empty() {
        errors.push("the case has no model cut".to_string());
    }
    let mut worst_contact = 0.0f64;
    for tip in t.tips {
        let d = surface_dist(arr(&tip["contact"]), model);
        worst_contact = worst_contact.max(d);
        if d > CONTACT_ON_MODEL_MM {
            errors.push(format!("truth tip {}: contact {d:.2} mm off the cut model", Truth::name(tip)));
        }
    }
    // Lines inside the support mesh, each in mesh of its own (see `in_own_mesh`), with no long
    // stretch bare of it. The primitives come in the order of `primitives`: every stretch of a
    // support's line, its foot pieces, then the tip cones, then the braces.
    let prims = primitives(&t);
    let elements = elements(&t);
    let shells = shells(support, &prims, &elements);
    let in_mesh = |p: V3| winding(p, support) > 0.5 || surface_dist(p, support) <= LINE_IN_MESH_MM;
    let mut worst_bare = 0.0f64;
    let mut line = |what: String, l: &[V3], errors: &mut Vec<String>| {
        let k = prims.iter().position(|q| {
            (q.0 - l[0]).norm() < 1e-9 && (q.1 - l[0]).dot(l[1] - l[0]) > 0.0 && seg_dist1(l[1], q.0, q.1) < 1e-6
        });
        // Roots and rafts are drawn differently by design: under the root zone the mesh round the
        // line is enough.
        let inside = |p: V3| {
            in_mesh(p) && (p[2] < Z_ROOT + 0.5 || k.is_none_or(|k| in_own_mesh(p, &shells, &prims, &elements, k)))
        };
        let (f, bare, at) = walk(l, BARE_STEP, &inside);
        if f < LINE_SHARE {
            errors.push(format!("truth {what}: {:.0}% of its line is in support mesh of its own", 100.0 * f));
        }
        worst_bare = worst_bare.max(bare);
        if let Some(p) = at.filter(|_| bare > BARE_MM) {
            errors.push(format!(
                "truth {what}: {bare:.1} mm of its line from [{:.2}, {:.2}, {:.2}] is bare of support mesh of its own",
                p[0], p[1], p[2]
            ));
        }
    };
    for s in t.supports {
        for w in pts(&s["line"]).windows(2) {
            line(format!("support {}", s["id"].as_str().unwrap_or("?")), w, &mut errors);
        }
    }
    for tip in t.tips.iter().filter(|x| num(&x["length"]) > 1e-6) {
        // The cone, less its last 0.1 mm (the point).
        let (c, s) = (arr(&tip["contact"]), arr(&tip["socket"]));
        let l = (s - c).norm();
        line(format!("tip cone of {}", Truth::name(tip)), &[s, c + (s - c) * (0.1 / l.max(0.1)).min(0.5)], &mut errors);
    }
    for b in t.braces {
        line(format!("brace {}", b["id"].as_str().unwrap_or("?")), &[arr(&b["a"]), arr(&b["b"])], &mut errors);
    }
    // The support mesh explained by the truth (above the root zone; the raft is not modelled).
    let explained_by = |p: V3| prims.iter().any(|&(a, b, ra, rb)| cone_gap(p, a, b, ra, rb) <= MESH_EXPLAINED_MM);
    let corners: Vec<V3> =
        support.iter().flat_map(|t| t.iter().map(|p| V3(*p))).filter(|p| p[2] >= Z_ROOT + 0.5).collect();
    let explained = corners.iter().filter(|&&p| explained_by(p)).count();
    let mesh_share = if corners.is_empty() { 1.0 } else { explained as f64 / corners.len() as f64 };
    if mesh_share < MESH_SHARE {
        errors.push(format!("the truth explains {:.0}% of the support mesh", 100.0 * mesh_share));
    }
    // Every connected piece of the support mesh is the truth's, down to the plate; the raft (the
    // last `raft_triangles` of the mesh) is the raft's.
    let n_raft = (num(&truth["raft_triangles"]) as usize).min(support.len());
    let (body, raft) = support.split_at(support.len() - n_raft);
    let mut worst_piece = 1.0f64;
    for piece in pieces(body) {
        let corners: Vec<V3> = piece.iter().flat_map(|&i| body[i].iter().map(|p| V3(*p))).collect();
        let f = corners.iter().filter(|&&p| explained_by(p)).count() as f64 / corners.len() as f64;
        worst_piece = worst_piece.min(f);
        if f < PIECE_SHARE {
            let c = corners.iter().fold(V3([0.0; 3]), |a, &p| a + p) / corners.len() as f64;
            errors.push(format!(
                "the truth explains {:.0}% of a piece of the support mesh ({} triangles round [{:.2}, {:.2}, {:.2}])",
                100.0 * f,
                piece.len(),
                c[0],
                c[1],
                c[2]
            ));
        }
    }
    // Within a piece, no stretch of mesh is left over (the stub of a brace to a support outside
    // the case, welded into a kept shell).
    let mut worst_stray = 0.0f64;
    for (size, c) in stray_stretches(body, &explained_by) {
        worst_stray = worst_stray.max(size);
        if size > BARE_MM {
            errors.push(format!(
                "{size:.1} mm of the support mesh round [{:.2}, {:.2}, {:.2}] is not the truth's",
                c[0], c[1], c[2]
            ));
        }
    }
    let raft_top = raft.iter().flat_map(|t| t.iter().map(|p| p[2])).fold(0.0, f64::max);
    if raft_top > RAFT_TOP_MM {
        errors.push(format!("the raft reaches {raft_top:.1} mm"));
    }
    // The originals' own clip and loops.
    let (a, b) = seg_ends(isect);
    let (mids, lens) = mids_lens(&a, &b);
    let clip_mm: f64 = mids.iter().zip(&lens).filter(|(m, _)| !at_a_tip(**m, &t)).map(|(_, l)| *l).sum();
    let (ta, tb) = seg_ends(touch);
    let (tmids, _) = mids_lens(&ta, &tb);
    let contacts: Vec<V3> = t.tips.iter().map(|x| arr(&x["contact"])).collect();
    let mut loops = Vec::new();
    for tip in t.tips {
        let lp = tip_loop(&loop_tip(tip), &tmids, &ta, &tb, &contacts).filter(|l| l.d >= 0.05);
        if lp.is_none() {
            errors.push(format!("truth tip {}: the original support makes no loop on the cut model", Truth::name(tip)));
        }
        loops.push(lp);
    }
    let numbers = json!({"worst_contact_mm": worst_contact, "mesh_explained": mesh_share, "original_clip_mm": clip_mm,
                         "worst_bare_mm": worst_bare, "worst_piece_explained": worst_piece, "worst_stray_mm": worst_stray,
                         "loops": loops.iter().map(|l| l.as_ref().map(|l| l.d)).collect::<Vec<_>>()});
    Validated { errors, loops, touch_mids: tmids, clip_mm, numbers }
}

/// Stage 4 on a build: 4a clips (nothing but tips inside the model, beyond what the originals clip
/// themselves) on the build at tip offset 0, and 4b loops (a loop at every truth tip, at the
/// original's place and size, and no contact the originals do not have) on the build at
/// DragonFruit's own offset. `isect` is the offset 0 build against the model, `touch` the other
/// build against the shifted model; `labels` names the build element of an `isect` triangle.
pub fn stage4(truth: &Value, isect: &[Seg], touch: &[Seg], labels: &dyn Fn(u32) -> String, v: &Validated) -> Value {
    let t = Truth::new(truth);
    let (a, b) = seg_ends(isect);
    let (mids, lens) = mids_lens(&a, &b);
    let clip: Vec<bool> = mids.iter().map(|m| !at_a_tip(*m, &t)).collect();
    let clip_mm: f64 = lens.iter().zip(&clip).filter(|x| *x.1).map(|x| *x.0).sum();
    let mut who: Vec<(String, f64)> = Vec::new();
    for i in (0..isect.len()).filter(|&i| clip[i]) {
        let e = labels(isect[i].0);
        match who.iter_mut().find(|w| w.0 == e) {
            Some(w) => w.1 += lens[i],
            None => who.push((e, lens[i])),
        }
    }
    let mut errors = Vec::new();
    let clip_pass = clip_mm <= CLIP_MM + v.clip_mm * 1.2;
    if !clip_pass {
        who.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap());
        let at =
            clip.iter().position(|&x| x).map(|i| format!("[{:.2}, {:.2}, {:.2}]", mids[i][0], mids[i][1], mids[i][2]));
        errors.push(format!(
            "clips the model: {clip_mm:.2} mm of {} near {} (the originals clip {:.2} mm)",
            who.first().map(|w| w.0.as_str()).unwrap_or("?"),
            at.unwrap_or_default(),
            v.clip_mm
        ));
    }
    let (ta, tb) = seg_ends(touch);
    let (tmids, tlens) = mids_lens(&ta, &tb);
    let contacts: Vec<V3> = t.tips.iter().map(|x| arr(&x["contact"])).collect();
    let mut assigned = vec![false; touch.len()];
    let (mut missing, mut worst_pos, mut worst_size) = (0usize, 0.0f64, 0.0f64);
    let mut loops = Vec::new();
    let mut loop_errors = 0;
    for (k, tip) in t.tips.iter().enumerate() {
        let name = Truth::name(tip);
        // A tip with no loop of its own on the originals is a truth problem (validate reports it).
        let Some(orig) = &v.loops[k] else { continue };
        let Some(lp) = tip_loop(&loop_tip(tip), &tmids, &ta, &tb, &contacts).filter(|l| l.d >= 0.05) else {
            missing += 1;
            errors.push(format!("tip {name}: no contact loop"));
            continue;
        };
        for (x, m) in assigned.iter_mut().zip(&lp.mask) {
            *x |= *m;
        }
        let pos = (lp.centre - orig.centre).norm();
        let ratio = lp.d / orig.d;
        loops.push(json!({"tip": name, "d": lp.d, "original_d": orig.d, "pos": pos}));
        worst_pos = worst_pos.max(pos);
        if (ratio - 1.0).abs() > (worst_size - 1.0f64).abs() || worst_size == 0.0 {
            worst_size = ratio;
        }
        if pos > LOOP_POS_MM {
            loop_errors += 1;
            errors.push(format!("tip {name}: loop {pos:.2} mm off the original's"));
        }
        if (ratio * ratio - 1.0).abs() > AREA_SHARE {
            loop_errors += 1;
            errors.push(format!("tip {name}: loop {:.2} mm, the original's is {:.2}", lp.d, orig.d));
        }
    }
    let near = |set: &[V3], m: V3| set.iter().any(|&x| (x - m).norm() <= 0.3);
    let clipped: Vec<V3> = mids.iter().zip(&clip).filter(|x| *x.1).map(|x| *x.0).collect();
    let extra: f64 = (0..touch.len())
        .filter(|&i| !assigned[i] && !near(&v.touch_mids, tmids[i]) && !near(&clipped, tmids[i]))
        .map(|i| tlens[i])
        .sum::<f64>()
        / 2.0;
    if extra > EXTRA_MM {
        errors.push(format!("{extra:.2} mm of contact the originals don't have"));
    }
    let loop_pass = missing == 0 && loop_errors == 0 && extra <= EXTRA_MM;
    json!({"clip_pass": clip_pass, "loop_pass": loop_pass, "errors": errors, "clip_mm": clip_mm, "original_clip_mm": v.clip_mm,
           "clip_who": who.iter().map(|(k, x)| (k.clone(), json!(x))).collect::<serde_json::Map<_, _>>(),
           "missing": missing, "worst_pos": worst_pos, "worst_size": worst_size, "extra_mm": extra, "loops": loops})
}
