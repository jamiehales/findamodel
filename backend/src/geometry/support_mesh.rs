//! Meshes the supports a VOXL `SUPP` document describes ([`super::voxl::supports_document`]):
//! findamodel's own picture of what the slicer prints from the document, for the viewer and for
//! comparing with the original supports.
//!
//! Every entity is drawn as the document places it, in the document's frame (millimetres, Z-up),
//! from simple closed solids that may overlap one another:
//! - a root: a solid of revolution about its transform's up axis, a disk `diskHeight` tall at the
//!   root's `diameter`, then a cone `coneHeight` tall narrowing to the width of the shaft it carries
//!   (the shaft starts at the root's top);
//! - a shaft segment (trunk, branch, twig, stick or kickstand): a round frustum of the segment's
//!   `diameter` from its start (the root's top, the parent knot, or its bottom joint) to its top
//!   joint (or a kickstand's host knot);
//! - a joint or knot: a ball of its `diameter` at its `pos`, once however many segments share it.
//!   Positions are drawn as written: the writer projects knots onto their segments itself;
//! - a brace: a frustum of its profile's `diameter` between its two knots;
//! - a contact cone of the `disk` profile: one frustum from `pos` run on into the model by the tip
//!   offset along `surfaceNormal` (`contactDiameterMm` wide) up to its socket joint
//!   (`bodyDiameterMm` wide), with flat ends and no disk (as the writer's contact placement expects);
//! - a contact cone of the `sphere` profile: a ball `sphereRadiusRatio` times the contact radius,
//!   its centre `sphereDepthMm` plus the tip offset below `pos` along the surface normal (without a
//!   depth, seated `sqrt(R^2 - r^2)` outside the surface less the offset, so it crosses the surface
//!   at the contact width), and a frustum from the ball's centre (`contactDiameterMm`) to the socket
//!   (`bodyDiameterMm`);
//! - a twig's contact disk: a cylinder `contactDiameterMm` wide along `surfaceNormal` from the tip
//!   offset below `pos` to `diskLengthOverride` off it, capped by a ball as wide.
//!
//! Round shapes are polygons whose vertices lie on the stated radius, with enough sides that the
//! flats come within [`SAGITTA_MM`] of the round (between [`MIN_SIDES`] and [`MAX_SIDES`]).

use std::collections::{HashMap, HashSet};

use glam::{DQuat, DVec3};
use serde_json::Value;

use super::{Triangle, Vec3};

/// The flats of a polygonal round come at most this far inside the round.
const SAGITTA_MM: f64 = 0.01;
/// Fewest sides of a round; even, so a ball has a ring at its equator.
const MIN_SIDES: usize = 8;
/// Most sides of a round.
const MAX_SIDES: usize = 48;
/// Shorter solids than this are left out (a segment between two joints at one point).
const MIN_LENGTH_MM: f64 = 1e-6;

/// The supports of `document` as triangles (three Z-up vertices each), contact cones and twig disks
/// run `tip_offset_mm` on into the model along the surface normal.
pub fn build(document: &Value, tip_offset_mm: f64) -> Vec<[[f32; 3]; 3]> {
    let mut mesh = Mesh::default();
    Builder::new(document, tip_offset_mm).draw(&mut mesh);
    mesh.triangles.iter().map(|t| t.map(|p| [p.x as f32, p.y as f32, p.z as f32])).collect()
}

/// [`build`] as [`Triangle`]s with face normals, in the same Z-up frame.
pub fn build_triangles(document: &Value, tip_offset_mm: f64) -> Vec<Triangle> {
    build(document, tip_offset_mm)
        .into_iter()
        .map(|[a, b, c]| {
            let (a, b, c) = (Vec3::new(a[0], a[1], a[2]), Vec3::new(b[0], b[1], b[2]), Vec3::new(c[0], c[1], c[2]));
            Triangle::new(a, b, c, (b - a).cross(c - a).normalized())
        })
        .collect()
}

/// Number of sides for a round of radius `r` whose flats stay within [`SAGITTA_MM`].
fn sides_for(r: f64) -> usize {
    if r <= SAGITTA_MM {
        return MIN_SIDES;
    }
    let n = (std::f64::consts::PI / (1.0 - SAGITTA_MM / r).acos()).ceil() as usize;
    (n + n % 2).clamp(MIN_SIDES, MAX_SIDES)
}

#[derive(Default)]
struct Mesh {
    triangles: Vec<[DVec3; 3]>,
}

