//! Port of gen.py's cases: every case function, the CASES registry (id, title, real, styles),
//! DROPPED, NO_N8, variants() and vid(). Each case is small, with one thing under test.

use crate::geom::{self, Tri};
use crate::scene::PRef::{Post as Pp, Sup as Sp};
use crate::scene::{
    Base, Brace, Kick, Line, PRef, Part, Post, Prim, ROOT_TOP, Scene, Shape, Sup, TipArgs, Z, radians, strs,
};
use crate::vec::{V3, mat_vec, pairwise_sum, v3};
use serde_json::{Value, json};

type R = Result<(), String>;
pub type CaseFn = Box<dyn Fn(&mut Scene) -> R + Send + Sync>;

pub struct CaseDef {
    pub id: String,
    pub title: String,
    pub real: String,
    pub styles: Vec<&'static str>,
    pub run: CaseFn,
}

const ALL: &[&str] = &["sep", "joined", "welded"];
const SEP: &[&str] = &["sep"];
const SJ: &[&str] = &["sep", "joined"];

pub const SIDES: [usize; 5] = [4, 8, 12, 16, 32];

/// Cases the owner dropped; still defined, but not generated, run or counted.
pub const DROPPED: &[(&str, &str)] = &[
    ("m04_midair", "owner marked drop"),
    ("s04_stub", "owner marked drop"),
    ("z04_short01", "owner marked drop"),
    ("z04_short03", "owner marked drop"),
    ("z04_short06", "owner marked drop"),
    ("p16_inverted", "owner dropped: none of the 158 real models is inside out"),
    (
        "x02_welded",
        "owner dropped (merge, 2026-11-10): two X-brace rods welded only at the crossing; no real model in the 158 has it",
    ),
];

/// Cases whose 8-sided variants are not generated.
pub const NO_N8: &[&str] = &["c09_flatwall"];

pub fn is_dropped(id: &str) -> bool {
    DROPPED.iter().any(|(d, _)| *d == id)
}

pub fn variants(c: &CaseDef) -> Vec<(usize, i64, &'static str)> {
    let mut out: Vec<(usize, i64, &'static str)> = vec![(8, 0, "sep")];
    out.extend(SIDES.iter().filter(|&&n| n != 8).map(|&n| (n, 0, "sep")));
    out.extend([(8, 17, "sep"), (8, 45, "sep"), (16, 30, "sep")]);
    if c.styles.contains(&"joined") {
        out.extend([(8, 0, "joined"), (16, 0, "joined")]);
    }
    if c.styles.contains(&"welded") {
        out.extend([(8, 0, "welded"), (16, 0, "welded")]);
    }
    if NO_N8.contains(&c.id.as_str()) {
        out.retain(|v| v.0 != 8);
    }
    out
}

pub fn vid(cid: &str, n: usize, rot: i64, style: &str) -> String {
    format!("{cid}__n{n}_r{rot}_{style}")
}

fn t() -> TipArgs {
    TipArgs::default()
}

/// TipArgs for a branch: its shaft defaults to 0.8 mm.
fn tb() -> TipArgs {
    TipArgs { d: 0.8, ..TipArgs::default() }
}

fn hit_up(s: &Scene, x: f64, y: f64) -> Result<(V3, V3), String> {
    s.hit(v3(x, y, 0.0), v3(0.0, 0.0, 1.0))
}

fn plate_sup(name: &str, tip: usize, foot: V3, shaft_d: f64, line: Vec<V3>, joints: Vec<V3>) -> Sup {
    Sup {
        name: name.into(),
        tip,
        base: Base::Plate { xy: [foot[0], foot[1]], root_d: 4.0 },
        shaft_d,
        l: Line { line, joints, seg_d: None },
    }
}

// ---- stepped tapers ----
/// Cut positions (between s0 and s1) and step widths of a radius profile r_of(s) split into k steps.
pub fn truth_steps(r_of: &dyn Fn(f64) -> f64, s0: f64, s1: f64, k: usize, snaps: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = geom::py_round((s1 - s0) / 0.2).max(6) as usize;
    let ss: Vec<f64> = (0..n).map(|i| s0 + (i as f64 + 0.5) * (s1 - s0) / n as f64).collect();
    let a: Vec<f64> = ss.iter().map(|&x| r_of(x)).map(|r| r * r).collect();
    let (mut c1, mut c2) = (vec![0.0], vec![0.0]);
    let (mut s1a, mut s2a) = (0.0, 0.0);
    for &x in &a {
        s1a += x;
        s2a += x * x;
        c1.push(s1a);
        c2.push(s2a);
    }
    let cost = |i: usize, j: usize| {
        let d = c1[j] - c1[i];
        (c2[j] - c2[i]) - d * d / (j.saturating_sub(i).max(1)) as f64
    };
    let cuts: Vec<usize> = if k == 2 {
        let mut best = (f64::INFINITY, 0);
        for i in 2..n - 1 {
            let c = cost(0, i) + cost(i, n);
            if c < best.0 {
                best = (c, i);
            }
        }
        vec![best.1]
    } else {
        let mut best = (f64::INFINITY, (0, 0));
        for i in 2..n.saturating_sub(3) {
            for j in i + 2..n - 1 {
                let c = cost(0, i) + cost(i, j) + cost(j, n);
                if c < best.0 {
                    best = (c, (i, j));
                }
            }
        }
        vec![best.1.0, best.1.1]
    };
    let mut cs = Vec::new();
    for c in cuts {
        let x = s0 + c as f64 * (s1 - s0) / n as f64;
        let mut near: Option<f64> = None;
        for &q in snaps {
            if (q - x).abs() <= 1.5 && near.is_none_or(|b| (q - x).abs() < (b - x).abs()) {
                near = Some(q);
            }
        }
        cs.push(near.unwrap_or(x));
    }
    let mut edges = vec![s0];
    edges.extend(&cs);
    edges.push(s1);
    let d = edges
        .windows(2)
        .map(|w| {
            let sel: Vec<f64> = ss.iter().zip(&a).filter(|(s, _)| **s >= w[0] && **s < w[1]).map(|(_, a)| *a).collect();
            2.0 * (pairwise_sum(&sel) / sel.len() as f64).sqrt()
        })
        .collect();
    (cs, d)
}

/// Write the truth support (or post) line as k steps along its straight line (bottom point to top point).
pub fn stepped(l: &mut Line, r_of: &dyn Fn(f64) -> f64, k: usize, snaps: &[f64]) {
    let (lo, hi) = (l.line[0], l.line[l.line.len() - 1]);
    let len = (hi - lo).norm();
    let (cuts, d) = truth_steps(r_of, 0.0, len, k, snaps);
    let js: Vec<V3> = cuts.iter().map(|c| lo + (hi - lo) * (c / len)).collect();
    l.joints = js.clone();
    l.line = std::iter::once(lo).chain(js).chain(std::iter::once(hi)).collect();
    l.seg_d = Some(d);
}

/// Python's str() of a float, for the few notes that print one.
fn pyf(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e16 { format!("{x:.1}") } else { format!("{x}") }
}

// ---- trunks ----
fn tilted(s: &mut Scene, deg: f64) -> R {
    s.slab();
    s.trunk("a", (-2.0, 1.4), &TipArgs { lean: (radians(deg).tan(), 0.0), ..t() })?;
    Ok(())
}

fn rings_shaft(s: &mut Scene, step: f64, shared: bool, lap: f64) -> R {
    s.slab();
    let d = 1.0;
    let o = (2.2, 1.4);
    let (contact, normal) = hit_up(s, o.0, o.1)?;
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, 1.0, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    let mut z = 1.5;
    let part = s.part("a-shaft", "shaft", v3(o.0, o.1, 1.5), wide, d / 2.0);
    let mut prof = Vec::new();
    while z < wide[2] - 1e-9 {
        let hi = (z + step).min(wide[2]);
        if shared {
            prof.push((z - 1.5, d / 2.0));
        } else {
            s.prims.push(Prim::rod(
                v3(o.0, o.1, z - lap),
                v3(o.0, o.1, hi + lap),
                d / 2.0,
                d / 2.0,
                (false, false),
                part,
            ));
        }
        z = hi;
    }
    if shared {
        prof.push((wide[2] - 1.5, d / 2.0));
        s.prims.push(Prim::new(Shape::Rev { a: v3(o.0, o.1, 1.5), axis: v3(0.0, 0.0, 1.0), profile: prof }, part));
    }
    s.root("a-root", o, d, 4.0);
    let foot = v3(o.0, o.1, ROOT_TOP);
    s.supports.push(plate_sup("a", tip, foot, d, vec![foot, wide], vec![]));
    Ok(())
}

fn taper(s: &mut Scene, d0: f64, d1: f64, steps: usize) -> R {
    s.slab();
    let o = (2.2, 1.4);
    let (contact, normal) = hit_up(s, o.0, o.1)?;
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, d1, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    let expect = strs(&["shaft", "cone_other", "post"]);
    s.rod_full(
        "a-shaft",
        "shaft",
        v3(o.0, o.1, 1.5),
        wide,
        d0 / 2.0,
        Some(d1 / 2.0),
        (false, true),
        true,
        Some(expect),
        None,
    );
    s.root("a-root", o, d0, 4.0);
    let foot = v3(o.0, o.1, ROOT_TOP);
    s.supports.push(plate_sup("a", tip, foot, (d0 + d1) / 2.0, vec![foot, wide], vec![]));
    if steps > 1 {
        let (z0, z1) = (1.5, wide[2]);
        let k = s.supports.len() - 1;
        stepped(&mut s.supports[k].l, &|x| (d0 + (d1 - d0) * (ROOT_TOP + x - z0) / (z1 - z0)) / 2.0, steps, &[]);
        s.notes.push(format!("A {:.1}:1 taper: written as {steps} constant-width segments (owner's stepped-taper rule); shaft_d stays the mean", d0 / d1));
    } else {
        s.notes.push("DragonFruit has one width per segment here, so the oracle draws the mean width".into());
    }
    Ok(())
}

/// A trunk whose lowest `length` mm (from the root top) taper from d0 up to d1, then a d1 rod to the tip.
fn part_taper(s: &mut Scene, d0: f64, d1: f64, length: f64, steps: usize) -> R {
    s.slab();
    let o = (2.2, 1.4);
    let (contact, normal) = hit_up(s, o.0, o.1)?;
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, d1, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    let z1 = ROOT_TOP + length;
    let expect = strs(&["shaft", "cone_other", "post"]);
    let ra = d0 / 2.0 + (d0 - d1) / 2.0 * 0.5 / length;
    s.rod_full(
        "a-cone",
        "shaft",
        v3(o.0, o.1, 1.5),
        v3(o.0, o.1, z1),
        ra,
        Some(d1 / 2.0),
        (false, true),
        true,
        Some(expect),
        None,
    );
    s.rod_ext("a-shaft", "shaft", v3(o.0, o.1, z1), wide, d1 / 2.0, None, (false, true));
    s.root("a-root", o, d0, 4.0);
    let foot = v3(o.0, o.1, ROOT_TOP);
    s.supports.push(plate_sup("a", tip, foot, d1, vec![foot, wide], vec![]));
    if steps > 1 {
        let k = s.supports.len() - 1;
        stepped(&mut s.supports[k].l, &|x| (d0 + (d1 - d0) * (x / length).min(1.0)) / 2.0, steps, &[]);
    }
    let tail = if steps > 1 { format!("{steps} width steps") } else { "under the step threshold, one width".into() };
    s.notes.push(format!("A {:.2}:1 taper over {:.0} mm under a {} mm rod: {tail}", d0 / d1, length, pyf(d1)));
    Ok(())
}

fn t12_taper_long(s: &mut Scene) -> R {
    s.slab_with(1.0, 64.0, 4.0, &[], (-20.0, -12.0), (20.0, 12.0));
    let o = (2.2, 1.4);
    let (contact, normal) = hit_up(s, o.0, o.1)?;
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, 1.3, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    let expect = strs(&["shaft", "cone_other", "post"]);
    s.rod_full("a-shaft", "shaft", v3(o.0, o.1, 1.5), wide, 1.3, Some(0.65), (false, true), true, Some(expect), None);
    s.root("a-root", o, 2.6, 4.0);
    let foot = v3(o.0, o.1, ROOT_TOP);
    s.supports.push(plate_sup("a", tip, foot, 1.95, vec![foot, wide], vec![]));
    let z1 = wide[2];
    let k = s.supports.len() - 1;
    stepped(&mut s.supports[k].l, &|x| (2.6 + (1.3 - 2.6) * (ROOT_TOP + x - 1.5) / (z1 - 1.5)) / 2.0, 3, &[]);
    Ok(())
}

fn t13_taper_post(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (-3.0, 0.0), &t())?;
    let top = 14.0;
    s.rod_ext(
        "p-post",
        "post",
        v3(3.0, 0.0, 1.5),
        v3(3.0, 0.0, top),
        1.0,
        Some(0.5 + 0.5 * (top - 1.5) / (top - 1.5) * 0.0),
        (true, false),
    );
    s.root("p-root", (3.0, 0.0), 2.0, 4.0);
    let (foot, topp) = (v3(3.0, 0.0, ROOT_TOP), v3(3.0, 0.0, top));
    s.posts.push(Post {
        name: "p".into(),
        foot,
        top: topp,
        d: 1.5,
        root_d: 4.0,
        l: Line { line: vec![foot, topp], ..Default::default() },
        removed: false,
    });
    let p = s.posts.len() - 1;
    s.brace("br", Sp(a), 9.0, Pp(p), 13.0, 0.6)?;
    stepped(&mut s.posts[p].l, &|x| (2.0 + (1.0 - 2.0) * (ROOT_TOP + x - 1.5) / (top - 1.5)) / 2.0, 3, &[]);
    s.notes.push("A post narrowing 2:1 to a brace at its top: three width steps".into());
    Ok(())
}

