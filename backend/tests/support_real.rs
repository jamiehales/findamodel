//! The real-structure support suite: support structures cut out of real pre-supported models,
//! with the expected result taken from each model's Lychee project.
//!
//! Every case of `support_real/cases.json` is a folder `support_real/<case>/` holding
//! `supports.stl` (the original support shells of one structure, or of a self-contained section of
//! one, with the raft under it), `model.stl` (small closed cuts of the model where the supports meet
//! it, never the whole model), `truth.json` (the Lychee supports in the STL's frame: tips, routes,
//! shaft lines and braces) and `expected.png` (a picture of the expected result).
//! `scripts/real-support-cases.py` cuts the cases; `scripts/real-support-catalogue.py` renders
//! `docs/support-reader-real-tests.md`.
//!
//! Each case is read by the support reader and checked:
//! - validate: the truth against its own meshes (contacts on the cut model, lines inside the
//!   support mesh, each in a shell of its own and not in a neighbour's alone, with no stretch
//!   over 1 mm bare of it, the mesh explained by the truth as a whole
//!   and in every connected piece, a loop at every tip). A case that fails here is a test problem,
//!   not a reader problem;
//! - 1 tips: every truth tip found by contact, none extra;
//! - 1 tip shape: contact width, direction, cone body width and socket;
//! - 2 routes: each tip's line from the contact down to the plate or the model (the same line under
//!   another base passes: which support of a shared shaft is the trunk is the author's choice);
//! - 2 shafts: shaft lines covered both ways;
//! - 2 braces: every brace by both ends, none extra, no posts or legs Lychee does not have (a
//!   brace into the root zone may be a leg, a brace through a pillar one brace per stretch, and
//!   the post under a missed tip counts against the tips only: see `support_real/score.rs`);
//! - 2 joints: no more bends per support than Lychee's;
//! - 4a clips and 4b loops on the nominal build (`support_synthetic/nominal.rs`), against what
//!   the original supports themselves clip and the loops they make.
//!
//! The results are held to `support_real/expected.json` (per case, which stages pass) exactly as
//! the synthetic suite's: a stage that stops or starts passing fails the test.
//! `REAL_UPDATE=1 cargo test -p findamodel --test support_real` rewrites the file;
//! `REAL_DUMP=<dir>` also writes each case's meshes, truth, layout, nominal build and results there.
#![allow(dead_code)]

#[path = "support_synthetic/measure.rs"]
mod measure;
#[path = "support_synthetic/nominal.rs"]
mod nominal;
#[path = "support_synthetic/reading.rs"]
mod reading;
#[path = "support_real/score.rs"]
mod real;
#[path = "support_synthetic/score.rs"]
mod score;
#[path = "support_synthetic/shapes.rs"]
mod shapes;
#[path = "support_synthetic/vec.rs"]
mod vec;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use findamodel::geometry::support_reader;
use findamodel::geometry::{Triangle, Vec3};
use rayon::prelude::*;
use serde_json::{Value, json};

const STAGES: [&str; 9] = [
    "validate",
    "1 tips",
    "1 tip shape",
    "2 routes",
    "2 shafts",
    "2 braces",
    "2 joints",
    "4a clips (nominal)",
    "4b loops (nominal)",
];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("support_real")
}

/// The case list (`cases.json`): the data the fixtures were cut from.
fn case_ids() -> Vec<String> {
    let spec: Value =
        serde_json::from_slice(&std::fs::read(dir().join("cases.json")).expect("cases.json")).expect("json");
    spec["cases"].as_array().expect("cases").iter().map(|c| c["case"].as_str().expect("case id").to_string()).collect()
}

fn triangles(t: &[[[f32; 3]; 3]]) -> Vec<Triangle> {
    let v = |p: [f32; 3]| Vec3::new(p[0], p[1], p[2]);
    t.iter().map(|t| Triangle::new(v(t[0]), v(t[1]), v(t[2]), Vec3::new(0.0, 0.0, 1.0))).collect()
}

struct Outcome {
    stages: Value,
    dump: Option<Value>,
}

fn run(id: &str, keep: bool) -> Outcome {
    let d = dir().join(id);
    let model32 = measure::read_stl(&d.join("model.stl")).expect("model.stl");
    let support32 = measure::read_stl(&d.join("supports.stl")).expect("supports.stl");
    let truth: Value = serde_json::from_slice(&std::fs::read(d.join("truth.json")).expect("truth.json")).expect("json");
    let raft = truth["raft_mm"].as_f64().expect("raft_mm") as f32;
    let model: Vec<[measure::P; 3]> = model32.iter().map(measure::widen).collect();
    let support: Vec<[measure::P; 3]> = support32.iter().map(measure::widen).collect();
    // The truth against its own meshes.
    let v =
        real::validate(&truth, &model, &support, &measure::isect(&support, &model), &measure::touch(&support, &model));
    // The reading.
    let r = support_reader::read(&triangles(&model32), &triangles(&support32), raft);
    let lay = reading::layout(&r.classification);
    let s = real::layout_stages(&truth, &lay);
    // Clips on the build at tip offset 0, loops on the one at DragonFruit's own offset.
    let nm = nominal::build(&lay, &r.classification.layout, nominal::CLIP_TIP_OFFSET_MM);
    let nm_loops = nominal::build(&lay, &r.classification.layout, nominal::LOOP_TIP_OFFSET_MM);
    let tris64 = |n: &nominal::Nominal| -> Vec<[[f64; 3]; 3]> { n.tris.iter().map(|t| t.map(|p| p.0)).collect() };
    let (isect, touch) = (measure::isect(&tris64(&nm), &model), measure::touch(&tris64(&nm_loops), &model));
    let labels = |i: u32| nm.labels[i as usize].name().to_string();
    let s4 = real::stage4(&truth, &isect, &touch, &labels, &v);
    let stages = json!({
        "validate": v.errors.is_empty(),
        "1 tips": s["tips"]["pass"],
        "1 tip shape": s["shape"]["pass"],
        "2 routes": s["routes"]["pass"],
        "2 shafts": s["shafts"]["pass"],
        "2 braces": s["braces"]["pass"],
        "2 joints": s["joints"]["pass"],
        "4a clips (nominal)": s4["clip_pass"],
        "4b loops (nominal)": s4["loop_pass"],
    });
    let dump = keep.then(|| {
        json!({
            "id": id, "title": truth["title"], "situation": truth["situation"], "stages": stages,
            "validate": {"errors": v.errors, "numbers": v.numbers}, "layout_stages": s, "s4": s4,
            "check": r.classification.check, "exceptions": r.classification.exceptions,
            "nominal": nm.tris.iter().map(|t| t.map(|p| p.0.map(|x| x as f32))).collect::<Vec<_>>(),
            "layout": lay,
        })
    });
    Outcome { stages, dump }
}