impl Mesh {
    /// A closed solid of revolution about `axis` from `base`: profile `(t, r)` pairs along the axis,
    /// `r = 0` for a pole. The ends are capped flat where their radius is not 0.
    fn revolve(&mut self, base: DVec3, axis: DVec3, profile: &[(f64, f64)], rotation: Option<DQuat>) {
        let axis = axis.normalize_or_zero();
        if axis == DVec3::ZERO || profile.len() < 2 {
            return;
        }
        let r_max = profile.iter().map(|&(_, r)| r).fold(0.0, f64::max);
        if r_max <= 0.0 {
            return;
        }
        let n = sides_for(r_max);
        let (u, v) = match rotation {
            Some(q) => (q * DVec3::X, q * DVec3::Y),
            None => {
                let u = axis.any_orthonormal_vector();
                (u, axis.cross(u))
            }
        };
        let ring = |t: f64, r: f64| -> Vec<DVec3> {
            let c = base + axis * t;
            if r <= 0.0 {
                return vec![c];
            }
            (0..n)
                .map(|k| {
                    let a = std::f64::consts::TAU * k as f64 / n as f64;
                    c + (u * a.cos() + v * a.sin()) * r
                })
                .collect()
        };
        let rings: Vec<Vec<DVec3>> = profile.iter().map(|&(t, r)| ring(t, r)).collect();
        let first = &rings[0];
        if first.len() > 1 {
            let c = base + axis * profile[0].0;
            for k in 0..n {
                self.triangles.push([c, first[(k + 1) % n], first[k]]);
            }
        }
        let last = &rings[rings.len() - 1];
        if last.len() > 1 {
            let c = base + axis * profile[profile.len() - 1].0;
            for k in 0..n {
                self.triangles.push([c, last[k], last[(k + 1) % n]]);
            }
        }
        for pair in rings.windows(2) {
            let (lo, hi) = (&pair[0], &pair[1]);
            for k in 0..n {
                let j = (k + 1) % n;
                match (lo.len() > 1, hi.len() > 1) {
                    (false, false) => {}
                    (false, true) => self.triangles.push([lo[0], hi[j], hi[k]]),
                    (true, false) => self.triangles.push([lo[k], lo[j], hi[0]]),
                    (true, true) => {
                        self.triangles.push([lo[k], lo[j], hi[j]]);
                        self.triangles.push([lo[k], hi[j], hi[k]]);
                    }
                }
            }
        }
    }

    /// A round frustum from `a` (diameter `da`) to `b` (diameter `db`), flat ends.
    fn frustum(&mut self, a: DVec3, da: f64, b: DVec3, db: f64) {
        let length = (b - a).length();
        if length < MIN_LENGTH_MM || (da <= 0.0 && db <= 0.0) {
            return;
        }
        self.revolve(a, b - a, &[(0.0, da.max(0.0) / 2.0), (length, db.max(0.0) / 2.0)], None);
    }