fn t14_taper_knot(s: &mut Scene) -> R {
    s.slab();
    let o = (0.0, 0.0);
    let (contact, normal) = hit_up(s, o.0, o.1)?;
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, 0.9, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    let expect = strs(&["shaft", "cone_other", "post"]);
    s.rod_full("a-shaft", "shaft", v3(0.0, 0.0, 1.5), wide, 0.9, Some(0.45), (false, true), true, Some(expect), None);
    s.root("a-root", o, 1.8, 4.0);
    let foot = v3(0.0, 0.0, ROOT_TOP);
    s.supports.push(plate_sup("a", tip, foot, 1.35, vec![foot, wide], vec![]));
    let a = s.supports.len() - 1;
    let z1 = wide[2];
    s.branch("b", Sp(a), 10.5, (7.0, 0.0), &tb(), false, None)?;
    stepped(
        &mut s.supports[a].l,
        &|x| (1.8 + (0.9 - 1.8) * (ROOT_TOP + x - 1.5) / (z1 - 1.5)) / 2.0,
        3,
        &[10.5 - ROOT_TOP],
    );
    s.notes.push("The step nearest the branch's knot (z 10.5) is cut there, not beside it".into());
    Ok(())
}

fn t15_taper_kick(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    let knot = s.on(Sp(a), 12.0)?;
    let o = v3(6.0, 0.0, ROOT_TOP);
    let u = knot - o;
    let low = o - u * ((ROOT_TOP - 1.5) / u[2]);
    s.rod_ext("k-leg", "leg", low, knot, 0.8, Some(0.4), (true, true));
    s.root("k-root", (6.0, 0.0), 1.6, 3.0);
    let mut k = Kick {
        name: "k".into(),
        host: Sp(a),
        knot,
        foot: o,
        d: 1.2,
        root_d: 3.0,
        l: Line { line: vec![o, knot], ..Default::default() },
    };
    let big_l = (knot - low).norm();
    let off = (o - low).norm();
    stepped(&mut k.l, &|x| (1.6 + (0.8 - 1.6) * (off + x) / big_l) / 2.0, 3, &[]);
    s.kicks.push(k);
    s.notes.push("A leg narrowing 2:1 up to its knot: three width steps (DragonFruit's own kickstand taper is drawn but not sliced)".into());
    Ok(())
}

fn t09_chamfer(s: &mut Scene) -> R {
    s.slab();
    let o = (2.2, 1.4);
    let (contact, normal) = hit_up(s, o.0, o.1)?;
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, 1.0, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    let length = wide[2] - 1.5;
    s.rev(
        "a-shaft",
        "shaft",
        v3(o.0, o.1, 1.5),
        v3(0.0, 0.0, 1.0),
        vec![(0.0, 0.3), (0.3, 0.5), (length - 0.3, 0.5), (length, 0.3)],
        0.5,
        0.5,
        true,
    );
    s.root("a-root", o, 1.0, 4.0);
    let foot = v3(o.0, o.1, ROOT_TOP);
    s.supports.push(plate_sup("a", tip, foot, 1.0, vec![foot, wide], vec![]));
    Ok(())
}

// ---- tips ----
fn tip_kind(s: &mut Scene, o: TipArgs) -> R {
    s.slab();
    s.trunk("a", (2.2, 1.4), &o)?;
    Ok(())
}

fn bent_tip(s: &mut Scene, deg: f64) -> R {
    s.slab();
    let n = s.n;
    let r = 0.5;
    let joint = v3(0.0, 0.0, 16.0);
    let direction = v3(radians(deg).tan(), 0.0, 1.0).unit();
    let (contact, normal) = s.hit(joint, direction)?;
    let narrow = contact + direction * 0.3;
    let bis = (v3(0.0, 0.0, 1.0) + direction).unit();
    let (_, u, v) = geom::frame(v3(0.0, 0.0, 1.0));
    let rod_low: Vec<V3> = (0..n)
        .map(|k| {
            let ang = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            joint + (u * ang.cos() + v * ang.sin()) * r - v3(0.0, 0.0, joint[2] - 1.5)
        })
        .collect();
    let zax = v3(0.0, 0.0, 1.0);
    let mitre: Vec<V3> = rod_low
        .iter()
        .map(|&p| {
            let tt = (joint - p).dot(bis) / zax.dot(bis);
            p + zax * tt
        })
        .collect();
    let rn = 0.2;
    let ring_n: Vec<V3> = mitre
        .iter()
        .map(|&q| {
            let d = q - joint;
            let d = d - direction * d.dot(direction);
            narrow + d.unit() * rn
        })
        .collect();
    let mut tris: Vec<Tri> = Vec::new();
    let low_c = v3(0.0, 0.0, 1.5);
    for k in 0..n {
        let j = (k + 1) % n;
        tris.extend([
            [low_c, rod_low[j], rod_low[k]],
            [rod_low[k], rod_low[j], mitre[j]],
            [rod_low[k], mitre[j], mitre[k]],
        ]);
        tris.extend([
            [mitre[k], mitre[j], ring_n[j]],
            [mitre[k], ring_n[j], ring_n[k]],
            [narrow, ring_n[k], ring_n[j]],
        ]);
    }
    let length = (contact - joint).norm();
    let part = s.part_full("a-tip", "tip", narrow, joint, rn, Some(r), true, None);
    s.part("a-shaft", "shaft", low_c, joint, r);
    s.raw(part, tris);
    s.root("a-root", (0.0, 0.0), 1.0, 4.0);
    let at = rn + (r - rn) * 0.3 / (length + 0.3);
    s.tips.push(crate::scene::Tip {
        name: "a-tip".into(),
        contact,
        normal,
        socket: joint,
        narrow,
        contact_d: 2.0 * at,
        body_d: 1.0,
        part: Some(part),
        kind: "flat".into(),
        dir: direction,
    });
    let tip = s.tips.len() - 1;
    let foot = v3(0.0, 0.0, ROOT_TOP);
    s.supports.push(plate_sup("a", tip, foot, 1.0, vec![foot, joint], vec![]));
    Ok(())
}

fn rod_end(s: &mut Scene, d: f64, name: &str) -> R {
    s.slab();
    if name == "micro" {
        s.trunk("big", (10.0, 3.0), &t())?;
    }
    let o = v3(2.2, 1.4, 0.0);
    let (contact, normal) = s.hit(o, v3(0.0, 0.0, 1.0))?;
    let tip = s.bare_tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), d);
    s.rod_ext("a-shaft", name, v3(2.2, 1.4, 1.5), contact + v3(0.0, 0.0, 0.1), d / 2.0, None, (true, false));
    s.root("a-root", (o[0], o[1]), d, 4.0);
    let foot = v3(2.2, 1.4, ROOT_TOP);
    let sock = s.tips[tip].socket;
    s.supports.push(plate_sup("a", tip, foot, d, vec![foot, sock], vec![]));
    Ok(())
}

fn p08_thin(s: &mut Scene) -> R {
    s.slab_with(0.5, Z, 0.6, &[], (-20.0, -12.0), (20.0, 12.0));
    s.trunk("a", (2.2, 1.4), &TipArgs { bury: 1.2, dn: 0.3, ..t() })?;
    s.notes.push("The cone's narrow end emerges past the wall's far face. The contact is the near face.".into());
    Ok(())
}

