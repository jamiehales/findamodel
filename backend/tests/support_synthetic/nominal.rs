//! A nominal build of a support layout (the suite's layout JSON), meshed roughly as DragonFruit
//! builds it, so that stage 4's clip and loop checks can run without DragonFruit:
//! - a root under every plate support, post and kickstand: a disk 0.5 mm tall at root_d, then a
//!   cone up to z 2.0 narrowing to the pillar's width;
//! - shaft, post and kickstand segments as round frusta of their width (seg_d per segment where
//!   given, else shaft_d or d);
//! - joint balls (width + JOINT_EXTRA_MM) at every bend inside a pillar, and knots (host width +
//!   0.1) where a branch or kickstand meets its host and at both ends of every brace (brace
//!   width + 0.1);
//! - a contact cone at every tip, from the contact the VOXL writer writes for it
//!   (`voxl::written_tip`: lean-tip placement, blunt ends wider than their rod sunk, sphere tips
//!   when the writer has them on), built as DragonFruit's slicer builds it at a given tip offset
//!   (`CLIP_TIP_OFFSET_MM` for the clip check, `LOOP_TIP_OFFSET_MM` for the loops);
//! - at a twig's ends (a strut the writer writes with contact disks, `voxl::support_strut_ends`), the
//!   disks and the joints standing off them instead of contact cones;
//! - braces as frusta of their width.
//!
//! Every solid is closed and has `SIDES` sides.

use crate::vec::V3;
use findamodel::geometry::Vec3;
use findamodel::geometry::support_layout::{ContactTip, SupportBase, SupportLayout};
use findamodel::geometry::voxl::{SPHERE_TIPS_ENABLED, StrutEnds, WrittenTip, support_strut_ends, written_tip};
use serde_json::Value;

pub const SIDES: usize = 32;
const BALL_LAT: usize = 16;
const ROOT_TOP: f64 = 2.0;
const ROOT_DISK: f64 = 0.5;
/// The tip offsets of the DragonFruit builds stage 4 is scored on: clips (4a) on the round trip's
/// offset 0 build (`mesh/`), loops (4b) on the build at DragonFruit's own 0.05 mm (`mesh_tipoffset/`,
/// as the slicer runs it). DragonFruit starts a contact cone (or a twig's disk) this far below the
/// written contact, along the surface normal.
pub const CLIP_TIP_OFFSET_MM: f64 = 0.0;
pub const LOOP_TIP_OFFSET_MM: f64 = 0.05;
/// How much wider than its pillar a joint ball at a bend inside one pillar is.
/// (0.0: DragonFruit's own builds show no wider ball at a bend; 0.1 clipped c04 and c05.)
pub const JOINT_EXTRA_MM: f64 = 0.0;
/// How much wider than its host a knot ball (branch or kickstand on a host, brace ends) is.
pub const KNOT_EXTRA_MM: f64 = 0.1;

/// The kind of element a nominal triangle belongs to (for clip reports).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    Root,
    Shaft,
    Post,
    Kickstand,
    Brace,
    Ball,
    Cone,
}

impl Element {
    pub fn name(self) -> &'static str {
        match self {
            Element::Root => "root",
            Element::Shaft => "shaft",
            Element::Post => "post",
            Element::Kickstand => "kickstand",
            Element::Brace => "brace",
            Element::Ball => "ball",
            Element::Cone => "cone",
        }
    }
}

#[derive(Default)]
pub struct Nominal {
    pub tris: Vec<[V3; 3]>,
    pub labels: Vec<Element>,
    /// The tip offset it is built at.
    offset: f64,
}

fn arr(x: &Value) -> V3 {
    V3([0, 1, 2].map(|k| x[k].as_f64().unwrap()))
}
fn num(x: &Value) -> f64 {
    x.as_f64().unwrap()
}
fn pts(x: &Value) -> Vec<V3> {
    x.as_array().map(|a| a.iter().map(arr).collect()).unwrap_or_default()
}