    /// A ball of diameter `d` centred on `c`.
    fn ball(&mut self, c: DVec3, d: f64) {
        let r = d / 2.0;
        if r <= 0.0 {
            return;
        }
        let bands = sides_for(r) / 2;
        let profile: Vec<(f64, f64)> = (0..=bands)
            .map(|k| {
                let a = std::f64::consts::PI * k as f64 / bands as f64;
                let radius = if k == 0 || k == bands { 0.0 } else { r * a.sin() };
                (r * (1.0 - a.cos()), radius)
            })
            .collect();
        self.revolve(c - DVec3::Z * r, DVec3::Z, &profile, None);
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn point(v: &Value) -> Option<DVec3> {
    Some(DVec3::new(num(&v["x"])?, num(&v["y"])?, num(&v["z"])?))
}

fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// A root as the document gives it.
struct Root {
    pos: DVec3,
    rotation: DQuat,
    diameter: f64,
    disk_height: f64,
    cone_height: f64,
}

impl Root {
    fn read(v: &Value) -> Option<Root> {
        let transform = &v["transform"];
        let rot = &transform["rot"];
        let rotation = match (num(&rot["x"]), num(&rot["y"]), num(&rot["z"]), num(&rot["w"])) {
            (Some(x), Some(y), Some(z), Some(w)) => DQuat::from_xyzw(x, y, z, w).normalize(),
            _ => DQuat::IDENTITY,
        };
        let rotation = if rotation.is_finite() { rotation } else { DQuat::IDENTITY };
        Some(Root {
            pos: point(&transform["pos"])?,
            rotation,
            diameter: num(&v["diameter"])?,
            disk_height: num(&v["diskHeight"]).unwrap_or(0.0),
            cone_height: num(&v["coneHeight"]).unwrap_or(0.0),
        })
    }

    fn up(&self) -> DVec3 {
        self.rotation * DVec3::Z
    }

    /// Where the shaft leaves the root.
    fn top(&self) -> DVec3 {
        self.pos + self.up() * (self.disk_height + self.cone_height)
    }
}

/// A joint or knot: a ball at `pos`.
#[derive(Clone, Copy)]
struct Ball {
    pos: DVec3,
    diameter: f64,
}

impl Ball {
    fn read(v: &Value) -> Option<Ball> {
        Some(Ball { pos: point(&v["pos"])?, diameter: num(&v["diameter"]).unwrap_or(0.0) })
    }
}

struct Builder<'a> {
    document: &'a Value,
    offset: f64,
    roots: HashMap<&'a str, Root>,
    knots: HashMap<&'a str, Ball>,
    joints: HashMap<&'a str, Ball>,
}

impl<'a> Builder<'a> {
    fn new(document: &'a Value, offset: f64) -> Builder<'a> {
        let mut roots = HashMap::new();
        let mut knots = HashMap::new();
        let mut joints = HashMap::new();
        let mut add_root = |v: &'a Value| {
            if let (Some(id), Some(root)) = (v["id"].as_str(), Root::read(v)) {
                roots.insert(id, root);
            }
        };
        array(&document["roots"]).iter().for_each(&mut add_root);
        array(&document["kickstands"]).iter().for_each(|k| add_root(&k["root"]));
        let mut add_knot = |v: &'a Value| {
            if let (Some(id), Some(ball)) = (v["id"].as_str(), Ball::read(v)) {
                knots.insert(id, ball);
            }
        };
        array(&document["knots"]).iter().for_each(&mut add_knot);
        array(&document["kickstands"]).iter().for_each(|k| add_knot(&k["hostKnot"]));
        for entity in Self::shafted(document) {
            for segment in array(&entity["segments"]) {
                for end in [&segment["bottomJoint"], &segment["topJoint"]] {
                    if let (Some(id), Some(ball)) = (end["id"].as_str(), Ball::read(end)) {
                        joints.entry(id).or_insert(ball);
                    }
                }
            }
        }
        Builder { document, offset, roots, knots, joints }
    }

    /// Every entity with segments: trunks, branches, twigs, sticks and kickstands.
    fn shafted(document: &Value) -> impl Iterator<Item = &Value> {
        ["trunks", "branches", "twigs", "sticks"]
            .into_iter()
            .flat_map(|key| array(&document[key]).iter())
            .chain(array(&document["kickstands"]).iter().map(|k| &k["kickstand"]))
    }

    fn draw(&self, mesh: &mut Mesh) {
        let doc = self.document;
        for trunk in array(&doc["trunks"]) {
            let root = trunk["rootId"].as_str().and_then(|id| self.roots.get(id));
            if let Some(root) = root {
                let shaft = num(&trunk["baseDiameterMm"]).or_else(|| first_diameter(trunk)).unwrap_or(0.0);
                self.root(mesh, root, shaft);
            }
            self.segments(mesh, trunk, root.map(Root::top), None);
            self.contact(mesh, &trunk["contactCone"]);
        }
        for branch in array(&doc["branches"]) {
            let start = branch["parentKnotId"].as_str().and_then(|id| self.knots.get(id)).map(|k| k.pos);
            self.segments(mesh, branch, start, None);
            self.contact(mesh, &branch["contactCone"]);
        }
        for twig in array(&doc["twigs"]) {
            self.segments(mesh, twig, None, None);
            for key in ["contactDiskA", "contactDiskB"] {
                self.disk(mesh, &twig[key]);
            }
        }
        for stick in array(&doc["sticks"]) {
            self.segments(mesh, stick, None, None);
            for key in ["contactConeA", "contactConeB"] {
                self.contact(mesh, &stick[key]);
            }
        }
        for bundle in array(&doc["kickstands"]) {
            let kickstand = &bundle["kickstand"];
            let root = kickstand["rootId"].as_str().and_then(|id| self.roots.get(id));
            if let Some(root) = root {
                let shaft =
                    first_diameter(kickstand).or_else(|| num(&kickstand["profile"]["bodyDiameterMm"])).unwrap_or(0.0);
                self.root(mesh, root, shaft);
            }
            let host = kickstand["hostKnotId"].as_str().and_then(|id| self.knots.get(id)).map(|k| k.pos);
            self.segments(mesh, kickstand, root.map(Root::top), host);
        }
        for brace in array(&doc["braces"]) {
            let end = |key: &str| brace[key].as_str().and_then(|id| self.knots.get(id)).map(|k| k.pos);
            if let (Some(a), Some(b), Some(d)) =
                (end("startKnotId"), end("endKnotId"), num(&brace["profile"]["diameter"]))
            {
                mesh.frustum(a, d, b, d);
            }
        }

        // Every joint and knot once, however many segments, braces or branches share it.
        let mut drawn = HashSet::new();
        for entity in Self::shafted(doc) {
            for segment in array(&entity["segments"]) {
                for end in [&segment["bottomJoint"], &segment["topJoint"]] {
                    if let Some(id) = end["id"].as_str()
                        && drawn.insert(id)
                        && let Some(joint) = self.joints.get(id)
                    {
                        mesh.ball(joint.pos, joint.diameter);
                    }
                }
            }
        }
        let knots = array(&doc["knots"]).iter().chain(array(&doc["kickstands"]).iter().map(|k| &k["hostKnot"]));
        for knot in knots {
            if let Some(id) = knot["id"].as_str()
                && drawn.insert(id)
                && let Some(ball) = self.knots.get(id)
            {
                mesh.ball(ball.pos, ball.diameter);
            }
        }
    }