fn slanted(s: &mut Scene, deg: f64, normal_tip: bool) -> R {
    s.angled(deg, Z + 6.0);
    if !normal_tip {
        s.trunk("a", (2.2, 1.4), &t())?;
        return Ok(());
    }
    // Tip along the surface normal, on a joint ball over a straight shaft.
    let o = v3(2.2, 1.4, 0.0);
    let (contact, normal) = s.hit(o, v3(0.0, 0.0, 1.0))?;
    let direction = -normal;
    let tip = s.tip("a-tip", contact, normal, direction, 3.0, 0.4, 1.0, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    s.ball("a-joint", wide, 0.65, true);
    let foot = v3(wide[0], wide[1], ROOT_TOP);
    s.rod("a-shaft", "shaft", v3(wide[0], wide[1], 1.5), wide, 0.5);
    s.root("a-root", (foot[0], foot[1]), 1.0, 4.0);
    s.supports.push(plate_sup("a", tip, foot, 1.0, vec![foot, wide], vec![]));
    Ok(())
}

fn p12_sphere(s: &mut Scene) -> R {
    s.sphere(v3(0.0, 0.0, 35.0), 15.0);
    s.trunk("a", (4.0, 2.0), &t())?;
    Ok(())
}

fn p13_coarse(s: &mut Scene) -> R {
    s.slab_with(1000.0, Z, 4.0, &[], (-20.0, -12.0), (20.0, 12.0));
    s.trunk("a", (2.2, 1.4), &t())?;
    Ok(())
}

fn p14_open(s: &mut Scene) -> R {
    s.slab_with(1.0, Z, 4.0, &["top", "left"], (-20.0, -12.0), (20.0, 12.0));
    s.trunk("a", (2.2, 1.4), &t())?;
    Ok(())
}

fn p15_nonman(s: &mut Scene) -> R {
    s.slab();
    // One plane through the slab at y = 0, sharing the grid's edges.
    let sheet = geom::grid_face(v3(-20.0, 0.0, 20.0), v3(40.0, 0.0, 0.0), v3(0.0, 0.0, 4.0), 1.0, false);
    s.model_tris.push(sheet);
    s.model_open = true;
    s.trunk("a", (2.2, 0.4), &t())?;
    Ok(())
}

fn p17_mildtaper(s: &mut Scene) -> R {
    s.slab();
    s.trunk("a", (2.2, 1.4), &TipArgs { d: 0.5, dn: 0.4, ..t() })?;
    Ok(())
}

fn p16_inverted(s: &mut Scene) -> R {
    s.slab();
    s.model_tris[0] = s.model_tris[0].iter().map(|t| [t[2], t[1], t[0]]).collect();
    s.trunk("a", (2.2, 1.4), &t())?;
    Ok(())
}

// ---- leaning tip continuing into a brace ----
fn lean_brace(s: &mut Scene, cone_only: bool, ball: bool) -> R {
    s.slab();
    if cone_only {
        let a = s.post("a", (0.0, 0.0), 18.1);
        let b = s.trunk("b", (6.0, 0.0), &t())?;
        let z0 = 20.0 - 3.0 * 0.5f64.sqrt();
        let lean =
            s.branch("lean", Pp(a), z0, (-3.0 * 0.5f64.sqrt(), 0.0), &TipArgs { dw: Some(0.8), ..tb() }, true, None)?;
        s.brace("br", Pp(a), z0, Sp(b), z0 - 6.0, 0.8)?;
        // The column under the leaning tip is that tip's own trunk, not a post with a branch off it.
        s.posts[a].removed = true;
        for part in s.parts.iter_mut() {
            if part.name == "a-post" {
                part.kind = "shaft".into();
                part.expect = strs(&["shaft", "post"]);
            }
        }
        let foot = v3(0.0, 0.0, ROOT_TOP);
        let sock = s.tips[s.supports[lean].tip].socket;
        let l = &mut s.supports[lean];
        l.base = Base::Plate { xy: [foot[0], foot[1]], root_d: 4.0 };
        l.l.line = vec![foot, sock];
        l.shaft_d = 1.0;
        s.braces.last_mut().unwrap().pa = Sp(lean);
    } else {
        let a = s.trunk("a", (0.0, 0.0), &t())?;
        let b = s.trunk("b", (6.0, 0.0), &t())?;
        s.branch("lean", Sp(a), 12.0, (-8.0, 0.0), &TipArgs { d: 0.6, ..tb() }, false, None)?;
        s.brace("br", Sp(a), 12.0, Sp(b), 6.0, 0.6)?;
        if ball {
            let c = s.on(Sp(a), 12.0)?;
            s.ball("j", c, 0.7, false);
        }
    }
    Ok(())
}

// ---- X braces ----
fn x_brace(s: &mut Scene, kind: &str, d: f64, post_d: f64, spacing: f64, z0: f64, rise: f64) -> R {
    s.slab();
    let dn = 0.4f64.min(post_d * 0.5);
    let a = s.trunk("a", (-spacing / 2.0, 0.0), &TipArgs { d: post_d, dn, ..t() })?;
    let b = s.trunk("b", (spacing / 2.0, 0.0), &TipArgs { d: post_d, dn, ..t() })?;
    match kind {
        "sep" => {
            s.brace("x1", Sp(a), z0, Sp(b), z0 + rise, d)?;
            s.brace("x2", Sp(a), z0 + rise, Sp(b), z0, d)?;
        }
        "welded" => {
            s.brace_full("x1", Sp(a), z0, Sp(b), z0 + rise, d, (0.0, 0.0), Some("x".into()))?;
            s.brace_full("x2", Sp(a), z0 + rise, Sp(b), z0, d, (0.0, 0.0), Some("x".into()))?;
        }
        "cut" => {
            let (p1, p2) = (s.on(Sp(a), z0)?, s.on(Sp(b), z0 + rise)?);
            s.on(Sp(a), z0 + rise)?;
            s.on(Sp(b), z0)?;
            let mid = (p1 + p2) / 2.0;
            for (name, pa, pz, pb, qz) in [("x1", Sp(a), z0, Sp(b), z0 + rise), ("x2", Sp(a), z0 + rise, Sp(b), z0)] {
                let (u, w) = (s.on(pa, pz)?, s.on(pb, qz)?);
                let part = s.part(name, "brace", u, w, d / 2.0);
                s.braces.push(Brace { name: name.into(), part, pa, pb, a: u, b: w, d, hosts_any: None });
                for (lo, hi) in [(u, mid), (mid, w)] {
                    s.prims.push(Prim::rod(lo, hi, d / 2.0, d / 2.0, (true, true), part));
                }
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

// ---- zigzag, rungs and brace ends ----
fn two(s: &mut Scene, xa: f64, xb: f64) -> Result<(PRef, PRef), String> {
    let a = s.trunk("a", (xa, 0.0), &t())?;
    let b = s.trunk("b", (xb, 0.0), &t())?;
    Ok((Sp(a), Sp(b)))
}

fn z01_chain(s: &mut Scene) -> R {
    s.slab();
    let (a, b) = two(s, -3.0, 3.0)?;
    s.brace("z1", a, 6.0, b, 9.0, 0.6)?;
    s.brace("z2", b, 9.0, a, 12.0, 0.6)?;
    s.brace("z3", a, 12.0, b, 15.0, 0.6)?;
    Ok(())
}

fn z02_rungs(s: &mut Scene) -> R {
    s.slab();
    let (a, b) = two(s, -3.0, 3.0)?;
    s.brace("r1", a, 8.0, b, 8.0, 0.6)?;
    s.brace("r2", a, 12.0, b, 12.0, 0.6)?;
    Ok(())
}

fn z03_ballend(s: &mut Scene) -> R {
    s.slab();
    let (a, b) = two(s, -4.0, 4.0)?;
    let c = s.on(a, 8.0)?;
    s.ball("j", c, 0.9, false);
    s.on(b, 12.0)?;
    s.brace_full("br", a, 8.0, b, 12.0, 0.6, (0.85, 0.0), None)?;
    Ok(())
}

fn short_of(s: &mut Scene, gap: f64) -> R {
    s.slab();
    let (a, b) = two(s, -4.0, 4.0)?;
    s.brace("y", a, 8.0, b, 14.0, 0.6)?;
    let p = Pp(s.post_full("p", (0.0, 2.4), 15.0, 0.6, 4.0, true, (0.0, 0.0), None));
    // A level brace from p towards y's middle, ending gap short of y's surface.
    let mid = (s.on(a, 8.0)? + s.on(b, 14.0)?) / 2.0;
    let start = s.on(p, mid[2])?;
    let u = (mid - start).unit();
    let end = mid - u * (0.3 + gap);
    let idx = s.rod_ext("h", "brace", start, end, 0.3, None, (true, false));
    s.braces.push(Brace {
        name: "h".into(),
        part: idx,
        pa: p,
        pb: a,
        a: start,
        b: end,
        d: 0.6,
        hosts_any: Some(strs(&["a", "b"])),
    });
    Ok(())
}

fn raft_band(s: &mut Scene, low: f64) -> R {
    s.slab();
    let (a, b) = two(s, -3.0, 3.0)?;
    s.brace("br", a, low + 6.0, b, low, 0.8)?;
    Ok(())
}

fn short_brace(s: &mut Scene, length: f64, d: f64, post_d: f64) -> R {
    s.slab();
    // Axis to axis length; a slight rise so that it is a diagonal.
    let rise = length * radians(25.0).sin();
    let span = length * radians(25.0).cos();
    let o = TipArgs { d: post_d, dw: Some(post_d), rootd: 1.5, ..t() };
    let a = s.trunk("a", (-span / 2.0, 0.0), &o)?;
    let b = s.trunk("b", (span / 2.0, 0.0), &o)?;
    s.brace("br", Sp(a), 10.0, Sp(b), 10.0 + rise, d)?;
    Ok(())
}

// ---- fans and trees ----
fn fan_alt(s: &mut Scene, n: i64) {
    s.counts_alt = json!([{"trunks": 1, "branches": n - 1, "posts": 0, "struts": 0, "kickstands": 0, "braces": 0}]);
}

fn fan(s: &mut Scene, pts: &[(f64, f64)], hub: bool) -> R {
    s.slab();
    let p = Pp(s.post_full("p", (0.0, 0.0), 16.5, 1.0, 4.0, false, (0.0, 0.0), Some(strs(&["post", "shaft"]))));
    fan_alt(s, pts.len() as i64);
    if hub {
        let c = s.on(p, 16.5)?;
        s.ball("hub", c, 0.8, false);
    }
    for (i, &xy) in pts.iter().enumerate() {
        s.branch(&format!("f{i}"), p, 16.5, xy, &tb(), true, None)?;
    }
    Ok(())
}

fn f03_two(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (0.0, 0.0), &TipArgs { length: 5.0, ..t() })?;
    let socket = s.tips[s.supports[a].tip].socket;
    let l0 = s.supports[a].l.line[0];
    s.supports[a].l.line = vec![l0, socket];
    s.branch("b", Sp(a), socket[2], (3.5, 0.0), &TipArgs { d: 1.0, ..tb() }, true, None)?;
    s.tree_free = true;
    s.counts_alt = json!([{"trunks": 0, "branches": 2, "posts": 1, "struts": 0, "kickstands": 0, "braces": 0}]);
    Ok(())
}

fn f04_branch(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    s.branch("b", Sp(a), 9.0, (7.0, 0.0), &tb(), false, None)?;
    Ok(())
}

fn sleeve(s: &mut Scene, off: f64, note: &str) -> R {
    s.slab();
    s.boxed(v3(5.0, -3.0, 18.0), v3(9.0, 3.0, 20.0));
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    s.branch("b", Sp(a), 9.0, (7.0, 0.0), &tb(), false, Some(18.0))?;
    s.rod_full("b-sleeve", "shaft", v3(off, 0.0, 1.5), v3(off, 0.0, 9.0), 0.5, None, (true, true), false, None, None);
    s.notes.push(note.into());
    Ok(())
}

fn s14_sleeve_twin(s: &mut Scene) -> R {
    s.slab();
    s.boxed(v3(4.0, -3.0, 18.0), v3(8.0, 3.0, 20.0));
    s.boxed(v3(-8.0, -3.0, 18.0), v3(-4.0, 3.0, 20.0));
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    s.branch("b", Sp(a), 6.0, (6.0, 0.0), &tb(), false, Some(18.0))?;
    s.branch("c", Sp(a), 9.0, (-6.0, 0.0), &tb(), false, Some(18.0))?;
    s.rod_full("b-sleeve", "shaft", v3(0.05, 0.0, 1.5), v3(0.05, 0.0, 6.0), 0.5, None, (true, true), false, None, None);
    s.rod_full(
        "c-sleeve",
        "shaft",
        v3(-0.05, 0.0, 1.5),
        v3(-0.05, 0.0, 9.0),
        0.5,
        None,
        (true, true),
        false,
        None,
        None,
    );
    s.notes
        .push("Two branches' shafts doubled down one trunk's column to the plate: both sleeves of the trunk.".into());
    Ok(())
}

fn s15_sleeve_thin(s: &mut Scene) -> R {
    s.slab();
    s.boxed(v3(5.0, -3.0, 18.0), v3(9.0, 3.0, 20.0));
    let a = s.trunk("a", (0.0, 0.0), &TipArgs { d: 1.5, ..t() })?;
    s.branch("b", Sp(a), 9.0, (7.0, 0.0), &TipArgs { d: 1.0, ..tb() }, false, Some(18.0))?;
    s.rod_full("b-sleeve", "shaft", v3(0.0, 0.0, 1.5), v3(0.0, 0.0, 9.0), 0.5, None, (true, true), false, None, None);
    s.notes.push("A thin rod wholly inside a wider trunk: a sleeve.".into());
    Ok(())
}

fn f05_branch2(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    let b = s.branch("b", Sp(a), 6.0, (8.0, 0.0), &tb(), false, None)?;
    s.branch("c", Sp(b), 11.0, (6.5, 4.0), &tb(), false, None)?;
    Ok(())
}

// ---- posts and legs ----
fn s01_post(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (-3.0, 0.0), &t())?;
    let p = s.post("p", (3.0, 0.0), 12.0);
    s.brace("br", Sp(a), 7.0, Pp(p), 11.0, 0.6)?;
    Ok(())
}

fn s02_kick(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    s.kick("k", Sp(a), 11.0, (5.0, 0.0), 0.8, 3.0)?;
    Ok(())
}

fn s03_leg(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (-3.0, 0.0), &t())?;
    let p = s.post("p", (3.0, 0.0), 12.0);
    s.brace("br", Sp(a), 7.0, Pp(p), 11.0, 0.6)?;
    s.kick("k", Pp(p), 9.0, (7.0, 0.0), 0.8, 3.0)?;
    Ok(())
}

fn s04_stub(s: &mut Scene) -> R {
    s.slab();
    let (a, b) = two(s, -3.0, 3.0)?;
    s.brace("br", a, 5.0, b, 5.0, 0.6)?;
    s.post_full("stub", (0.0, 0.0), 4.9, 1.0, 1.0, false, (0.0, 0.0), Some(strs(&["brace", "post"])));
    s.counts_alt = json!([{"trunks": 2, "branches": 0, "posts": 0, "struts": 0, "kickstands": 0, "braces": 2},
                          {"trunks": 2, "branches": 0, "posts": 0, "struts": 0, "kickstands": 1, "braces": 1}]);
    s.notes.push("Owner on A12: 'could be a brace or a leg, really a brace'. Either label passes; it must be exported as a part.".into());
    Ok(())
}

fn touching(s: &mut Scene, gap: f64, along_y: bool) -> R {
    s.slab();
    let d = 1.0;
    if along_y {
        s.trunk("a", (0.0, -(d + gap) / 2.0), &t())?;
        s.trunk("b", (0.0, (d + gap) / 2.0), &t())?;
    } else {
        s.trunk("a", (-(d + gap) / 2.0, 0.0), &t())?;
        s.trunk("b", ((d + gap) / 2.0, 0.0), &t())?;
    }
    Ok(())
}

fn near_rods(s: &mut Scene) -> R {
    s.slab();
    s.trunk("a", (-0.05, 0.0), &t())?;
    s.post("b", (0.05, 0.0), 12.0);
    Ok(())
}

fn parallel_thin(s: &mut Scene, spacing: f64, d: f64) -> R {
    s.slab();
    let o = TipArgs { d, dn: 0.3, rootd: 1.5, ..t() };
    s.trunk("a", (-spacing / 2.0 + 3.0, 0.0), &o)?;
    s.trunk("b", (spacing / 2.0 + 3.0, 0.0), &o)?;
    Ok(())
}

fn parallel_braces(s: &mut Scene, spacing: f64, d: f64) -> R {
    s.slab();
    let (a, b) = two(s, -4.0, 4.0)?;
    s.brace("p1", a, 9.0, b, 13.0, d)?;
    s.brace("p2", a, 9.0 + spacing, b, 13.0 + spacing, d)?;
    Ok(())
}

// ---- micro ----
fn micro_model(s: &mut Scene, d: f64, span: f64) -> R {
    s.slab();
    s.boxed(v3(-7.0, -6.0, 8.0), v3(7.0, 6.0, 20.0 - span));
    s.trunk("big", (11.0, 3.0), &t())?;
    let (low, up) = (v3(2.2, 1.4, 20.0 - span), v3(2.2, 1.4, 20.0));
    let tu = s.bare_tip("m-tip", up, v3(0.0, 0.0, -1.0), v3(0.0, 0.0, 1.0), d);
    let tl = s.bare_tip("m-foot", low, v3(0.0, 0.0, 1.0), v3(0.0, 0.0, -1.0), d);
    s.rod_ext("m", "micro", low - v3(0.0, 0.0, 0.1), up + v3(0.0, 0.0, 0.1), d / 2.0, None, (false, false));
    // Flat rod ends: a twig, whatever its span (the owner's rule for struts and micro supports alike).
    s.written.push(("m".into(), "twig"));
    let line = vec![s.tips[tl].socket, s.tips[tu].socket];
    s.supports.push(Sup {
        name: "m".into(),
        tip: tu,
        base: Base::Lower { tip: tl },
        shaft_d: d,
        l: Line { line, ..Default::default() },
    });
    Ok(())
}

fn micro_pillar(s: &mut Scene, d: f64) -> R {
    s.slab();
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    let at = s.on(Sp(a), 14.0)?;
    let aim = v3(3.0, 0.0, 20.0);
    let direction = (aim - at).unit();
    let (contact, normal) = s.hit(at, direction)?;
    let tip = s.bare_tip("m-tip", contact, normal, direction, d);
    s.rod_ext("m", "micro", at, contact + direction * 0.1, d / 2.0, None, (true, false));
    let line = vec![at, s.tips[tip].socket];
    s.supports.push(Sup {
        name: "m".into(),
        tip,
        base: Base::Parent { host: Sp(a), at },
        shaft_d: d,
        l: Line { line, ..Default::default() },
    });
    Ok(())
}

fn m04_midair(s: &mut Scene) -> R {
    s.slab();
    s.trunk("a", (6.0, 0.0), &t())?;
    s.rod_full("m", "floating", v3(-5.0, 3.0, 12.0), v3(-5.0, 3.0, 20.1), 0.3, None, (false, false), false, None, None);
    Ok(())
}

// ---- rafts ----
fn raft_scene(s: &mut Scene, kind: &str) -> R {
    s.slab();
    let pts = [(-4.0, -4.0), (4.0, -4.0), (-4.0, 4.0), (4.0, 4.0)];
    let iname = |x: f64| format!("{}", x as i64);
    match kind {
        "tiles" => {
            for x in [-6.0, -2.0, 2.0, 6.0] {
                for y in [-6.0, -2.0, 2.0, 6.0] {
                    s.raft_box(
                        &format!("tile{}{}", iname(x), iname(y)),
                        v3(x - 2.2, y - 2.2, 0.0),
                        v3(x + 2.2, y + 2.2, 1.2),
                    );
                }
            }
        }
        "thick" => {
            for x in [-4.0, 4.0] {
                for y in [-4.0, 4.0] {
                    s.raft_box(
                        &format!("tile{}{}", iname(x), iname(y)),
                        v3(x - 4.2, y - 4.2, 0.0),
                        v3(x + 4.2, y + 4.2, 3.6),
                    );
                }
            }
        }
        "discs" => {
            for &(x, y) in &pts {
                let idx = s.part_full(
                    &format!("disc{}{}", iname(x), iname(y)),
                    "raft",
                    v3(x, y, 0.0),
                    v3(x, y, 1.4),
                    3.0,
                    None,
                    false,
                    None,
                );
                let mut p = Prim::new(
                    Shape::Rev { a: v3(x, y, 0.0), axis: v3(0.0, 0.0, 1.0), profile: vec![(0.0, 3.0), (1.4, 3.0)] },
                    idx,
                );
                p.raft = true;
                s.prims.push(p);
            }
        }
        "rim" => {
            s.raft_box("plate", v3(-9.0, -9.0, 0.0), v3(9.0, 9.0, 1.0));
            for (name, lo, hi) in [
                ("r1", v3(-9.0, -9.0, 0.0), v3(9.0, -8.0, 2.5)),
                ("r2", v3(-9.0, 8.0, 0.0), v3(9.0, 9.0, 2.5)),
                ("r3", v3(-9.0, -9.0, 0.0), v3(-8.0, 9.0, 2.5)),
                ("r4", v3(8.0, -9.0, 0.0), v3(9.0, 9.0, 2.5)),
            ] {
                s.raft_box(name, lo, hi);
            }
        }
        "web" => {
            for (i, &(x, y)) in pts.iter().enumerate() {
                let (x2, y2) = if i != 1 { pts[(i + 1) % 4] } else { pts[3] };
                let idx =
                    s.part_full(&format!("bar{i}"), "raft", v3(x, y, 0.8), v3(x2, y2, 0.8), 0.5, None, false, None);
                let mut p = Prim::rod(v3(x, y, 0.8), v3(x2, y2, 0.8), 0.5, 0.5, (false, false), idx);
                p.raft = true;
                s.prims.push(p);
                let b = s.part_full(&format!("node{i}"), "raft", v3(x, y, 0.8), v3(x, y, 0.8), 0.9, None, false, None);
                let mut p = Prim::new(
                    Shape::Rev {
                        a: v3(x, y, -0.1),
                        axis: v3(0.0, 0.0, 1.0),
                        profile: geom::ball_profile(0.9, s.n.max(6)),
                    },
                    b,
                );
                p.raft = true;
                s.prims.push(p);
            }
        }
        _ => unreachable!(),
    }
    for (i, &xy) in pts[..2].iter().enumerate() {
        let rootd = if ["tiles", "rim", "thick"].contains(&kind) { 2.0 } else { 4.0 };
        s.trunk(&format!("t{i}"), xy, &TipArgs { rootd, ..t() })?;
    }
    s.brace("br", Sp(0), 9.0, Sp(1), 13.0, 0.6)?;
    Ok(())
}

/// Owner, 2026-11-10: the real pads behind I12 are about 1.5 mm tall or less, inside the raft band. Was 3.6.
pub const PAD_H: f64 = 1.5;

fn pad_prim(s: &mut Scene, top: f64) {
    let idx = s.part_full("pad", "raft", v3(0.0, 0.0, 0.0), v3(0.0, 0.0, top), 8.0, None, false, None);
    let mut p = Prim::new(
        Shape::Rev { a: V3::ZERO, axis: v3(0.0, 0.0, 1.0), profile: vec![(0.0, 8.0), (0.6, 8.0), (top, 7.0)] },
        idx,
    );
    p.raft = true;
    s.prims.push(p);
}

fn big_pad(s: &mut Scene, shell: bool) -> R {
    s.slab();
    let (contact, normal) = s.hit(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))?;
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, 1.0, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    if shell {
        let h = wide[2];
        let rod = s.part("a-shaft", "shaft", v3(0.0, 0.0, PAD_H), wide, 0.5);
        let pad = s.part_full("pad", "raft", v3(0.0, 0.0, 0.0), v3(0.0, 0.0, PAD_H), 8.0, None, false, None);
        let mut p = Prim::new(
            Shape::Rev {
                a: V3::ZERO,
                axis: v3(0.0, 0.0, 1.0),
                profile: vec![(0.0, 8.0), (0.6, 8.0), (PAD_H, 7.0), (PAD_H, 0.5), (h, 0.5)],
            },
            rod,
        );
        p.pad_part = Some(pad);
        s.prims.push(p);
    } else {
        pad_prim(s, PAD_H);
        s.rod("a-shaft", "shaft", v3(0.0, 0.0, PAD_H - 0.2), wide, 0.5);
    }
    let foot = v3(0.0, 0.0, ROOT_TOP);
    s.supports.push(plate_sup("a", tip, foot, 1.0, vec![foot, wide], vec![]));
    s.notes.push("The 16 mm pad is 1.5 mm tall, inside the 3.0 mm raft band (changed from 3.6 mm at the owner's request: the real pads behind I12 are 1.5 mm or less). It is raft: never a root, ball or shaft, and the post's shaft keeps its own width.".into());
    Ok(())
}

fn r04_padbrace(s: &mut Scene) -> R {
    s.slab();
    pad_prim(s, 3.6);
    let o = TipArgs { rootd: 2.0, ..t() };
    let a = s.trunk("a", (-3.0, 0.0), &o)?;
    let b = s.trunk("b", (3.0, 0.0), &o)?;
    s.brace("br", Sp(a), 4.5, Sp(b), 7.5, 0.6)?;
    s.notes.push(
        "A short brace whose low end sits just above the pad, as case I12: a brace, not a post on a root.".into(),
    );
    Ok(())
}

fn r05_padfoot(s: &mut Scene) -> R {
    s.slab();
    pad_prim(s, PAD_H);
    let o = TipArgs { rootd: 2.0, ..t() };
    let a = s.trunk("a", (-3.0, 0.0), &o)?;
    let b = s.trunk("b", (3.0, 0.0), &o)?;
    let (lo, hi) = (v3(-3.0, 0.0, PAD_H + 0.1), s.on(Sp(b), 7.5)?);
    let part = s.rod("br", "brace", lo, hi, 0.3);
    s.braces.push(Brace { name: "br".into(), part, pa: Sp(a), pb: Sp(b), a: lo, b: hi, d: 0.6, hosts_any: None });
    s.notes.push("Case I12 itself (added at the owner's request): a slanted brace whose low end lands on the 1.5 mm pad at a post's foot. It stays a brace: not a leg, post or root.".into());
    Ok(())
}

// ---- hollow ----
fn h01_strut(s: &mut Scene) -> R {
    s.hollow(3.0, 20.0, None);
    s.strut("a", v3(2.2, 1.4, 3.0), v3(2.2, 1.4, 20.0), 1.0, 3.9)?;
    Ok(())
}

fn h02_rungs(s: &mut Scene) -> R {
    s.hollow(3.0, 20.0, None);
    let a = s.strut("a", v3(-3.0, 1.4, 3.0), v3(-3.0, 1.4, 20.0), 1.0, 3.9)?;
    let b = s.strut("b", v3(3.0, 1.4, 3.0), v3(3.0, 1.4, 20.0), 1.0, 3.9)?;
    s.brace("r1", Sp(a), 9.0, Sp(b), 9.0, 0.6)?;
    s.brace("r2", Sp(a), 13.0, Sp(b), 13.0, 0.6)?;
    Ok(())
}

fn h03_vee(s: &mut Scene) -> R {
    s.hollow(3.0, 20.0, None);
    s.strut("a", v3(0.0, 0.0, 3.0), v3(-2.5, 0.0, 20.0), 1.0, 3.9)?;
    s.strut("b", v3(0.0, 0.0, 3.0), v3(2.5, 0.0, 20.0), 1.0, 3.9)?;
    Ok(())
}

fn h04_short(s: &mut Scene) -> R {
    s.hollow(3.0, 20.0, Some((v3(-8.0, -6.0, 3.0), v3(8.0, 6.0, 16.0))));
    s.strut("a", v3(2.2, 1.4, 16.0), v3(2.2, 1.4, 20.0), 1.0, 1.2)?;
    Ok(())
}

fn h05_flat_strut(s: &mut Scene) -> R {
    // A model-to-model rod with flat ends (no cones) is a twig, whatever its span.
    s.hollow(3.0, 20.0, Some((v3(-8.0, -6.0, 3.0), v3(8.0, 6.0, 16.0))));
    let (low, up) = (v3(2.2, 1.4, 16.0), v3(2.2, 1.4, 20.0));
    let tu = s.bare_tip("a-tip", up, v3(0.0, 0.0, -1.0), v3(0.0, 0.0, 1.0), 1.0);
    let tl = s.bare_tip("a-foot", low, v3(0.0, 0.0, 1.0), v3(0.0, 0.0, -1.0), 1.0);
    s.rod_full(
        "a-shaft",
        "shaft",
        low - v3(0.0, 0.0, 0.1),
        up + v3(0.0, 0.0, 0.1),
        0.5,
        None,
        (false, false),
        true,
        Some(strs(&["shaft", "contact_rod", "micro"])),
        None,
    );
    // Each rod end inside the model, bounded by one loop, is a tip (the touch rule): the end, from the rod's
    // face 0.1 mm inside the model to the tip's socket, is the tip's part. It is truth only (the rod reads as
    // one strut, so it has no reading of its own to check).
    for (tip, end) in [(tu, up + v3(0.0, 0.0, 0.1)), (tl, low - v3(0.0, 0.0, 0.1))] {
        let name = s.tips[tip].name.clone();
        let socket = s.tips[tip].socket;
        s.parts.push(Part {
            name,
            kind: "tip".into(),
            a: end,
            b: socket,
            ra: 0.5,
            rb: 0.5,
            required: false,
            expect: vec![],
        });
        s.tips[tip].part = Some(s.parts.len() - 1);
    }
    let line = vec![s.tips[tl].socket, s.tips[tu].socket];
    s.supports.push(Sup {
        name: "a".into(),
        tip: tu,
        base: Base::Lower { tip: tl },
        shaft_d: 1.0,
        l: Line { line, ..Default::default() },
    });
    s.written.push(("a".into(), "twig"));
    s.notes.push("Flat rod ends on both faces: written as a twig (contact disks), not a stick.".into());
    Ok(())
}

// ---- bends ----
fn bend_common(s: &mut Scene, tip_x: f64, bend: V3, foot: V3, ball: bool, note: &str) -> R {
    let (contact, normal) = hit_up(s, tip_x, 0.0)?;
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, 1.0, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    let low = foot + (foot - bend) * (0.5 / (bend[2] - ROOT_TOP));
    s.rod("a-shaft", "shaft", bend, wide, 0.5);
    s.rod("a-shaft2", "shaft", low, bend, 0.5);
    s.root("a-root", (foot[0], foot[1]), 1.0, 4.0);
    if ball {
        s.ball("j", bend, 0.65, false);
    }
    s.supports.push(plate_sup("a", tip, foot, 1.0, vec![foot, bend, wide], vec![bend]));
    s.notes.push(note.into());
    Ok(())
}

fn bent(s: &mut Scene, turn_deg: f64, ball: bool) -> R {
    s.slab();
    s.boxed(v3(-8.0, -6.0, 14.0), v3(3.8, 6.0, 20.0));
    let bend = v3(5.2, 0.0, 12.0);
    let tn = radians(turn_deg).tan();
    let foot = v3(5.2 - tn * (bend[2] - ROOT_TOP), 0.0, ROOT_TOP);
    bend_common(s, 5.2, bend, foot, ball, "The straight line from foot to socket clips the block.")
}

fn walk_sharp(s: &mut Scene) -> R {
    s.slab();
    s.boxed(v3(-3.0, -3.0, 6.0), v3(1.2, 3.0, 10.0));
    let (contact, normal) = (v3(0.0, 0.0, 20.0), v3(0.0, 0.0, -1.0));
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, 1.0, 0.3, "flat", None);
    let wide = s.tips[tip].socket;
    let (p1, p2) = (v3(0.0, 0.0, 12.0), v3(7.0, 0.0, 8.0));
    let low = v3(7.0, 0.0, 1.5);
    let foot = v3(7.0, 0.0, ROOT_TOP);
    s.rod("a-shaft", "shaft", p1, wide, 0.5);
    s.rod("a-shaft2", "shaft", p2, p1, 0.5);
    s.rod("a-shaft3", "shaft", low, p2, 0.5);
    s.root("a-root", (foot[0], foot[1]), 1.0, 4.0);
    s.supports.push(plate_sup("a", tip, foot, 1.0, vec![foot, p2, p1, wide], vec![p2, p1]));
    s.notes.push(
        "Turns of 60 degrees: part of the model lies under the top of the shaft, so running it straight down clips it."
            .into(),
    );
    Ok(())
}