/// Every case, held to `expected.json`.
#[test]
fn every_real_case_reads_as_before() {
    let ids = case_ids();
    let dump_dir = std::env::var("REAL_DUMP").ok().map(PathBuf::from);
    if let Some(d) = &dump_dir {
        std::fs::create_dir_all(d).expect("dump dir");
    }
    let got: BTreeMap<String, Value> = ids
        .par_iter()
        .map(|id| {
            let o = run(id, dump_dir.is_some());
            if let (Some(d), Some(v)) = (&dump_dir, &o.dump) {
                std::fs::write(d.join(format!("{id}.json")), serde_json::to_vec(v).expect("json")).expect("dump");
            }
            (id.clone(), o.stages)
        })
        .collect();
    let path = dir().join("expected.json");
    if std::env::var("REAL_UPDATE").is_ok() {
        std::fs::write(&path, serde_json::to_string_pretty(&got).expect("json") + "\n").expect("expected.json");
        return;
    }
    let expected: BTreeMap<String, Value> =
        std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut changed = Vec::new();
    for (id, stages) in &got {
        match expected.get(id) {
            Some(want) if want == stages => {}
            Some(want) => changed.push(format!("{id}: expected {want}, got {stages}")),
            None => changed.push(format!("{id}: not in expected.json (got {stages})")),
        }
    }
    for id in expected.keys().filter(|id| !got.contains_key(*id)) {
        changed.push(format!("{id}: in expected.json but not in cases.json"));
    }
    let all = |k: &str| got.values().filter(|s| s[k] == true).count();
    eprintln!(
        "{} cases: {}",
        got.len(),
        STAGES.iter().map(|k| format!("{k} {}", all(k))).collect::<Vec<_>>().join(", ")
    );
    assert!(changed.is_empty(), "{} cases changed:\n{}", changed.len(), changed.join("\n"));
}

/// Every listed case has its four files, a model cut that is far smaller than a model, and a
/// truth naming its source.
#[test]
fn every_real_case_has_its_fixtures() {
    for id in case_ids() {
        let d = dir().join(&id);
        for f in ["model.stl", "supports.stl", "truth.json", "expected.png"] {
            assert!(d.join(f).is_file(), "{id}: {f} is missing");
        }
        let truth: Value = serde_json::from_slice(&std::fs::read(d.join("truth.json")).unwrap()).unwrap();
        for k in ["model_id", "creator", "lychee_ids"] {
            assert!(!truth["source"][k].is_null(), "{id}: truth.json has no source.{k}");
        }
        assert!(truth["situation"].is_string() && truth["title"].is_string(), "{id}: no situation or title");
        let png = std::fs::read(d.join("expected.png")).unwrap();
        assert!(png.starts_with(b"\x89PNG"), "{id}: expected.png is not a PNG");
        let mesh = std::fs::metadata(d.join("model.stl")).unwrap().len()
            + std::fs::metadata(d.join("supports.stl")).unwrap().len();
        assert!(mesh < 1_500_000, "{id}: {mesh} bytes of mesh");
    }
}

/// The scorer on a hand-made truth and layout: a trunk with a branch and a brace to a second
/// trunk, read exactly, then with the branch read as standing on the plate and the brace lost.
#[test]
fn the_scorer_tells_a_right_reading_from_a_wrong_one() {
    let tip = |id: &str, c: [f64; 3], s: [f64; 3], base: &str, route: Value| {
        json!({"support": id, "end": "tip", "contact": c, "axis": [0.0, 0.0, -1.0], "socket": s, "contact_d": 0.4,
               "body_d": 1.0, "length": 2.0, "base": base, "normal": [0.0, 0.0, -1.0], "route": route})
    };
    let truth = json!({
        "supports": [
            {"id": "a", "role": "trunk", "line": [[0, 0, 0], [0, 0, 18]], "d": 1.0, "joints": 0},
            {"id": "b", "role": "branch", "line": [[0, 0, 10], [4, 0, 18]], "d": 1.0, "joints": 0},
            {"id": "c", "role": "trunk", "line": [[8, 0, 0], [8, 0, 18]], "d": 1.0, "joints": 0},
        ],
        "tips": [
            tip("a", [0.0, 0.0, 20.0], [0.0, 0.0, 18.0], "plate", json!([[0, 0, 20], [0, 0, 18], [0, 0, 0]])),
            tip("b", [4.0, 0.0, 20.0], [4.0, 0.0, 18.0], "parent", json!([[4, 0, 20], [4, 0, 18], [0, 0, 10], [0, 0, 0]])),
            tip("c", [8.0, 0.0, 20.0], [8.0, 0.0, 18.0], "plate", json!([[8, 0, 20], [8, 0, 18], [8, 0, 0]])),
        ],
        "braces": [{"id": "x", "a": [0, 0, 5], "b": [8, 0, 9], "d": 1.0}],
    });
    let rtip = |c: [f64; 3], s: [f64; 3]| json!({"contact": c, "socket": s, "contact_d": 0.42, "body_d": 1.0});
    let right = json!({
        "supports": [
            {"tip": rtip([0.0, 0.0, 20.0], [0.0, 0.0, 18.0]), "base": {"plate": [0, 0], "root_d": 2.0}, "joints": [], "shaft_d": 1.0},
            {"tip": rtip([4.0, 0.0, 20.0], [4.0, 0.0, 18.0]), "base": {"parent": 0, "at": [0, 0, 10]}, "joints": [], "shaft_d": 1.0},
            {"tip": rtip([8.0, 0.0, 20.0], [8.0, 0.0, 18.0]), "base": {"plate": [8, 0], "root_d": 2.0}, "joints": [], "shaft_d": 1.0},
        ],
        "posts": [], "kickstands": [],
        "braces": [{"lower": [0, null, [0, 0, 5]], "upper": [2, null, [8, 0, 9]], "d": 1.0}],
    });
    let s = real::layout_stages(&truth, &right);
    for k in ["tips", "shape", "routes", "shafts", "braces", "joints"] {
        assert!(s[k]["pass"] == true, "{k}: {}", s[k]);
    }
    let mut wrong = right.clone();
    wrong["supports"][1]["base"] = json!({"plate": [4, 0], "root_d": 2.0});
    wrong["supports"][1]["joints"] = json!([[4, 0, 9]]);
    wrong["braces"] = json!([]);
    let s = real::layout_stages(&truth, &wrong);
    assert!(s["tips"]["pass"] == true, "{}", s["tips"]);
    for k in ["routes", "shafts", "braces", "joints"] {
        assert!(s[k]["pass"] == false, "{k}: {}", s[k]);
    }
    // Trunk and branch swapped (the branch's tip keeps the shared shaft down, the trunk's tip hangs
    // from it): the same lines under other names, so the routes still pass.
    let mut swapped = right.clone();
    swapped["supports"][0] = json!({"tip": rtip([4.0, 0.0, 20.0], [4.0, 0.0, 18.0]), "base": {"plate": [0, 0], "root_d": 2.0},
                                    "joints": [[0, 0, 10]], "shaft_d": 1.0});
    swapped["supports"][1] = json!({"tip": rtip([0.0, 0.0, 20.0], [0.0, 0.0, 18.0]), "base": {"parent": 0, "at": [0, 0, 10]},
                                    "joints": [], "shaft_d": 1.0});
    let s = real::layout_stages(&truth, &swapped);
    assert!(s["routes"]["pass"] == true && s["routes"]["same_line_other_base"] == 2, "{}", s["routes"]);
    assert!(s["shafts"]["pass"] == true, "{}", s["shafts"]);
    // A tip moved 0.5 mm is missing and extra.
    let mut moved = right.clone();
    moved["supports"][2]["tip"]["contact"] = json!([8.5, 0.0, 20.0]);
    let s = real::layout_stages(&truth, &moved);
    assert!(s["tips"]["missing"] == 1 && s["tips"]["extra"] == 1, "{}", s["tips"]);
}

