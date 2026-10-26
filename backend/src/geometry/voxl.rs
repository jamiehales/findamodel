//! Writes DragonFruit's VOXL scene container (Open-Resin-Alliance/DragonFruit,
//! `docs/dev/voxl-format-spec.md`): a 16-byte header, a chunk directory, then zlib-compressed
//! chunks. Scene state is JSON (`META`, `SCNE`, `MODL`, `SUPP`) and each distinct mesh is one
//! binary STL `MESH` chunk. Placements that share a mesh point at the first placement's chunk,
//! which raises the container version from 2 to 3. A mesh's support layout is written to
//! `SUPP` as DragonFruit roots, trunks, branches, twigs, sticks, braces, kickstands and knots
//! for every placement of it.

use std::io::{Seek, SeekFrom, Write};

use anyhow::{Result, anyhow, bail};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::support_layout::{
    ContactTip, ROOT_CONE_HEIGHT_MM, ROOT_DISK_HEIGHT_MM, SupportBase, SupportLayout, SupportPath,
};
use super::{Triangle, Vec3, saver};

const MAGIC: &[u8; 4] = b"VOXL";
const HEADER_SIZE: usize = 16;
const DIR_ENTRY_SIZE: usize = 20;
const COMPRESSION_ZLIB: u16 = 1;
/// Container version when every placement has its own `MESH` chunk.
const VERSION_UNIQUE_MESHES: u16 = 2;
/// Container version when placements share `MESH` chunks via `mesh.chunkIndex`.
const VERSION_SHARED_MESHES: u16 = 3;
/// DragonFruit's default model colour.
const MODEL_COLOR: &str = "#a3a3a3";
/// DragonFruit draws joints this much wider than their shaft.
const JOINT_DIAMETER_OFFSET_MM: f32 = 0.1;
/// Contact tip defaults DragonFruit's own tips use.
const TIP_PENETRATION_MM: f32 = 0.1;
const TIP_DISK_THICKNESS_MM: f32 = 0.1;
const TIP_MAX_STANDOFF_MM: f32 = 0.25;
/// DragonFruit places a strut between two model surfaces as a twig (contact disks) when its
/// contacts are closer than this, and as a stick (contact cones) otherwise.
const STICK_MIN_SPAN_MM: f32 = 5.0;
/// A twig's end joints are this much wider than its contact disks, and stand off the disk by
/// their radius plus this clearance.
const TWIG_JOINT_RATIO: f32 = 1.1;
const TWIG_JOINT_CLEARANCE_MM: f32 = 0.05;
/// DragonFruit marks down a brace knot within this share of its segment's length of either end
/// (`t <= 0.02` or `t >= 0.98`), so brace knots are kept just inside it.
const BRACE_KNOT_END_SHARE: f64 = 0.025;
/// Support coordinates are rounded to this many decimal places.
const COORDINATE_DECIMALS: i32 = 4;

/// A mesh in the VOXL frame: millimetres, right-handed, Z-up.
pub struct VoxlMesh<'a> {
    pub name: &'a str,
    pub triangles: &'a [Triangle],
    /// Supports in the same frame as `triangles`, written for every placement of the mesh.
    pub supports: Option<&'a SupportLayout>,
}

/// One model on the plate. A vertex `v` of `meshes[mesh]` lands at
/// `Rz(rotation_z) * v + translation`, with the bed centre at the origin.
pub struct VoxlPlacement {
    pub mesh: usize,
    pub rotation_z: f64,
    pub translation: [f64; 3],
}

struct ChunkEntry {
    tag: [u8; 4],
    index: u16,
    offset: u64,
    compressed_size: u64,
    uncompressed_size: u64,
}

struct StoredMesh {
    owner: usize,
    centre: Vec3,
    triangle_count: usize,
    size_bytes: usize,
    sha256: String,
}