    /// A root: a disk at its diameter, then a cone up to its top at the shaft's width.
    fn root(&self, mesh: &mut Mesh, root: &Root, shaft: f64) {
        let r = root.diameter / 2.0;
        let (disk, top) = (root.disk_height, root.disk_height + root.cone_height);
        let mut profile = vec![(0.0, r)];
        if disk > MIN_LENGTH_MM {
            profile.push((disk, r));
        }
        if top - disk > MIN_LENGTH_MM {
            profile.push((top, shaft / 2.0));
        }
        mesh.revolve(root.pos, root.up(), &profile, Some(root.rotation));
    }

    /// An entity's segments, each from its start (`start` for the first, else its bottom joint) to
    /// its top joint (`end` for a last segment without one).
    fn segments(&self, mesh: &mut Mesh, entity: &Value, start: Option<DVec3>, end: Option<DVec3>) {
        let segments = array(&entity["segments"]);
        let joint = |v: &Value| v["id"].as_str().and_then(|id| self.joints.get(id)).map(|j| j.pos);
        for (i, segment) in segments.iter().enumerate() {
            let from = joint(&segment["bottomJoint"]).or(if i == 0 { start } else { None });
            let to = joint(&segment["topJoint"]).or(if i + 1 == segments.len() { end } else { None });
            if let (Some(a), Some(b), Some(d)) = (from, to, num(&segment["diameter"])) {
                mesh.frustum(a, d, b, d);
            }
        }
    }

    /// A contact cone: a frustum from the contact (run into the model by the tip offset) to its
    /// socket; a sphere tip's ball and the frustum from the ball's centre.
    fn contact(&self, mesh: &mut Mesh, cone: &Value) {
        let (Some(pos), Some(normal)) = (point(&cone["pos"]), point(&cone["surfaceNormal"])) else { return };
        let n = normal.normalize_or(DVec3::Z);
        let profile = &cone["profile"];
        let (Some(contact), Some(body)) = (num(&profile["contactDiameterMm"]), num(&profile["bodyDiameterMm"])) else {
            return;
        };
        let socket = cone["socketJointId"].as_str().and_then(|id| self.joints.get(id)).map(|j| j.pos).or_else(|| {
            let axis = point(&cone["normal"])?.normalize_or(n);
            let disk = num(&cone["diskLengthOverride"]).unwrap_or(0.0);
            Some(pos + n * disk + axis * num(&profile["lengthMm"])?)
        });
        let Some(socket) = socket else { return };
        if profile["type"].as_str() == Some("sphere") {
            let small = contact / 2.0;
            let big = num(&profile["sphereRadiusRatio"]).unwrap_or(1.0) * small;
            let lift = match num(&profile["sphereDepthMm"]) {
                Some(depth) => -(depth + self.offset),
                None => (big * big - small * small).max(0.0).sqrt() - self.offset,
            };
            let centre = pos + n * lift;
            mesh.ball(centre, 2.0 * big);
            mesh.frustum(centre, contact, socket, body);
        } else {
            mesh.frustum(pos - n * self.offset, contact, socket, body);
        }
    }

