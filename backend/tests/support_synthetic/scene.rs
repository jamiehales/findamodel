//! Port of gen.Scene: model bodies, truth parts, support primitives, the support mesh in its three
//! styles, the truth surface samples and the rotation applied on output.

use crate::geom::{self, Tri};
use crate::rng::Rng;
use crate::shapes::inside_shapes;
use crate::vec::{V3, gemm_row, gemv_rows, mat_vec, pairwise_sum, v3};
use serde_json::{Value, json};
use std::collections::HashMap;

pub const Z: f64 = 20.0;
pub const RAFT: f64 = 3.0;
pub const ROOT_TOP: f64 = 2.0;
/// A separate shell's end runs this far into what it joins.
pub const OV: f64 = 0.1;
/// A ball tip sinks this share of its radius into the model.
pub const BALL_SINK: f64 = 0.5;

pub fn radians(deg: f64) -> f64 {
    deg * (std::f64::consts::PI / 180.0)
}

/// gen.LABEL: the labels a part of each type may be read as.
fn label(kind: &str) -> Vec<String> {
    let l: &[&str] = match kind {
        "tip" => &["tip"],
        "shaft" => &["shaft"],
        "post" | "leg" => &["post"],
        "brace" => &["brace"],
        "micro" => &["micro"],
        "contact_rod" => &["contact_rod"],
        "floating" => &["floating"],
        _ => &[],
    };
    l.iter().map(|s| s.to_string()).collect()
}

pub fn strs(x: &[&str]) -> Vec<String> {
    x.iter().map(|s| s.to_string()).collect()
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub kind: String,
    pub a: V3,
    pub b: V3,
    pub ra: f64,
    pub rb: f64,
    pub required: bool,
    pub expect: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Shape {
    Rod { a: V3, b: V3, ra: f64, rb: f64, ext: (bool, bool) },
    Rev { a: V3, axis: V3, profile: Vec<(f64, f64)> },
    Raw { tris: Vec<Tri> },
}

#[derive(Clone, Debug)]
pub struct Prim {
    pub shape: Shape,
    pub part: usize,
    pub weld: Option<String>,
    pub raft: bool,
    pub wide_ext: bool,
    pub pad_part: Option<usize>,
}

impl Prim {
    pub fn new(shape: Shape, part: usize) -> Prim {
        Prim { shape, part, weld: None, raft: false, wide_ext: false, pad_part: None }
    }
    pub fn rod(a: V3, b: V3, ra: f64, rb: f64, ext: (bool, bool), part: usize) -> Prim {
        Prim::new(Shape::Rod { a, b, ra, rb, ext }, part)
    }
}

#[derive(Clone, Debug)]
pub struct Tip {
    pub name: String,
    pub contact: V3,
    pub normal: V3,
    pub socket: V3,
    pub narrow: V3,
    pub contact_d: f64,
    pub body_d: f64,
    pub part: Option<usize>,
    pub kind: String,
    pub dir: V3,
}

/// A support or post, as a brace, branch or kickstand names it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PRef {
    Sup(usize),
    Post(usize),
}

#[derive(Clone, Debug)]
pub enum Base {
    Plate { xy: [f64; 2], root_d: f64 },
    Parent { host: PRef, at: V3 },
    Lower { tip: usize },
}

/// The line, joints and width steps a support, post or kickstand shares.
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub line: Vec<V3>,
    pub joints: Vec<V3>,
    pub seg_d: Option<Vec<f64>>,
}

#[derive(Clone, Debug)]
pub struct Sup {
    pub name: String,
    pub tip: usize,
    pub base: Base,
    pub shaft_d: f64,
    pub l: Line,
}

#[derive(Clone, Debug)]
pub struct Post {
    pub name: String,
    pub foot: V3,
    pub top: V3,
    pub d: f64,
    pub root_d: f64,
    pub l: Line,
    pub removed: bool,
}