fn grazing_tip(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (-6.0, 0.0), &t())?;
    s.branch("b", Sp(a), 16.5, (2.0, 0.0), &TipArgs { d: 0.8, length: 3.0, ..tb() }, false, None)?;
    Ok(())
}

fn short_tip(s: &mut Scene) -> R {
    s.slab();
    s.trunk("a", (2.2, 1.4), &TipArgs { length: 0.4, dn: 0.3, bury: 0.3, dw: Some(1.0), ..t() })?;
    Ok(())
}

fn bend_close(s: &mut Scene) -> R {
    s.slab();
    s.boxed(v3(-8.0, -6.0, 12.0), v3(4.655, 6.0, 20.0));
    bend_common(
        s,
        5.2,
        v3(5.2, 0.0, 14.0),
        v3(9.0, 0.0, ROOT_TOP),
        false,
        "The bend clears the model's face by 0.045 mm; DragonFruit's joint sphere is 0.1 mm wider than its shaft.",
    )
}

fn bend_shallow(s: &mut Scene) -> R {
    s.slab();
    s.boxed(v3(1.0, -2.0, 9.6), v3(4.67, 2.0, 10.4));
    bend_common(
        s,
        5.0,
        v3(5.2, 0.0, 10.0),
        v3(5.0, 0.0, ROOT_TOP),
        false,
        "A 0.2 mm dodge round a model part; straightening the line clips it by 0.17 mm.",
    )
}