/// A hair support's direction is read along the support, not from a socket a few hundredths of a
/// millimetre from the contact.
#[test]
fn a_hair_support_s_direction_is_taken_along_the_support() {
    let truth = json!({
        "supports": [{"id": "h", "role": "strut", "line": [[0, 0, 10.0], [0, 0, 10.2]], "d": 0.3, "joints": 0}],
        "tips": [
            {"support": "h", "end": "tip", "contact": [0.0, 0.0, 10.2], "axis": [0.0, 0.0, -1.0], "socket": [0.0, 0.0, 10.2],
             "contact_d": 0.3, "body_d": 1.0, "length": 0.0, "base": "model", "route": [[0, 0, 10.2], [0, 0, 10.0]]},
            {"support": "h", "end": "base", "contact": [0.0, 0.0, 10.0], "axis": [0.0, 0.0, 1.0], "socket": [0.0, 0.0, 10.0],
             "contact_d": 0.3, "body_d": 1.0, "length": 0.0, "base": "model", "route": [[0, 0, 10.0], [0, 0, 10.2]]},
        ],
        "braces": [],
    });
    // The sockets stand 0.03 mm off the contacts, sideways.
    let lay = json!({
        "supports": [{
            "tip": {"contact": [0.0, 0.0, 10.2], "socket": [0.02, 0.0, 10.18], "contact_d": 0.3, "body_d": 0.3},
            "lower": {"contact": [0.0, 0.0, 10.0], "socket": [-0.02, 0.0, 10.02], "contact_d": 0.3, "body_d": 0.3},
            "base": "model", "joints": [], "shaft_d": 0.3,
        }],
        "posts": [], "kickstands": [], "braces": [],
    });
    let s = real::layout_stages(&truth, &lay);
    assert!(s["tips"]["pass"] == true && s["shape"]["pass"] == true, "{}", s["shape"]);
    // A cone tip's direction is still its contact to its socket.
    let r = real::Reader::new(&json!({
        "supports": [{"tip": {"contact": [0.0, 0.0, 20.0], "socket": [2.0, 0.0, 18.0], "contact_d": 0.4, "body_d": 1.0},
                      "base": {"plate": [2, 0], "root_d": 2.0}, "joints": [], "shaft_d": 1.0}],
        "posts": [], "kickstands": [], "braces": [],
    }));
    let d = r.tip_dir(&r.tips[0]);
    assert!((d[0] - d[2].abs()).abs() < 1e-9 && d[0] > 0.0, "{d:?}");
}

/// The winding number tells inside from outside on a closed solid.
#[test]
fn the_winding_number_is_one_inside_a_closed_solid() {
    let (a, b, c, d) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let tetra = [[a, c, b], [a, b, d], [b, c, d], [c, a, d]];
    assert!((real::winding(vec::V3([0.2, 0.2, 0.2]), &tetra) - 1.0).abs() < 1e-9);
    assert!(real::winding(vec::V3([1.0, 1.0, 1.0]), &tetra).abs() < 1e-9);
}

/// A truth tip on a cone, for the scorer's own tests.
fn cone_tip(id: &str, c: [f64; 3], s: [f64; 3], base: &str, route: Value) -> Value {
    json!({"support": id, "end": "tip", "contact": c, "axis": [0.0, 0.0, -1.0], "socket": s, "contact_d": 0.4,
           "body_d": 1.0, "length": 2.0, "base": base, "normal": [0.0, 0.0, -1.0], "route": route})
}

/// A reader support on the plate at `foot`, straight up to its socket `s` and its contact `c`.
fn plate_support(c: [f64; 3], s: [f64; 3], foot: [f64; 2]) -> Value {
    json!({"tip": {"contact": c, "socket": s, "contact_d": 0.42, "body_d": 1.0},
           "base": {"plate": foot, "root_d": 2.0}, "joints": [], "shaft_d": 1.0})
}