    /// A twig's contact disk: a cylinder along the surface normal from the tip offset below the
    /// contact to the disk's length off it, capped by a ball as wide.
    fn disk(&self, mesh: &mut Mesh, disk: &Value) {
        let (Some(pos), Some(normal), Some(d)) =
            (point(&disk["pos"]), point(&disk["surfaceNormal"]), num(&disk["contactDiameterMm"]))
        else {
            return;
        };
        let n = normal.normalize_or(DVec3::Z);
        let length =
            num(&disk["diskLengthOverride"]).or_else(|| num(&disk["profile"]["diskThicknessMm"])).unwrap_or(0.0);
        let top = pos + n * length;
        mesh.frustum(pos - n * self.offset, d, top, d);
        mesh.ball(top, d);
    }
}

/// The diameter of an entity's first segment.
fn first_diameter(entity: &Value) -> Option<f64> {
    array(&entity["segments"]).first().and_then(|s| num(&s["diameter"]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const OFFSET: f64 = 0.05;

    fn p(x: f64, y: f64, z: f64) -> Value {
        json!({ "x": x, "y": y, "z": z })
    }

    fn doc(extra: Value) -> Value {
        let mut d = json!({
            "version": 1, "roots": [], "trunks": [], "branches": [], "leaves": [], "twigs": [],
            "sticks": [], "braces": [], "knots": [], "kickstands": [],
        });
        for (k, v) in extra.as_object().unwrap() {
            d[k] = v.clone();
        }
        d
    }

    fn mesh(document: &Value) -> Vec<[DVec3; 3]> {
        build(document, OFFSET).iter().map(|t| t.map(|p| DVec3::new(p[0].into(), p[1].into(), p[2].into()))).collect()
    }

    /// Every edge is used once in each direction: a closed, consistently wound surface.
    fn assert_closed(tris: &[[DVec3; 3]]) {
        let key = |p: DVec3| [p.x, p.y, p.z].map(|c| (c * 1e4).round() as i64);
        let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
        for t in tris {
            for k in 0..3 {
                let (a, b) = (key(t[k]), key(t[(k + 1) % 3]));
                *edges.entry((a, b)).or_default() += 1;
                *edges.entry((b, a)).or_default() -= 1;
            }
        }
        assert!(edges.values().all(|&c| c == 0), "open or inconsistently wound");
    }

    /// Signed volume (positive for outward winding).
    fn volume(tris: &[[DVec3; 3]]) -> f64 {
        tris.iter().map(|t| t[0].dot(t[1].cross(t[2])) / 6.0).sum()
    }

    fn bounds(tris: &[[DVec3; 3]]) -> (DVec3, DVec3) {
        tris.iter().flatten().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), &p| (lo.min(p), hi.max(p)))
    }

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// The volume of a polygonal frustum of `n` sides inscribed in radii `ra`, `rb` over `h`.
    fn polygon_frustum(n: usize, ra: f64, rb: f64, h: f64) -> f64 {
        let k = n as f64 * (std::f64::consts::TAU / n as f64).sin() / 2.0;
        k * h * (ra * ra + ra * rb + rb * rb) / 3.0
    }

    fn joint(id: &str, at: Value, d: f64) -> Value {
        json!({ "id": id, "pos": at, "diameter": d })
    }

    fn root(id: &str, x: f64, y: f64, d: f64) -> Value {
        json!({ "id": id, "transform": { "pos": p(x, y, 0.0), "rot": { "x": 0, "y": 0, "z": 0, "w": 1 } },
                "diameter": d, "diskHeight": 0.5, "coneHeight": 1.5 })
    }

    #[test]
    fn a_frustum_is_closed_and_has_the_stated_radii() {
        let mut m = Mesh::default();
        m.frustum(DVec3::ZERO, 1.0, DVec3::new(0.0, 0.0, 3.0), 0.4);
        assert_closed(&m.triangles);
        let n = sides_for(0.5);
        assert!(near(volume(&m.triangles), polygon_frustum(n, 0.5, 0.2, 3.0), 1e-9));
        for p in m.triangles.iter().flatten() {
            let r = p.truncate().length();
            if near(p.z, 0.0, 1e-9) {
                assert!(r < 1e-9 || near(r, 0.5, 1e-9), "{r}");
            } else {
                assert!(r < 1e-9 || near(r, 0.2, 1e-9), "{r}");
            }
        }
    }

    #[test]
    fn a_ball_is_closed_and_round() {
        let mut m = Mesh::default();
        m.ball(DVec3::new(1.0, 2.0, 3.0), 0.8);
        assert_closed(&m.triangles);
        for p in m.triangles.iter().flatten() {
            assert!(near((*p - DVec3::new(1.0, 2.0, 3.0)).length(), 0.4, 1e-9));
        }
        let v = volume(&m.triangles);
        let true_v = 4.0 / 3.0 * std::f64::consts::PI * 0.4f64.powi(3);
        assert!(v > 0.9 * true_v && v <= true_v, "{v} vs {true_v}");
    }

    #[test]
    fn sides_keep_the_flats_close_to_the_round() {
        for r in [0.05, 0.2, 0.5, 1.0, 3.0] {
            let n = sides_for(r);
            assert!(n.is_multiple_of(2) && (MIN_SIDES..=MAX_SIDES).contains(&n));
            let sag = r * (1.0 - (std::f64::consts::PI / n as f64).cos());
            assert!(sag <= SAGITTA_MM + 1e-12 || n == MAX_SIDES, "{r}: {sag}");
        }
    }

    #[test]
    fn a_trunk_stands_on_its_root_and_reaches_its_contact() {
        let top = joint("j1", p(1.0, 2.0, 8.0), 0.6);
        let d = doc(json!({
            "roots": [root("r", 1.0, 2.0, 2.4)],
            "trunks": [{
                "id": "t", "typeId": "trunk", "rootId": "r", "baseDiameterMm": 0.5,
                "segments": [{ "id": "s0", "diameter": 0.5, "topJoint": top }],
                "contactCone": {
                    "pos": p(1.0, 2.0, 10.0), "normal": p(0.0, 0.0, 1.0), "surfaceNormal": p(0.0, 0.0, -1.0),
                    "diskLengthOverride": 0.1,
                    "profile": { "type": "disk", "contactDiameterMm": 0.3, "bodyDiameterMm": 0.5, "lengthMm": 1.9 },
                    "socketJointId": "j1",
                },
            }],
        }));
        let tris = mesh(&d);
        let (lo, hi) = bounds(&tris);
        // The root's disk on the plate, 2.4 wide.
        assert!(near(lo.z, 0.0, 1e-6) && near(hi.x - lo.x, 2.4, 1e-6), "{lo} {hi}");
        // The cone runs the tip offset on past the contact.
        assert!(near(hi.z, 10.0 + OFFSET, 1e-5), "{hi}");

        // Each piece on its own: root, shaft, cone, socket ball; each closed.
        let mut m = Mesh::default();
        let b = Builder::new(&d, OFFSET);
        b.root(&mut m, &b.roots["r"], 0.5);
        assert_closed(&m.triangles);
        let (lo, hi) = bounds(&m.triangles);
        assert!(near(hi.z, 2.0, 1e-9) && near(lo.z, 0.0, 1e-9));
        // The cone's top ring is the shaft's width.
        let top_ring: Vec<_> = m.triangles.iter().flatten().filter(|p| near(p.z, 2.0, 1e-9)).collect();
        assert!(top_ring.iter().all(|p| {
            let r = (p.truncate() - glam::DVec2::new(1.0, 2.0)).length();
            r < 1e-9 || near(r, 0.25, 1e-9)
        }));
        assert_closed(&tris);
    }

    #[test]
    fn a_shared_joint_is_drawn_once() {
        let j = |id: &str, z: f64| joint(id, p(0.0, 0.0, z), 0.6);
        let d = doc(json!({
            "roots": [root("r", 0.0, 0.0, 2.0)],
            "trunks": [{
                "id": "t", "rootId": "r", "baseDiameterMm": 0.5,
                "segments": [
                    { "id": "s0", "diameter": 0.5, "topJoint": j("a", 5.0) },
                    { "id": "s1", "diameter": 0.5, "bottomJoint": j("a", 5.0), "topJoint": j("b", 9.0) },
                ],
            }],
        }));
        let tris = mesh(&d);
        assert_closed(&tris);
        let ball_tris = {
            let mut m = Mesh::default();
            m.ball(DVec3::ZERO, 0.6);
            m.triangles.len()
        };
        let mut m = Mesh::default();
        let b = Builder::new(&d, OFFSET);
        b.root(&mut m, &b.roots["r"], 0.5);
        m.frustum(DVec3::new(0.0, 0.0, 2.0), 0.5, DVec3::new(0.0, 0.0, 5.0), 0.5);
        m.frustum(DVec3::new(0.0, 0.0, 5.0), 0.5, DVec3::new(0.0, 0.0, 9.0), 0.5);
        // Root, two segments and two balls (a, b): the joint the segments share is one ball.
        assert_eq!(tris.len(), m.triangles.len() + 2 * ball_tris);
        let (_, hi) = bounds(&tris);
        assert!(near(hi.z, 9.3, 1e-6), "{hi}");
    }

    #[test]
    fn a_branch_hangs_from_its_knot_and_a_brace_joins_two_knots() {
        let d = doc(json!({
            "knots": [
                { "id": "k", "pos": p(0.0, 0.0, 4.0), "diameter": 0.7, "t": 0.5 },
                { "id": "k1", "pos": p(0.0, 0.0, 3.0), "diameter": 0.5 },
                { "id": "k2", "pos": p(4.0, 0.0, 6.0), "diameter": 0.5 },
            ],
            "branches": [{
                "id": "b", "parentKnotId": "k",
                "segments": [{ "id": "s", "diameter": 0.4, "topJoint": joint("j", p(3.0, 0.0, 8.0), 0.5) }],
            }],
            "braces": [{ "id": "x", "startKnotId": "k1", "endKnotId": "k2", "profile": { "diameter": 0.4 } }],
        }));
        let tris = mesh(&d);
        assert_closed(&tris);
        let mut m = Mesh::default();
        m.frustum(DVec3::new(0.0, 0.0, 4.0), 0.4, DVec3::new(3.0, 0.0, 8.0), 0.4);
        m.frustum(DVec3::new(0.0, 0.0, 3.0), 0.4, DVec3::new(4.0, 0.0, 6.0), 0.4);
        m.ball(DVec3::new(3.0, 0.0, 8.0), 0.5);
        m.ball(DVec3::new(0.0, 0.0, 4.0), 0.7);
        m.ball(DVec3::new(0.0, 0.0, 3.0), 0.5);
        m.ball(DVec3::new(4.0, 0.0, 6.0), 0.5);
        assert!(near(volume(&tris), volume(&m.triangles), 1e-5));
        assert_eq!(tris.len(), m.triangles.len());
    }

    #[test]
    fn a_sphere_tip_is_seated_at_its_depth() {
        let cone = |depth: Option<f64>| {
            let mut c = json!({
                "pos": p(0.0, 0.0, 10.0), "normal": p(0.0, 0.0, 1.0), "surfaceNormal": p(0.0, 0.0, -1.0),
                "profile": { "type": "sphere", "contactDiameterMm": 0.4, "bodyDiameterMm": 0.6,
                             "lengthMm": 2.0, "sphereRadiusRatio": 2.0 },
                "socketJointId": "s",
            });
            if let Some(depth) = depth {
                c["profile"]["sphereDepthMm"] = json!(depth);
            }
            doc(json!({
                "knots": [{ "id": "k", "pos": p(0.0, 0.0, 6.0), "diameter": 0.7 }],
                "branches": [{ "id": "b", "parentKnotId": "k",
                    "segments": [{ "id": "g", "diameter": 0.6, "topJoint": joint("s", p(0.0, 0.0, 8.0), 0.7) }],
                    "contactCone": c }],
            }))
        };
        // Ball radius 0.4 (ratio 2 of the 0.2 contact radius), centre depth + offset into the model
        // (above the contact, the surface faces down).
        let (_, hi) = bounds(&mesh(&cone(Some(0.1))));
        assert!(near(hi.z, 10.0 + 0.1 + OFFSET + 0.4, 1e-6), "{hi}");
        // A negative depth: the centre outside the model.
        let (_, hi) = bounds(&mesh(&cone(Some(-0.3))));
        assert!(near(hi.z, 10.0 - 0.3 + OFFSET + 0.4, 1e-6), "{hi}");
        // No depth: seated sqrt(R^2 - r^2) outside, less the offset, so it crosses the surface
        // (run on by the offset) at the contact's width.
        let tris = mesh(&cone(None));
        let (_, hi) = bounds(&tris);
        let lift = (0.4f64 * 0.4 - 0.2 * 0.2).sqrt() - OFFSET;
        assert!(near(hi.z, 10.0 - lift + 0.4, 1e-6), "{hi}");
        assert_closed(&tris);
    }

    #[test]
    fn a_twig_has_disks_on_its_contacts() {
        let d = doc(json!({
            "twigs": [{
                "id": "t", "typeId": "twig",
                "segments": [{ "id": "s", "diameter": 0.3,
                    "bottomJoint": joint("a", p(0.0, 0.0, 1.2), 0.33),
                    "topJoint": joint("b", p(0.0, 0.0, 3.8), 0.33) }],
                "contactDiskA": { "pos": p(0.0, 0.0, 1.0), "surfaceNormal": p(0.0, 0.0, 1.0),
                    "diskLengthOverride": 0.2, "contactDiameterMm": 0.3, "profile": { "type": "disk" } },
                "contactDiskB": { "pos": p(0.0, 0.0, 4.0), "surfaceNormal": p(0.0, 0.0, -1.0),
                    "diskLengthOverride": 0.2, "contactDiameterMm": 0.3, "profile": { "type": "disk" } },
            }],
        }));
        let tris = mesh(&d);
        assert_closed(&tris);
        let (lo, hi) = bounds(&tris);
        // Each disk runs the tip offset into the model at its end.
        assert!(near(lo.z, 1.0 - OFFSET, 1e-6) && near(hi.z, 4.0 + OFFSET, 1e-6), "{lo} {hi}");
        let mut m = Mesh::default();
        m.frustum(DVec3::new(0.0, 0.0, 1.0 - OFFSET), 0.3, DVec3::new(0.0, 0.0, 1.2), 0.3);
        m.ball(DVec3::new(0.0, 0.0, 1.2), 0.3);
        m.frustum(DVec3::new(0.0, 0.0, 4.0 + OFFSET), 0.3, DVec3::new(0.0, 0.0, 3.8), 0.3);
        m.ball(DVec3::new(0.0, 0.0, 3.8), 0.3);
        m.frustum(DVec3::new(0.0, 0.0, 1.2), 0.3, DVec3::new(0.0, 0.0, 3.8), 0.3);
        m.ball(DVec3::new(0.0, 0.0, 1.2), 0.33);
        m.ball(DVec3::new(0.0, 0.0, 3.8), 0.33);
        assert_eq!(tris.len(), m.triangles.len());
        assert!(near(volume(&tris), volume(&m.triangles), 1e-5));
    }

    #[test]
    fn a_stick_has_cones_at_both_ends_and_a_kickstand_reaches_its_host() {
        let cone = |at: f64, n: f64, socket: &str| {
            json!({ "pos": p(0.0, 0.0, at), "normal": p(0.0, 0.0, -n), "surfaceNormal": p(0.0, 0.0, n),
                    "profile": { "type": "disk", "contactDiameterMm": 0.2, "bodyDiameterMm": 0.5, "lengthMm": 1.0 },
                    "socketJointId": socket })
        };
        let d = doc(json!({
            "sticks": [{
                "id": "s", "typeId": "stick",
                "segments": [{ "id": "g", "diameter": 0.5,
                    "bottomJoint": joint("a", p(0.0, 0.0, 2.0), 0.6), "topJoint": joint("b", p(0.0, 0.0, 9.0), 0.6) }],
                "contactConeA": cone(1.0, 1.0, "a"),
                "contactConeB": cone(10.0, -1.0, "b"),
            }],
            "kickstands": [{
                "kickstand": { "id": "k", "rootId": "kr", "hostKnotId": "hk",
                    "segments": [{ "id": "g0", "diameter": 0.6 }],
                    "profile": { "bodyDiameterMm": 0.6 } },
                "root": root("kr", 5.0, 0.0, 2.0),
                "hostKnot": { "id": "hk", "pos": p(8.0, 0.0, 6.0), "diameter": 0.8 },
            }],
        }));
        let tris = mesh(&d);
        assert_closed(&tris);
        let (lo, hi) = bounds(&tris);
        assert!(near(lo.z, 0.0, 1e-6) && near(hi.z, 10.0 + OFFSET, 1e-6), "{lo} {hi}");
        // The host knot's ball is the kickstand's far end.
        assert!(near(hi.x, 8.4, 1e-6), "{hi}");
    }

    #[test]
    fn an_empty_document_builds_nothing() {
        assert!(build(&doc(json!({})), OFFSET).is_empty());
        assert!(build(&Value::Null, OFFSET).is_empty());
    }
}