/// Writes a VOXL scene with one plate. `out` must be positioned at the start of the file.
pub fn write_voxl<W: Write + Seek>(
    mut out: W,
    meshes: &[VoxlMesh<'_>],
    placements: &[VoxlPlacement],
    plate_name: Option<&str>,
) -> Result<W> {
    // MESH chunk indices are u16 and name the owning placement.
    if placements.len() > usize::from(u16::MAX) {
        bail!("VOXL files hold at most {} models", u16::MAX);
    }
    let mut owners: Vec<Option<usize>> = vec![None; meshes.len()];
    for (i, placement) in placements.iter().enumerate() {
        let owner = owners.get_mut(placement.mesh).ok_or_else(|| anyhow!("Placement {i} names an unknown mesh"))?;
        owner.get_or_insert(i);
    }
    let mut used: Vec<(usize, usize)> =
        owners.iter().enumerate().filter_map(|(mesh, owner)| owner.map(|o| (o, mesh))).collect();
    used.sort_unstable();

    let chunk_count = used.len() + 4;
    out.write_all(&vec![0u8; HEADER_SIZE + chunk_count * DIR_ENTRY_SIZE])?;
    let mut entries = Vec::with_capacity(chunk_count);

    // Each mesh is stored centred on its bounding box, which is the local frame DragonFruit
    // places from; `transform.position` is where that centre ends up.
    let mut stored: Vec<Option<StoredMesh>> = (0..meshes.len()).map(|_| None).collect();
    for &(owner, mesh_index) in &used {
        let mesh = &meshes[mesh_index];
        let centre = bbox_centre(mesh.triangles);
        let triangle_count = u32::try_from(mesh.triangles.len()).map_err(|_| anyhow!("Mesh is too large for VOXL"))?;
        let mut stl = Vec::with_capacity(84 + mesh.triangles.len() * 50);
        let centred =
            mesh.triangles.iter().map(|t| Triangle::new(t.v0 - centre, t.v1 - centre, t.v2 - centre, t.normal));
        saver::write_stl(&mut stl, triangle_count, centred, Some(mesh.name))?;
        let sha256 = hex::encode(Sha256::digest(&stl));
        entries.push(write_chunk(&mut out, *b"MESH", owner as u16, &stl)?);
        stored[mesh_index] =
            Some(StoredMesh { owner, centre, triangle_count: mesh.triangles.len(), size_bytes: stl.len(), sha256 });
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let meta = json!({
        "generator": "findamodel",
        "generatorVersion": env!("CARGO_PKG_VERSION"),
        "createdAt": now,
        "updatedAt": now,
        "units": "mm",
        "coordinateSystem": "right-handed-z-up",
    });

    let plate_id = Uuid::new_v4().to_string();
    let mut plate = json!({ "id": plate_id });
    let name = plate_name.map(str::trim).filter(|n| !n.is_empty());
    if let Some(name) = name {
        plate["name"] = json!(name);
    }
    let mut scene = json!({
        "activeModelId": null,
        "selectedModelIds": [],
        "plates": [plate],
        "activePlateId": plate_id,
    });
    if let Some(name) = name {
        scene["plateName"] = json!(name);
    }

    let model_ids: Vec<String> = placements.iter().map(|_| Uuid::new_v4().to_string()).collect();
    let models: Vec<Value> = placements
        .iter()
        .enumerate()
        .map(|(i, placement)| {
            let mesh = stored[placement.mesh].as_ref().expect("every placed mesh is stored");
            let name = meshes[placement.mesh].name;
            let (s, c) = placement.rotation_z.sin_cos();
            let (cx, cy, cz) = (f64::from(mesh.centre.x), f64::from(mesh.centre.y), f64::from(mesh.centre.z));
            let [tx, ty, tz] = placement.translation;
            let mut mesh_ref = json!({
                "mode": "embedded-chunk",
                "fileName": name,
                "mimeType": "model/stl",
                "uncompressedSizeBytes": mesh.size_bytes,
                "sha256": mesh.sha256,
            });
            if mesh.owner != i {
                mesh_ref["chunkIndex"] = json!(mesh.owner);
            }
            json!({
                "id": model_ids[i],
                "name": name,
                "visible": true,
                "color": MODEL_COLOR,
                "polygonCount": mesh.triangle_count,
                "transform": {
                    "position": { "x": cx * c - cy * s + tx, "y": cx * s + cy * c + ty, "z": cz + tz },
                    "rotation": { "x": 0, "y": 0, "z": placement.rotation_z },
                    "scale": { "x": 1, "y": 1, "z": 1 },
                },
                "mesh": mesh_ref,
            })
        })
        .collect();

    let mut supports = SupportDocument::default();
    for (placement, model_id) in placements.iter().zip(&model_ids) {
        if let Some(layout) = meshes[placement.mesh].supports {
            supports.add(layout, model_id, placement);
        }
    }
    let supports = json!({
        "version": 1,
        "meta": { "source": "findamodel", "objectCenter": { "x": 0, "y": 0, "z": 0 } },
        "roots": supports.roots,
        "trunks": supports.trunks,
        "branches": supports.branches,
        "leaves": [],
        "twigs": supports.twigs,
        "sticks": supports.sticks,
        "braces": supports.braces,
        "knots": supports.knots,
        "kickstands": supports.kickstands,
    });

    for (tag, value) in [(*b"META", meta), (*b"SCNE", scene), (*b"MODL", Value::Array(models)), (*b"SUPP", supports)] {
        entries.push(write_chunk(&mut out, tag, 0, &serde_json::to_vec(&value)?)?);
    }

    let version = if used.len() < placements.len() { VERSION_SHARED_MESHES } else { VERSION_UNIQUE_MESHES };
    let mut preamble = Vec::with_capacity(HEADER_SIZE + entries.len() * DIR_ENTRY_SIZE);
    preamble.extend_from_slice(MAGIC);
    preamble.extend_from_slice(&version.to_le_bytes());
    preamble.extend_from_slice(&0u16.to_le_bytes());
    preamble.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    preamble.extend_from_slice(&0u32.to_le_bytes());
    for entry in &entries {
        let fits = |v: u64| u32::try_from(v).map_err(|_| anyhow!("VOXL files are limited to 4 GiB"));
        preamble.extend_from_slice(&entry.tag);
        preamble.extend_from_slice(&entry.index.to_le_bytes());
        preamble.extend_from_slice(&COMPRESSION_ZLIB.to_le_bytes());
        preamble.extend_from_slice(&fits(entry.offset)?.to_le_bytes());
        preamble.extend_from_slice(&fits(entry.compressed_size)?.to_le_bytes());
        preamble.extend_from_slice(&fits(entry.uncompressed_size)?.to_le_bytes());
    }
    out.seek(SeekFrom::Start(0))?;
    out.write_all(&preamble)?;
    out.seek(SeekFrom::End(0))?;
    Ok(out)
}

/// DragonFruit support entities (`DragonfruitImportFormat`), in plate coordinates.
#[derive(Default)]
struct SupportDocument {
    roots: Vec<Value>,
    trunks: Vec<Value>,
    branches: Vec<Value>,
    twigs: Vec<Value>,
    sticks: Vec<Value>,
    braces: Vec<Value>,
    knots: Vec<Value>,
    /// Each a `{ root, hostKnot, kickstand }` bundle (`KickstandBuildResult`): a kickstand's
    /// root and host knot travel with it rather than in `roots` and `knots`.
    kickstands: Vec<Value>,
}

/// A shaft segment's id and its plate-coordinate ends.
struct Span {
    id: String,
    start: [f64; 3],
    end: [f64; 3],
}

impl SupportDocument {
    fn add(&mut self, layout: &SupportLayout, model_id: &str, placement: &VoxlPlacement) {
        let (s, c) = placement.rotation_z.sin_cos();
        let turn = |v: Vec3| {
            let (x, y) = (f64::from(v.x), f64::from(v.y));
            [x * c - y * s, x * s + y * c, f64::from(v.z)]
        };
        let place = |v: Vec3| {
            let [x, y, z] = turn(v);
            let [tx, ty, tz] = placement.translation;
            [x + tx, y + ty, z + tz]
        };

        // Each support's segments, so later supports can hang knots on them.
        let mut shafts: Vec<Vec<Span>> = Vec::with_capacity(layout.supports.len());
        for support in &layout.supports {
            if let SupportBase::Model(lower) = &support.base {
                shafts.push(self.add_strut(support, lower, model_id, &place, &turn));
                continue;
            }
            let joint_diameter = support.shaft_diameter_mm + JOINT_DIAMETER_OFFSET_MM;
            let socket_id = Uuid::new_v4().to_string();
            let mut ends: Vec<[f64; 3]> = support.joints.iter().map(|&p| place(p)).collect();
            ends.push(place(support.tip.socket));
            let joints: Vec<Value> = ends
                .iter()
                .enumerate()
                .map(|(i, &pos)| {
                    let id = if i + 1 == ends.len() { socket_id.clone() } else { Uuid::new_v4().to_string() };
                    json!({ "id": id, "pos": point(pos), "diameter": round(joint_diameter) })
                })
                .collect();

            // A branch hangs from a knot on its parent's shaft, which is where DragonFruit starts
            // its first segment (not the attach point itself, which can lie off that shaft).
            let knot = match &support.base {
                SupportBase::Shaft { parent, point: at } => {
                    let host = layout.supports[*parent].shaft_diameter_mm;
                    Some(knot_on(&shafts[*parent], place(*at), host + JOINT_DIAMETER_OFFSET_MM, KnotUse::Host))
                }
                _ => None,
            };

            // Segment 0 starts at the root top or the knot; each later one at the joint below.
            let mut start = match &knot {
                Some(knot) => position_of(&knot["pos"]),
                None => place(support.shaft_start()),
            };
            let mut spans = Vec::with_capacity(joints.len());
            let mut segments = Vec::with_capacity(joints.len());
            for (i, joint) in joints.iter().enumerate() {
                let id = Uuid::new_v4().to_string();
                let mut segment = json!({ "id": id, "diameter": round(support.shaft_diameter_mm), "topJoint": joint });
                if i > 0 {
                    segment["bottomJoint"] = joints[i - 1].clone();
                }
                segments.push(segment);
                spans.push(Span { id, start, end: ends[i] });
                start = ends[i];
            }

            let contact_cone = contact_cone(&support.tip, &socket_id, &place, &turn);
            match &support.base {
                SupportBase::Plate { x, y, diameter_mm } => {
                    let root = root(model_id, place(Vec3::new(*x, *y, 0.0)), *diameter_mm);
                    let root_id = root["id"].clone();
                    self.roots.push(root);
                    self.trunks.push(json!({
                        "id": Uuid::new_v4().to_string(),
                        "modelId": model_id,
                        "typeId": "trunk",
                        "rootId": root_id,
                        "baseDiameterMm": round(support.shaft_diameter_mm),
                        "segments": segments,
                        "contactCone": contact_cone,
                    }));
                }
                SupportBase::Shaft { .. } => {
                    let knot = knot.expect("a branch has a knot");
                    let knot_id = knot["id"].clone();
                    self.knots.push(knot);
                    self.branches.push(json!({
                        "id": Uuid::new_v4().to_string(),
                        "modelId": model_id,
                        "typeId": "branch",
                        "parentKnotId": knot_id,
                        "segments": segments,
                        "contactCone": contact_cone,
                    }));
                }
                SupportBase::Model(_) => unreachable!("struts are written above"),
            }
            shafts.push(spans);
        }

        // Each brace hangs from a knot on each of the two supports it joins, lower end first.
        // DragonFruit prints a brace as wide as its knots (less the joint offset), so they
        // carry the brace's own diameter.
        for brace in &layout.braces {
            let mut knot_ids = Vec::with_capacity(2);
            for end in [&brace.lower, &brace.upper] {
                let diameter = brace.diameter_mm + JOINT_DIAMETER_OFFSET_MM;
                let knot = knot_on(&shafts[end.support], place(end.point), diameter, KnotUse::Brace);
                knot_ids.push(knot["id"].clone());
                self.knots.push(knot);
            }
            self.braces.push(json!({
                "id": Uuid::new_v4().to_string(),
                "modelId": model_id,
                "typeId": "brace",
                "startKnotId": knot_ids[0],
                "endKnotId": knot_ids[1],
                "profile": { "diameter": round(brace.diameter_mm) },
            }));
        }

        // Each kickstand stands on its own root and hangs from a knot on the support it holds
        // up: a column segment up to each joint, then the leaning segment up to the knot.
        for kickstand in &layout.kickstands {
            let host = layout.supports[kickstand.support].shaft_diameter_mm + JOINT_DIAMETER_OFFSET_MM;
            let knot = knot_on(&shafts[kickstand.support], place(kickstand.knot), host, KnotUse::Host);
            let foot = kickstand.foot;
            let root = root(model_id, place(Vec3::new(foot.x, foot.y, 0.0)), kickstand.root_diameter_mm);
            let joint_diameter = round(kickstand.diameter_mm + JOINT_DIAMETER_OFFSET_MM);
            let joints: Vec<Value> = kickstand
                .joints
                .iter()
                .map(|&p| json!({ "id": Uuid::new_v4().to_string(), "pos": point(place(p)), "diameter": joint_diameter }))
                .collect();
            let segments: Vec<Value> = (0..=joints.len())
                .map(|i| {
                    let mut segment =
                        json!({ "id": Uuid::new_v4().to_string(), "diameter": round(kickstand.diameter_mm) });
                    if i > 0 {
                        segment["bottomJoint"] = joints[i - 1].clone();
                    }
                    if let Some(joint) = joints.get(i) {
                        segment["topJoint"] = joint.clone();
                    }
                    segment
                })
                .collect();
            let diameter = round(kickstand.diameter_mm);
            self.kickstands.push(json!({
                "kickstand": {
                    "id": Uuid::new_v4().to_string(),
                    "modelId": model_id,
                    "typeId": "kickstand",
                    "rootId": root["id"],
                    "hostKnotId": knot["id"],
                    "hostSegmentId": knot["parentShaftId"],
                    "hostMinT": 0,
                    "segments": segments,
                    "profile": {
                        "bodyDiameterMm": diameter,
                        "terminalStartDiameterMm": diameter,
                        "terminalEndDiameterMm": diameter,
                    },
                },
                "root": root,
                "hostKnot": knot,
            }));
        }
    }

    /// A strut between two parts of the model, lower end first (A), as DragonFruit's own
    /// placement would make it: a twig when the contacts are close, otherwise a stick.
    fn add_strut(
        &mut self,
        support: &SupportPath,
        lower: &ContactTip,
        model_id: &str,
        place: &dyn Fn(Vec3) -> [f64; 3],
        turn: &dyn Fn(Vec3) -> [f64; 3],
    ) -> Vec<Span> {
        let upper = &support.tip;
        let diameter = support.shaft_diameter_mm;
        let twig = (upper.contact - lower.contact).length() < STICK_MIN_SPAN_MM;
        // A twig's shaft runs between joints standing off its disks; a stick's between its sockets.
        let (joint_diameter, standoff) = if twig {
            let joint = diameter * TWIG_JOINT_RATIO;
            (joint, (joint / 2.0 + TWIG_JOINT_CLEARANCE_MM).max(TIP_DISK_THICKNESS_MM))
        } else {
            (diameter + JOINT_DIAMETER_OFFSET_MM, 0.0)
        };
        let end = |tip: &ContactTip| if twig { tip.contact + tip.surface_normal * standoff } else { tip.socket };
        let (a, b) = (end(lower), end(upper));

        let mut ends = vec![place(a)];
        ends.extend(support.joints.iter().map(|&p| place(p)));
        ends.push(place(b));
        let joints: Vec<Value> = ends
            .iter()
            .map(|&pos| json!({ "id": Uuid::new_v4().to_string(), "pos": point(pos), "diameter": round(joint_diameter) }))
            .collect();
        let mut spans = Vec::with_capacity(ends.len() - 1);
        let mut segments = Vec::with_capacity(ends.len() - 1);
        for i in 1..ends.len() {
            let id = Uuid::new_v4().to_string();
            segments.push(json!({
                "id": id,
                "diameter": round(diameter),
                "bottomJoint": joints[i - 1],
                "topJoint": joints[i],
            }));
            spans.push(Span { id, start: ends[i - 1], end: ends[i] });
        }

        let mut strut = json!({
            "id": Uuid::new_v4().to_string(),
            "modelId": model_id,
            "typeId": if twig { "twig" } else { "stick" },
            "segments": segments,
        });
        if twig {
            let disk = |tip: &ContactTip, toward: Vec3| {
                json!({
                    "id": Uuid::new_v4().to_string(),
                    "pos": point(place(tip.contact)),
                    "surfaceNormal": point(turn(tip.surface_normal)),
                    "coneAxis": point(turn((toward - tip.contact).normalized())),
                    "diskLengthOverride": round(standoff),
                    "profile": {
                        "type": "disk",
                        "diskThicknessMm": round(TIP_DISK_THICKNESS_MM),
                        "maxStandoffMm": round(TIP_MAX_STANDOFF_MM),
                        "standoffAngleThreshold": std::f64::consts::FRAC_PI_4,
                    },
                    "contactDiameterMm": round(diameter),
                })
            };
            strut["contactDiskA"] = disk(lower, b);
            strut["contactDiskB"] = disk(upper, a);
            self.twigs.push(strut);
        } else {
            let socket_id = |joint: &Value| joint["id"].as_str().expect("joint id").to_string();
            strut["contactConeA"] = contact_cone(lower, &socket_id(&joints[0]), place, turn);
            strut["contactConeB"] = contact_cone(upper, &socket_id(&joints[joints.len() - 1]), place, turn);
            self.sticks.push(strut);
        }
        spans
    }
}

/// The tip as a DragonFruit contact cone: a disk standing off the surface along its normal,
/// then a cone from the disk to the socket.
fn contact_cone(
    tip: &ContactTip,
    socket_id: &str,
    place: &dyn Fn(Vec3) -> [f64; 3],
    turn: &dyn Fn(Vec3) -> [f64; 3],
) -> Value {
    let disk = (tip.contact_diameter_mm / 2.0).max(TIP_DISK_THICKNESS_MM);
    let axis = tip.socket - (tip.contact + tip.surface_normal * disk);
    let length = axis.length();
    let direction = if length > 1e-4 { axis * (1.0 / length) } else { tip.surface_normal };
    json!({
        "id": Uuid::new_v4().to_string(),
        "pos": point(place(tip.contact)),
        "normal": point(turn(direction)),
        "surfaceNormal": point(turn(tip.surface_normal)),
        "diskLengthOverride": round(disk),
        "profile": {
            "type": "disk",
            "contactDiameterMm": round(tip.contact_diameter_mm),
            "bodyDiameterMm": round(tip.body_diameter_mm),
            "lengthMm": round(length),
            "penetrationMm": round(TIP_PENETRATION_MM),
            "diskThicknessMm": round(TIP_DISK_THICKNESS_MM),
            "maxStandoffMm": round(TIP_MAX_STANDOFF_MM),
            "standoffAngleThreshold": std::f64::consts::FRAC_PI_4,
        },
        "socketJointId": socket_id,
    })
}

/// A root standing on the plate at `pos`.
fn root(model_id: &str, pos: [f64; 3], diameter_mm: f32) -> Value {
    json!({
        "id": Uuid::new_v4().to_string(),
        "modelId": model_id,
        "transform": { "pos": point(pos), "rot": { "x": 0, "y": 0, "z": 0, "w": 1 } },
        "diameter": round(diameter_mm),
        "diskHeight": round(ROOT_DISK_HEIGHT_MM),
        "coneHeight": round(ROOT_CONE_HEIGHT_MM),
    })
}

/// What a knot holds: a branch or kickstand hanging from it, or one end of a brace.
#[derive(Clone, Copy, PartialEq)]
enum KnotUse {
    Host,
    Brace,
}

/// A knot `diameter_mm` wide at the point of `spans` closest to `at`.
///
/// On opening a scene DragonFruit projects every knot back onto its segment, and moves a
/// brace's knot to another segment of the support when that scores better: a knot within
/// [`BRACE_KNOT_END_SHARE`] of either end of its segment is marked down, so it would go to a
/// neighbouring segment, wherever that projects. A brace's knot is kept that far inside its
/// segment, and is flagged as an imported brace end so DragonFruit keeps its diameter.
fn knot_on(spans: &[Span], at: [f64; 3], diameter_mm: f32, used_by: KnotUse) -> Value {
    let project = |span: &Span| {
        let d: Vec<f64> = (0..3).map(|k| span.end[k] - span.start[k]).collect();
        let length_sq: f64 = d.iter().map(|v| v * v).sum();
        let t = if length_sq > 0.0 {
            ((0..3).map(|k| (at[k] - span.start[k]) * d[k]).sum::<f64>() / length_sq).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let pos = [0, 1, 2].map(|k| span.start[k] + d[k] * t);
        let miss: f64 = (0..3).map(|k| (pos[k] - at[k]).powi(2)).sum();
        (miss, t)
    };
    let (span, (_, t)) = spans
        .iter()
        .map(|span| (span, project(span)))
        .min_by(|a, b| a.1.0.total_cmp(&b.1.0))
        .expect("a support has at least one segment");
    let t = match used_by {
        KnotUse::Host => t,
        KnotUse::Brace => t.clamp(BRACE_KNOT_END_SHARE, 1.0 - BRACE_KNOT_END_SHARE),
    };
    let pos = [0, 1, 2].map(|k| span.start[k] + (span.end[k] - span.start[k]) * t);
    json!({
        "id": Uuid::new_v4().to_string(),
        "parentShaftId": span.id,
        "t": t,
        "pos": point(pos),
        "diameter": round(diameter_mm),
        "normalizationHint": if used_by == KnotUse::Brace { "braceImported" } else { "project" },
    })
}

fn round(v: impl Into<f64>) -> f64 {
    let scale = 10f64.powi(COORDINATE_DECIMALS);
    (v.into() * scale).round() / scale
}

/// A point written by [`point`], as DragonFruit reads it back.
fn position_of(v: &Value) -> [f64; 3] {
    ["x", "y", "z"].map(|k| v[k].as_f64().unwrap_or(0.0))
}

fn point([x, y, z]: [f64; 3]) -> Value {
    json!({ "x": round(x), "y": round(y), "z": round(z) })
}

fn write_chunk<W: Write + Seek>(out: &mut W, tag: [u8; 4], index: u16, data: &[u8]) -> Result<ChunkEntry> {
    let offset = out.stream_position()?;
    let mut encoder = ZlibEncoder::new(&mut *out, Compression::default());
    encoder.write_all(data)?;
    encoder.finish()?;
    let compressed_size = out.stream_position()? - offset;
    Ok(ChunkEntry { tag, index, offset, compressed_size, uncompressed_size: data.len() as u64 })
}

fn bbox_centre(triangles: &[Triangle]) -> Vec3 {
    let mut vertices = triangles.iter().flat_map(|t| [t.v0, t.v1, t.v2]);
    let Some(first) = vertices.next() else { return Vec3::default() };
    let (min, max) = vertices.fold((first, first), |(min, max), v| {
        (
            Vec3::new(min.x.min(v.x), min.y.min(v.y), min.z.min(v.z)),
            Vec3::new(max.x.max(v.x), max.y.max(v.y), max.z.max(v.z)),
        )
    });
    (min + max) * 0.5
}

/// A decompressed chunk: tag, index and data.
#[cfg(test)]
pub(crate) type ParsedChunk = ([u8; 4], u16, Vec<u8>);

/// Test helper: parses a VOXL file into its version and decompressed chunks, checking the
/// header, chunk bounds and decompressed sizes.
#[cfg(test)]
pub(crate) fn read_voxl(bytes: &[u8]) -> (u16, Vec<ParsedChunk>) {
    use std::io::Read;

    let u16_at = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(&bytes[0..4], MAGIC);
    let count = u32_at(8);
    let chunks = (0..count)
        .map(|i| {
            let base = HEADER_SIZE + i * DIR_ENTRY_SIZE;
            let tag: [u8; 4] = bytes[base..base + 4].try_into().unwrap();
            assert_eq!(u16_at(base + 6), COMPRESSION_ZLIB);
            let (offset, compressed, uncompressed) = (u32_at(base + 8), u32_at(base + 12), u32_at(base + 16));
            assert!(offset >= HEADER_SIZE + count * DIR_ENTRY_SIZE && offset + compressed <= bytes.len());
            let mut data = Vec::new();
            flate2::read::ZlibDecoder::new(&bytes[offset..offset + compressed]).read_to_end(&mut data).unwrap();
            assert_eq!(data.len(), uncompressed);
            (tag, u16_at(base + 4), data)
        })
        .collect();
    (u16_at(4), chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Triangle {
        let v = |p: [f32; 3]| Vec3::new(p[0], p[1], p[2]);
        Triangle::new(v(a), v(b), v(c), Vec3::new(0.0, 0.0, 1.0))
    }

    fn write(meshes: &[VoxlMesh<'_>], placements: &[VoxlPlacement], name: Option<&str>) -> Vec<u8> {
        write_voxl(std::io::Cursor::new(Vec::new()), meshes, placements, name).unwrap().into_inner()
    }

    fn json_chunk(chunks: &[ParsedChunk], tag: &[u8; 4]) -> Value {
        let (_, _, data) = chunks.iter().find(|(t, _, _)| t == tag).unwrap();
        serde_json::from_slice(data).unwrap()
    }

    fn placement(mesh: usize, x: f64) -> VoxlPlacement {
        VoxlPlacement { mesh, rotation_z: 0.0, translation: [x, 0.0, 0.0] }
    }

    #[test]
    fn shared_meshes_store_one_chunk_and_raise_the_version() {
        let a = [tri([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 5.0])];
        let b =
            [tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 1.0]), tri([0.0; 3], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0])];
        let meshes = [
            VoxlMesh { name: "a.stl", triangles: &a, supports: None },
            VoxlMesh { name: "b.stl", triangles: &b, supports: None },
        ];
        let bytes = write(&meshes, &[placement(0, 0.0), placement(0, 20.0), placement(1, 40.0)], None);

        let (version, chunks) = read_voxl(&bytes);
        assert_eq!(version, VERSION_SHARED_MESHES);
        let mesh_chunks: Vec<_> = chunks.iter().filter(|(t, _, _)| t == b"MESH").collect();
        assert_eq!(mesh_chunks.iter().map(|(_, i, _)| *i).collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!(u32::from_le_bytes(mesh_chunks[1].2[80..84].try_into().unwrap()), 2);

        let models = json_chunk(&chunks, b"MODL");
        assert_eq!(models[1]["mesh"]["chunkIndex"], 0);
        assert!(models[0]["mesh"].get("chunkIndex").is_none());
        assert!(models[2]["mesh"].get("chunkIndex").is_none());
        assert_eq!(models[2]["polygonCount"], 2);
        assert_eq!(models[0]["mesh"]["sha256"], hex::encode(Sha256::digest(&mesh_chunks[0].2)));
        assert_eq!(models[0]["mesh"]["uncompressedSizeBytes"], mesh_chunks[0].2.len());

        assert_eq!(json_chunk(&chunks, b"META")["coordinateSystem"], "right-handed-z-up");
        assert_eq!(json_chunk(&chunks, b"SUPP")["roots"], json!([]));
    }

    #[test]
    fn unique_meshes_keep_version_two() {
        let a = [tri([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])];
        let meshes = [
            VoxlMesh { name: "a.stl", triangles: &a, supports: None },
            VoxlMesh { name: "b.stl", triangles: &a, supports: None },
        ];
        let (version, chunks) = read_voxl(&write(&meshes, &[placement(0, 0.0), placement(1, 5.0)], None));
        assert_eq!(version, VERSION_UNIQUE_MESHES);
        let models = json_chunk(&chunks, b"MODL");
        assert!(models.as_array().unwrap().iter().all(|m| m["mesh"].get("chunkIndex").is_none()));
    }

    #[test]
    fn placement_matches_dragonfruit_transform_rule() {
        let a = [tri([2.0, 4.0, 0.0], [12.0, 4.0, 0.0], [2.0, 9.0, 6.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: None }];
        let p = VoxlPlacement { mesh: 0, rotation_z: 0.7, translation: [30.0, -15.0, 0.0] };
        let (_, chunks) = read_voxl(&write(&meshes, &[p], None));
        let model = &json_chunk(&chunks, b"MODL")[0];
        let (_, _, stl) = chunks.iter().find(|(t, _, _)| t == b"MESH").unwrap();

        // DragonFruit: centre the stored soup on its bbox, rotate about Z, translate by position.
        let f = |at: usize| f64::from(f32::from_le_bytes(stl[at..at + 4].try_into().unwrap()));
        let stored: Vec<[f64; 3]> =
            (0..3).map(|v| [f(84 + 12 + v * 12), f(88 + 12 + v * 12), f(92 + 12 + v * 12)]).collect();
        let axis = |k: usize| {
            let (lo, hi) = stored.iter().fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v[k]), hi.max(v[k])));
            (lo + hi) / 2.0
        };
        let centre = [axis(0), axis(1), axis(2)];
        let t = &model["transform"];
        let rz = t["rotation"]["z"].as_f64().unwrap();
        let pos = ["x", "y", "z"].map(|k| t["position"][k].as_f64().unwrap());

        let (s, c) = 0.7f64.sin_cos();
        for (stored, original) in stored.iter().zip([a[0].v0, a[0].v1, a[0].v2]) {
            let l = [stored[0] - centre[0], stored[1] - centre[1], stored[2] - centre[2]];
            let (rs, rc) = rz.sin_cos();
            let placed = [l[0] * rc - l[1] * rs + pos[0], l[0] * rs + l[1] * rc + pos[1], l[2] + pos[2]];
            let (ox, oy, oz) = (f64::from(original.x), f64::from(original.y), f64::from(original.z));
            let expected = [ox * c - oy * s + 30.0, ox * s + oy * c - 15.0, oz];
            for k in 0..3 {
                assert!((placed[k] - expected[k]).abs() < 1e-4, "{placed:?} != {expected:?}");
            }
        }
    }

    #[test]
    fn plate_name_is_written_to_the_scene() {
        let a = [tri([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: None }];
        let (_, chunks) = read_voxl(&write(&meshes, &[placement(0, 0.0)], Some(" Dragons ")));
        let scene = json_chunk(&chunks, b"SCNE");
        assert_eq!(scene["plates"][0]["name"], "Dragons");
        assert_eq!(scene["plateName"], "Dragons");
        assert_eq!(scene["activePlateId"], scene["plates"][0]["id"]);

        let (_, chunks) = read_voxl(&write(&meshes, &[placement(0, 0.0)], Some("  ")));
        let scene = json_chunk(&chunks, b"SCNE");
        assert!(scene["plates"][0].get("name").is_none() && scene.get("plateName").is_none());
    }

    fn xyz(v: &Value) -> [f64; 3] {
        ["x", "y", "z"].map(|k| v[k].as_f64().unwrap())
    }

    fn near(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3)
    }

    #[test]
    fn supports_are_written_as_dragonfruit_entities_in_plate_coordinates() {
        use crate::geometry::support_layout::{ContactTip, ROOT_TOP_MM, SupportBase, SupportPath};

        let tip = |contact: Vec3, socket: Vec3| ContactTip {
            contact,
            surface_normal: Vec3::new(0.0, 0.0, -1.0),
            socket,
            contact_diameter_mm: 0.3,
            body_diameter_mm: 0.8,
        };
        let layout = SupportLayout {
            braces: vec![],
            kickstands: vec![],
            supports: vec![
                SupportPath {
                    tip: tip(Vec3::new(1.0, 2.0, 10.0), Vec3::new(1.0, 2.0, 8.0)),
                    base: SupportBase::Plate { x: 1.0, y: 2.0, diameter_mm: 3.0 },
                    joints: vec![Vec3::new(1.0, 2.0, 5.0)],
                    shaft_diameter_mm: 1.0,
                },
                SupportPath {
                    tip: tip(Vec3::new(3.0, 2.0, 9.0), Vec3::new(3.0, 2.0, 7.0)),
                    base: SupportBase::Shaft { parent: 0, point: Vec3::new(1.0, 2.0, 4.0) },
                    joints: vec![],
                    shaft_diameter_mm: 0.6,
                },
            ],
        };
        let a = [tri([0.0, 0.0, 10.0], [4.0, 0.0, 10.0], [0.0, 4.0, 12.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: Some(&layout) }];
        let p = VoxlPlacement { mesh: 0, rotation_z: std::f64::consts::FRAC_PI_2, translation: [10.0, 0.0, 0.0] };
        let (_, chunks) = read_voxl(&write(&meshes, &[p], None));
        let supp = json_chunk(&chunks, b"SUPP");
        let model_id = json_chunk(&chunks, b"MODL")[0]["id"].clone();
        // Rotated a quarter turn about Z, then moved 10 mm along X.
        let place = |x: f64, y: f64, z: f64| [10.0 - y, x, z];

        let root = &supp["roots"][0];
        assert_eq!(root["modelId"], model_id);
        assert!(near(xyz(&root["transform"]["pos"]), place(1.0, 2.0, 0.0)));
        let root_top = root["diskHeight"].as_f64().unwrap() + root["coneHeight"].as_f64().unwrap();
        assert!((root_top - f64::from(ROOT_TOP_MM)).abs() < 1e-6);

        let trunk = &supp["trunks"][0];
        assert_eq!(
            (trunk["typeId"].as_str(), &trunk["rootId"], &trunk["modelId"]),
            (Some("trunk"), &root["id"], &model_id)
        );
        let segments = trunk["segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2);
        assert!(segments[0].get("bottomJoint").is_none());
        assert!(near(xyz(&segments[0]["topJoint"]["pos"]), place(1.0, 2.0, 5.0)));
        assert_eq!(segments[1]["bottomJoint"]["id"], segments[0]["topJoint"]["id"]);
        assert_eq!(segments[1]["topJoint"]["id"], trunk["contactCone"]["socketJointId"]);

        // DragonFruit's socket: contact, out along the surface normal by the disk, then along
        // the cone axis by the cone length.
        let cone = &trunk["contactCone"];
        let (pos, normal, surface) = (xyz(&cone["pos"]), xyz(&cone["normal"]), xyz(&cone["surfaceNormal"]));
        let (disk, length) =
            (cone["diskLengthOverride"].as_f64().unwrap(), cone["profile"]["lengthMm"].as_f64().unwrap());
        let socket = [0, 1, 2].map(|k| pos[k] + surface[k] * disk + normal[k] * length);
        assert!(near(socket, place(1.0, 2.0, 8.0)), "{socket:?}");
        assert!(near(xyz(&segments[1]["topJoint"]["pos"]), place(1.0, 2.0, 8.0)));
        assert_eq!(cone["profile"]["contactDiameterMm"], 0.3);

        // The branch hangs from a knot on the trunk segment from the root top to z = 5.
        let knot = &supp["knots"][0];
        assert_eq!(knot["parentShaftId"], segments[0]["id"]);
        assert!((knot["t"].as_f64().unwrap() - 2.0 / 3.0).abs() < 1e-3);
        assert!(near(xyz(&knot["pos"]), place(1.0, 2.0, 4.0)));
        let branch = &supp["branches"][0];
        assert_eq!((branch["typeId"].as_str(), &branch["parentKnotId"]), (Some("branch"), &knot["id"]));
        assert_eq!(branch["segments"].as_array().unwrap().len(), 1);
        assert_eq!(supp["roots"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn struts_are_twigs_when_short_and_sticks_when_long() {
        use crate::geometry::support_layout::{ContactTip, SupportBase, SupportPath};

        let end = |contact: Vec3, socket: Vec3, normal: Vec3| ContactTip {
            contact,
            surface_normal: normal,
            socket,
            contact_diameter_mm: 0.4,
            body_diameter_mm: 0.5,
        };
        let (down, up) = (Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.0, 1.0));
        let strut = |top: f32| SupportPath {
            tip: end(Vec3::new(0.0, 0.0, top), Vec3::new(0.0, 0.0, top - 0.8), down),
            base: SupportBase::Model(end(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 10.8), up)),
            joints: vec![],
            shaft_diameter_mm: 0.5,
        };
        let layout = SupportLayout { supports: vec![strut(12.0), strut(18.0)], ..SupportLayout::default() };
        let a = [tri([0.0, 0.0, 10.0], [4.0, 0.0, 10.0], [0.0, 4.0, 12.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: Some(&layout) }];
        let (_, chunks) = read_voxl(&write(&meshes, &[placement(0, 0.0)], None));
        let supp = json_chunk(&chunks, b"SUPP");
        assert!(supp["trunks"].as_array().unwrap().is_empty() && supp["roots"].as_array().unwrap().is_empty());

        // 2 mm apart: a twig, its joints standing off each disk along the surface normal.
        let twig = &supp["twigs"][0];
        assert_eq!(twig["typeId"], "twig");
        let segment = &twig["segments"][0];
        let standoff = twig["contactDiskA"]["diskLengthOverride"].as_f64().unwrap();
        assert!(near(xyz(&twig["contactDiskA"]["pos"]), [0.0, 0.0, 10.0]));
        assert!(near(xyz(&twig["contactDiskB"]["pos"]), [0.0, 0.0, 12.0]));
        assert!(near(xyz(&segment["bottomJoint"]["pos"]), [0.0, 0.0, 10.0 + standoff]));
        assert!(near(xyz(&segment["topJoint"]["pos"]), [0.0, 0.0, 12.0 - standoff]));
        assert!(standoff * 2.0 > segment["bottomJoint"]["diameter"].as_f64().unwrap());
        assert_eq!(twig["contactDiskA"]["contactDiameterMm"], 0.5);

        // 8 mm apart: a stick, a contact cone at each end meeting the shaft at its sockets.
        let stick = &supp["sticks"][0];
        assert_eq!(stick["typeId"], "stick");
        let segment = &stick["segments"][0];
        assert_eq!(stick["contactConeA"]["socketJointId"], segment["bottomJoint"]["id"]);
        assert_eq!(stick["contactConeB"]["socketJointId"], segment["topJoint"]["id"]);
        assert!(near(xyz(&segment["bottomJoint"]["pos"]), [0.0, 0.0, 10.8]));
        assert!(near(xyz(&segment["topJoint"]["pos"]), [0.0, 0.0, 17.2]));
        let cone = &stick["contactConeA"];
        let (pos, normal, surface) = (xyz(&cone["pos"]), xyz(&cone["normal"]), xyz(&cone["surfaceNormal"]));
        let (disk, length) =
            (cone["diskLengthOverride"].as_f64().unwrap(), cone["profile"]["lengthMm"].as_f64().unwrap());
        assert!(near([0, 1, 2].map(|k| pos[k] + surface[k] * disk + normal[k] * length), [0.0, 0.0, 10.8]));
    }

    #[test]
    fn braces_hang_from_knots_on_the_supports_they_join() {
        use crate::geometry::support_layout::{BraceEnd, BraceLayout, ContactTip, SupportBase, SupportPath};

        let trunk = |x: f32| SupportPath {
            tip: ContactTip {
                contact: Vec3::new(x, 0.0, 10.0),
                surface_normal: Vec3::new(0.0, 0.0, -1.0),
                socket: Vec3::new(x, 0.0, 8.0),
                contact_diameter_mm: 0.3,
                body_diameter_mm: 0.8,
            },
            base: SupportBase::Plate { x, y: 0.0, diameter_mm: 3.0 },
            joints: vec![],
            shaft_diameter_mm: 0.8,
        };
        let layout = SupportLayout {
            supports: vec![trunk(0.0), trunk(3.0)],
            braces: vec![BraceLayout {
                lower: BraceEnd { support: 1, point: Vec3::new(3.0, 0.0, 4.0) },
                upper: BraceEnd { support: 0, point: Vec3::new(0.0, 0.0, 6.0) },
                diameter_mm: 0.6,
            }],
            kickstands: vec![],
        };
        let a = [tri([0.0, 0.0, 10.0], [4.0, 0.0, 10.0], [0.0, 4.0, 12.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: Some(&layout) }];
        let (_, chunks) = read_voxl(&write(&meshes, &[placement(0, 0.0)], None));
        let supp = json_chunk(&chunks, b"SUPP");

        let brace = &supp["braces"][0];
        assert_eq!(brace["typeId"], "brace");
        assert_eq!(brace["modelId"], json_chunk(&chunks, b"MODL")[0]["id"]);
        assert_eq!(brace["profile"]["diameter"], 0.6);
        let knot = |id: &Value| supp["knots"].as_array().unwrap().iter().find(|k| k["id"] == *id).unwrap().clone();
        let segment_ids = |t: usize| {
            supp["trunks"][t]["segments"].as_array().unwrap().iter().map(|s| s["id"].clone()).collect::<Vec<_>>()
        };
        let (start, end) = (knot(&brace["startKnotId"]), knot(&brace["endKnotId"]));
        assert!(segment_ids(1).contains(&start["parentShaftId"]));
        assert!(segment_ids(0).contains(&end["parentShaftId"]));
        assert!(near(xyz(&start["pos"]), [3.0, 0.0, 4.0]));
        assert!(near(xyz(&end["pos"]), [0.0, 0.0, 6.0]));
        // DragonFruit prints a brace as wide as its knots less the joint offset, and keeps an
        // imported brace knot's diameter when it normalises the knots on opening the scene.
        for knot in [&start, &end] {
            assert_eq!(knot["diameter"], 0.7);
            assert_eq!(knot["normalizationHint"], "braceImported");
        }
    }

    #[test]
    fn brace_knots_stay_clear_of_their_segment_ends() {
        use crate::geometry::support_layout::{BraceEnd, BraceLayout, ContactTip, SupportBase, SupportPath};

        // Trunks bending at z = 5; the brace's lower end is at that joint, its upper end below
        // the root top.
        let trunk = |x: f32| SupportPath {
            tip: ContactTip {
                contact: Vec3::new(x, 0.0, 10.0),
                surface_normal: Vec3::new(0.0, 0.0, -1.0),
                socket: Vec3::new(x, 0.0, 8.0),
                contact_diameter_mm: 0.3,
                body_diameter_mm: 0.8,
            },
            base: SupportBase::Plate { x, y: 0.0, diameter_mm: 3.0 },
            joints: vec![Vec3::new(x, 0.0, 5.0)],
            shaft_diameter_mm: 0.8,
        };
        let layout = SupportLayout {
            supports: vec![trunk(0.0), trunk(3.0)],
            braces: vec![BraceLayout {
                lower: BraceEnd { support: 0, point: Vec3::new(0.0, 0.0, 5.0) },
                upper: BraceEnd { support: 1, point: Vec3::new(3.0, 0.0, 1.0) },
                diameter_mm: 0.6,
            }],
            kickstands: vec![],
        };
        let a = [tri([0.0, 0.0, 10.0], [4.0, 0.0, 10.0], [0.0, 4.0, 12.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: Some(&layout) }];
        let (_, chunks) = read_voxl(&write(&meshes, &[placement(0, 0.0)], None));
        let supp = json_chunk(&chunks, b"SUPP");

        // DragonFruit moves a brace knot at either end of its segment to another segment, so
        // each is kept just inside its segment, as close to the brace's end as that allows.
        let knots = supp["knots"].as_array().unwrap();
        for knot in knots {
            let t = knot["t"].as_f64().unwrap();
            assert!((BRACE_KNOT_END_SHARE..=1.0 - BRACE_KNOT_END_SHARE).contains(&t), "{t}");
        }
        let (lower, upper) = (xyz(&knots[0]["pos"]), xyz(&knots[1]["pos"]));
        assert!((lower[2] - 5.0).abs() <= 3.0 * BRACE_KNOT_END_SHARE + 1e-6, "{lower:?}");
        assert!((upper[2] - 2.0 - 3.0 * BRACE_KNOT_END_SHARE).abs() < 1e-3, "{upper:?}");
    }

    #[test]
    fn a_branch_starts_at_its_knot_so_knots_on_it_follow_the_shaft_dragonfruit_builds() {
        use crate::geometry::support_layout::{BraceEnd, BraceLayout, ContactTip, SupportBase, SupportPath};

        let tip = |x: f32, z: f32| ContactTip {
            contact: Vec3::new(x, 0.0, z),
            surface_normal: Vec3::new(0.0, 0.0, -1.0),
            socket: Vec3::new(x, 0.0, z - 2.0),
            contact_diameter_mm: 0.3,
            body_diameter_mm: 0.8,
        };
        // A trunk at x = 0, and a branch attached a millimetre off its side: DragonFruit starts
        // the branch at its knot on the trunk, so a brace on the branch hangs from that line.
        let layout = SupportLayout {
            supports: vec![
                SupportPath {
                    tip: tip(0.0, 12.0),
                    base: SupportBase::Plate { x: 0.0, y: 0.0, diameter_mm: 3.0 },
                    joints: vec![],
                    shaft_diameter_mm: 1.0,
                },
                SupportPath {
                    tip: tip(4.0, 12.0),
                    base: SupportBase::Shaft { parent: 0, point: Vec3::new(1.0, 0.0, 4.0) },
                    joints: vec![],
                    shaft_diameter_mm: 0.8,
                },
            ],
            braces: vec![BraceLayout {
                lower: BraceEnd { support: 0, point: Vec3::new(0.0, 0.0, 7.0) },
                upper: BraceEnd { support: 1, point: Vec3::new(2.0, 0.0, 6.0) },
                diameter_mm: 0.5,
            }],
            kickstands: vec![],
        };
        let a = [tri([0.0, 0.0, 12.0], [4.0, 0.0, 12.0], [0.0, 4.0, 14.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: Some(&layout) }];
        let (_, chunks) = read_voxl(&write(&meshes, &[placement(0, 0.0)], None));
        let supp = json_chunk(&chunks, b"SUPP");

        let knots = supp["knots"].as_array().unwrap();
        let branch_knot = xyz(&knots[0]["pos"]);
        assert!(near(branch_knot, [0.0, 0.0, 4.0]), "{branch_knot:?}");
        assert!(near(xyz(&supp["branches"][0]["segments"][0]["topJoint"]["pos"]), [4.0, 0.0, 10.0]));
        // The brace's knot on the branch lies on the line from the branch's knot to its socket.
        let on_branch = xyz(&knots[2]["pos"]);
        let (along, off) = ([4.0, 0.0, 6.0], [on_branch[0], on_branch[1], on_branch[2] - 4.0]);
        let cross = [
            along[1] * off[2] - along[2] * off[1],
            along[2] * off[0] - along[0] * off[2],
            along[0] * off[1] - along[1] * off[0],
        ];
        assert!(cross.iter().map(|c| c * c).sum::<f64>().sqrt() < 1e-3, "{on_branch:?}");
    }

    #[test]
    fn kickstands_stand_on_their_own_root_and_hang_from_a_knot_on_their_support() {
        use crate::geometry::support_layout::{ContactTip, KickstandLayout, ROOT_TOP_MM, SupportBase, SupportPath};

        let layout = SupportLayout {
            supports: vec![SupportPath {
                tip: ContactTip {
                    contact: Vec3::new(0.0, 0.0, 12.0),
                    surface_normal: Vec3::new(0.0, 0.0, -1.0),
                    socket: Vec3::new(0.0, 0.0, 10.0),
                    contact_diameter_mm: 0.3,
                    body_diameter_mm: 0.8,
                },
                base: SupportBase::Plate { x: 0.0, y: 0.0, diameter_mm: 3.0 },
                joints: vec![],
                shaft_diameter_mm: 0.8,
            }],
            kickstands: vec![
                // A leg leaning straight from its root, and one leaning off the top of a column.
                KickstandLayout {
                    support: 0,
                    knot: Vec3::new(0.0, 0.0, 6.0),
                    joints: vec![],
                    foot: Vec3::new(3.0, 0.0, ROOT_TOP_MM),
                    root_diameter_mm: 2.0,
                    diameter_mm: 0.6,
                },
                KickstandLayout {
                    support: 0,
                    knot: Vec3::new(0.0, 0.0, 8.0),
                    joints: vec![Vec3::new(0.0, 4.0, 5.0)],
                    foot: Vec3::new(0.0, 4.0, ROOT_TOP_MM),
                    root_diameter_mm: 2.0,
                    diameter_mm: 0.6,
                },
            ],
            ..SupportLayout::default()
        };
        let a = [tri([0.0, 0.0, 12.0], [4.0, 0.0, 12.0], [0.0, 4.0, 14.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: Some(&layout) }];
        let p = VoxlPlacement { mesh: 0, rotation_z: std::f64::consts::FRAC_PI_2, translation: [10.0, 0.0, 0.0] };
        let (_, chunks) = read_voxl(&write(&meshes, &[p], None));
        let supp = json_chunk(&chunks, b"SUPP");
        let model_id = json_chunk(&chunks, b"MODL")[0]["id"].clone();
        let place = |x: f64, y: f64, z: f64| [10.0 - y, x, z];

        // The bundles carry their own roots and knots; only the trunk's are in the flat lists.
        assert_eq!(supp["roots"].as_array().unwrap().len(), 1);
        assert!(supp["knots"].as_array().unwrap().is_empty());
        let bundles = supp["kickstands"].as_array().unwrap();
        assert_eq!(bundles.len(), 2);
        let trunk_segment = supp["trunks"][0]["segments"][0]["id"].clone();

        for (bundle, knot_at, root_at) in [
            (&bundles[0], place(0.0, 0.0, 6.0), place(3.0, 0.0, 0.0)),
            (&bundles[1], place(0.0, 0.0, 8.0), place(0.0, 4.0, 0.0)),
        ] {
            let (kickstand, root, knot) = (&bundle["kickstand"], &bundle["root"], &bundle["hostKnot"]);
            assert_eq!(kickstand["typeId"], "kickstand");
            assert_eq!(kickstand["modelId"], model_id);
            assert_eq!(root["modelId"], model_id);
            assert_eq!(kickstand["rootId"], root["id"]);
            assert_eq!(kickstand["hostKnotId"], knot["id"]);
            assert_eq!((&kickstand["hostSegmentId"], &knot["parentShaftId"]), (&trunk_segment, &trunk_segment));
            assert_eq!(kickstand["hostMinT"], 0);
            assert!(near(xyz(&knot["pos"]), knot_at), "{}", knot["pos"]);
            assert!(near(xyz(&root["transform"]["pos"]), root_at), "{}", root["transform"]["pos"]);
            let root_top = root["diskHeight"].as_f64().unwrap() + root["coneHeight"].as_f64().unwrap();
            assert!((root_top - f64::from(ROOT_TOP_MM)).abs() < 1e-6);
            assert_eq!(knot["diameter"], 0.9);
            for field in ["bodyDiameterMm", "terminalStartDiameterMm", "terminalEndDiameterMm"] {
                assert_eq!(kickstand["profile"][field], 0.6);
            }
            // DragonFruit starts the first segment at the root top and ends the last at the knot,
            // so neither carries a joint there.
            let segments = kickstand["segments"].as_array().unwrap();
            assert!(segments[0].get("bottomJoint").is_none());
            assert!(segments.last().unwrap().get("topJoint").is_none());
        }

        let straight = bundles[0]["kickstand"]["segments"].as_array().unwrap();
        assert_eq!(straight.len(), 1);
        let bent = bundles[1]["kickstand"]["segments"].as_array().unwrap();
        assert_eq!(bent.len(), 2);
        assert_eq!(bent[0]["topJoint"]["id"], bent[1]["bottomJoint"]["id"]);
        assert!(near(xyz(&bent[0]["topJoint"]["pos"]), place(0.0, 4.0, 5.0)));
        assert_eq!(bent[0]["topJoint"]["diameter"], 0.7);
    }

    #[test]
    fn unknown_mesh_index_is_rejected() {
        let result = write_voxl(std::io::Cursor::new(Vec::new()), &[], &[placement(0, 0.0)], None);
        assert!(result.is_err());
    }
}
