//! Stages 3 to 6 of the reader: the whole tree classified and laid out.
//!
//! Input: the pieces stage's output (every closed solid with its fit and its connections on the
//! raw mesh) and the touch-rule contacts. Output: a role for every piece, a route for every
//! contact, the exceptions with reasons, and the layout the VOXL writer writes.
//!
//! The owner's rules:
//! - contact: every touch-rule contact is a tip, at the loop centroid, sized from the loop.
//! - route: each tip follows its own trunk down: along its piece to the end, and at each joint the
//!   way that best continues the rod (ways within 25 degrees of straight down first, then the
//!   least turn; never up while anything else goes on), to the plate, a footing on the model, or a
//!   rod an earlier trunk owns (a branch).
//! - brace: a piece no route uses that joins two routed supports, posts or legs.
//! - post or leg: a piece no route uses standing on the plate: a leg where its top leans on a
//!   support, else a post.
//! - micro: a route between two contacts on the model no wider than [`MICRO_D`].
//!
//! An unread piece is a straight stretch between its ends at its width, used like any rod.

mod layout;
mod prepare;
mod rest;
mod route;
mod tips;

use std::collections::BTreeMap;

use serde::Serialize;

use super::contacts::Contact;
use super::math::{P, add, dist, dot, mul, sub};
use super::pieces::{PieceKind, Pieces, Section, SectionKind};
use crate::geometry::support_layout::SupportLayout;
use prepare::RawPiece;

/// DragonFruit's root top: a 0.5 mm disk under a 1.5 mm cone.
pub const ROOT_TOP: f64 = 2.0;
/// A piece reaching this low stands on the plate.
pub const PLATE_Z: f64 = 0.05;
/// A piece on the plate at least this wide is raft.
pub const RAFT_MIN_XY: f64 = 8.0;
/// cos(25 degrees): a way within 25 degrees of straight down is "down".
pub const NEAR_VERTICAL: f64 = 0.906_307_787_036_649_9;
/// A way whose direction's vertical part is within this of level is level.
pub const LEVEL: f64 = 0.15;
/// A strut no wider than this (median, mm) is a micro support.
pub const MICRO_D: f64 = 0.6;
/// The narrowest and widest contacts written.
pub const MIN_CD: f64 = 0.05;
pub const MAX_CD: f64 = 3.0;
/// The share of a circle's area DragonFruit's polygon contact draws: the written contact is the
/// loop's width divided by its square root.
pub const DRAWN_AREA_SHARE: f64 = 0.955;
/// Bends of a support's line are kept only where it leaves the straight run by more than this.
pub const STRAIGHT_MM: f64 = 0.12;
/// A sleeve runs inside a host shaft within 5 degrees of its line ...
pub const SLEEVE_COS: f64 = 0.996_194_698_091_745_5;
/// ... its centre line within this share of the host's radius over the stretch they share ...
pub const SLEEVE_OFF: f64 = 0.3;
/// ... and at least this share of its volume shared with the host.
pub const SLEEVE_SHARE: f64 = 0.6;
/// A blunt tip (no cone) is this long: its length is arbitrary, a short one keeps the shaft on the
/// rod.
pub const BLUNT_MM: f64 = 0.35;
/// A cone that ends where its route does keeps at most this much shaft to hang from the knot.
pub const STUB_MM: f64 = 0.2;
/// Contacts are routed highest first, lowered by this share of their route's detour (its length
/// less the height it drops): the tip that goes most straight down owns a shared trunk.
pub const DETOUR_W: f64 = 0.5;
/// The two foot cones of a vee are at least 8 degrees apart (cos 8 degrees).
pub const VEE_MIN_COS: f64 = 0.990_268_068_741_570_3;
/// Two foot cones in one loop are a vee unless their lines cross this far past the contact (a hub
/// inside the model).
pub const VEE_HUB_MM: f64 = 0.25;
/// Stepped tapers: a second width needs this gain in ideal overlap on a taper section ...
pub const STEP_GAIN2: f64 = 0.08;
/// ... a third this much more.
pub const STEP_GAIN3: f64 = 0.045;
/// Only a taper section at least this long on the written shaft earns steps.
pub const STEP_MIN_MM: f64 = 4.0;
/// A cut within this of a joint or knot already on the shaft moves onto it.
pub const STEP_SNAP_MM: f64 = 1.5;
/// The shaft's radius is sampled this often for the steps.
pub const STEP_SAMPLE_MM: f64 = 0.2;
/// Narrowing: a shaft thicker than this ...
pub const NARROW_MIN_D: f64 = 1.0;
/// ... whose round build would come within this of a model face is written narrower ...
pub const NARROW_CLEAR_MM: f64 = 0.02;
/// ... by at most this (inside the synthetic suite's 0.15 mm width tolerance) ...
pub const NARROW_MAX_MM: f64 = 0.14;
/// ... and only where the round build reaches at most this far into the model (a polygon's flat
/// beside it: the sagitta of a 12-sided 3 mm shaft is 0.051 mm, of an 8-sided 1.5 mm one 0.057).
pub const NARROW_INSIDE_MM: f64 = 0.08;
/// The socket's joint ball clears the model's surface by this much ...
pub const SOCKET_CLEAR_MM: f64 = 0.05;
/// ... where the shaft leaves the tip within 30 degrees of the normal (cos 30 degrees) ...
pub const SOCKET_SQUARE_COS: f64 = 0.866_025_403_784_438_6;
/// ... where moving the socket by at most this much clears it (a longer move made a real tip clip
/// the model elsewhere).
pub const SOCKET_MOVE_MM: f64 = 0.3;