fn low_tip(s: &mut Scene, z: f64) -> R {
    s.slab_with(1.0, z, 4.0, &[], (-20.0, -12.0), (20.0, 12.0));
    s.trunk("a", (2.2, 1.4), &TipArgs { rootd: 2.0, ..t() })?;
    Ok(())
}

fn short_shaft(s: &mut Scene) -> R {
    s.slab();
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    let at = s.on(Sp(a), 15.0)?;
    let d = (v3(4.0, 0.0, 20.0) - at).unit();
    let (contact, normal) = s.hit(at, d)?;
    let length = (contact - at).norm() - 0.8;
    let tip = s.tip("b-tip", contact, normal, d, length, 0.4, 0.8, 0.3, "flat", None);
    let sock = s.tips[tip].socket;
    s.rod("b-shaft", "shaft", at, sock, 0.4);
    s.supports.push(Sup {
        name: "b".into(),
        tip,
        base: Base::Parent { host: Sp(a), at },
        shaft_d: 0.8,
        l: Line { line: vec![at, sock], ..Default::default() },
    });
    Ok(())
}

/// Not one thing under test: a small lattice, to see what isolated cases hide.
fn cage(s: &mut Scene) -> R {
    s.slab_with(1.0, Z, 4.0, &[], (-16.0, -16.0), (16.0, 16.0));
    let mut grid: Vec<((i64, i64), PRef)> = Vec::new();
    for (i, x) in [-8.0, 0.0, 8.0].into_iter().enumerate() {
        for (j, y) in [-8.0, 0.0, 8.0].into_iter().enumerate() {
            let sp = s.trunk(&format!("t{i}{j}"), (x, y), &TipArgs { d: 0.8, dn: 0.35, rootd: 2.0, ..t() })?;
            grid.push(((i as i64, j as i64), Sp(sp)));
        }
    }
    let get = |g: &[((i64, i64), PRef)], k: (i64, i64)| g.iter().find(|(kk, _)| *kk == k).map(|(_, p)| *p);
    let mut k = 0;
    for &((i, j), p) in &grid {
        for (di, dj) in [(1, 0), (0, 1)] {
            let Some(q) = get(&grid, (i + di, j + dj)) else { continue };
            let m = ((i + j) % 2) as f64;
            s.brace(&format!("x{k}a"), p, 6.0 + m, q, 11.0 + m, 0.5)?;
            s.brace(&format!("x{k}b"), p, 11.0 + m, q, 6.0 + m, 0.5)?;
            k += 1;
        }
    }
    for (name, host, z, target) in [
        ("n1", (0, 0), 13.0, (-4.0, -4.0)),
        ("n2", (2, 0), 14.0, (5.0, -3.0)),
        ("n3", (1, 2), 13.5, (3.5, 5.0)),
        ("n4", (1, 1), 14.0, (2.5, 3.0)),
    ] {
        s.branch(
            name,
            get(&grid, host).unwrap(),
            z,
            target,
            &TipArgs { d: 0.6, dn: 0.3, length: 2.5, ..tb() },
            false,
            None,
        )?;
    }
    s.kick("k1", get(&grid, (0, 1)).unwrap(), 9.5, (-12.5, 0.0), 0.6, 2.0)?;
    Ok(())
}

