//! Runs the backend's support reader on Z-up STL files.
//!
//! support-reader contacts MODEL.stl SUPPORT.stl OUT.json       touch-rule contacts
//! support-reader read MODEL.stl SUPPORT.stl OUT_PREFIX [RAFT_MM]   layout and classification JSON
//! support-reader classify MODEL.stl SUPPORT.stl CONTACTS.json OUT_PREFIX   the same, contacts given
//! support-reader voxl MODEL.stl SUPPORT.stl OUT.voxl [RAFT_MM] [CONTACTS.json] [--class OUT_PREFIX]
//!                                                           the VOXL export (with --class also the
//!                                                           layout and classification JSON)
//! support-reader overlap A.stl B.stl M.stl [--raft Z] ...   volume overlap of two support meshes
//! support-reader touch orig|built|selftest ...              touch-rule contacts and built tips
//! See README.md for every command's arguments and output.
//! support-reader pieces MODEL.stl SUPPORT.stl CONTACTS.json OUT.json
//!                                                           pieces, contacts taken from a JSON
//!                                                           file with {"contacts": [{at, d}]}
use findamodel::geometry::support_reader::{self, contacts, math, pieces};
use findamodel::geometry::voxl::{self, VoxlMesh, VoxlPlacement};
use findamodel::geometry::{Triangle, Vec3};
use serde_json::{Value, json};
use std::path::Path;

mod overlap;
mod touch;