#[derive(Clone, Debug)]
pub struct Brace {
    pub name: String,
    pub part: usize,
    pub pa: PRef,
    pub pb: PRef,
    pub a: V3,
    pub b: V3,
    pub d: f64,
    pub hosts_any: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Kick {
    pub name: String,
    pub host: PRef,
    pub knot: V3,
    pub foot: V3,
    pub d: f64,
    pub root_d: f64,
    pub l: Line,
}

/// Optional arguments of Scene.tip / trunk / branch, with gen.py's defaults.
#[derive(Clone, Debug)]
pub struct TipArgs {
    pub d: f64,
    pub length: f64,
    pub dn: f64,
    pub dw: Option<f64>,
    pub bury: f64,
    pub kind: &'static str,
    pub rootd: f64,
    pub balld: Option<f64>,
    pub root: bool,
    pub lean: (f64, f64),
}

impl Default for TipArgs {
    fn default() -> Self {
        TipArgs {
            d: 1.0,
            length: 3.0,
            dn: 0.4,
            dw: None,
            bury: 0.3,
            kind: "flat",
            rootd: 4.0,
            balld: None,
            root: true,
            lean: (0.0, 0.0),
        }
    }
}

pub struct Scene {
    pub n: usize,
    pub rot: i64,
    pub style: String,
    pub parts: Vec<Part>,
    pub prims: Vec<Prim>,
    pub model_tris: Vec<Vec<Tri>>,
    pub shapes: Vec<Value>,
    pub supports: Vec<Sup>,
    pub posts: Vec<Post>,
    pub braces: Vec<Brace>,
    pub kicks: Vec<Kick>,
    pub tips: Vec<Tip>,
    pub flex: i64,
    pub counts_alt: Value,
    pub notes: Vec<String>,
    pub model_open: bool,
    pub tree_free: bool,
    pub passes_through: bool,
    /// How struts are to be written, by support name: "twig" (flat rod ends) or "stick" (cone
    /// tips), the owner's rule whatever the span. Struts not named are not checked.
    pub written: Vec<(String, &'static str)>,
}

pub fn lst(p: V3) -> Value {
    json!([p[0], p[1], p[2]])
}

impl Scene {
    pub fn new(sides: usize, rot: i64, style: &str) -> Scene {
        Scene {
            n: sides,
            rot,
            style: style.to_string(),
            parts: vec![],
            prims: vec![],
            model_tris: vec![],
            shapes: vec![],
            supports: vec![],
            posts: vec![],
            braces: vec![],
            kicks: vec![],
            tips: vec![],
            flex: 0,
            counts_alt: Value::Null,
            notes: vec![],
            model_open: false,
            tree_free: false,
            passes_through: false,
            written: vec![],
        }
    }

    // ---- bodies ----
    pub fn slab_with(&mut self, h: f64, z: f64, thick: f64, skip: &[&str], lo: (f64, f64), hi: (f64, f64)) {
        let (box_lo, box_hi) = (v3(lo.0, lo.1, z), v3(hi.0, hi.1, z + thick));
        self.model_tris.push(geom::grid_box(box_lo, box_hi, h, skip));
        self.shapes.push(json!({"kind": "box", "lo": lst(box_lo), "hi": lst(box_hi)}));
        self.model_open = self.model_open || !skip.is_empty();
    }

    pub fn slab(&mut self) {
        self.slab_with(1.0, Z, 4.0, &[], (-20.0, -12.0), (20.0, 12.0));
    }

    pub fn boxed(&mut self, lo: V3, hi: V3) {
        self.model_tris.push(geom::grid_box(lo, hi, 1.0, &[]));
        self.shapes.push(json!({"kind": "box", "lo": lst(lo), "hi": lst(hi)}));
    }

    /// A slab turned about Y through (0, 0, z): its underside slopes by deg.
    pub fn angled(&mut self, deg: f64, z: f64) {
        let half = v3(22.0, 12.0, 2.0);
        let t = geom::grid_box(-half, half, 1.0, &[]);
        let a = radians(deg);
        let rm = [[a.cos(), 0.0, a.sin()], [0.0, 1.0, 0.0], [-a.sin(), 0.0, a.cos()]];
        let c = v3(0.0, 0.0, z);
        self.model_tris.push(t.iter().map(|tr| tr.map(|p| gemm_row(p, &rm) + c)).collect());
        self.shapes.push(json!({"kind": "obox", "c": lst(c), "R": rm, "half": lst(half)}));
    }

    pub fn sphere(&mut self, c: V3, r: f64) {
        self.model_tris.push(geom::sphere_mesh(c, r, 64, 48));
        self.shapes.push(json!({"kind": "sphere", "c": lst(c), "r": r}));
    }