/// Composite: Eastman-like posts tied by chained diagonals, with a fan off a post top.
fn zigzag_wall(s: &mut Scene) -> R {
    s.slab_with(1.0, Z, 4.0, &[], (-20.0, -8.0), (20.0, 8.0));
    let mut posts = Vec::new();
    for i in 0..5 {
        posts.push(Sp(s.trunk(
            &format!("t{i}"),
            (-12.0 + 6.0 * i as f64, 0.0),
            &TipArgs { d: 0.7, dn: 0.35, rootd: 2.0, ..t() },
        )?));
    }
    let mut k = 0;
    for w in posts.windows(2) {
        let (a, b) = (w[0], w[1]);
        for z0 in [5.0, 9.0, 13.0] {
            s.brace(&format!("z{k}"), a, z0, b, z0 + 2.0, 0.45)?;
            s.brace(&format!("z{k}b"), b, z0 + 2.0, a, z0 + 4.0, 0.45)?;
            k += 1;
        }
    }
    let o = TipArgs { d: 0.5, dn: 0.3, length: 2.2, ..tb() };
    s.branch("n1", posts[2], 15.5, (2.5, 2.0), &o, false, None)?;
    s.branch("n2", posts[2], 15.5, (-2.5, 2.0), &o, false, None)?;
    Ok(())
}

/// A thick polygon rod with a flat facing a wall of the model.
fn flat_wall(s: &mut Scene) -> R {
    let (gap, d) = (0.05, 3.0);
    s.slab_with(1.0, Z, 4.0, &[], (-10.0, -12.0), (30.0, 12.0));
    let a = radians(90.0 + 180.0 / s.n as f64);
    let rm = [[a.cos(), -a.sin(), 0.0], [a.sin(), a.cos(), 0.0], [0.0, 0.0, 1.0]];
    let half = v3(4.0, 8.0, 8.0);
    let n = mat_vec(&rm, v3(1.0, 0.0, 0.0));
    let r = d / 2.0;
    let flat = r * (std::f64::consts::PI / s.n as f64).cos();
    let c = -n * (flat + gap + half[0]) + v3(0.0, 0.0, 12.0);
    let bx: Vec<Tri> =
        geom::grid_box(-half, half, 1.0, &[]).iter().map(|t| t.map(|p| crate::vec::gemm_row(p, &rm) + c)).collect();
    s.model_tris.push(bx);
    s.shapes.push(json!({"kind": "obox", "c": crate::scene::lst(c), "R": rm, "half": crate::scene::lst(half)}));
    s.trunk("a", (0.0, 0.0), &TipArgs { d, dn: 1.0, length: 4.0, rootd: 6.0, ..t() })?;
    s.notes.push("A flat of the shaft clears the wall by 0.05 mm (owner, merge round 4: was 0.03 mm, the touch rule's flush gap, which made the truth count a flush contact). Read at its corners, a round shaft is wider by the polygon's sagitta (0.051 mm at 12 sides, 0.029 at 16, 0.007 at 32); 8 sides (0.114 mm) is not generated for this case.".into());
    Ok(())
}

fn down_tip(s: &mut Scene, cone_only: bool) -> R {
    s.slab();
    s.boxed(v3(3.0, -6.0, 4.0), v3(14.0, 6.0, 8.0));
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    s.branch("b", Sp(a), 12.0, (6.0, 0.0), &TipArgs { d: 0.8, ..tb() }, cone_only, Some(8.0))?;
    s.notes.push("A tip pointing down onto an upward face of the model, off a trunk that carries on below it.".into());
    Ok(())
}

/// A thin shaft on a thick post: one line, two widths.
fn width_change(s: &mut Scene) -> R {
    s.slab();
    let (r_thin, r_thick) = (0.4, 0.65);
    // A wall that clears the thin shaft by 0.1 mm, and that the thick one would clip.
    s.boxed(v3(r_thin + 0.1, -5.0, 13.0), v3(8.0, 5.0, 16.0));
    let p = s.post_full("p", (0.0, 0.0), 12.0, 2.0 * r_thick, 4.0, false, (0.0, 0.0), None);
    s.branch("b", Pp(p), 12.0, (0.0, 0.0), &TipArgs { d: 2.0 * r_thin, dn: 0.4, ..tb() }, false, None)?;
    s.notes.push("Written as one support it takes one width. Truth needs both (a post and a branch); the wall clears the thin part by 0.1 mm.".into());
    Ok(())
}

fn contact_ball(s: &mut Scene) -> R {
    s.slab();
    let o = v3(2.2, 1.4, 0.0);
    let (contact, normal) = s.hit(o, v3(0.0, 0.0, 1.0))?;
    let tip = s.tip("a-tip", contact, normal, v3(0.0, 0.0, 1.0), 3.0, 0.4, 1.0, 0.0, "flat", None);
    s.ball("a-ball", contact, 0.5, false);
    s.tips[tip].contact_d = 1.0;
    let wide = s.tips[tip].socket;
    let foot = v3(2.2, 1.4, ROOT_TOP);
    s.rod("a-shaft", "shaft", v3(2.2, 1.4, 1.5), wide, 0.5);
    s.root("a-root", (foot[0], foot[1]), 1.0, 4.0);
    s.supports.push(plate_sup("a", tip, foot, 1.0, vec![foot, wide], vec![]));
    s.notes.push("The contact is the ball's: 1 mm across at the surface against a 0.4 mm cone end.".into());
    Ok(())
}

/// A fan of rods from a hub inside the model, with no post below it.
fn model_tree(s: &mut Scene) -> R {
    s.slab();
    s.boxed(v3(-8.0, -8.0, 8.0), v3(8.0, 8.0, 12.0));
    let hub = v3(0.0, 0.0, 11.5);
    let mut first: Option<usize> = None;
    for (i, (x, y)) in [(3.0, 0.0), (-3.0, 0.0), (0.0, 3.0)].into_iter().enumerate() {
        let contact = v3(x * 1.2, y * 1.2, Z);
        let direction = (contact - hub).unit();
        let (contact, normal) = s.hit(hub + direction * 2.0, direction)?;
        let tip = s.tip(&format!("f{i}-tip"), contact, normal, direction, 3.0, 0.4, 1.0, 0.3, "flat", None);
        let wide = s.tips[tip].socket;
        s.rod_ext(&format!("f{i}-shaft"), "shaft", hub, wide, 0.5, None, (false, true));
        match first {
            None => {
                let entry = hub + direction * ((12.0 - hub[2]) / direction[2]);
                let low = s.bare_tip(&format!("f{i}-foot"), entry, v3(0.0, 0.0, 1.0), -direction, 1.0);
                let line = vec![s.tips[low].socket, wide];
                s.supports.push(Sup {
                    name: format!("f{i}"),
                    tip,
                    base: Base::Lower { tip: low },
                    shaft_d: 1.0,
                    l: Line { line, ..Default::default() },
                });
                first = Some(s.supports.len() - 1);
            }
            Some(f) => {
                let at = s.supports[f].l.line[0];
                s.supports.push(Sup {
                    name: format!("f{i}"),
                    tip,
                    base: Base::Parent { host: Sp(f), at },
                    shaft_d: 1.0,
                    l: Line { line: vec![hub, wide], ..Default::default() },
                });
            }
        }
    }
    s.tree_free = true;
    s.passes_through = true; // Each rod enters the block on its own way to the hub.
    s.notes.push(
        "Nothing stands under the hub: set on the plate, the shaft passes right through the lower part of the model."
            .into(),
    );
    Ok(())
}

fn lower_block(s: &mut Scene, skip: &[&str]) {
    s.slab();
    let (lo, hi) = (v3(3.0, -6.0, 4.0), v3(14.0, 6.0, 8.0));
    s.model_tris.push(geom::grid_box(lo, hi, 1.0, skip));
    s.shapes.push(json!({"kind": "box", "lo": crate::scene::lst(lo), "hi": crate::scene::lst(hi)}));
    s.model_open = !skip.is_empty();
}

/// A blunt rod from a trunk whose low end stops inside the block: a tip by the owner's rule.
fn low_end_tip(s: &mut Scene, a: usize) -> R {
    let at = s.on(Sp(a), 14.0)?;
    let aim = v3(5.5, 0.0, 8.0);
    let d = (aim - at).unit();
    let (contact, normal) = s.hit(at, d)?;
    let tip = s.bare_tip("e-tip", contact, normal, d, 0.6);
    s.rod_full(
        "e",
        "contact_rod",
        at,
        contact + d * 0.3,
        0.3,
        None,
        (true, false),
        true,
        Some(strs(&["contact_rod", "micro"])),
        None,
    );
    let sock = s.tips[tip].socket;
    s.supports.push(Sup {
        name: "e".into(),
        tip,
        base: Base::Parent { host: Sp(a), at },
        shaft_d: 0.6,
        l: Line { line: vec![at, sock], ..Default::default() },
    });
    Ok(())
}

fn end_in(s: &mut Scene, skip: &[&str], sheet: bool) -> R {
    lower_block(s, skip);
    if sheet {
        s.model_tris.push(geom::grid_face(v3(3.0, 0.0, 4.0), v3(11.0, 0.0, 0.0), v3(0.0, 0.0, 4.0), 1.0, false));
        s.model_open = true;
    }
    let a = s.trunk("a", (0.0, 0.0), &t())?;
    low_end_tip(s, a)
}

fn n04_buried(s: &mut Scene) -> R {
    lower_block(s, &[]);
    s.trunk("a", (0.0, 0.0), &t())?;
    s.rod_full("inner", "raft", v3(5.0, 0.0, 5.0), v3(12.0, 0.0, 7.0), 0.3, None, (false, false), false, None, None);
    s.passes_through = true;
    Ok(())
}

fn through_wall(s: &mut Scene, skip: &[&str]) -> R {
    s.slab();
    let (lo, hi) = (v3(-10.0, -10.0, 12.0), v3(10.0, 10.0, 13.0));
    s.model_tris.push(geom::grid_box(lo, hi, 1.0, skip));
    s.shapes.push(json!({"kind": "box", "lo": crate::scene::lst(lo), "hi": crate::scene::lst(hi)}));
    s.model_open = !skip.is_empty();
    s.trunk("a", (2.2, 1.4), &t())?;
    s.passes_through = true;
    s.notes.push("The shaft passes through a thin wall on its way: truth has that crossing too.".into());
    Ok(())
}

fn n07_cavity(s: &mut Scene) -> R {
    s.hollow(9.0, 20.0, None);
    s.rod_full("c", "raft", v3(2.2, 1.4, 1.5), v3(2.2, 1.4, 13.0), 0.5, None, (false, false), false, None, None);
    s.root("c-root", (2.2, 1.4), 1.0, 4.0);
    s.flex = 1;
    s.passes_through = true;
    s.notes.push("A rod crossing the shell and ending in the cavity: neither a contact nor required.".into());
    Ok(())
}