fn v(p: Vec3) -> V3 {
    V3([p.x, p.y, p.z].map(f64::from))
}

/// A frustum (start, start width, end, end width).
pub type Cone = (V3, f64, V3, f64);

/// The contact of one tip as DragonFruit builds what the writer writes for it: a cone
/// (start, start width, socket, body width) and, for a sphere tip, its ball (centre, width).
pub fn tip_cone(tip: &ContactTip, offset: f64) -> (Cone, Option<(V3, f64)>) {
    let n = v(tip.surface_normal.normalized());
    let (socket, body) = (v(tip.socket), f64::from(tip.body_diameter_mm));
    match written_tip(tip, SPHERE_TIPS_ENABLED) {
        WrittenTip::Cone { pos, diameter_mm } => ((v(pos) - n * offset, f64::from(diameter_mm), socket, body), None),
        WrittenTip::Sphere { contact_diameter_mm: end, radius_ratio, depth_mm } => {
            let big = radius_ratio * end / 2.0;
            // The centre below the surface by the written depth plus the offset, or DragonFruit's own seat:
            // sqrt(R^2 - r^2) outside the surface, less the offset.
            let lift = match depth_mm {
                Some(d) => -(d + offset),
                None => (big * big - end * end / 4.0).max(0.0).sqrt() - offset,
            };
            let centre = v(tip.contact) + n * lift;
            ((centre, end, socket, body.max(end)), Some((centre, 2.0 * big)))
        }
    }
}

fn frame(axis: V3) -> (V3, V3) {
    let helper = if axis[2].abs() < 0.9 { V3([0.0, 0.0, 1.0]) } else { V3([1.0, 0.0, 0.0]) };
    let u = axis.cross(helper).unit();
    (u, axis.cross(u))
}

impl Nominal {
    fn push(&mut self, t: [V3; 3], e: Element) {
        self.tris.push(t);
        self.labels.push(e);
    }

    /// A closed shape of revolution about the axis from `a`; profile (t, r), r = 0 for a pole.
    fn revolve(&mut self, a: V3, axis: V3, profile: &[(f64, f64)], e: Element) {
        let axis = axis.unit();
        let (u, v) = frame(axis);
        let n = SIDES;
        let rings: Vec<(V3, Option<Vec<V3>>)> = profile
            .iter()
            .map(|&(t, r)| {
                let c = a + axis * t;
                if r <= 1e-9 {
                    (c, None)
                } else {
                    let ang = |k: usize| 2.0 * std::f64::consts::PI * k as f64 / n as f64;
                    (c, Some((0..n).map(|k| c + (u * ang(k).cos() + v * ang(k).sin()) * r).collect()))
                }
            })
            .collect();
        if let (c0, Some(r0)) = &rings[0] {
            for k in 0..n {
                self.push([*c0, r0[(k + 1) % n], r0[k]], e);
            }
        }
        if let (c1, Some(r1)) = &rings[rings.len() - 1] {
            for k in 0..n {
                self.push([*c1, r1[k], r1[(k + 1) % n]], e);
            }
        }
        for w in rings.windows(2) {
            let ((cl, lo), (ch, hi)) = (&w[0], &w[1]);
            match (lo, hi) {
                (None, None) => {}
                (None, Some(hi)) => (0..n).for_each(|k| self.push([*cl, hi[(k + 1) % n], hi[k]], e)),
                (Some(lo), None) => (0..n).for_each(|k| self.push([lo[k], lo[(k + 1) % n], *ch], e)),
                (Some(lo), Some(hi)) => {
                    for k in 0..n {
                        let j = (k + 1) % n;
                        self.push([lo[k], lo[j], hi[j]], e);
                        self.push([lo[k], hi[j], hi[k]], e);
                    }
                }
            }
        }
    }

    /// A frustum from a (width da) to b (width db).
    pub fn frustum(&mut self, a: V3, da: f64, b: V3, db: f64, e: Element) {
        let l = (b - a).norm();
        if l < 1e-9 {
            return;
        }
        self.revolve(a, b - a, &[(0.0, da / 2.0), (l, db / 2.0)], e);
    }