/// Two trunks 4 mm apart with one brace from `low` on the first to `high` on the second.
fn two_braced_trunks(low: [f64; 3], high: [f64; 3]) -> Value {
    json!({
        "supports": [
            {"id": "a", "role": "trunk", "base": "plate", "line": [[0, 0, 0], [0, 0, 18]], "d": 1.0, "joints": 0},
            {"id": "c", "role": "trunk", "base": "plate", "line": [[4, 0, 0], [4, 0, 18]], "d": 1.0, "joints": 0},
        ],
        "tips": [
            cone_tip("a", [0.0, 0.0, 20.0], [0.0, 0.0, 18.0], "plate", json!([[0, 0, 20], [0, 0, 18], [0, 0, 0]])),
            cone_tip("c", [4.0, 0.0, 20.0], [4.0, 0.0, 18.0], "plate", json!([[4, 0, 20], [4, 0, 18], [4, 0, 0]])),
        ],
        "braces": [{"id": "x", "a": low, "b": high, "d": 1.0}],
    })
}

/// A brace that runs into its host's root zone may be read as a leg standing on its line; a leg
/// off that line, or one for a brace that ends above the root zone, is still a fault.
#[test]
fn a_brace_into_the_root_zone_may_be_read_as_a_leg() {
    let truth = two_braced_trunks([0.0, 0.0, 1.0], [4.0, 0.0, 5.0]);
    let reading = |foot: [f64; 3]| {
        json!({
            "supports": [plate_support([0.0, 0.0, 20.0], [0.0, 0.0, 18.0], [0.0, 0.0]),
                         plate_support([4.0, 0.0, 20.0], [4.0, 0.0, 18.0], [4.0, 0.0])],
            "posts": [], "braces": [],
            "kickstands": [{"support": 1, "knot": [4.0, 0.0, 5.0], "joints": [], "foot": foot, "root_d": 2.0, "d": 1.0}],
        })
    };
    // The brace's line crosses the root's top at x = 1.
    let s = real::layout_stages(&truth, &reading([1.0, 0.0, 2.0]));
    assert!(s["braces"]["pass"] == true && s["braces"]["as_legs"] == 1, "{}", s["braces"]);
    assert!(s["shafts"]["pass"] == true, "{}", s["shafts"]);
    // A leg that reaches the same knot from elsewhere is a leg Lychee does not have.
    let s = real::layout_stages(&truth, &reading([2.5, 1.5, 2.0]));
    assert!(s["braces"]["pass"] == false && s["braces"]["as_legs"] == 0, "{}", s["braces"]);
    // A brace that ends above the root zone must be read as a brace.
    let above = two_braced_trunks([0.0, 0.0, 2.5], [4.0, 0.0, 6.5]);
    let mut lay = reading([0.0, 0.0, 2.0]);
    lay["kickstands"][0]["knot"] = json!([4.0, 0.0, 6.5]);
    lay["kickstands"][0]["joints"] = json!([[0.0, 0.0, 2.5]]);
    let s = real::layout_stages(&above, &lay);
    assert!(s["braces"]["pass"] == false && s["braces"]["as_legs"] == 0, "{}", s["braces"]);
}

/// A branch that joins its trunk inside the trunk's foot may be read as a support with a foot of
/// its own; a branch that joins higher may not.
#[test]
fn a_branch_from_inside_a_trunk_s_foot_may_stand_on_the_plate() {
    let truth = |attach: [f64; 3], socket: [f64; 3], contact: [f64; 3]| {
        json!({
            "supports": [
                {"id": "a", "role": "trunk", "base": "plate", "line": [[0, 0, 0], [0, 0, 18]], "d": 1.0, "joints": 0},
                {"id": "b", "role": "branch", "base": "parent", "host": "a", "attach": attach, "line": [attach, socket], "d": 1.0, "joints": 0},
            ],
            "tips": [
                cone_tip("a", [0.0, 0.0, 20.0], [0.0, 0.0, 18.0], "plate", json!([[0, 0, 20], [0, 0, 18], [0, 0, 0]])),
                cone_tip("b", contact, socket, "parent", json!([contact, socket, attach, [0, 0, 0]])),
            ],
            "braces": [],
        })
    };
    let reading = |socket: [f64; 3], contact: [f64; 3], foot: [f64; 2]| {
        json!({"supports": [plate_support([0.0, 0.0, 20.0], [0.0, 0.0, 18.0], [0.0, 0.0]), plate_support(contact, socket, foot)],
               "posts": [], "kickstands": [], "braces": []})
    };
    // A long branch from 1 mm up: the reader's foot is where its line passes the root's top.
    let t = truth([0.0, 0.0, 1.0], [4.0, 0.0, 5.0], [4.0, 0.0, 7.0]);
    let s = real::layout_stages(&t, &reading([4.0, 0.0, 5.0], [4.0, 0.0, 7.0], [1.0, 0.0]));
    assert!(s["routes"]["pass"] == true && s["routes"]["own_foot"] == 1, "{}", s["routes"]);
    assert!(s["shafts"]["pass"] == true, "{}", s["shafts"]);
    // Standing somewhere else, its line differs: still wrong.
    let s = real::layout_stages(&t, &reading([4.0, 0.0, 5.0], [4.0, 0.0, 7.0], [4.0, 0.0]));
    assert!(s["routes"]["pass"] == false && s["routes"]["own_foot"] == 0, "{}", s["routes"]);
    // A branch too short to compare as a line passes on its foot alone when it joins in the foot...
    let short = truth([0.0, 0.0, 1.5], [0.6, 0.0, 2.1], [0.6, 0.0, 2.6]);
    let s = real::layout_stages(&short, &reading([0.6, 0.0, 2.1], [0.6, 0.0, 2.6], [0.5, 0.0]));
    assert!(s["routes"]["pass"] == true && s["routes"]["own_foot"] == 1, "{}", s["routes"]);
    // ...and not when it joins above the root zone.
    let high = truth([0.0, 0.0, 3.5], [0.6, 0.0, 4.1], [0.6, 0.0, 4.6]);
    let s = real::layout_stages(&high, &reading([0.6, 0.0, 4.1], [0.6, 0.0, 4.6], [0.6, 0.0]));
    assert!(s["routes"]["pass"] == false, "{}", s["routes"]);
}