fn load_stl(path: &Path) -> Vec<Triangle> {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if b.len() < 84 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(b[80..84].try_into().unwrap()) as usize;
    let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    (0..n)
        .map(|i| {
            let base = 84 + i * 50;
            let p = |k: usize| Vec3::new(f(base + k * 12), f(base + k * 12 + 4), f(base + k * 12 + 8));
            Triangle::new(p(1), p(2), p(3), p(0))
        })
        .collect()
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let t0 = std::time::Instant::now();
    match args.first().map(String::as_str).unwrap_or("") {
        // The measuring tools print their JSON on stdout and nothing else.
        "overlap" => return overlap::run(&args[1..]),
        "touch" => return touch::run(&args[1..]),
        "contacts" => {
            let model = math::points_of(&load_stl(Path::new(&args[1])));
            let support = math::points_of(&load_stl(Path::new(&args[2])));
            let c = contacts::read(&model, &support, 3.5);
            std::fs::write(&args[3], json!({ "contacts": c }).to_string()).unwrap();
        }
        "pieces" => {
            let model = math::points_of(&load_stl(Path::new(&args[1])));
            let support = math::points_of(&load_stl(Path::new(&args[2])));
            let t: Value = serde_json::from_slice(&std::fs::read(&args[3]).unwrap()).unwrap();
            let cs: Vec<(math::P, f64)> = t["contacts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    let p = |k: usize| c["at"][k].as_f64().unwrap();
                    ([p(0), p(1), p(2)], c["d"].as_f64().unwrap_or(0.5))
                })
                .collect();
            let p = pieces::read(&support, &model, &cs, pieces::RAY_PITCH_MM);
            std::fs::write(&args[4], serde_json::to_string(&p).unwrap()).unwrap();
        }
        "read" => {
            // read MODEL.stl SUPPORT.stl OUT_PREFIX [RAFT_MM]: OUT_PREFIX.layout.json, .class.json
            let model = load_stl(Path::new(&args[1]));
            let support = load_stl(Path::new(&args[2]));
            let raft: f32 = args.get(4).map(|x| x.parse().unwrap()).unwrap_or(3.0);
            let r = support_reader::read(&model, &support, raft);
            std::fs::write(format!("{}.layout.json", args[3]), r.classification.layout_json.to_string()).unwrap();
            std::fs::write(format!("{}.class.json", args[3]), serde_json::to_string(&r.classification).unwrap())
                .unwrap();
        }
        "classify" => {
            // classify MODEL.stl SUPPORT.stl CONTACTS.json OUT_PREFIX: as read, with the contacts given
            let model = math::points_of(&load_stl(Path::new(&args[1])));
            let support = math::points_of(&load_stl(Path::new(&args[2])));
            let t: Value = serde_json::from_slice(&std::fs::read(&args[3]).unwrap()).unwrap();
            let cs: Vec<contacts::Contact> = serde_json::from_value(t["contacts"].clone()).unwrap();
            let at: Vec<(math::P, f64)> = cs.iter().map(|c| (c.at, c.d)).collect();
            let p = pieces::read(&support, &model, &at, pieces::RAY_PITCH_MM);
            let c = support_reader::classify::classify(&p, &cs, &model);
            std::fs::write(format!("{}.layout.json", args[4]), c.layout_json.to_string()).unwrap();
            std::fs::write(format!("{}.class.json", args[4]), serde_json::to_string(&c).unwrap()).unwrap();
        }
        "voxl" => {
            // voxl MODEL.stl SUPPORT.stl OUT.voxl [RAFT_MM] [CONTACTS.json]: the whole export, as the
            // app writes it (the model without its supports, with the read supports as DragonFruit's);
            // with CONTACTS.json the touch-rule contacts are taken from it (prototype comparisons).
            let class_out = args.iter().position(|a| a == "--class").map(|k| {
                let prefix = args[k + 1].clone();
                args.drain(k..k + 2);
                prefix
            });
            let model = load_stl(Path::new(&args[1]));
            let support = load_stl(Path::new(&args[2]));
            let raft: f32 = args.get(4).map(|x| x.parse().unwrap()).unwrap_or(3.0);
            let t_read = std::time::Instant::now();
            let classification = match args.get(5) {
                Some(path) => {
                    let (mp, sp) = (math::points_of(&model), math::points_of(&support));
                    let t: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                    let cs: Vec<contacts::Contact> = serde_json::from_value(t["contacts"].clone()).unwrap();
                    let at: Vec<(math::P, f64)> = cs.iter().map(|c| (c.at, c.d)).collect();
                    let p = pieces::read(&sp, &mp, &at, pieces::RAY_PITCH_MM);
                    support_reader::classify::classify(&p, &cs, &mp)
                }
                None => support_reader::read(&model, &support, raft).classification,
            };
            let read_s = t_read.elapsed().as_secs_f64();
            if let Some(prefix) = &class_out {
                std::fs::write(format!("{prefix}.layout.json"), classification.layout_json.to_string()).unwrap();
                std::fs::write(format!("{prefix}.class.json"), serde_json::to_string(&classification).unwrap())
                    .unwrap();
            }
            let layout = &classification.layout;
            // LAYOUT_DEBUG=PATH: the layout as the writer takes it (Rust debug format), for comparisons.
            if let Ok(path) = std::env::var("LAYOUT_DEBUG") {
                std::fs::write(path, format!("{layout:#?}")).unwrap();
            }
            let t_write = std::time::Instant::now();
            let mesh = VoxlMesh { name: "model.stl", triangles: &model, supports: Some(layout) };
            let placement = VoxlPlacement { mesh: 0, rotation_z: 0.0, translation: [0.0; 3] };
            // SPHERE_TIPS=1 writes ball tips as sphere tips whatever the backend's setting (they
            // need the owner's DragonFruit with sphereDepthMm).
            let sphere = std::env::var("SPHERE_TIPS").map(|v| v == "1").unwrap_or(voxl::SPHERE_TIPS_ENABLED);
            let out =
                voxl::write_voxl_with(std::io::Cursor::new(Vec::new()), &[mesh], &[placement], None, sphere).unwrap();
            std::fs::write(&args[3], out.into_inner()).unwrap();
            // One JSON line for the round-trip runner: the read (contacts, pieces, classification) and the
            // VOXL write, in seconds.
            println!(
                "{}",
                json!({ "read_s": read_s, "write_s": t_write.elapsed().as_secs_f64(), "sphere_tips": sphere })
            );
        }
        other => panic!("unknown command {other}"),
    }
    eprintln!("{} {:.2}s", args[0], t0.elapsed().as_secs_f64());
}