    pub fn ball(&mut self, c: V3, d: f64) {
        let r = d / 2.0;
        let prof: Vec<(f64, f64)> = (0..=BALL_LAT)
            .map(|k| {
                let a = std::f64::consts::PI * k as f64 / BALL_LAT as f64;
                (r * (1.0 - a.cos()), r * a.sin())
            })
            .collect();
        self.revolve(c - V3([0.0, 0.0, r]), V3([0.0, 0.0, 1.0]), &prof, Element::Ball);
    }

    /// A root on the plate at (x, y): disk at root_d, then a cone up to ROOT_TOP at width d.
    pub fn root(&mut self, x: f64, y: f64, root_d: f64, d: f64) {
        let r = root_d / 2.0;
        self.revolve(
            V3([x, y, 0.0]),
            V3([0.0, 0.0, 1.0]),
            &[(0.0, r), (ROOT_DISK, r), (ROOT_TOP, d / 2.0)],
            Element::Root,
        );
    }

    /// A pillar along a polyline, one width per segment, with a joint ball at every inner point.
    fn pillar(&mut self, line: &[V3], widths: &[f64], e: Element) {
        for (i, w) in line.windows(2).enumerate() {
            self.frustum(w[0], widths[i], w[1], widths[i], e);
        }
        for i in 1..line.len().saturating_sub(1) {
            self.ball(line[i], widths[i - 1].max(widths[i]) + JOINT_EXTRA_MM);
        }
    }

    /// A twig's contact disk as DragonFruit slices it: a cylinder `d` wide from the tip offset
    /// below the contact to `length` off it along the surface normal, capped by a ball as wide.
    fn disk(&mut self, tip: &ContactTip, d: f64, length: f64) {
        let (c, n) = (v(tip.contact), v(tip.surface_normal.normalized()));
        self.frustum(c - n * self.offset, d, c + n * length, d, Element::Cone);
        self.ball(c + n * length, d);
    }

    fn tip(&mut self, tip: &ContactTip) {
        let ((a, da, b, db), ball) = tip_cone(tip, self.offset);
        self.frustum(a, da, b, db, Element::Cone);
        if let Some((c, d)) = ball {
            self.ball(c, d);
        }
    }
}

/// Segment widths of a pillar: seg_d if it has one per segment, else the one width.
fn widths(x: &Value, one: f64, segs: usize) -> Vec<f64> {
    match x.get("seg_d").and_then(|v| v.as_array()) {
        Some(a) if a.len() == segs => a.iter().map(num).collect(),
        _ => vec![one; segs],
    }
}

/// The width of a pillar (support index, or post index past the supports) at the point nearest p.
fn host_width(lay: &Value, lines: &[(Vec<V3>, Vec<f64>)], i: usize, p: V3) -> f64 {
    let _ = lay;
    let Some((line, w)) = lines.get(i) else { return 1.0 };
    let mut best = (f64::INFINITY, w.first().copied().unwrap_or(1.0));
    for (k, s) in line.windows(2).enumerate() {
        let d = s[1] - s[0];
        let t = ((p - s[0]).dot(d) / d.dot(d).max(1e-12)).clamp(0.0, 1.0);
        let dist = (p - (s[0] + d * t)).norm();
        if dist < best.0 {
            best = (dist, w[k]);
        }
    }
    best.1
}