    pub fn hollow(&mut self, floor: f64, ceil: f64, block: Option<(V3, V3)>) {
        let (out_lo, out_hi) = (v3(-20.0, -12.0, 0.0), v3(20.0, 12.0, ceil + 4.0));
        let (in_lo, in_hi) = (v3(-17.0, -9.0, floor), v3(17.0, 9.0, ceil));
        let cavity: Vec<Tri> = geom::grid_box(in_lo, in_hi, 1.0, &[]).into_iter().map(|t| [t[2], t[1], t[0]]).collect();
        self.model_tris.push(geom::grid_box(out_lo, out_hi, 1.0, &[]));
        self.model_tris.push(cavity);
        self.shapes.push(
            json!({"kind": "hollow", "lo": lst(out_lo), "hi": lst(out_hi), "ilo": lst(in_lo), "ihi": lst(in_hi)}),
        );
        if let Some((lo, hi)) = block {
            self.boxed(lo, hi);
        }
    }

    pub fn mesh(&self) -> Vec<Tri> {
        self.model_tris.concat()
    }

    pub fn hit(&self, origin: V3, direction: V3) -> Result<(V3, V3), String> {
        match geom::ray_first_hit(&self.mesh(), origin, direction) {
            Some((_, p, n)) => Ok((p, n)),
            None => Err(format!("no surface from {origin:?} along {direction:?}")),
        }
    }

    // ---- parts ----
    #[allow(clippy::too_many_arguments)]
    pub fn part_full(
        &mut self,
        name: &str,
        kind: &str,
        a: V3,
        b: V3,
        ra: f64,
        rb: Option<f64>,
        required: bool,
        expect: Option<Vec<String>>,
    ) -> usize {
        let rb = rb.unwrap_or(ra);
        let expect = match expect {
            Some(e) if !e.is_empty() => e,
            _ => label(kind),
        };
        self.parts.push(Part { name: name.to_string(), kind: kind.to_string(), a, b, ra, rb, required, expect });
        self.parts.len() - 1
    }

