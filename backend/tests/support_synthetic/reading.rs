//! The reader's output in the synthetic suite's shapes: its "reading" (one element per straight
//! section of every piece, in the suite's vocabulary, for stage 1) and its layout (for stage 2,
//! with a strut's base written as "model" and its lower tip beside it). Only the vocabulary is
//! mapped (trunk, branch, strut and sleeve are the suite's "shaft"; a leg is a "post"; a knot is a
//! "ball"); nothing here is tuned to the suite.

use findamodel::geometry::support_reader::classify::{Classification, Owner, Role};
use findamodel::geometry::support_reader::pieces::SectionKind;
use findamodel::geometry::voxl::support_strut_ends;
use serde_json::{Value, json};

fn role_name(r: Role) -> &'static str {
    match r {
        Role::Tip => "tip",
        Role::Trunk => "trunk",
        Role::Branch => "branch",
        Role::Strut => "strut",
        Role::Micro => "micro",
        Role::Post => "post",
        Role::Leg => "leg",
        Role::Brace => "brace",
        Role::Root => "root",
        Role::Sleeve => "sleeve",
        Role::Knot => "knot",
        Role::Raft => "raft",
        Role::Loose => "loose",
        Role::Floating => "floating",
        Role::BraceOnBrace => "brace_on_brace",
    }
}

fn vocab(r: &str) -> &str {
    match r {
        "trunk" | "branch" | "strut" | "sleeve" => "shaft",
        "leg" => "post",
        "knot" => "ball",
        other => other,
    }
}

/// The suite's reading of a classification.
pub fn reading(c: &Classification) -> Value {
    let mut els = Vec::new();
    for p in &c.classified {
        if p.raft {
            continue;
        }
        let role = role_name(p.role.unwrap_or(Role::Floating));
        let sup = match p.owner {
            Some(Owner::Support(i))
                if i < c.routes.len() && ["tip", "trunk", "branch", "strut", "micro"].contains(&role) =>
            {
                Some(&c.routes[i])
            }
            _ => None,
        };
        let base = sup.map(|s| vocab(role_name(s.role)));
        if p.ball {
            let r = p.width / 2.0;
            els.push(json!({"a": p.a, "b": p.a, "r": r, "ra": r, "rb": r, "kind": "ball", "ball": true, "cone": false, "length": 0.0}));
            continue;
        }
        let secs: Vec<(SectionKind, f64, f64, f64, f64)> = if p.secs.is_empty() {
            vec![(SectionKind::Rod, 0.0, p.l, p.width / 2.0, p.width / 2.0)]
        } else {
            p.secs.iter().map(|s| (s.k, s.t0, s.t1, s.r0, s.r1)).collect()
        };
        let has_cone = secs.iter().any(|s| s.0 == SectionKind::Cone);
        // On the piece holding the contact, a section that starts past the tip's socket (the shaft
        // of a tip cone welded onto its tapered shaft) and widens (or keeps its width) going on
        // down the route is the support's shaft.
        let past: Option<(f64, f64, f64)> = match sup {
            Some(s) if role == "tip" && !s.path.is_empty() && s.path[0].0 == p.i => {
                let (_, t0, t1) = s.path[0];
                Some((t0, if t1 >= t0 { 1.0 } else { -1.0 }, s.tip_len))
            }
            _ => None,
        };
        for (k, t0, t1, r0, r1) in secs {
            if k == SectionKind::Ball {
                continue;
            }
            let (a, b) = (p.at(t0), p.at(t1));
            let mut kind = vocab(role).to_string();
            if role == "tip" {
                // Of the piece holding the contact, the cone and short stretches are the tip; a
                // long rod is the support's shaft (a micro support's rod is micro; a blunt rod
                // with no cone is a contact rod).
                let long_rod = k == SectionKind::Rod && t1 - t0 > 1.0;
                let is_past = past.is_some_and(|(pt0, sg, tl)| {
                    ((t0 - pt0) * sg).min((t1 - pt0) * sg) >= tl - 0.1 && (r1 - r0) * sg >= -0.02 * r0.max(r1)
                });
                if base == Some("micro") {
                    kind = "micro".into();
                } else if is_past && t1 - t0 > 1.0 {
                    kind = base.unwrap_or("tip").into();
                } else if long_rod {
                    kind = if has_cone { base.unwrap_or("tip").into() } else { "contact_rod".into() };
                }
            }
            els.push(json!({"a": a, "b": b, "r": (r0 + r1) / 2.0, "ra": r0, "rb": r1, "kind": kind, "ball": false,
                            "cone": k == SectionKind::Cone, "length": t1 - t0}));
        }
    }
    json!({"elements": els, "unread": c.check.unread, "plate": 3.0})
}

/// The layout in the suite's shape: a strut's base is "model", with its lower tip beside it.
/// A strut also says how the writer writes it ("written": "twig" or "stick", from
/// `voxl::support_strut_ends`).
pub fn layout(c: &Classification) -> Value {
    let mut lay = c.layout_json.clone();
    for (i, s) in lay["supports"].as_array_mut().expect("supports").iter_mut().enumerate() {
        if let Some(low) = s["base"].get("lower").cloned() {
            s["lower"] = low;
            s["base"] = json!("model");
            if let Some(ends) = support_strut_ends(&c.layout, i) {
                s["written"] = json!(if ends.twig { "twig" } else { "stick" });
            }
        }
    }
    lay
}