/// Meshes a layout: `lay` in the suite's shape, `written` the same layout as the writer takes it
/// (its tips, as the writer places them); DragonFruit's tip offset `offset`.
pub fn build(lay: &Value, written: &SupportLayout, offset: f64) -> Nominal {
    let mut m = Nominal { offset, ..Nominal::default() };
    let empty = vec![];
    let sups = lay["supports"].as_array().unwrap_or(&empty);
    let posts = lay["posts"].as_array().unwrap_or(&empty);
    let mut lines: Vec<(Vec<V3>, Vec<f64>)> = Vec::new();
    // A strut's ends as the writer writes them (a twig's shaft ends stand off its disks).
    let strut = |i: usize| -> Option<(&ContactTip, StrutEnds)> {
        match (&written.supports[i].base, support_strut_ends(written, i)) {
            (SupportBase::Model(lower), Some(ends)) => Some((lower, ends)),
            _ => None,
        }
    };
    for (i, s) in sups.iter().enumerate() {
        let b = &s["base"];
        let mut line = Vec::new();
        if let Some(p) = b.get("plate") {
            line.push(V3([num(&p[0]), num(&p[1]), ROOT_TOP]));
        } else if let Some(at) = b.get("at") {
            line.push(arr(at));
        } else if s.get("lower").is_some_and(|l| !l.is_null()) {
            line.push(arr(&s["lower"]["socket"]));
        }
        line.extend(pts(&s["joints"]));
        line.push(arr(&s["tip"]["socket"]));
        if let Some((lower, ends)) = strut(i).filter(|e| e.1.twig) {
            line[0] = v(ends.end(lower));
            let last = line.len() - 1;
            line[last] = v(ends.end(&written.supports[i].tip));
        }
        let w = widths(s, num(&s["shaft_d"]), line.len() - 1);
        lines.push((line, w));
    }
    for p in posts {
        let mut line = vec![arr(&p["foot"])];
        line.extend(pts(&p["joints"]));
        line.push(arr(&p["top"]));
        let w = widths(p, num(&p["d"]), line.len() - 1);
        lines.push((line, w));
    }
    for (i, s) in sups.iter().enumerate() {
        let (line, w) = &lines[i];
        m.pillar(line, w, Element::Shaft);
        let b = &s["base"];
        if let Some(p) = b.get("plate") {
            m.root(num(&p[0]), num(&p[1]), num(&b["root_d"]), w[0]);
        } else if let Some(at) = b.get("at") {
            let host = b["parent"].as_u64().unwrap() as usize;
            m.ball(arr(at), host_width(lay, &lines, host, arr(at)) + KNOT_EXTRA_MM);
        }
        let path = &written.supports[i];
        match strut(i) {
            Some((lower, ends)) if ends.twig => {
                let d = f64::from(path.shaft_diameter_mm);
                for tip in [lower, &path.tip] {
                    m.disk(tip, d, f64::from(ends.standoff_mm));
                    m.ball(v(ends.end(tip)), f64::from(ends.joint_diameter_mm));
                }
            }
            Some((lower, _)) => {
                m.tip(&path.tip);
                m.tip(lower);
            }
            None => m.tip(&path.tip),
        }
    }
    for (j, p) in posts.iter().enumerate() {
        let (line, w) = &lines[sups.len() + j];
        m.pillar(line, w, Element::Post);
        m.root(line[0][0], line[0][1], num(&p["root_d"]), w[0]);
    }
    for b in lay["braces"].as_array().unwrap_or(&empty) {
        let d = num(&b["d"]);
        let (lo, up) = (arr(&b["lower"][2]), arr(&b["upper"][2]));
        m.frustum(lo, d, up, d, Element::Brace);
        m.ball(lo, d + KNOT_EXTRA_MM);
        m.ball(up, d + KNOT_EXTRA_MM);
    }
    for k in lay["kickstands"].as_array().unwrap_or(&empty) {
        let mut line = vec![arr(&k["foot"])];
        line.extend(pts(&k["joints"]));
        let knot = arr(&k["knot"]);
        line.push(knot);
        let w = widths(k, num(&k["d"]), line.len() - 1);
        m.pillar(&line, &w, Element::Kickstand);
        m.root(line[0][0], line[0][1], num(&k["root_d"]), w[0]);
        let host = k["support"].as_u64().unwrap() as usize;
        m.ball(knot, host_width(lay, &lines, host, knot) + KNOT_EXTRA_MM);
    }
    m
}