fn n08_close(s: &mut Scene) -> R {
    s.slab();
    let o = TipArgs { d: 0.3, dn: 0.12, dw: Some(0.3), length: 1.5, rootd: 2.0, ..t() };
    s.trunk("a", (2.0, 1.4), &o)?;
    s.trunk("b", (2.4, 1.4), &o)?;
    Ok(())
}

fn n09_tilted(s: &mut Scene) -> R {
    s.angled(-20.0, 26.0);
    let lean = (-radians(-20.0).tan(), 0.0);
    let a = s.trunk("a", (-4.0, 0.0), &TipArgs { lean, ..t() })?;
    let b = s.trunk("b", (4.0, 0.0), &TipArgs { lean, ..t() })?;
    s.brace("br", Sp(a), 9.0, Sp(b), 13.0, 0.6)?;
    Ok(())
}

/// Python's str() of a float with the '.' removed, as the case ids use it.
fn tag(x: f64) -> String {
    pyf(x).replace('.', "")
}

/// The CASES registry, in gen.py's order.
pub fn cases() -> Vec<CaseDef> {
    let mut c: Vec<CaseDef> = Vec::new();
    let mut add = |id: &str, title: &str, real: &str, styles: &[&'static str], run: CaseFn| {
        c.push(CaseDef { id: id.into(), title: title.into(), real: real.into(), styles: styles.to_vec(), run });
    };
    add(
        "t01_trunk",
        "Straight trunk on a root",
        "",
        ALL,
        Box::new(|s| {
            s.slab();
            s.trunk("a", (2.2, 1.4), &t())?;
            Ok(())
        }),
    );
    add("t02_tilt10", "Tilted trunk, 10 degrees", "", ALL, Box::new(|s| tilted(s, 10.0)));
    add("t03_tilt25", "Tilted trunk, 25 degrees", "", ALL, Box::new(|s| tilted(s, 25.0)));
    add(
        "t04_rings_short",
        "Shaft cut into 0.6 mm sections, separate shells overlapping 0.05 mm",
        "",
        SEP,
        Box::new(|s| rings_shaft(s, 0.6, false, 0.05)),
    );
    add(
        "t04_rings_abut",
        "Shaft cut into 0.6 mm sections, ends exactly meeting (their caps weld)",
        "",
        SEP,
        Box::new(|s| rings_shaft(s, 0.6, false, 0.0)),
    );
    add(
        "t05_rings_long",
        "Shaft cut into 2 mm sections, separate shells overlapping 0.05 mm",
        "",
        SEP,
        Box::new(|s| rings_shaft(s, 2.0, false, 0.05)),
    );
    add(
        "t06_rings_shared",
        "Shaft with many rings in one shell",
        "",
        SEP,
        Box::new(|s| rings_shaft(s, 0.6, true, 0.0)),
    );
    add(
        "t07_taper_strong",
        "Tapered shaft, 1.6 to 0.8 mm (2:1, three width steps)",
        "",
        ALL,
        Box::new(|s| taper(s, 1.6, 0.8, 3)),
    );
    add(
        "t08_taper_mild",
        "Tapered shaft, 1.1 to 0.9 mm (reads as a rod, one width)",
        "",
        ALL,
        Box::new(|s| taper(s, 1.1, 0.9, 1)),
    );
    add(
        "t10_taper_over",
        "Trunk tapering 1.55 to 1.0 mm over its lowest 7 mm (just over the step threshold: two widths)",
        "",
        ALL,
        Box::new(|s| part_taper(s, 1.55, 1.0, 7.0, 2)),
    );
    add(
        "t11_taper_under",
        "Trunk tapering 1.4 to 1.0 mm over its lowest 7 mm (just under the step threshold: one width)",
        "",
        ALL,
        Box::new(|s| part_taper(s, 1.4, 1.0, 7.0, 1)),
    );
    add(
        "t12_taper_long",
        "Long strong taper, 2.6 to 1.3 mm over about 60 mm (as fd7a5cf0: three widths)",
        "fd7a5cf0",
        ALL,
        Box::new(t12_taper_long),
    );
    add(
        "t13_taper_post",
        "Tapered post with no tip, 2.0 to 1.0 mm, braced at its top to a trunk (three widths)",
        "",
        ALL,
        Box::new(t13_taper_post),
    );
    add(
        "t14_taper_knot",
        "Tapered trunk, 1.8 to 0.9 mm, with a branch partway: a step reuses the branch's knot",
        "",
        ALL,
        Box::new(t14_taper_knot),
    );
    add(
        "t15_taper_kick",
        "Tapered kickstand, 1.6 to 0.8 mm up to its knot (three width steps)",
        "",
        SEP,
        Box::new(t15_taper_kick),
    );
    add("t09_chamfer", "Rod with chamfered ends", "", SEP, Box::new(t09_chamfer));
    add("p01_flat", "Tip: flat cap", "", ALL, Box::new(|s| tip_kind(s, TipArgs { kind: "flat", ..t() })));
    add("p02_dome", "Tip: dome", "", ALL, Box::new(|s| tip_kind(s, TipArgs { kind: "dome", ..t() })));
    add("p03_ball", "Tip: separate ball shell", "", SEP, Box::new(|s| tip_kind(s, TipArgs { kind: "ball", ..t() })));
    add("p04_weldball", "Tip: welded ball", "", SEP, Box::new(|s| tip_kind(s, TipArgs { kind: "weldball", ..t() })));
    add(
        "p05_bent30",
        "Tip bent 30 degrees, joined through a mitre ring (regular to within the ring tolerance)",
        "",
        SEP,
        Box::new(|s| bent_tip(s, 30.0)),
    );
    add(
        "p05_bent60",
        "Tip bent 60 degrees, joined through an irregular ring",
        "",
        SEP,
        Box::new(|s| bent_tip(s, 60.0)),
    );
    add("p06_rodend", "Tip: plain rod end with no cone", "", ALL, Box::new(|s| rod_end(s, 1.0, "contact_rod")));
    for (depth, tg) in [(0.0, "0"), (0.1, "01"), (0.5, "05"), (2.0, "2")] {
        add(
            &format!("p07_bury{tg}"),
            &format!("Tip buried {} mm", pyf(depth)),
            "",
            ALL,
            Box::new(move |s| {
                tip_kind(s, TipArgs { kind: "flat", bury: depth, length: 3.5, dw: Some(1.2), d: 1.2, ..t() })
            }),
        );
    }
    add("p08_thin", "Tip through a thin wall (0.6 mm)", "", ALL, Box::new(p08_thin));
    add("p09_slant30", "Upright tip on a 30 degree slanted surface", "", ALL, Box::new(|s| slanted(s, 30.0, false)));
    add("p10_slant50", "Upright tip on a 50 degree slanted surface", "", ALL, Box::new(|s| slanted(s, 50.0, false)));
    add(
        "p11_slantn",
        "Tip normal to a 40 degree surface, bent at a ball",
        "",
        SJ,
        Box::new(|s| slanted(s, 40.0, true)),
    );
    add("p12_sphere", "Tip on a sphere", "", ALL, Box::new(p12_sphere));
    add("p13_coarse", "Tip on a coarse box (12 triangles)", "", SEP, Box::new(p13_coarse));
    add("p14_open", "Tip on an open model (top and side faces missing)", "", SEP, Box::new(p14_open));
    add("p15_nonman", "Tip on a non-manifold model (internal sheet on a shared edge)", "", SEP, Box::new(p15_nonman));
    add("p17_mildtaper", "Tip with a mild taper, 0.4 to 0.5 mm, on a 0.5 mm shaft", "", ALL, Box::new(p17_mildtaper));
    add("p16_inverted", "Tip on a model with inverted normals", "", SEP, Box::new(p16_inverted));
    add(
        "l01_lean_shaft",
        "Leaning shaft and tip on a post, a brace carrying on in line",
        "I2, I10",
        ALL,
        Box::new(|s| lean_brace(s, false, false)),
    );
    add(
        "l02_lean_cone",
        "Leaning tip landing straight on a post, a brace carrying on in line",
        "I2, I10",
        ALL,
        Box::new(|s| lean_brace(s, true, false)),
    );
    add(
        "l03_lean_ball",
        "Leaning shaft, joint ball on the post, a brace carrying on in line",
        "I2, I10",
        ALL,
        Box::new(|s| lean_brace(s, false, true)),
    );
    add("x01_sep", "X brace: separate rods", "", ALL, Box::new(|s| x_brace(s, "sep", 0.6, 1.0, 8.0, 8.0, 6.0)));
    add(
        "x02_welded",
        "X brace: welded at the crossing",
        "",
        SEP,
        Box::new(|s| x_brace(s, "welded", 0.6, 1.0, 8.0, 8.0, 6.0)),
    );
    add(
        "x03_cut",
        "X brace: cut into two halves at the crossing",
        "",
        SJ,
        Box::new(|s| x_brace(s, "cut", 0.6, 1.0, 8.0, 8.0, 6.0)),
    );
    add(
        "x04_samewidth",
        "X brace: same width as the posts",
        "",
        ALL,
        Box::new(|s| x_brace(s, "sep", 1.0, 1.0, 8.0, 8.0, 6.0)),
    );
    add(
        "x05_thin",
        "X brace: thinner than the posts",
        "",
        ALL,
        Box::new(|s| x_brace(s, "sep", 0.3, 1.0, 8.0, 8.0, 6.0)),
    );
    for sp in [1.5, 2.0, 3.0] {
        add(
            &format!("x06_close{}", tag(sp)),
            &format!("X brace: posts {} mm apart", pyf(sp)),
            "",
            ALL,
            Box::new(move |s| x_brace(s, "sep", 0.3, 0.5, sp, 8.0, sp)),
        );
    }
    add("z01_chain", "Zigzag chain sharing ends", "", ALL, Box::new(z01_chain));
    add("z02_rungs", "Level rungs", "", ALL, Box::new(z02_rungs));
    add("z03_ballend", "Brace ending at a joint ball", "", SEP, Box::new(z03_ballend));
    for gap in [0.1, 0.3, 0.6] {
        add(
            &format!("z04_short{}", tag(gap)),
            &format!("Brace ending {} mm short of its neighbour", pyf(gap)),
            "H2",
            SEP,
            Box::new(move |s| short_of(s, gap)),
        );
    }
    for low in [2.5, 3.2] {
        add(
            &format!("z05_band{}", tag(low)),
            &format!("Brace ending low in the raft band (z = {})", pyf(low)),
            "B4, C2",
            ALL,
            Box::new(move |s| raft_band(s, low)),
        );
    }
    for ln in [1.0, 1.4, 1.8] {
        add(
            &format!("z06_brief{}", tag(ln)),
            &format!("Short brace, {} mm axis to axis", pyf(ln)),
            "D1, E5",
            ALL,
            Box::new(move |s| short_brace(s, ln, 0.6, 0.6)),
        );
    }
    add("z07_brief18w", "Short brace, 1.8 mm, 1.3 mm wide", "D1", ALL, Box::new(|s| short_brace(s, 1.8, 1.3, 0.6)));
    add(
        "f01_hub",
        "Fan of tip cones into one hub on a post",
        "",
        SJ,
        Box::new(|s| fan(s, &[(-2.2, 0.3), (2.2, -0.3), (0.3, 2.2), (-0.3, -2.2)], true)),
    );
    add(
        "f02_overlap",
        "Fan with the cones overlapping, no hub",
        "",
        SJ,
        Box::new(|s| fan(s, &[(-1.2, 0.2), (1.2, -0.2), (0.2, 1.2)], false)),
    );
    add("f03_two", "Two tips sharing one shaft", "", ALL, Box::new(f03_two));
    add("f04_branch", "A branch off a trunk", "", ALL, Box::new(f04_branch));
    add(
        "s12_sleeve",
        "A branch's shaft doubled inside its trunk's column down to the plate",
        "a4278c33",
        SEP,
        Box::new(|s| {
            sleeve(
                s,
                0.05,
                "A shaft running inside the trunk along its line to the branch's tip (real model a4278c33): part of the trunk, a sleeve that adds nothing to the build.",
            )
        }),
    );
    add(
        "s13_sleeve_part",
        "A doubled rod 0.12 mm off its trunk's line, about 85% shared (as e1c9688c)",
        "e1c9688c",
        SEP,
        Box::new(|s| {
            sleeve(
                s,
                0.12,
                "A 1.0 mm rod 0.12 mm off a 1.0 mm trunk's line (0.85 of its volume inside the trunk): a sleeve under the owner's rule (share >= 0.6); the sliver outside the trunk is not built.",
            )
        }),
    );
    add(
        "s14_sleeve_twin",
        "Two doubled rods in one column, up to two different branches (as a4278c33)",
        "a4278c33",
        SEP,
        Box::new(s14_sleeve_twin),
    );
    add(
        "s15_sleeve_thin",
        "A 1.0 mm doubled rod inside a 1.5 mm trunk (as e1c9688c's piece 75)",
        "e1c9688c",
        SEP,
        Box::new(s15_sleeve_thin),
    );
    add("f05_branch2", "Branches of a branch", "", ALL, Box::new(f05_branch2));
    add("s01_post", "Post with no tip, braced to a trunk", "", ALL, Box::new(s01_post));
    add("s02_kick", "Kickstand on a root", "", ALL, Box::new(s02_kick));
    add("s03_leg", "Leg off a post", "", ALL, Box::new(s03_leg));
    add("s04_stub", "Short upright stub from the plate up to a low brace", "A12", ALL, Box::new(s04_stub));
    add("s05_touch_face", "Side-by-side rods touching along a wall", "", SEP, Box::new(|s| touching(s, 0.0, false)));
    add(
        "s06_touch_edge",
        "Side-by-side rods sharing a wall edge (welds)",
        "",
        SEP,
        Box::new(|s| touching(s, 0.0, true)),
    );
    add("s07_touch_lap", "Side-by-side rods overlapping by 0.1 mm", "", SEP, Box::new(|s| touching(s, -0.1, false)));
    add(
        "s09_parallel07",
        "Parallel thin shafts with a 0.1 mm gap between them",
        "",
        SEP,
        Box::new(|s| parallel_thin(s, 0.7, 0.6)),
    );
    add(
        "s10_parallel06",
        "Parallel thin shafts 0.05 mm apart, 0.4 mm wide",
        "",
        SEP,
        Box::new(|s| parallel_thin(s, 0.45, 0.4)),
    );
    add(
        "s11_parbrace",
        "Two parallel braces 0.8 mm apart (0.2 mm gap)",
        "",
        SEP,
        Box::new(|s| parallel_braces(s, 0.8, 0.6)),
    );
    add("s08_touch_near", "Near-coincident rods, 0.1 mm centre to centre", "", SEP, Box::new(near_rods));
    for d in [0.3, 0.5] {
        add(
            &format!("m01_mm{}", tag(d)),
            &format!("Micro support, model to model, {} mm wide, 8 mm long", pyf(d)),
            "E2, E12",
            SEP,
            Box::new(move |s| micro_model(s, d, 8.0)),
        );
        add(
            &format!("m02_mms{}", tag(d)),
            &format!("Micro support, model to model, {} mm wide, 3 mm long", pyf(d)),
            "E2, E12",
            SEP,
            Box::new(move |s| micro_model(s, d, 3.0)),
        );
    }
    for d in [0.3, 0.5] {
        add(
            &format!("m03_pm{}", tag(d)),
            &format!("Micro support, pillar to model, {} mm wide", pyf(d)),
            "E2, E12",
            ALL,
            Box::new(move |s| micro_pillar(s, d)),
        );
    }
    add("m04_midair", "A rod at the model with a mid-air end (model fault, left out)", "", SEP, Box::new(m04_midair));
    for (kind, title) in [
        ("tiles", "square tiles"),
        ("discs", "discs"),
        ("rim", "a slab with a rim"),
        ("web", "a line web"),
        ("thick", "tiles above the raft band (3.6 mm)"),
    ] {
        add(&format!("r01_{kind}"), &format!("Raft: {title}"), "", SEP, Box::new(move |s| raft_scene(s, kind)));
    }
    add("r02_pad", "Large raft pad as its own shell", "I12", SEP, Box::new(|s| big_pad(s, false)));
    add("r03_padshell", "Large raft pad in one shell with its post", "I12", SEP, Box::new(|s| big_pad(s, true)));
    add("r04_padbrace", "Large raft pad with a rod near it that passes close by", "I12", SEP, Box::new(r04_padbrace));
    add(
        "r05_padfoot",
        "Large raft pad with a slanted brace whose low end lands on it",
        "I12",
        SEP,
        Box::new(r05_padfoot),
    );
    add("h01_strut", "Hollow-model strut between two faces", "", ALL, Box::new(h01_strut));
    add("h02_rungs", "Hollow-model struts tied by rungs", "", ALL, Box::new(h02_rungs));
    add("h03_vee", "Hollow-model struts in a pair meeting at one contact", "", SEP, Box::new(h03_vee));
    add("h04_short", "Short hollow-model strut, 4 mm", "", SEP, Box::new(h04_short));
    add(
        "h05_flat_strut",
        "Short flat-ended strut, 4 mm, 1 mm wide, rod ends square on both faces (a twig)",
        "",
        SEP,
        Box::new(h05_flat_strut),
    );
    add("b01_bend25", "Shaft bending 25 degrees round part of the model", "", ALL, Box::new(|s| bent(s, 25.0, false)));
    add("b02_bend25b", "The same with a joint ball at the bend", "", SJ, Box::new(|s| bent(s, 25.0, true)));
    add("b03_bend45", "Shaft bending 45 degrees round part of the model", "", ALL, Box::new(|s| bent(s, 45.0, false)));
    add("c01_sharpturn", "Shaft turning 60 degrees over a lower part of the model", "", SJ, Box::new(walk_sharp));
    add("c02_grazing", "Tip meeting the surface at a shallow angle (about 28 degrees)", "", SEP, Box::new(grazing_tip));
    add(
        "c03_shorttip",
        "Tip only 0.4 mm long on a 1 mm shaft: the joint sits close to the model",
        "",
        SEP,
        Box::new(short_tip),
    );
    add("c04_jointclose", "A bend 0.045 mm clear of the model", "", SEP, Box::new(bend_close));
    add("c05_shallowbend", "A 0.2 mm dodge round part of the model", "", SEP, Box::new(bend_shallow));
    add(
        "c06_lowtip",
        "Tip with its socket below the root's top (contact at z = 4)",
        "",
        SEP,
        Box::new(|s| low_tip(s, 4.0)),
    );
    add(
        "c07_lowtip2",
        "Tip with its shaft all inside the raft band (contact at z = 6)",
        "",
        SEP,
        Box::new(|s| low_tip(s, 6.0)),
    );
    add("c08_shortshaft", "A branch with a shaft only 0.8 mm long", "", SEP, Box::new(short_shaft));
    add("k01_cage", "Composite: 3 by 3 cage of X braces with branches and a kickstand", "", SEP, Box::new(cage));
    add("k02_zigzag", "Composite: wall of posts tied by chained diagonals, with a fan", "", SEP, Box::new(zigzag_wall));
    add(
        "c09_flatwall",
        "3 mm polygon shaft with a flat 0.05 mm from a model wall",
        "ff110379",
        SEP,
        Box::new(flat_wall),
    );
    add(
        "c10_downtip",
        "Tip pointing down onto a ledge, shaft off a trunk",
        "ff110379",
        SJ,
        Box::new(|s| down_tip(s, false)),
    );
    add(
        "c11_downcone",
        "Tip pointing down onto a ledge, cone straight off a trunk",
        "ff110379",
        SJ,
        Box::new(|s| down_tip(s, true)),
    );
    add(
        "c12_widthchange",
        "A thin shaft carrying on from a thick post, past a wall",
        "ff110379",
        SJ,
        Box::new(width_change),
    );
    add(
        "p18_contactball",
        "Tip with a 1 mm ball centred on the contact, wider than the cone's end",
        "",
        SEP,
        Box::new(contact_ball),
    );
    add(
        "k03_modeltree",
        "A fan of rods from a hub inside the model, no post below",
        "ff110379, cad11189",
        SJ,
        Box::new(model_tree),
    );
    add("n01_endin", "A non-tip rod ending in the model", "", SEP, Box::new(|s| end_in(s, &[], false)));
    add(
        "n02_endin_open",
        "A rod ending in an open body (top face missing)",
        "",
        SEP,
        Box::new(|s| end_in(s, &["top", "right"], false)),
    );
    add("n03_endin_nonman", "A rod ending in a non-manifold body", "", SEP, Box::new(|s| end_in(s, &[], true)));
    add("n04_buried", "A part wholly inside the model, not to be exported", "", SEP, Box::new(n04_buried));
    add(
        "n05_through",
        "A shaft passing through a wall, then a tip on the body beyond",
        "",
        SEP,
        Box::new(|s| through_wall(s, &[])),
    );
    add(
        "n06_through_open",
        "The same with the wall's body open",
        "",
        SEP,
        Box::new(|s| through_wall(s, &["left", "front"])),
    );
    add("n07_cavity", "A rod crossing a shell and ending in the cavity", "", SEP, Box::new(n07_cavity));
    add("n08_close", "Two tips 0.4 mm apart", "", SEP, Box::new(n08_close));
    add(
        "n09_tilted",
        "A tilted model: leaning trunks square to its underside, with a brace",
        "",
        SEP,
        Box::new(n09_tilted),
    );
    c
}