    pub fn part(&mut self, name: &str, kind: &str, a: V3, b: V3, ra: f64) -> usize {
        self.part_full(name, kind, a, b, ra, None, true, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rod_full(
        &mut self,
        name: &str,
        kind: &str,
        a: V3,
        b: V3,
        ra: f64,
        rb: Option<f64>,
        ext: (bool, bool),
        required: bool,
        expect: Option<Vec<String>>,
        weld: Option<String>,
    ) -> usize {
        let rb = rb.unwrap_or(ra);
        let idx = self.part_full(name, kind, a, b, ra, Some(rb), required, expect);
        let mut p = Prim::rod(a, b, ra, rb, ext, idx);
        p.weld = weld;
        self.prims.push(p);
        idx
    }

    pub fn rod(&mut self, name: &str, kind: &str, a: V3, b: V3, ra: f64) -> usize {
        self.rod_full(name, kind, a, b, ra, None, (true, true), true, None, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rod_ext(
        &mut self,
        name: &str,
        kind: &str,
        a: V3,
        b: V3,
        ra: f64,
        rb: Option<f64>,
        ext: (bool, bool),
    ) -> usize {
        self.rod_full(name, kind, a, b, ra, rb, ext, true, None, None)
    }

    pub fn ball_full(&mut self, name: &str, c: V3, r: f64, weld: Option<String>, required: bool) -> usize {
        let idx = self.part_full(name, "ball", c, c, r, Some(r), required, None);
        let mut p = Prim::new(
            Shape::Rev {
                a: c - v3(0.0, 0.0, r),
                axis: v3(0.0, 0.0, 1.0),
                profile: geom::ball_profile(r, self.n.max(6)),
            },
            idx,
        );
        p.weld = weld;
        self.prims.push(p);
        idx
    }

    pub fn ball(&mut self, name: &str, c: V3, r: f64, required: bool) -> usize {
        self.ball_full(name, c, r, None, required)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rev(
        &mut self,
        name: &str,
        kind: &str,
        a: V3,
        axis: V3,
        profile: Vec<(f64, f64)>,
        ra: f64,
        rb: f64,
        required: bool,
    ) -> usize {
        let axis = axis.unit();
        let length = profile[profile.len() - 1].0;
        let idx = self.part_full(name, kind, a, a + axis * length, ra, Some(rb), required, None);
        self.prims.push(Prim::new(Shape::Rev { a, axis, profile }, idx));
        idx
    }

    pub fn raw(&mut self, part: usize, tris: Vec<Tri>) {
        self.prims.push(Prim::new(Shape::Raw { tris }, part));
    }

    pub fn raft_box(&mut self, name: &str, lo: V3, hi: V3) -> usize {
        let half = v3(0.0, 0.0, (hi[2] - lo[2]) / 2.0);
        let mid = (lo + hi) / 2.0;
        let dxy = hi - lo;
        let ra = (dxy[0] * dxy[0] + dxy[1] * dxy[1]).sqrt() / 2.0;
        let idx = self.part_full(name, "raft", mid - half, mid + half, ra, None, false, None);
        let mut p = Prim::new(Shape::Raw { tris: geom::grid_box(lo, hi, 1000.0, &[]) }, idx);
        p.raft = true;
        self.prims.push(p);
        idx
    }

    pub fn root(&mut self, name: &str, foot_xy: (f64, f64), d: f64, rootd: f64) -> usize {
        let (x, y) = foot_xy;
        self.rev(
            name,
            "root",
            v3(x, y, 0.0),
            v3(0.0, 0.0, 1.0),
            vec![(0.0, rootd / 2.0), (0.5, rootd / 2.0), (ROOT_TOP, d / 2.0)],
            rootd / 2.0,
            d / 2.0,
            false,
        )
    }

    /// direction runs towards the model. The cone's axis passes through contact.
    #[allow(clippy::too_many_arguments)]
    pub fn tip(
        &mut self,
        name: &str,
        contact: V3,
        normal: V3,
        direction: V3,
        length: f64,
        dn: f64,
        dw: f64,
        bury: f64,
        kind: &str,
        balld: Option<f64>,
    ) -> usize {
        let direction = direction.unit();
        let (rn, rw) = (dn / 2.0, dw / 2.0);
        let is_ball = kind == "ball" || kind == "weldball";
        let (mut rball, mut sink) = (0.0, 0.0);
        let (wide, narrow) = if is_ball {
            rball = balld.unwrap_or(dn * 1.5) / 2.0;
            sink = rball * BALL_SINK;
            (contact - direction * length, contact - direction * (rball - sink))
        } else {
            (contact - direction * length, contact + direction * bury)
        };
        let total = length + bury;
        let mut at = rn + (rw - rn) * bury / total;
        if is_ball {
            at = (rball * rball - (rball - sink) * (rball - sink)).sqrt();
        }
        let up = narrow - wide;
        let weld = if kind == "weldball" { Some(format!("{name}-tip")) } else { None };
        let part = if kind == "dome" {
            let mut prof = vec![(0.0, 0.0)];
            let hp = std::f64::consts::PI / 2.0;
            for k in 1..5 {
                let t = rn * (1.0 - (hp * k as f64 / 4.0).cos());
                prof.push((t, rn * (hp * k as f64 / 4.0).sin()));
            }
            prof.push((total, rw));
            self.rev(name, "tip", narrow, -up, prof, rn, rw, true)
        } else {
            self.rod_full(name, "tip", narrow, wide, rn, Some(rw), (false, false), true, None, weld.clone())
        };
        if is_ball {
            self.ball_full(&format!("{name}-ball"), narrow, rball, weld, false);
        }
        self.tips.push(Tip {
            name: name.to_string(),
            contact,
            normal,
            socket: wide,
            narrow,
            contact_d: 2.0 * at,
            body_d: dw,
            part: Some(part),
            kind: kind.to_string(),
            dir: direction,
        });
        self.tips.len() - 1
    }

    pub fn bare_tip(&mut self, name: &str, contact: V3, normal: V3, direction: V3, d: f64) -> usize {
        let direction = direction.unit();
        self.tips.push(Tip {
            name: name.to_string(),
            contact,
            normal,
            socket: contact - direction * 0.35,
            narrow: contact,
            contact_d: d,
            body_d: d,
            part: None,
            kind: "bare".to_string(),
            dir: direction,
        });
        self.tips.len() - 1
    }

    // ---- supports ----
    pub fn trunk(&mut self, name: &str, xy: (f64, f64), o: &TipArgs) -> Result<usize, String> {
        let dw = o.dw.unwrap_or(o.d);
        let org = v3(xy.0, xy.1, 0.0);
        let direction = v3(o.lean.0, o.lean.1, 1.0).unit();
        let (contact, normal) = self.hit(org, direction)?;
        let tip =
            self.tip(&format!("{name}-tip"), contact, normal, direction, o.length, o.dn, dw, o.bury, o.kind, o.balld);
        let at = |z: f64| org + direction * (z / direction[2]);
        let (wide, foot) = (self.tips[tip].socket, at(ROOT_TOP));
        self.rod(&format!("{name}-shaft"), "shaft", at(1.5), wide, o.d / 2.0);
        if o.root {
            self.root(&format!("{name}-root"), (foot[0], foot[1]), o.d, o.rootd);
        }
        self.supports.push(Sup {
            name: name.to_string(),
            tip,
            base: Base::Plate { xy: [foot[0], foot[1]], root_d: o.rootd },
            shaft_d: o.d,
            l: Line { line: vec![foot, wide], ..Default::default() },
        });
        Ok(self.supports.len() - 1)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn post_full(
        &mut self,
        name: &str,
        xy: (f64, f64),
        top: f64,
        d: f64,
        rootd: f64,
        free_top: bool,
        lean: (f64, f64),
        expect: Option<Vec<String>>,
    ) -> usize {
        let o = v3(xy.0, xy.1, 0.0);
        let direction = v3(lean.0, lean.1, 1.0).unit();
        let at = |z: f64| o + direction * (z / direction[2]);
        let (foot, topp) = (at(ROOT_TOP), at(top));
        self.rod_full(
            &format!("{name}-post"),
            "post",
            at(1.5),
            topp,
            d / 2.0,
            None,
            (true, !free_top),
            true,
            expect,
            None,
        );
        self.root(&format!("{name}-root"), (foot[0], foot[1]), d, rootd);
        self.posts.push(Post {
            name: name.to_string(),
            foot,
            top: topp,
            d,
            root_d: rootd,
            l: Line { line: vec![foot, topp], ..Default::default() },
            removed: false,
        });
        self.posts.len() - 1
    }

    pub fn post(&mut self, name: &str, xy: (f64, f64), top: f64) -> usize {
        self.post_full(name, xy, top, 1.0, 4.0, true, (0.0, 0.0), None)
    }

    pub fn line_of(&self, r: PRef) -> &Line {
        match r {
            PRef::Sup(i) => &self.supports[i].l,
            PRef::Post(i) => &self.posts[i].l,
        }
    }

    pub fn name_of(&self, r: PRef) -> &str {
        match r {
            PRef::Sup(i) => &self.supports[i].name,
            PRef::Post(i) => &self.posts[i].name,
        }
    }

    /// Scene.on: the point at height z on a support's or post's line.
    pub fn on(&self, pillar: PRef, z: f64) -> Result<V3, String> {
        let line = &self.line_of(pillar).line;
        for w in line.windows(2) {
            let (lo, hi) = (w[0], w[1]);
            if lo[2].min(hi[2]) - 1e-9 <= z && z <= lo[2].max(hi[2]) + 1e-9 && (hi[2] - lo[2]).abs() > 1e-9 {
                return Ok(lo + (hi - lo) * ((z - lo[2]) / (hi[2] - lo[2])));
            }
        }
        Err(format!("{z} off {}", self.name_of(pillar)))
    }

    /// Scene.branch. o.d defaults to 0.8 here (the caller sets it); cone_only and target_z as in gen.py.
    #[allow(clippy::too_many_arguments)]
    pub fn branch(
        &mut self,
        name: &str,
        host: PRef,
        z: f64,
        target_xy: (f64, f64),
        o: &TipArgs,
        cone_only: bool,
        target_z: Option<f64>,
    ) -> Result<usize, String> {
        let dw = o.dw.unwrap_or(o.d);
        let at = self.on(host, z)?;
        let aim = v3(target_xy.0, target_xy.1, target_z.unwrap_or(Z));
        let direction = (aim - at).unit();
        let (contact, normal) = self.hit(at, direction)?;
        let mut length = o.length;
        if cone_only {
            length = (contact - at).norm();
        }
        let tip = self.tip(&format!("{name}-tip"), contact, normal, direction, length, o.dn, dw, o.bury, o.kind, None);
        let wide = self.tips[tip].socket;
        if cone_only {
            self.prims.last_mut().unwrap().wide_ext = true;
        }
        if !cone_only {
            self.rod(&format!("{name}-shaft"), "shaft", at, wide, o.d / 2.0);
        }
        self.supports.push(Sup {
            name: name.to_string(),
            tip,
            base: Base::Parent { host, at },
            shaft_d: o.d,
            l: Line { line: vec![at, wide], ..Default::default() },
        });
        Ok(self.supports.len() - 1)
    }

    /// Between contacts on two model faces.
    pub fn strut(&mut self, name: &str, low: V3, up: V3, d: f64, length: f64) -> Result<usize, String> {
        let (dn, bury, kind) = (0.4, 0.3, "flat");
        let direction = (up - low).unit();
        let (_, nu) = self.hit((low + up) / 2.0, direction)?;
        let (_, nl) = self.hit((low + up) / 2.0, -direction)?;
        let tu = self.tip(&format!("{name}-tip"), up, nu, direction, length, dn, d, bury, kind, None);
        let tl = self.tip(&format!("{name}-foot"), low, nl, -direction, length, dn, d, bury, kind, None);
        let (ls, us) = (self.tips[tl].socket, self.tips[tu].socket);
        self.rod(&format!("{name}-shaft"), "shaft", ls, us, d / 2.0);
        self.written.push((name.to_string(), "stick"));
        self.supports.push(Sup {
            name: name.to_string(),
            tip: tu,
            base: Base::Lower { tip: tl },
            shaft_d: d,
            l: Line { line: vec![ls, us], ..Default::default() },
        });
        Ok(self.supports.len() - 1)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn brace_full(
        &mut self,
        name: &str,
        pa: PRef,
        za: f64,
        pb: PRef,
        zb: f64,
        d: f64,
        trim: (f64, f64),
        weld: Option<String>,
    ) -> Result<usize, String> {
        let (a, b) = (self.on(pa, za)?, self.on(pb, zb)?);
        let u = (b - a).unit();
        let (aa, bb) = (a + u * trim.0, b - u * trim.1);
        let ext = (trim.0 == 0.0, trim.1 == 0.0);
        let idx = self.rod_full(name, "brace", aa, bb, d / 2.0, None, ext, true, None, weld);
        self.braces.push(Brace { name: name.to_string(), part: idx, pa, pb, a, b, d, hosts_any: None });
        Ok(idx)
    }

    pub fn brace(&mut self, name: &str, pa: PRef, za: f64, pb: PRef, zb: f64, d: f64) -> Result<usize, String> {
        self.brace_full(name, pa, za, pb, zb, d, (0.0, 0.0), None)
    }

    pub fn kick(
        &mut self,
        name: &str,
        host: PRef,
        z: f64,
        foot_xy: (f64, f64),
        d: f64,
        rootd: f64,
    ) -> Result<(), String> {
        let knot = self.on(host, z)?;
        let o = v3(foot_xy.0, foot_xy.1, ROOT_TOP);
        let u = knot - o;
        let low = o - u * ((ROOT_TOP - 1.5) / u[2]);
        self.rod(&format!("{name}-leg"), "leg", low, knot, d / 2.0);
        self.root(&format!("{name}-root"), foot_xy, d, rootd);
        self.kicks.push(Kick { name: name.to_string(), host, knot, foot: o, d, root_d: rootd, l: Line::default() });
        Ok(())
    }

    // ---- output ----
    pub fn prim_tris(&self, prim: &Prim, extend: bool) -> Vec<Tri> {
        match &prim.shape {
            Shape::Raw { tris } => tris.clone(),
            Shape::Rev { a, axis, profile } => geom::revolve_tris(*a, *axis, profile, self.n),
            &Shape::Rod { a, b, ra, rb, ext } => {
                let (mut a, mut b, mut rb2) = (a, b, rb);
                let u = (b - a).unit();
                if extend && prim.wide_ext {
                    // A separate cone's wide end runs on into the joint, so its cap doesn't weld to its neighbours'.
                    rb2 += (rb2 - ra) / (b - a).norm() * OV;
                    b += u * OV;
                }
                if extend && ra == rb {
                    a -= u * (if ext.0 { OV } else { 0.0 });
                    b += u * (if ext.1 { OV } else { 0.0 });
                }
                let length = (b - a).norm();
                geom::revolve_tris(a, u, &[(0.0, ra), (length, rb2)], self.n)
            }
        }
    }

    pub fn prim_polys(&self, prim: &Prim) -> Vec<Vec<V3>> {
        match &prim.shape {
            Shape::Raw { tris } => tris.iter().map(|t| t.to_vec()).collect(),
            Shape::Rev { a, axis, profile } => geom::revolve_polys(*a, *axis, profile, self.n),
            &Shape::Rod { a, b, ra, rb, .. } => {
                geom::revolve_polys(a, (b - a).unit(), &[(0.0, ra), ((b - a).norm(), rb)], self.n)
            }
        }
    }

    /// Splits rods where another part's end lands on their axis, so their end caps' centres weld.
    pub fn joined_prims(&self) -> Vec<Prim> {
        let mut nodes = Vec::new();
        for p in &self.prims {
            if let Shape::Rod { a, b, .. } = p.shape {
                nodes.push(a);
                nodes.push(b);
            }
        }
        let mut out = Vec::new();
        for p in &self.prims {
            let Shape::Rod { a, b, ra, rb, ext } = p.shape else {
                out.push(p.clone());
                continue;
            };
            if ra != rb {
                out.push(p.clone());
                continue;
            }
            let u = b - a;
            let ll = u.dot(u);
            let mut cuts: Vec<f64> = Vec::new();
            for &q in &nodes {
                let t = (q - a).dot(u) / ll;
                if 0.02 < t && t < 0.98 && (a + u * ((q - a).dot(u) / ll) - q).norm() < 1e-6 {
                    let r = py_round9(t);
                    if !cuts.contains(&r) {
                        cuts.push(r);
                    }
                }
            }
            cuts.sort_by(f64::total_cmp);
            let mut ts = vec![0.0];
            ts.extend(cuts);
            ts.push(1.0);
            for w in ts.windows(2) {
                let mut q = p.clone();
                q.shape = Shape::Rod { a: a + u * w[0], b: a + u * w[1], ra, rb, ext };
                out.push(q);
            }
        }
        out
    }

    fn union_report(&self, solids: Vec<Vec<Vec<V3>>>) -> (Vec<Tri>, bool, usize, usize) {
        let polys = geom::union_polys(solids);
        let (tris, ok) = geom::repair_t_junctions(&geom::polys_to_tris(&polys));
        let (bad, odd) = geom::closed_stats(&tris);
        (tris, ok, bad, odd)
    }

    /// (triangles, closed-check result of any union).
    pub fn support_mesh(&self) -> (Vec<Tri>, Value) {
        let is_root = |p: &Prim| self.parts[p.part].kind == "root";
        if self.style == "welded" {
            let solids: Vec<_> =
                self.prims.iter().filter(|p| !p.raft && !is_root(p)).map(|p| self.prim_polys(p)).collect();
            let (mut tris, ok, bad, odd) = self.union_report(solids);
            let report = json!({"union_open_edges": bad, "union_odd_edges": odd, "union_repair_done": ok});
            for p in self.prims.iter().filter(|p| p.raft || is_root(p)) {
                tris.extend(self.prim_tris(p, false));
            }
            return (tris, report);
        }
        let prims = if self.style == "sep" { self.prims.clone() } else { self.joined_prims() };
        let mut out = Vec::new();
        let mut order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, Vec<Prim>> = HashMap::new();
        for p in prims {
            if let Some(w) = &p.weld {
                if !groups.contains_key(w) {
                    order.push(w.clone());
                }
                groups.entry(w.clone()).or_default().push(p);
            } else {
                out.extend(self.prim_tris(&p, self.style == "sep"));
            }
        }
        let mut report = serde_json::Map::new();
        for name in order {
            let solids = groups[&name].iter().map(|p| self.prim_polys(p)).collect();
            let (tris, _, bad, odd) = self.union_report(solids);
            report.insert(format!("weld:{name}"), json!({"open": bad, "odd": odd}));
            out.extend(tris);
        }
        (out, Value::Object(report))
    }

    /// Truth's printed surface: samples on each part's shells, not inside another part or the model.
    pub fn samples(&self) -> (Vec<V3>, Vec<u16>) {
        let mut rng = Rng::new(20261109);
        let (mut pts, mut tags): (Vec<V3>, Vec<usize>) = (vec![], vec![]);
        for p in &self.prims {
            let part = &self.parts[p.part];
            if part.kind == "raft" || part.kind == "root" {
                continue;
            }
            let t = self.prim_tris(p, false);
            let e1: Vec<V3> = t.iter().map(|x| x[1] - x[0]).collect();
            let e2: Vec<V3> = t.iter().map(|x| x[2] - x[0]).collect();
            let areas: Vec<f64> = e1.iter().zip(&e2).map(|(a, b)| a.cross(*b).norm() / 2.0).collect();
            let total = pairwise_sum(&areas);
            if total <= 0.0 {
                continue;
            }
            let count = (total / 0.015).clamp(40.0, 1500.0) as usize;
            let prob: Vec<f64> = areas.iter().map(|a| a / total).collect();
            let pick = rng.choice(&prob, count);
            let mut u = rng.random(count);
            let mut v = rng.random(count);
            for k in 0..count {
                if u[k] + v[k] > 1.0 {
                    u[k] = 1.0 - u[k];
                    v[k] = 1.0 - v[k];
                }
            }
            for k in 0..count {
                let i = pick[k];
                let q = t[i][0] + e1[i] * u[k] + e2[i] * v[k];
                let mut tag = p.part;
                if let Some(pp) = p.pad_part
                    && (q[0] * q[0] + q[1] * q[1]).sqrt() > 0.6
                {
                    tag = pp;
                }
                pts.push(q);
                tags.push(tag);
            }
        }
        let mut keep: Vec<bool> = pts.iter().map(|p| p[2] >= RAFT + 0.5).collect();
        let ins = inside_shapes(&Value::Array(self.shapes.clone()), &pts);
        for (k, i) in keep.iter_mut().zip(ins) {
            *k &= !i;
        }
        for (i, part) in self.parts.iter().enumerate() {
            let q: Vec<usize> = (0..pts.len()).filter(|&k| keep[k] && tags[k] != i).collect();
            let sub: Vec<V3> = q.iter().map(|&k| pts[k]).collect();
            let inside = inside_part(part, &sub, self.n);
            for (&k, ins) in q.iter().zip(inside) {
                keep[k] &= !ins;
            }
        }
        let mut out = (vec![], vec![]);
        for k in 0..pts.len() {
            if keep[k] {
                out.0.push(pts[k]);
                out.1.push(tags[k] as u16);
            }
        }
        out
    }

    fn rot_matrix(&self) -> [[f64; 3]; 3] {
        let a = radians(self.rot as f64);
        let (c, s) = (a.cos(), a.sin());
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    }

    /// Scene._turn of one point (numpy gemv order).
    pub fn turn(&self, p: V3) -> V3 {
        mat_vec(&self.rot_matrix(), p)
    }

    /// Scene._turn of an (N, 3) array (numpy gemm order).
    pub fn turn_rows(&self, pts: &[V3]) -> Vec<V3> {
        let m = self.rot_matrix();
        pts.iter().map(|&p| gemm_row(p, &m)).collect()
    }

    pub fn turn_tris(&self, tris: &[Tri]) -> Vec<Tri> {
        let m = self.rot_matrix();
        tris.iter().map(|t| t.map(|p| gemm_row(p, &m))).collect()
    }
}

/// Python's round(x, 9): the nearest double to x correctly rounded to 9 decimals (ties to even).
pub fn py_round9(x: f64) -> f64 {
    format!("{x:.9}").parse().unwrap()
}

/// gen.inside_part: points inside a part's (shrunk) analytic body.
pub fn inside_part(part: &Part, pts: &[V3], n: usize) -> Vec<bool> {
    let (a, b) = (part.a, part.b);
    let shrink = (std::f64::consts::PI / n as f64).cos() - 0.01;
    if part.kind == "raft" || a == b {
        return pts.iter().map(|p| (*p - a).norm() <= part.ra * shrink).collect();
    }
    let d = b - a;
    let rel: Vec<V3> = pts.iter().map(|p| *p - a).collect();
    let dd = d.dot(d);
    let ts: Vec<f64> = gemv_rows(&rel, d).into_iter().map(|x| x / dd).collect();
    rel.iter()
        .zip(ts)
        .map(|(r, t)| {
            let rr = part.ra + (part.rb - part.ra) * t;
            let off = (*r - d * t).norm();
            t > -1e-6 && t < 1.0 + 1e-6 && off < rr * shrink
        })
        .collect()
}