/// A branch's shaft and a brace that leave its socket alike may be read for each other; where the
/// brace leaves more steeply, or another brace hangs on the shaft, the mesh tells them apart and
/// the swap is still a fault.
#[test]
fn a_shaft_and_a_brace_alike_at_a_socket_may_be_swapped() {
    // Two trunks 4 mm apart; a branch from the first up to a socket midway, and braces from the
    // second: `low` to the branch's socket, `extra` (if any) to the middle of its shaft.
    let truth = |low: [f64; 3], extra: bool| {
        let mut t = two_braced_trunks(low, [2.0, 0.0, 12.0]);
        t["supports"].as_array_mut().unwrap().push(json!(
            {"id": "b", "role": "branch", "base": "parent", "host": "a", "attach": [0, 0, 10],
             "line": [[0, 0, 10], [2, 0, 12]], "d": 1.0, "joints": 0}));
        t["tips"].as_array_mut().unwrap().push(cone_tip(
            "b",
            [2.0, 0.0, 14.0],
            [2.0, 0.0, 12.0],
            "parent",
            json!([[2, 0, 14], [2, 0, 12], [0, 0, 10], [0, 0, 0]]),
        ));
        t["braces"][0]["hosts"] = json!(["c", "b"]);
        if extra {
            t["braces"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": "y", "a": [4, 0, 11], "b": [1, 0, 11], "d": 1.0, "hosts": ["c", "b"]}));
        }
        t
    };
    // The reading with the two swapped: the tip hangs from the second trunk at the brace's low
    // end, and a brace runs along the Lychee shaft.
    let reading = |low: [f64; 3], extra: bool| {
        let mut braces = vec![json!({"lower": [0, 0.0, [0, 0, 10]], "upper": [2, 0.0, [2, 0, 12]]})];
        if extra {
            braces.push(json!({"lower": [0, 0.0, [1, 0, 11]], "upper": [1, 0.0, [4, 0, 11]]}));
        }
        json!({"supports": [
                   plate_support([0.0, 0.0, 20.0], [0.0, 0.0, 18.0], [0.0, 0.0]),
                   plate_support([4.0, 0.0, 20.0], [4.0, 0.0, 18.0], [4.0, 0.0]),
                   {"tip": {"contact": [2, 0, 14], "socket": [2, 0, 12], "contact_d": 0.42, "body_d": 1.0},
                    "base": {"parent": 1, "at": low}, "joints": [], "shaft_d": 1.0}],
               "posts": [], "kickstands": [], "braces": braces})
    };
    // Shaft and brace both leave the socket 45 degrees down, to either side.
    let s = real::layout_stages(&truth([4.0, 0.0, 10.0], false), &reading([4.0, 0.0, 10.0], false));
    assert!(s["routes"]["pass"] == true && s["routes"]["socket_swaps"] == 1, "{}", s["routes"]);
    assert!(s["braces"]["pass"] == true && s["shafts"]["pass"] == true, "{} {}", s["braces"], s["shafts"]);
    // The brace leaves 27 degrees steeper than the shaft: the reader could tell.
    let s = real::layout_stages(&truth([4.0, 0.0, 6.0], false), &reading([4.0, 0.0, 6.0], false));
    assert!(s["routes"]["pass"] == false && s["routes"]["socket_swaps"] == 0, "{}", s["routes"]);
    // A second brace hangs on the shaft, which a brace could not carry: the reader could tell.
    let s = real::layout_stages(&truth([4.0, 0.0, 10.0], true), &reading([4.0, 0.0, 10.0], true));
    assert!(s["routes"]["pass"] == false && s["routes"]["socket_swaps"] == 0, "{}", s["routes"]);
}

/// A Lychee brace that passes through a pillar may be read as one brace per stretch; stretches
/// that meet in mid air, or stop short of its ends, are not that brace.
#[test]
fn a_brace_through_a_pillar_may_be_read_in_stretches() {
    let truth = |pillar: bool| {
        let mut t = two_braced_trunks([0.0, 0.0, 4.0], [4.0, 0.0, 8.0]);
        if pillar {
            let mid = json!({"id": "m", "role": "trunk", "base": "plate", "line": [[2, 0, 0], [2, 0, 18]], "d": 1.0, "joints": 0});
            t["supports"].as_array_mut().unwrap().push(mid);
            let tip =
                cone_tip("m", [2.0, 0.0, 20.0], [2.0, 0.0, 18.0], "plate", json!([[2, 0, 20], [2, 0, 18], [2, 0, 0]]));
            t["tips"].as_array_mut().unwrap().push(tip);
        }
        t
    };
    let reading = |pillar: bool, braces: Value| {
        let mut sups = vec![
            plate_support([0.0, 0.0, 20.0], [0.0, 0.0, 18.0], [0.0, 0.0]),
            plate_support([4.0, 0.0, 20.0], [4.0, 0.0, 18.0], [4.0, 0.0]),
        ];
        if pillar {
            sups.push(plate_support([2.0, 0.0, 20.0], [2.0, 0.0, 18.0], [2.0, 0.0]));
        }
        json!({"supports": sups, "posts": [], "kickstands": [], "braces": braces})
    };
    let halves = json!([{"lower": [0, null, [0, 0, 4]], "upper": [2, null, [2, 0, 6]], "d": 1.0},
                        {"lower": [2, null, [2, 0, 6]], "upper": [1, null, [4, 0, 8]], "d": 1.0}]);
    let s = real::layout_stages(&truth(true), &reading(true, halves.clone()));
    assert!(s["braces"]["pass"] == true && s["braces"]["split"] == 1, "{}", s["braces"]);
    // With no pillar where the halves meet, they are two braces Lychee does not have.
    let s = real::layout_stages(&truth(false), &reading(false, halves));
    assert!(s["braces"]["pass"] == false && s["braces"]["split"] == 0, "{}", s["braces"]);
    // Halves that meet beside the pillar (0.5 mm off its line, inside a thick pillar's width) meet
    // nowhere in particular: where two braces cross, say.
    let beside = json!([{"lower": [0, null, [0, 0, 4]], "upper": [2, null, [1.5, 0, 5.5]], "d": 1.0},
                        {"lower": [2, null, [1.5, 0, 5.5]], "upper": [1, null, [4, 0, 8]], "d": 1.0}]);
    let s = real::layout_stages(&truth(true), &reading(true, beside));
    assert!(s["braces"]["pass"] == false && s["braces"]["split"] == 0, "{}", s["braces"]);
    // Only the first half: the brace is not there.
    let half = json!([{"lower": [0, null, [0, 0, 4]], "upper": [2, null, [2, 0, 6]], "d": 1.0}]);
    let s = real::layout_stages(&truth(true), &reading(true, half));
    assert!(s["braces"]["pass"] == false && s["braces"]["split"] == 0, "{}", s["braces"]);
    // Halves that leave a gap at the pillar.
    let gap = json!([{"lower": [0, null, [0, 0, 4]], "upper": [2, null, [1.5, 0, 5.5]], "d": 1.0},
                     {"lower": [2, null, [2.5, 0, 6.5]], "upper": [1, null, [4, 0, 8]], "d": 1.0}]);
    let s = real::layout_stages(&truth(true), &reading(true, gap));
    assert!(s["braces"]["pass"] == false && s["braces"]["split"] == 0, "{}", s["braces"]);
}

/// A trunk whose tip the reader misses is a post to it: that fails the tips stage, and the post
/// is not counted against the braces stage as well. A post anywhere else still is.
#[test]
fn the_post_under_a_missed_tip_counts_against_the_tips_only() {
    let truth = two_braced_trunks([0.0, 0.0, 5.0], [4.0, 0.0, 9.0]);
    let reading = |post_x: f64| {
        json!({
            "supports": [plate_support([0.0, 0.0, 20.0], [0.0, 0.0, 18.0], [0.0, 0.0])],
            "posts": [{"foot": [post_x, 0.0, 2.0], "top": [post_x, 0.0, 18.0], "joints": [], "root_d": 2.0, "d": 1.0}],
            "kickstands": [],
            "braces": [{"lower": [0, null, [0, 0, 5]], "upper": [1, null, [post_x, 0, 9]], "d": 1.0}],
        })
    };
    let s = real::layout_stages(&truth, &reading(4.0));
    assert!(s["tips"]["pass"] == false && s["tips"]["missing"] == 1, "{}", s["tips"]);
    assert!(s["braces"]["pass"] == true && s["braces"]["posts_of_missed_tips"] == 1, "{}", s["braces"]);
    // A post 2 mm from the trunk is no reading of it.
    let s = real::layout_stages(&truth, &reading(6.0));
    assert!(s["braces"]["pass"] == false && s["braces"]["posts_of_missed_tips"] == 0, "{}", s["braces"]);
    // With every tip read, a post is a post Lychee does not have.
    let mut all = reading(4.0);
    all["supports"].as_array_mut().unwrap().push(plate_support([4.0, 0.0, 20.0], [4.0, 0.0, 18.0], [4.0, 0.0]));
    all["posts"][0]["foot"] = json!([8.0, 0.0, 2.0]);
    all["posts"][0]["top"] = json!([8.0, 0.0, 18.0]);
    all["braces"] = json!([{"lower": [0, null, [0, 0, 5]], "upper": [1, null, [4, 0, 9]], "d": 1.0}]);
    let s = real::layout_stages(&truth, &all);
    assert!(s["tips"]["pass"] == true, "{}", s["tips"]);
    assert!(s["braces"]["pass"] == false && s["braces"]["posts_of_missed_tips"] == 0, "{}", s["braces"]);
}

/// A closed upright prism of `sides` sides from height `z0` to `z1` round (x, y), `r` to its corners.
fn prism(x: f64, y: f64, z0: f64, z1: f64, r: f64, sides: usize) -> Vec<[measure::P; 3]> {
    let ring: Vec<(f64, f64)> = (0..sides)
        .map(|i| {
            let a = i as f64 * std::f64::consts::TAU / sides as f64;
            (x + r * a.cos(), y + r * a.sin())
        })
        .collect();
    let mut out = Vec::new();
    for i in 0..sides {
        let (p, q) = (ring[i], ring[(i + 1) % sides]);
        out.push([[p.0, p.1, z0], [q.0, q.1, z0], [q.0, q.1, z1]]);
        out.push([[p.0, p.1, z0], [q.0, q.1, z1], [p.0, p.1, z1]]);
        out.push([[x, y, z0], [q.0, q.1, z0], [p.0, p.1, z0]]);
        out.push([[x, y, z1], [p.0, p.1, z1], [q.0, q.1, z1]]);
    }
    out
}

/// A closed prism of `sides` sides and radius `r` from `a` to `b`.
fn rod(a: [f64; 3], b: [f64; 3], r: f64, sides: usize) -> Vec<[measure::P; 3]> {
    let (a, b) = (vec::V3(a), vec::V3(b));
    let d = (b - a) / (b - a).norm();
    let u = vec::V3([d[2], 0.0, -d[0]]);
    let u = u / u.norm();
    let v = vec::V3([d[1] * u[2] - d[2] * u[1], d[2] * u[0] - d[0] * u[2], d[0] * u[1] - d[1] * u[0]]);
    let ring = |c: vec::V3, k: usize| {
        let t = 2.0 * std::f64::consts::PI * k as f64 / sides as f64;
        (c + u * (r * t.cos()) + v * (r * t.sin())).0
    };
    let mut out = Vec::new();
    for k in 0..sides {
        let (p0, p1, q0, q1) = (ring(a, k), ring(a, (k + 1) % sides), ring(b, k), ring(b, (k + 1) % sides));
        out.extend([[p0, p1, q1], [p0, q1, q0], [a.0, p1, p0], [b.0, q0, q1]]);
    }
    out
}

/// A truth line counts as covered only by mesh of its own: where it runs inside a neighbour's
/// rod alone, the case lacks its shell (b14_branch_on_strut), though a rod's end buried in its
/// host is no fault.
#[test]
fn validate_wants_every_line_in_a_shell_of_its_own() {
    // A trunk, and a branch of the same width leaving its centre line 17 degrees off.
    let truth = json!({
        "supports": [
            {"id": "a", "role": "trunk", "base": "plate", "line": [[0, 0, 0], [0, 0, 30]], "d": 1.5, "joints": 0,
             "foot": [0, 0], "foot_pieces": []},
            {"id": "b", "role": "branch", "base": "parent", "host": "a", "attach": [0, 0, 10],
             "line": [[0, 0, 10], [0.9, 0, 13]], "d": 1.5, "joints": 0},
        ],
        "tips": [], "braces": [], "raft_triangles": 0,
    });
    let errors = |mesh: &[[measure::P; 3]]| real::validate(&truth, &[], mesh, &[], &[]).errors;
    let trunk = prism(0.0, 0.0, 0.01, 30.0, 0.75, 12);
    // Without the branch's shell its line still lies in the support mesh (the trunk's), but in
    // none of its own beyond where it is buried in the trunk.
    let e = errors(&trunk);
    assert!(e.iter().any(|x| x.contains("support b") && x.contains("of its own")), "{e:?}");
    assert!(!e.iter().any(|x| x.contains("support a")), "{e:?}");
    // With its shell, the same truth validates.
    let mut both = trunk.clone();
    both.extend(rod([0.0, 0.0, 10.0], [0.9, 0.0, 13.0], 0.75, 12));
    assert!(errors(&both).is_empty(), "{:?}", errors(&both));
}

/// Validate's stricter checks: a truth line may not run bare of support mesh for over 1 mm, and
/// every connected piece of the support mesh must be the truth's or the raft's.
#[test]
fn validate_wants_mesh_along_every_line_and_a_truth_for_every_piece() {
    let truth = |feet: Value, raft: usize| {
        json!({
            "supports": [{"id": "a", "role": "trunk", "base": "plate", "line": [[0, 0, 0], [0, 0, 30]], "d": 1.0,
                          "joints": 0, "foot": [0, 0], "foot_pieces": feet}],
            "tips": [], "braces": [], "raft_triangles": raft,
        })
    };
    let errors = |t: &Value, mesh: &[[measure::P; 3]]| real::validate(t, &[], mesh, &[], &[]).errors;
    let has = |e: &[String], what: &str| e.iter().any(|x| x.contains(what));
    // The shaft starts a hair above the plate, as Lychee's do: a shell of its own beside its foot.
    let whole = prism(0.0, 0.0, 0.01, 30.0, 0.5, 12);
    assert!(errors(&truth(json!([]), 0), &whole).is_empty(), "{:?}", errors(&truth(json!([]), 0), &whole));
    // The same trunk with 1.5 mm of its shaft missing: 95% of the line is in the mesh, but a
    // stretch of it is bare.
    let mut broken = prism(0.0, 0.0, 0.01, 14.0, 0.5, 12);
    broken.extend(prism(0.0, 0.0, 15.5, 30.0, 0.5, 12));
    let e = errors(&truth(json!([]), 0), &broken);
    assert!(has(&e, "is bare of support mesh") && !has(&e, "of its line is in support mesh"), "{e:?}");
    // A gap of 0.6 mm (a joint between two shells) is no bare stretch.
    let mut jointed = prism(0.0, 0.0, 0.01, 14.0, 0.5, 12);
    jointed.extend(prism(0.0, 0.0, 14.6, 30.0, 0.5, 12));
    assert!(errors(&truth(json!([]), 0), &jointed).is_empty());
    // A foot disc is a piece of its own: the truth must hold it, as measured.
    let mut footed = whole.clone();
    footed.extend(prism(0.0, 0.0, 0.0, 0.6, 1.5, 12));
    let e = errors(&truth(json!([]), 0), &footed);
    assert!(has(&e, "of a piece of the support mesh (48 triangles"), "{e:?}");
    let disc = json!([{"z": [0.0, 0.6], "d": [3.0, 3.0], "sides": 12, "turn_deg": 0.0}]);
    assert!(errors(&truth(disc.clone(), 0), &footed).is_empty());
    // A stray piece beside the trunk is nobody's, unless it is the case's raft (the last triangles
    // of the mesh), which stays low.
    let mut stray = footed.clone();
    stray.extend(prism(3.0, 0.0, 0.0, 0.5, 1.0, 4));
    let e = errors(&truth(disc.clone(), 0), &stray);
    assert!(has(&e, "of a piece of the support mesh (16 triangles"), "{e:?}");
    assert!(errors(&truth(disc.clone(), 16), &stray).is_empty());
    let mut tall = footed.clone();
    tall.extend(prism(3.0, 0.0, 0.0, 9.0, 1.0, 4));
    assert!(has(&errors(&truth(disc, 16), &tall), "the raft reaches 9.0 mm"));
    // A stub welded into the trunk's own shell (sharing its corners) is too small a share of the
    // piece to show there: it is found as a stretch of mesh the truth does not explain.
    let mut stub = whole.clone();
    let top = stub.len() - 1;
    let (a, b) = (stub[top][1], stub[top][2]);
    stub.extend([[a, [2.5, 0.0, 30.0], [2.5, 0.0, 28.5]], [b, [2.5, 0.0, 28.5], [2.5, 0.0, 30.0]]]);
    let e = errors(&truth(json!([]), 0), &stub);
    assert!(has(&e, "of the support mesh round") && !has(&e, "of a piece"), "{e:?}");
    // The pieces of a mesh are its shells.
    assert_eq!(real::pieces(&stray).iter().map(Vec::len).collect::<Vec<_>>(), vec![48, 48, 16]);
}

/// A Lychee kickstand (an upright pillar, then a lean onto a support) must be read as one leg with
/// the same knee: a post under a brace is not it, nor a leg that stops at the knee, nor a leg with
/// more bends.
#[test]
fn a_kickstand_must_be_read_as_one_leg_with_its_knee() {
    let truth = json!({
        "supports": [
            {"id": "a", "role": "trunk", "base": "plate", "line": [[0, 0, 0], [0, 0, 18]], "d": 1.0, "joints": 0},
            {"id": "k", "role": "leg", "base": "plate", "host": "a", "attach": [0, 0, 13],
             "line": [[5, 0, 0], [5, 0, 8], [0, 0, 13]], "d": 1.0, "joints": 1},
        ],
        "tips": [cone_tip("a", [0.0, 0.0, 20.0], [0.0, 0.0, 18.0], "plate", json!([[0, 0, 20], [0, 0, 18], [0, 0, 0]]))],
        "braces": [],
    });
    let reading = |posts: Value, kickstands: Value, braces: Value| {
        json!({"supports": [plate_support([0.0, 0.0, 20.0], [0.0, 0.0, 18.0], [0.0, 0.0])],
               "posts": posts, "kickstands": kickstands, "braces": braces})
    };
    let leg = |joints: Value, knot: [f64; 3]| json!([{"support": 0, "knot": knot, "joints": joints, "foot": [5.0, 0.0, 2.0], "root_d": 2.0, "d": 1.0}]);
    // One leg with the knee.
    let s =
        real::layout_stages(&truth, &reading(json!([]), leg(json!([[5.0, 0.0, 8.0]]), [0.0, 0.0, 13.0]), json!([])));
    for k in ["shafts", "braces", "joints"] {
        assert!(s[k]["pass"] == true, "{k}: {}", s[k]);
    }
    assert!(s["braces"]["kickstands_read"] == 1, "{}", s["braces"]);
    // The pillar as a post and the lean as a brace.
    let post = json!([{"foot": [5.0, 0.0, 2.0], "joints": [], "top": [5.0, 0.0, 8.0], "root_d": 2.0, "d": 1.0}]);
    let brace = json!([{"lower": [1, null, [5.0, 0.0, 8.0]], "upper": [0, null, [0.0, 0.0, 13.0]], "d": 1.0}]);
    let s = real::layout_stages(&truth, &reading(post, json!([]), brace));
    assert!(s["braces"]["pass"] == false && s["braces"]["kickstands_read"] == 0, "{}", s["braces"]);
    assert!(s["braces"]["errors"].to_string().contains("kickstand k: not read as one leg"), "{}", s["braces"]);
    // A straight leg from the foot to the knot cuts the corner: not the kickstand's line.
    let s = real::layout_stages(&truth, &reading(json!([]), leg(json!([]), [0.0, 0.0, 13.0]), json!([])));
    assert!(s["braces"]["pass"] == false && s["braces"]["kickstands_read"] == 0, "{}", s["braces"]);
    // The same line with a joint Lychee does not have is the kickstand, with a joint too many.
    let bends = json!([[5.0, 0.0, 5.0], [5.0, 0.0, 8.0]]);
    let s = real::layout_stages(&truth, &reading(json!([]), leg(bends, [0.0, 0.0, 13.0]), json!([])));
    assert!(s["braces"]["pass"] == true && s["joints"]["pass"] == false, "{} {}", s["braces"], s["joints"]);
}

/// A Lychee brace lying wholly under the root's top is left out of the braces stage; one that
/// rises above it must still be read.
#[test]
fn a_brace_wholly_in_the_root_zone_is_not_compared() {
    let reading = json!({
        "supports": [plate_support([0.0, 0.0, 20.0], [0.0, 0.0, 18.0], [0.0, 0.0]),
                     plate_support([4.0, 0.0, 20.0], [4.0, 0.0, 18.0], [4.0, 0.0])],
        "posts": [], "braces": [], "kickstands": [],
    });
    let s = real::layout_stages(&two_braced_trunks([0.0, 0.0, 0.6], [4.0, 0.0, 1.9]), &reading);
    assert!(s["braces"]["pass"] == true && s["braces"]["under_root"] == 1, "{}", s["braces"]);
    // Its upper end 0.2 mm above the root's top: a brace the reader must have.
    let s = real::layout_stages(&two_braced_trunks([0.0, 0.0, 0.6], [4.0, 0.0, 2.2]), &reading);
    assert!(s["braces"]["pass"] == false && s["braces"]["under_root"] == 0, "{}", s["braces"]);
}

/// A branch hanging from a kickstand may be read as a support that stands in the kickstand's
/// foot and runs up its pillar to where the branch leaves it; a support that bends elsewhere, or
/// the same reading of a branch on a trunk, still has a joint too many.
#[test]
fn a_branch_on_a_kickstand_may_run_up_the_kickstand_s_pillar() {
    let truth = |host_role: &str| {
        json!({
            "supports": [
                {"id": "a", "role": "trunk", "base": "plate", "line": [[0, 0, 0], [0, 0, 18]], "d": 1.0, "joints": 0},
                {"id": "k", "role": host_role, "base": "plate", "host": "a", "attach": [0, 0, 13],
                 "line": [[5, 0, 0], [5, 0, 8], [0, 0, 13]], "d": 1.0, "joints": 1},
                {"id": "b", "role": "branch", "base": "parent", "host": "k", "attach": [5, 0, 5],
                 "line": [[5, 0, 5], [8, 0, 8]], "d": 1.0, "joints": 0},
            ],
            "tips": [
                cone_tip("a", [0.0, 0.0, 20.0], [0.0, 0.0, 18.0], "plate", json!([[0, 0, 20], [0, 0, 18], [0, 0, 0]])),
                cone_tip("b", [8.0, 0.0, 10.0], [8.0, 0.0, 8.0], "parent", json!([[8, 0, 10], [8, 0, 8], [5, 0, 5], [5, 0, 0]])),
            ],
            "braces": [],
        })
    };
    let reading = |joint: [f64; 3]| {
        let mut b = plate_support([8.0, 0.0, 10.0], [8.0, 0.0, 8.0], [5.0, 0.0]);
        b["joints"] = json!([joint]);
        json!({
            "supports": [plate_support([0.0, 0.0, 20.0], [0.0, 0.0, 18.0], [0.0, 0.0]), b],
            "posts": [], "braces": [],
            "kickstands": [{"support": 0, "knot": [0.0, 0.0, 13.0], "joints": [[5.0, 0.0, 8.0]], "foot": [5.0, 0.0, 2.0],
                            "root_d": 2.0, "d": 1.0}],
        })
    };
    let s = real::layout_stages(&truth("leg"), &reading([5.0, 0.0, 5.0]));
    for k in ["routes", "shafts", "braces", "joints"] {
        assert!(s[k]["pass"] == true, "{k}: {}", s[k]);
    }
    assert!(s["joints"]["on_kickstands"] == 1, "{}", s["joints"]);
    // The bend 1.5 mm from where the branch leaves the kickstand is a joint Lychee does not have.
    let s = real::layout_stages(&truth("leg"), &reading([5.0, 0.0, 3.5]));
    assert!(s["joints"]["pass"] == false && s["joints"]["on_kickstands"] == 0, "{}", s["joints"]);
    // On a trunk the branch must hang from the trunk: stood on the plate, its bend counts.
    let s = real::layout_stages(&truth("trunk"), &reading([5.0, 0.0, 5.0]));
    assert!(s["joints"]["pass"] == false && s["joints"]["on_kickstands"] == 0, "{}", s["joints"]);
}