/// What a support, post or leg piece belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Owner {
    Support(usize),
    Post(usize),
    Kick(usize),
}

impl Owner {
    pub fn support(self) -> Option<usize> {
        match self {
            Owner::Support(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Tip,
    Trunk,
    Branch,
    Strut,
    Micro,
    Post,
    Leg,
    Brace,
    Root,
    Sleeve,
    Knot,
    Raft,
    Loose,
    Floating,
    BraceOnBrace,
}

/// A join of one piece to another, at `t` along the first.
#[derive(Debug, Clone, Copy)]
pub struct Att {
    pub t: f64,
    pub q: usize,
    pub at: P,
    /// A near miss rather than an overlap or abutment.
    pub weak: bool,
}

/// A piece as classified.
#[derive(Debug, Clone)]
pub struct Piece {
    pub i: usize,
    pub kind: PieceKind,
    pub a: P,
    pub b: P,
    pub d: P,
    pub l: f64,
    pub width: f64,
    pub secs: Vec<Section>,
    pub lo: P,
    pub hi: P,
    pub vol: f64,
    pub ball: bool,
    pub r: f64,
    pub raft: bool,
    pub pad: bool,
    pub plate: bool,
    pub on_raft: bool,
    pub att: Vec<Att>,
    /// Stretches (t0, t1) routes, posts and legs own.
    pub claims: Vec<(f64, f64, Owner)>,
    pub role: Option<Role>,
    pub why: String,
    pub owner: Option<Owner>,
    pub sleeve: Option<usize>,
}

impl Piece {
    fn new(p: &RawPiece) -> Self {
        let (kind, sections) = if p.kind == PieceKind::Bar && p.tris > 64 {
            // A prism test passed by something with far too many faces for a bar (a unioned
            // lattice): unread.
            (PieceKind::Unread, Vec::new())
        } else {
            (p.kind, p.sections.clone())
        };
        let l = dist(p.a, p.b);
        let d = if l > 1e-9 { super::math::div(sub(p.b, p.a), l) } else { [0.0, 0.0, 1.0] };
        let secs = prepare::smooth_secs(&sections);
        // The longest rod section's radius, else half the width.
        let mut r = p.width / 2.0;
        let mut best = f64::NEG_INFINITY;
        for s in secs.iter().filter(|s| s.k == SectionKind::Rod) {
            if s.t1 - s.t0 > best {
                best = s.t1 - s.t0;
                r = 0.5 * (s.r0 + s.r1);
            }
        }
        Piece {
            i: p.id,
            kind,
            a: p.a,
            b: p.b,
            d,
            l,
            width: p.width,
            secs,
            lo: p.lo,
            hi: p.hi,
            vol: p.ray_volume.max(p.volume.abs()),
            ball: p.kind == PieceKind::Ball,
            r,
            raft: false,
            pad: false,
            plate: false,
            on_raft: false,
            att: Vec::new(),
            claims: Vec::new(),
            role: None,
            why: String::new(),
            owner: None,
            sleeve: None,
        }
    }

    pub fn at(&self, t: f64) -> P {
        add(self.a, mul(self.d, t))
    }

    pub fn t_of(&self, p: P) -> f64 {
        dot(sub(p, self.a), self.d).clamp(0.0, self.l)
    }

    pub fn r_at(&self, t: f64) -> f64 {
        for s in &self.secs {
            if s.t0 - 1e-6 <= t && t <= s.t1 + 1e-6 && s.k != SectionKind::Ball {
                let w = (t - s.t0) / (s.t1 - s.t0).max(1e-9);
                return s.r0 + (s.r1 - s.r0) * w;
            }
        }
        self.r
    }

    pub fn mid(&self) -> P {
        mul(add(self.a, self.b), 0.5)
    }
}

/// Distance from `x` to the segment from `a` to `b`, and the parameter along it.
pub(crate) fn seg_dist(a: P, b: P, x: P) -> (f64, f64) {
    let ab = sub(b, a);
    let l2 = dot(ab, ab).max(1e-12);
    let w = (dot(sub(x, a), ab) / l2).clamp(0.0, 1.0);
    (dist(x, add(a, mul(ab, w))), w)
}

/// Closest points of two segments (sampled, then refined around the best sample).
pub(crate) fn seg_closest(a0: P, b0: P, a1: P, b1: P) -> (f64, P, P) {
    let mut best: Option<(f64, f64, f64)> = None;
    for k in 0..41 {
        let u = k as f64 / 40.0;
        let p = add(a0, mul(sub(b0, a0), u));
        let (dd, w) = seg_dist(a1, b1, p);
        if best.is_none_or(|b| dd < b.0) {
            best = Some((dd, u, w));
        }
    }
    let mut best = best.expect("sampled");
    let (lo, hi) = ((best.1 - 0.025).max(0.0), (best.1 + 0.025).min(1.0));
    for k in 0..41 {
        let u = lo + (hi - lo) * k as f64 / 40.0;
        let p = add(a0, mul(sub(b0, a0), u));
        let (dd, w) = seg_dist(a1, b1, p);
        if dd < best.0 {
            best = (dd, u, w);
        }
    }
    (best.0, add(a0, mul(sub(b0, a0), best.1)), add(a1, mul(sub(b1, a1), best.2)))
}

/// How a route ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum End {
    Plate,
    /// On a stretch an earlier support, post or leg owns, at the point.
    Branch(Owner, P),
    /// On another contact: the route is a strut and that contact its lower tip.
    Foot(usize),
}

/// One stretch of a route: piece, from t0 to t1 along it.
pub type Step = (usize, f64, f64);

#[derive(Debug, Clone)]
pub struct Route {
    pub contact: usize,
    pub path: Vec<Step>,
    pub end: End,
    /// The pass it was found in: 0 strict, 1 with near misses, 2 also going up.
    pub pass: usize,
    pub foot_like: bool,
    pub role: Role,
    pub tip_len: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exception {
    pub kind: &'static str,
    pub at: P,
    pub why: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub piece: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
}

/// The fate of each contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContactState {
    /// The tip of a route.
    Routed,
    /// The lower tip of a strut.
    Foot,
    /// A routed support runs on through the model there.
    Through,
    Unrouted,
}

/// Counts of what the classifier did, for checks and logs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Check {
    pub pieces: usize,
    pub unread: usize,
    pub contacts: usize,
    pub contacts_routed: usize,
    pub contacts_unrouted: usize,
    pub contacts_through: usize,
    pub fallback_weak: usize,
    pub fallback_up: usize,
    pub raft_top: f64,
    pub sleeves_inside: usize,
    pub width_steps: usize,
    pub narrowed: usize,
    pub ball_seats_depth: usize,
    pub ball_seats_fallback: usize,
    pub supports: usize,
    pub posts: usize,
    pub braces: usize,
    pub kickstands: usize,
}

/// A piece's role, as read.
#[derive(Debug, Clone, Serialize)]
pub struct PieceRole {
    pub id: usize,
    pub role: Role,
    pub kind: PieceKind,
    pub owner: Option<usize>,
    pub why: String,
}

/// The classification: what every piece and contact is, and the layout to write.
#[derive(Debug, Clone, Serialize)]
pub struct Classification {
    pub check: Check,
    pub exceptions: Vec<Exception>,
    pub raft_top: f64,
    pub pieces: Vec<PieceRole>,
    pub contacts: Vec<ContactState>,
    #[serde(skip)]
    pub layout: SupportLayout,
    /// The layout as the prototype's JSON (for comparisons and the synthetic suite).
    #[serde(skip)]
    pub layout_json: serde_json::Value,
    /// Per piece: its axis, sections and role, for the synthetic suite's reading check.
    #[serde(skip)]
    pub classified: Vec<Piece>,
    #[serde(skip)]
    pub routes: Vec<Route>,
    /// Per route: contact, how it ends, its role and the pass that found it.
    pub supports: Vec<RouteSummary>,
}

/// A route as reported.
#[derive(Debug, Clone, Serialize)]
pub struct RouteSummary {
    pub contact: usize,
    pub end: &'static str,
    pub role: Role,
    pub pass: usize,
    pub line: Vec<P>,
    pub path: Vec<Step>,
}

/// The classifier's working state.
pub(crate) struct Reader<'a> {
    pub p: Vec<Piece>,
    pub c: &'a [Contact],
    pub model: &'a [[P; 3]],
    pub contact_pieces: Vec<Vec<prepare::Hit>>,
    pub conns: Vec<prepare::RawConn>,
    pub raft_top: f64,
    pub exceptions: Vec<Exception>,
    pub supports: Vec<Route>,
    /// Contact to the support that ends on it as a foot.
    pub consumed: BTreeMap<usize, usize>,
    /// Contact to its piece.
    pub cpiece: BTreeMap<usize, usize>,
    /// (contact, piece) to the contact's place along the piece.
    pub ct: BTreeMap<(usize, usize), f64>,
    /// Piece to the contacts that can end a route on it.
    pub on: BTreeMap<usize, Vec<usize>>,
    /// (contact, piece): the contact lies inside the piece, not only near it.
    pub c_in: BTreeMap<(usize, usize), bool>,
    /// Contact to the pieces it lies inside (parity, or the read profile).
    pub inside_at: BTreeMap<usize, std::collections::BTreeSet<usize>>,
    pub sup_of: BTreeMap<usize, usize>,
    pub through: BTreeMap<usize, usize>,
    pub sleeves: usize,
    pub posts: Vec<rest::Chain>,
    pub kicks: Vec<rest::Kick>,
    pub braces: Vec<rest::Brace>,
    pub root_p: BTreeMap<Owner, Vec<usize>>,
    pub steps: usize,
    pub narrowed: usize,
    pub seats: (usize, usize),
}