/// One generated variant: the turned model and support triangles, the truth and its samples.
pub struct Generated {
    pub model: Vec<[[f64; 3]; 3]>,
    pub support: Vec<[[f64; 3]; 3]>,
    pub truth: Value,
    pub samples: Vec<[f32; 3]>,
    pub tags: Vec<u16>,
}

/// gen.make for one variant, without writing files.
pub fn generate_with(c: &CaseDef, sides: usize, rot: i64, style: &str) -> Result<Generated, String> {
    let i = vid(&c.id, sides, rot, style);
    let mut s = Scene::new(sides, rot, style);
    (c.run)(&mut s)?;
    let (tris, report) = s.support_mesh();
    let (pts, tags) = s.samples();
    let mut t = s.truth(&i, &c.title, &c.real, json!({"base": c.id, "sides": sides, "rot": rot, "style": style}));
    t["mesh_report"] = report;
    let raw = |tr: Vec<Tri>| tr.into_iter().map(|t| t.map(|p| p.0)).collect::<Vec<_>>();
    Ok(Generated {
        support: raw(s.turn_tris(&tris)),
        model: raw(s.turn_tris(&s.mesh())),
        truth: t,
        samples: s.turn_rows(&pts).into_iter().map(V3::to_f32).collect(),
        tags,
    })
}

pub fn generate(case_id: &str, sides: usize, rot: i64, style: &str) -> Result<Generated, String> {
    let cs = cases();
    let c = cs.iter().find(|c| c.id == case_id).ok_or_else(|| format!("no case {case_id}"))?;
    generate_with(c, sides, rot, style)
}