impl<'a> Reader<'a> {
    pub fn on_of(&self, q: usize) -> &[usize] {
        self.on.get(&q).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// Classifies the pieces of a support mesh holding `model` up at `contacts`, and lays them out.
pub fn classify(pieces: &Pieces, contacts: &[Contact], model: &[[P; 3]]) -> Classification {
    let (p, raft_top, conns, contact_pieces) = prepare::load(pieces, contacts);
    let mut r = Reader {
        p,
        c: contacts,
        model,
        contact_pieces,
        conns,
        raft_top,
        exceptions: Vec::new(),
        supports: Vec::new(),
        consumed: BTreeMap::new(),
        cpiece: BTreeMap::new(),
        ct: BTreeMap::new(),
        on: BTreeMap::new(),
        c_in: BTreeMap::new(),
        inside_at: BTreeMap::new(),
        sup_of: BTreeMap::new(),
        through: BTreeMap::new(),
        sleeves: 0,
        posts: Vec::new(),
        kicks: Vec::new(),
        braces: Vec::new(),
        root_p: BTreeMap::new(),
        steps: 0,
        narrowed: 0,
        seats: (0, 0),
    };
    r.prepare();
    r.route();
    r.rest();
    r.roles();
    let lay = r.export();
    let mut lay = lay;
    r.ball_seats(&mut lay);
    let layout = layout::to_support_layout(&lay, model);
    let layout_json = layout::to_json(&lay);
    let routed: std::collections::BTreeSet<usize> = r.sup_of.keys().chain(r.consumed.keys()).copied().collect();
    let thr = r.through.keys().filter(|c| !routed.contains(c)).count();
    let check = Check {
        pieces: r.p.len(),
        unread: r.p.iter().filter(|p| p.kind == PieceKind::Unread && !p.raft).count(),
        contacts: contacts.len(),
        contacts_routed: routed.len(),
        contacts_unrouted: contacts.len() - routed.len() - thr,
        contacts_through: thr,
        fallback_weak: r.supports.iter().filter(|s| s.pass == 1 && !s.foot_like).count(),
        fallback_up: r.supports.iter().filter(|s| s.pass == 2 && !s.foot_like).count(),
        raft_top,
        sleeves_inside: r.sleeves,
        width_steps: r.steps,
        narrowed: r.narrowed,
        ball_seats_depth: r.seats.0,
        ball_seats_fallback: r.seats.1,
        supports: lay.supports.len(),
        posts: lay.posts.len(),
        braces: lay.braces.len(),
        kickstands: lay.kicks.len(),
    };
    let states = (0..contacts.len())
        .map(|ci| {
            if r.sup_of.contains_key(&ci) {
                ContactState::Routed
            } else if r.consumed.contains_key(&ci) {
                ContactState::Foot
            } else if r.through.contains_key(&ci) {
                ContactState::Through
            } else {
                ContactState::Unrouted
            }
        })
        .collect();
    let pieces_out =
        r.p.iter()
            .map(|p| PieceRole {
                id: p.i,
                role: p.role.unwrap_or(Role::Floating),
                kind: p.kind,
                owner: p.owner.and_then(Owner::support),
                why: p.why.clone(),
            })
            .collect();
    let summaries: Vec<RouteSummary> = r
        .supports
        .iter()
        .map(|s| RouteSummary {
            contact: s.contact,
            end: match s.end {
                End::Plate => "plate",
                End::Branch(..) => "branch",
                End::Foot(_) => "foot",
            },
            role: s.role,
            pass: s.pass,
            line: r.line_of(&s.path),
            path: s.path.clone(),
        })
        .collect();
    Classification {
        check,
        exceptions: r.exceptions,
        raft_top,
        pieces: pieces_out,
        contacts: states,
        layout,
        layout_json,
        supports: summaries,
        classified: r.p,
        routes: r.supports,
    }
}

#[cfg(test)]
mod tests {
    use super::super::contacts::tests::{boxm, frustum};
    use super::super::math::P;
    use super::super::read_points;
    use super::*;
    use crate::geometry::support_layout::SupportBase;

    /// A tip cone from `socket_z` up into the model's underside at `contact` z, on a rod down to
    /// the plate.
    fn trunk(x: f64, y: f64, contact: f64, socket: f64, rod_r: f64) -> Vec<[P; 3]> {
        let mut s = frustum([x, y, socket], [x, y, contact + 0.2], rod_r, 0.12, 16);
        s.extend(frustum([x, y, 0.0], [x, y, socket + 0.1], rod_r, rod_r, 16));
        s
    }

    fn slab(z: f64) -> Vec<[P; 3]> {
        boxm([-10.0, -10.0, z], [10.0, 10.0, z + 5.0])
    }

    #[test]
    fn a_tip_on_a_rod_to_the_plate_is_one_trunk_at_its_contact() {
        let model = slab(10.0);
        let r = read_points(&model, &trunk(1.0, 2.0, 10.0, 8.0, 0.4), 3.0);
        let lay = &r.classification.layout;
        assert_eq!(lay.supports.len(), 1, "{:?}", r.classification.exceptions);
        let s = &lay.supports[0];
        assert!(matches!(s.base, SupportBase::Plate { .. }));
        assert!((s.tip.contact.z - 10.0).abs() < 0.05 && (s.tip.contact.x - 1.0).abs() < 0.05, "{:?}", s.tip.contact);
        assert!((s.shaft_diameter_mm - 0.8).abs() < 0.05, "{}", s.shaft_diameter_mm);
        assert!(s.tip.surface_normal.z < -0.99, "the model's face under the contact faces down");
        assert_eq!(r.classification.contacts, vec![ContactState::Routed]);
        let roles: Vec<Role> = r.classification.pieces.iter().map(|p| p.role).collect();
        assert!(roles.contains(&Role::Tip) && roles.contains(&Role::Trunk), "{roles:?}");
    }

    #[test]
    fn a_second_tip_on_the_same_rod_branches_off_the_trunk() {
        let model = slab(10.0);
        let mut s = trunk(0.0, 0.0, 10.0, 8.0, 0.4);
        // A leaning rod from a second contact down onto the first trunk's shaft.
        s.extend(frustum([3.0, 0.0, 8.0], [3.0, 0.0, 10.2], 0.3, 0.12, 16));
        s.extend(frustum([0.1, 0.0, 5.0], [3.0, 0.0, 8.1], 0.3, 0.3, 16));
        let lay = read_points(&model, &s, 3.0).classification.layout;
        assert_eq!(lay.supports.len(), 2);
        let plate = lay.supports.iter().filter(|s| matches!(s.base, SupportBase::Plate { .. })).count();
        let branch = lay.supports.iter().filter(|s| matches!(s.base, SupportBase::Shaft { parent: 0, .. })).count();
        assert_eq!((plate, branch), (1, 1));
    }

    #[test]
    fn a_rod_between_two_contacts_is_a_strut_with_a_lower_tip() {
        // A shelf over a block: a rod with tips at both ends between them.
        let mut model = boxm([-10.0, -10.0, 20.0], [10.0, 10.0, 25.0]);
        model.extend(boxm([-10.0, -10.0, 0.0], [10.0, 10.0, 10.0]));
        let mut s = frustum([0.0, 0.0, 12.0], [0.0, 0.0, 20.2], 0.4, 0.12, 16);
        s.extend(frustum([0.0, 0.0, 9.8], [0.0, 0.0, 12.1], 0.12, 0.4, 16));
        let r = read_points(&model, &s, 3.0);
        let lay = &r.classification.layout;
        assert_eq!(lay.supports.len(), 1, "{:?}", r.classification.exceptions);
        let SupportBase::Model(lower) = &lay.supports[0].base else { panic!("{:?}", lay.supports[0].base) };
        assert!((lower.contact.z - 10.0).abs() < 0.05);
        assert!((lay.supports[0].tip.contact.z - 20.0).abs() < 0.05);
        assert!(lay.sticks.contains(&0), "cone tips at both ends make a stick");
    }

    #[test]
    fn a_rod_joining_two_trunks_is_a_brace() {
        let model = slab(10.0);
        let mut s = trunk(-3.0, 0.0, 10.0, 8.0, 0.4);
        s.extend(trunk(3.0, 0.0, 10.0, 8.0, 0.4));
        s.extend(frustum([-3.0, 0.0, 4.0], [3.0, 0.0, 6.0], 0.25, 0.25, 12));
        let lay = read_points(&model, &s, 3.0).classification.layout;
        assert_eq!(lay.supports.len(), 2);
        assert_eq!(lay.braces.len(), 1);
        let b = &lay.braces[0];
        assert_ne!(b.lower.support, b.upper.support);
        assert!((b.diameter_mm - 0.5).abs() < 0.05, "{}", b.diameter_mm);
        assert!(b.lower.point.z < b.upper.point.z);
    }

    #[test]
    fn a_rod_on_the_plate_touching_no_model_held_by_a_brace_is_a_post() {
        let model = slab(10.0);
        let mut s = trunk(-3.0, 0.0, 10.0, 8.0, 0.4);
        s.extend(frustum([3.0, 0.0, 0.0], [3.0, 0.0, 7.0], 0.4, 0.4, 16));
        s.extend(frustum([-3.0, 0.0, 4.0], [3.0, 0.0, 6.0], 0.25, 0.25, 12));
        let r = read_points(&model, &s, 3.0);
        let lay = &r.classification.layout;
        assert_eq!(
            (lay.supports.len(), lay.posts.len(), lay.braces.len()),
            (1, 1, 1),
            "{:?}",
            r.classification.exceptions
        );
        // The brace's end on the post names it by its index past the supports.
        let ends = [lay.braces[0].lower.support, lay.braces[0].upper.support];
        assert!(ends.contains(&1));
    }

    #[test]
    fn a_strongly_tapered_trunk_is_written_in_width_steps() {
        let model = slab(30.0);
        let mut s = frustum([0.0, 0.0, 28.0], [0.0, 0.0, 30.2], 0.6, 0.12, 24);
        // A shaft narrowing from 3 mm at the plate to 1.2 mm under the tip.
        s.extend(frustum([0.0, 0.0, 0.0], [0.0, 0.0, 28.1], 1.5, 0.6, 24));
        let r = read_points(&model, &s, 3.0);
        let lay = &r.classification.layout;
        assert_eq!(lay.supports.len(), 1);
        let steps = lay.segment_diameters.get(&0).expect("stepped widths");
        assert!(steps.len() >= 2, "{steps:?}");
        assert!(steps.windows(2).all(|w| w[0] > w[1]), "widths step down toward the tip: {steps:?}");
        assert_eq!(steps.len(), lay.supports[0].joints.len() + 1);
        assert!(r.classification.check.width_steps >= 1);
    }

    #[test]
    fn a_contact_with_no_route_down_is_an_exception() {
        // A cone stub in the model with nothing below it.
        let model = slab(10.0);
        let s = frustum([0.0, 0.0, 9.0], [0.0, 0.0, 10.2], 0.4, 0.12, 16);
        let r = read_points(&model, &s, 3.0);
        assert!(r.classification.layout.supports.is_empty());
        assert_eq!(r.classification.contacts, vec![ContactState::Unrouted]);
        assert!(
            r.classification.exceptions.iter().any(|e| e.why.starts_with("no route")),
            "{:?}",
            r.classification.exceptions
        );
    }

    #[test]
    fn a_ball_sunk_at_a_tip_is_part_of_the_tip_and_carries_sphere_tip_data() {
        let model = slab(10.0);
        let mut s = trunk(0.0, 0.0, 10.0, 8.0, 0.4);
        // A 0.6 mm ball sunk half way into the model at the cone's end.
        let ball = super::super::contacts::tests::ball([0.0, 0.0, 10.1], 0.3);
        s.extend(ball);
        let r = read_points(&model, &s, 3.0);
        let lay = &r.classification.layout;
        assert_eq!(lay.supports.len(), 1);
        let b = lay.supports[0].tip.ball.expect("a ball tip");
        assert!((b.diameter_mm - 0.6).abs() < 0.05, "{b:?}");
        assert!(r.classification.pieces.iter().filter(|p| p.kind == PieceKind::Ball).all(|p| p.role == Role::Tip));
    }

    #[test]
    fn the_median_is_numpys() {
        assert_eq!(rest::median(vec![3.0, 1.0, 2.0]), 2.0);
        assert_eq!(rest::median(vec![4.0, 1.0, 2.0, 3.0]), 2.5);
    }

    #[test]
    fn segment_distance_and_closest_points() {
        let (d, w) = seg_dist([0.0; 3], [2.0, 0.0, 0.0], [1.0, 1.0, 0.0]);
        assert!((d - 1.0).abs() < 1e-12 && (w - 0.5).abs() < 1e-12);
        let (d, a, b) = seg_closest([0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0]);
        assert!((d - 1.0).abs() < 1e-9);
        assert!((a[0] - 1.0).abs() < 1e-9 && (b[2] - 1.0).abs() < 1e-9);
    }
}
