//! Writes DragonFruit's VOXL scene container (Open-Resin-Alliance/DragonFruit,
//! `docs/dev/voxl-format-spec.md`): a 16-byte header, a chunk directory, then zlib-compressed
//! chunks. Scene state is JSON (`META`, `SCNE`, `MODL`, `SUPP`) and each distinct mesh is one
//! binary STL `MESH` chunk. Placements that share a mesh point at the first placement's chunk,
//! which raises the container version from 2 to 3. A mesh's support layout is written to
//! `SUPP` as DragonFruit roots, trunks, branches, twigs, sticks, braces, kickstands and knots
//! for every placement of it; posts with no contact of their own are trunks without a contact
//! cone.
//!
//! Model VOXL downloads are cached by `support_reader::CURRENT_SUPPORT_READER_VERSION`
//! (`services::voxl_cache`): bump it when what is written for a support layout changes.

use std::io::{Seek, SeekFrom, Write};

use anyhow::{Result, anyhow, bail};
use flate2::{Compress, Compression, FlushCompress, Status};
use rayon::prelude::*;
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
/// The tip offset the slicer applies by default: one layer of 0.05 mm with no 3D anti-aliasing
/// (`resolveEffectiveAaSettings`). The value stored as `penetrationMm` plays no part in slicing.
pub const NOMINAL_TIP_OFFSET_MM: f32 = 0.05;
/// Narrowest contact DragonFruit accepts (`contactDiameterMm`).
const MIN_WRITTEN_CONTACT_MM: f32 = 0.05;
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
/// A micro support's twig disks stand off by this share of the shaft width: the radius of the ball
/// DragonFruit caps each disk with, so the ball stays out of the model. The longer disks of other
/// twigs (the joint's radius plus clearance, at least the disk thickness) bend a micro support of a
/// few tenths of a millimetre into a kink (owner, 2026-11-11). Other twigs keep their disks: this
/// short stub sank their wider end joints into the model (5.9 mm2 of clip on 8e6618e3).
const MICRO_TWIG_STANDOFF_SHARE: f32 = 0.5;
/// DragonFruit marks down a brace knot within this share of its segment's length of either end
/// (`t <= 0.02` or `t >= 0.98`), so brace knots are kept just inside it.
const BRACE_KNOT_END_SHARE: f64 = 0.025;
/// Support coordinates are rounded to this many decimal places.
const COORDINATE_DECIMALS: i32 = 4;
/// Chunks are deflated in pieces of this size, in parallel. Each piece but the last ends in a
/// sync flush, so the pieces join into one ordinary zlib stream; starting each piece without
/// the previous one's history costs well under 1% in size.
const COMPRESSION_PIECE_BYTES: usize = 256 * 1024;
/// zlib stream header: deflate with a 32 KiB window at the default level.
const ZLIB_HEADER: [u8; 2] = [0x78, 0x9c];

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
    out: W,
    meshes: &[VoxlMesh<'_>],
    placements: &[VoxlPlacement],
    plate_name: Option<&str>,
) -> Result<W> {
    write_voxl_with(out, meshes, placements, plate_name, SPHERE_TIPS_ENABLED)
}

/// [`write_voxl`], with ball tips written as sphere tips or not (whatever
/// [`SPHERE_TIPS_ENABLED`] says), for the tests and the DragonFruit round-trip tool.
#[doc(hidden)]
pub fn write_voxl_with<W: Write + Seek>(
    mut out: W,
    meshes: &[VoxlMesh<'_>],
    placements: &[VoxlPlacement],
    plate_name: Option<&str>,
    sphere_tips: bool,
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
        let (sha256, compressed) = rayon::join(|| hex::encode(Sha256::digest(&stl)), || zlib_compress(&stl));
        entries.push(write_compressed_chunk(&mut out, *b"MESH", owner as u16, &compressed?, stl.len())?);
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

    let mut supports = SupportDocument { sphere_tips, ..SupportDocument::default() };
    for (placement, model_id) in placements.iter().zip(&model_ids) {
        if let Some(layout) = meshes[placement.mesh].supports {
            supports.add(layout, model_id, placement);
        }
    }
    let supports = supports.into_json();

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
    /// Ball tips are written as sphere tips ([`SPHERE_TIPS_ENABLED`]).
    sphere_tips: bool,
}

/// The `SUPP` chunk a VOXL holds for one model with `layout`'s supports, placed where it stands
/// (no rotation or translation), with ball tips written as sphere tips when `sphere_tips` (the
/// writer uses [`SPHERE_TIPS_ENABLED`]): the supports as DragonFruit reads them from the model's
/// VOXL, for [`super::support_mesh`] to build.
pub fn supports_document(layout: &SupportLayout, sphere_tips: bool) -> Value {
    let mut supports = SupportDocument { sphere_tips, ..SupportDocument::default() };
    let placement = VoxlPlacement { mesh: 0, rotation_z: 0.0, translation: [0.0; 3] };
    supports.add(layout, &Uuid::new_v4().to_string(), &placement);
    supports.into_json()
}

/// A shaft segment's id and its plate-coordinate ends.
struct Span {
    id: String,
    start: [f64; 3],
    end: [f64; 3],
    /// The segment's own diameter (a stepped shaft's segments differ).
    diameter: f32,
}

impl SupportDocument {
    /// The document as the `SUPP` chunk's JSON.
    fn into_json(self) -> Value {
        json!({
            "version": 1,
            "meta": { "source": "findamodel", "objectCenter": { "x": 0, "y": 0, "z": 0 } },
            "roots": self.roots,
            "trunks": self.trunks,
            "branches": self.branches,
            "leaves": [],
            "twigs": self.twigs,
            "sticks": self.sticks,
            "braces": self.braces,
            "knots": self.knots,
            "kickstands": self.kickstands,
        })
    }

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

        // Each post is a trunk with no contact cone: its last segment ends at its top joint. Branches
        // can hang from a post, so posts come first.
        let mut post_spans: Vec<Vec<Span>> = Vec::with_capacity(layout.posts.len());
        let (mut post_trunks, mut post_roots) = (Vec::new(), Vec::new());
        for (pi, post) in layout.posts.iter().enumerate() {
            let index = layout.supports.len() + pi;
            let line = post.line();
            let ends: Vec<[f64; 3]> = line.iter().map(|&p| place(p)).collect();
            // A joint is as wide as the wider segment it joins (plus the offset).
            let seg = |i: usize| layout.segment_diameter_mm(index, i);
            let joints: Vec<Value> = ends[1..]
                .iter()
                .enumerate()
                .map(|(i, &pos)| {
                    let d = if i + 2 < ends.len() { seg(i).max(seg(i + 1)) } else { seg(i) };
                    json!({ "id": Uuid::new_v4().to_string(), "pos": point(pos), "diameter": round(d + JOINT_DIAMETER_OFFSET_MM) })
                })
                .collect();
            let mut spans = Vec::with_capacity(joints.len());
            let segments: Vec<Value> = joints
                .iter()
                .enumerate()
                .map(|(i, joint)| {
                    let id = Uuid::new_v4().to_string();
                    let mut segment = json!({ "id": id, "diameter": round(seg(i)), "topJoint": joint });
                    if i > 0 {
                        segment["bottomJoint"] = joints[i - 1].clone();
                    }
                    spans.push(Span { id, start: ends[i], end: ends[i + 1], diameter: seg(i) });
                    segment
                })
                .collect();
            let root = root(model_id, place(Vec3::new(post.foot.x, post.foot.y, 0.0)), post.root_diameter_mm);
            post_trunks.push(json!({
                "id": Uuid::new_v4().to_string(),
                "modelId": model_id,
                "typeId": "trunk",
                "rootId": root["id"],
                "baseDiameterMm": round(seg(0)),
                "segments": segments,
            }));
            post_roots.push(root);
            post_spans.push(spans);
        }

        // Each support's segments, so later supports can hang knots on them.
        let mut shafts: Vec<Vec<Span>> = Vec::with_capacity(layout.supports.len());
        for (si, support) in layout.supports.iter().enumerate() {
            if let (SupportBase::Model(lower), Some(ends)) = (&support.base, support_strut_ends(layout, si)) {
                shafts.push(self.add_strut(support, lower, ends, model_id, &place, &turn));
                continue;
            }
            let seg = |i: usize| layout.segment_diameter_mm(si, i);
            let socket_id = Uuid::new_v4().to_string();
            let mut ends: Vec<[f64; 3]> = support.joints.iter().map(|&p| place(p)).collect();
            ends.push(place(support.tip.socket));
            // A joint is as wide as the wider segment it joins (plus the offset).
            let joints: Vec<Value> = ends
                .iter()
                .enumerate()
                .map(|(i, &pos)| {
                    let id = if i + 1 == ends.len() { socket_id.clone() } else { Uuid::new_v4().to_string() };
                    let d = if i + 1 < ends.len() { seg(i).max(seg(i + 1)) } else { seg(i) };
                    json!({ "id": id, "pos": point(pos), "diameter": round(d + JOINT_DIAMETER_OFFSET_MM) })
                })
                .collect();

            // A branch hangs from a knot on its parent's shaft, which is where DragonFruit starts
            // its first segment (not the attach point itself, which can lie off that shaft).
            let knot = match &support.base {
                SupportBase::Shaft { parent, point: at } => {
                    let spans = match parent.checked_sub(layout.supports.len()) {
                        Some(post) => &post_spans[post],
                        None => &shafts[*parent],
                    };
                    let host = nearest_span(spans, place(*at)).diameter;
                    Some(knot_on(spans, place(*at), host + JOINT_DIAMETER_OFFSET_MM, KnotUse::Host))
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
                let mut segment = json!({ "id": id, "diameter": round(seg(i)), "topJoint": joint });
                if i > 0 {
                    segment["bottomJoint"] = joints[i - 1].clone();
                }
                segments.push(segment);
                spans.push(Span { id, start, end: ends[i], diameter: seg(i) });
                start = ends[i];
            }

            let contact_cone = contact_cone(&support.tip, self.sphere_tips, &socket_id, &place, &turn);
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
                        "baseDiameterMm": round(seg(0)),
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

        // Posts are numbered on from the supports.
        shafts.extend(post_spans);
        self.trunks.extend(post_trunks);
        self.roots.extend(post_roots);

        // Each kickstand stands on its own root and hangs from a knot on the support it holds
        // up: a column segment up to each joint, then the leaning segment up to the knot.
        let mut legs: Vec<Vec<Span>> = Vec::with_capacity(layout.kickstands.len());
        for (ki, kickstand) in layout.kickstands.iter().enumerate() {
            let host =
                nearest_span(&shafts[kickstand.support], place(kickstand.knot)).diameter + JOINT_DIAMETER_OFFSET_MM;
            let knot = knot_on(&shafts[kickstand.support], place(kickstand.knot), host, KnotUse::Host);
            let foot = kickstand.foot;
            let root = root(model_id, place(Vec3::new(foot.x, foot.y, 0.0)), kickstand.root_diameter_mm);
            // A stepped leg's segments, bottom to top; a joint as wide as the wider segment it joins.
            let steps = layout.kickstand_segment_diameters.get(&ki);
            let seg = |i: usize| steps.and_then(|d| d.get(i).copied()).unwrap_or(kickstand.diameter_mm);
            let joints: Vec<Value> = kickstand
                .joints
                .iter()
                .enumerate()
                .map(|(i, &p)| json!({ "id": Uuid::new_v4().to_string(), "pos": point(place(p)), "diameter": round(seg(i).max(seg(i + 1)) + JOINT_DIAMETER_OFFSET_MM) }))
                .collect();
            // Segment 0 starts at the root top; each later one at the joint below.
            let mut ends: Vec<[f64; 3]> = vec![place(foot)];
            ends.extend(kickstand.joints.iter().map(|&p| place(p)));
            ends.push(place(kickstand.knot));
            let mut spans = Vec::with_capacity(joints.len() + 1);
            let segments: Vec<Value> = (0..=joints.len())
                .map(|i| {
                    let id = Uuid::new_v4().to_string();
                    let mut segment = json!({ "id": id, "diameter": round(seg(i)) });
                    if i > 0 {
                        segment["bottomJoint"] = joints[i - 1].clone();
                    }
                    if let Some(joint) = joints.get(i) {
                        segment["topJoint"] = joint.clone();
                    }
                    spans.push(Span { id, start: ends[i], end: ends[i + 1], diameter: seg(i) });
                    segment
                })
                .collect();
            legs.push(spans);
            let diameter = round(kickstand.diameter_mm);
            let terminal = round(seg(kickstand.joints.len()));
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
                        "terminalStartDiameterMm": terminal,
                        "terminalEndDiameterMm": terminal,
                    },
                },
                "root": root,
                "hostKnot": knot,
            }));
        }

        // Each brace hangs from a knot on each of the two supports or kickstands it joins, lower
        // end first. DragonFruit prints a brace as wide as its knots (less the joint offset), so
        // they carry the brace's own diameter.
        for brace in &layout.braces {
            let mut knot_ids = Vec::with_capacity(2);
            for end in [&brace.lower, &brace.upper] {
                let spans = match end.kickstand {
                    Some(k) => &legs[k],
                    None => &shafts[end.support],
                };
                let diameter = brace.diameter_mm + JOINT_DIAMETER_OFFSET_MM;
                let knot = knot_on(spans, place(end.point), diameter, KnotUse::Brace);
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
    }

    /// A strut between two parts of the model, lower end first (A), as DragonFruit's own
    /// placement would make it: a twig when the contacts are close, otherwise a stick.
    fn add_strut(
        &mut self,
        support: &SupportPath,
        lower: &ContactTip,
        strut_ends: StrutEnds,
        model_id: &str,
        place: &dyn Fn(Vec3) -> [f64; 3],
        turn: &dyn Fn(Vec3) -> [f64; 3],
    ) -> Vec<Span> {
        let upper = &support.tip;
        let diameter = support.shaft_diameter_mm;
        let StrutEnds { twig, joint_diameter_mm: joint_diameter, standoff_mm: standoff } = strut_ends;
        let (a, b) = (strut_ends.end(lower), strut_ends.end(upper));

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
            spans.push(Span { id, start: ends[i - 1], end: ends[i], diameter });
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
            strut["contactConeA"] = contact_cone(lower, self.sphere_tips, &socket_id(&joints[0]), place, turn);
            strut["contactConeB"] =
                contact_cone(upper, self.sphere_tips, &socket_id(&joints[joints.len() - 1]), place, turn);
            self.sticks.push(strut);
        }
        spans
    }
}

/// How a strut (a support between two parts of the model) is written at its ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrutEnds {
    /// A twig (contact disks `diameter` wide at both contacts), else a stick (contact cones).
    pub twig: bool,
    /// The width of the joints at both ends of the shaft.
    pub joint_diameter_mm: f32,
    /// A twig's disk length: its shaft's end joints stand this far off the contacts along the
    /// surface normal (0 for a stick).
    pub standoff_mm: f32,
}

impl StrutEnds {
    /// Where the shaft ends at `tip`: a twig's joint off the disk, a stick's socket.
    pub fn end(&self, tip: &ContactTip) -> Vec3 {
        if self.twig { tip.contact + tip.surface_normal * self.standoff_mm } else { tip.socket }
    }
}

/// How a strut of shaft width `diameter` between `lower` and `upper` is written. A strut with cone
/// tips at its ends (`stick`) is a stick and one with flat ends (`flat`) a twig, whatever its span
/// (the span cutoff is only DragonFruit's default for placing new struts); otherwise the cutoff.
/// A twig's shaft runs between joints standing off its disks; a stick's between its sockets. A
/// `micro` support with cone ends that is shorter than its two contact cones as DragonFruit builds
/// them (each cone to the far side of its socket joint) is a twig all the same: as a stick its
/// cones and socket joints overlap and build about twice its width at the contacts (owner,
/// 2026-11-11). A micro support's twig disks are short ([`MICRO_TWIG_STANDOFF_SHARE`]).
pub fn strut_ends(
    lower: &ContactTip,
    upper: &ContactTip,
    diameter: f32,
    stick: bool,
    flat: bool,
    micro: bool,
) -> StrutEnds {
    let span = (upper.contact - lower.contact).length();
    // A micro support shorter than its two contact cones as DragonFruit builds them (each from the
    // contact to the far side of its socket joint, which is wider than the shaft): a twig.
    let built_cone =
        |tip: &ContactTip| (tip.socket - tip.contact).length() + (diameter + JOINT_DIAMETER_OFFSET_MM) / 2.0;
    let too_short = micro && span < built_cone(lower) + built_cone(upper);
    let twig = flat || (stick && too_short) || (!stick && span < STICK_MIN_SPAN_MM);
    if twig {
        let joint = diameter * TWIG_JOINT_RATIO;
        let standoff = if micro {
            diameter * MICRO_TWIG_STANDOFF_SHARE
        } else {
            (joint / 2.0 + TWIG_JOINT_CLEARANCE_MM).max(TIP_DISK_THICKNESS_MM)
        };
        StrutEnds { twig, joint_diameter_mm: joint, standoff_mm: standoff }
    } else {
        StrutEnds { twig, joint_diameter_mm: diameter + JOINT_DIAMETER_OFFSET_MM, standoff_mm: 0.0 }
    }
}

/// How support `i` of a layout is written at its ends, if it is a strut.
pub fn support_strut_ends(layout: &SupportLayout, i: usize) -> Option<StrutEnds> {
    let path = layout.supports.get(i)?;
    let SupportBase::Model(lower) = &path.base else { return None };
    Some(strut_ends(
        lower,
        &path.tip,
        path.shaft_diameter_mm,
        layout.sticks.contains(&i),
        layout.twigs.contains(&i),
        layout.micro.contains(&i),
    ))
}

/// Where DragonFruit's sliced contact cone meets the surface, in the surface plane through the
/// written `pos`. The slicer (`appendContactConePrimitive`) builds one frustum from
/// `pos - surface_normal * tip offset` to the socket, `radius` wide at that end, with a flat cap
/// square to the frustum's axis; it builds no disk. With `x` along the axis' lean in the plane, a
/// point is inside where its axial position `t = x sin + offset cos` is past the cap (`t >= 0`) and
/// its distance from the axis is within the frustum's radius at `t`: an ellipse, cut by the cap's
/// chord once the cap leans out of the surface (`radius * sin > offset`). Returns the area and the
/// area centroid's distance along the lean.
fn sliced_contact(radius: f64, body: f64, length: f64, lean: f64, offset: f64) -> (f64, f64) {
    let (s, c) = (lean.sin(), lean.cos());
    let k = (body - radius) / length.max(1e-6);
    let r0 = radius + k * offset * c;
    let a = s * s * (1.0 + k * k) - 1.0;
    if a > -1e-6 {
        return (std::f64::consts::PI * r0 * r0, 0.0);
    }
    let b = 2.0 * s * (r0 * k + offset * c);
    let cc = r0 * r0 - offset * offset * s * s;
    let xc = -b / (2.0 * a);
    let h = ((cc - b * b / (4.0 * a)) / -a).max(0.0).sqrt();
    if h <= 0.0 {
        return (0.0, 0.0);
    }
    let cut = if s > 1e-9 { -offset * c / s } else { f64::NEG_INFINITY };
    let u0 = ((cut - xc) / h).clamp(-1.0, 1.0);
    let rest = std::f64::consts::FRAC_PI_4 - (u0 * (1.0 - u0 * u0).sqrt() + u0.asin()) / 2.0;
    let area = 2.0 * (-a).sqrt() * h * h * rest;
    if area <= 0.0 {
        return (0.0, 0.0);
    }
    let centroid = xc + 2.0 * (-a).sqrt() * h.powi(3) * (1.0 - u0 * u0).powf(1.5) / 3.0 / area;
    (area, centroid)
}

/// The contact position and diameter to write so the cone DragonFruit slices meets the surface in
/// a loop of the tip's contact area (a circle `contact_diameter_mm` wide), centred on the tip's
/// contact. The slicer runs the cone's base on into the model by the tip offset
/// ([`NOMINAL_TIP_OFFSET_MM`]) along the surface normal and leaves its flat cap square to the
/// cone's own axis, so a leaning cone meets the surface in an ellipse whose centre moves along the
/// lean, and once the cap leans out of the surface the ellipse is cut short by the cap. The
/// diameter is solved for the area and the position moved back against the lean by the loop's
/// offset; a straight tip only gets the taper over the tip offset.
fn sliced_contact_placement(tip: &ContactTip) -> (Vec3, f32) {
    let n = tip.surface_normal.normalized();
    if tip.contact_diameter_mm > tip.body_diameter_mm * 1.001 {
        return sunk_blunt_placement(tip, n);
    }
    let offset = f64::from(NOMINAL_TIP_OFFSET_MM);
    let body = f64::from(tip.body_diameter_mm) / 2.0;
    let target = std::f64::consts::PI * (f64::from(tip.contact_diameter_mm) / 2.0).powi(2);
    let mut pos = tip.contact;
    let mut diameter = tip.contact_diameter_mm;
    for _ in 0..4 {
        let start = pos - n * NOMINAL_TIP_OFFSET_MM;
        let axis = tip.socket - start;
        let length = f64::from(axis.length()).max(1e-3);
        let dir = if axis.length() > 1e-4 { axis * (1.0 / axis.length()) } else { n };
        let cos = f64::from(dir.dot(n)).clamp(-1.0, 1.0);
        let lean = cos.acos();
        let along = dir - n * dir.dot(n);
        let (mut lo, mut hi) = (0.0025_f64, (4.0 * f64::from(tip.contact_diameter_mm)).max(body * 2.0));
        for _ in 0..48 {
            let mid = (lo + hi) / 2.0;
            if sliced_contact(mid, body, length, lean, offset).0 < target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let radius = (lo + hi) / 2.0;
        diameter = (radius * 2.0) as f32;
        let (_, centroid) = sliced_contact(radius, body, length, lean, offset);
        if along.length() < 1e-4 || !centroid.is_finite() {
            break;
        }
        // The loop's centre moves by at most the contact's own width (a lean near 90 degrees would move it on
        // without bound).
        let shift = (centroid as f32).clamp(-tip.contact_diameter_mm, tip.contact_diameter_mm);
        let next = tip.contact - along.normalized() * shift;
        let moved = (next - pos).length();
        pos = next;
        if moved < 1e-4 {
            break;
        }
    }
    // Past about 75 degrees the cone runs along the surface rather than into it and the loop is no longer a measure
    // of the contact: the straight tip's width is written there, as before.
    let lean =
        f64::from((tip.socket - (tip.contact - n * NOMINAL_TIP_OFFSET_MM)).normalized().dot(n)).clamp(-1.0, 1.0).acos();
    if !diameter.is_finite() || lean > 75f64.to_radians() {
        let length = (tip.socket - tip.contact).length().max(1e-3);
        let (surface, body) = (tip.contact_diameter_mm / 2.0, tip.body_diameter_mm / 2.0);
        let base = (surface * (length + NOMINAL_TIP_OFFSET_MM) - body * NOMINAL_TIP_OFFSET_MM) / length;
        return (
            tip.contact,
            (base * 2.0).clamp(MIN_WRITTEN_CONTACT_MM, tip.contact_diameter_mm.max(MIN_WRITTEN_CONTACT_MM)),
        );
    }
    (pos, diameter.clamp(MIN_WRITTEN_CONTACT_MM, 2.0 * tip.contact_diameter_mm.max(MIN_WRITTEN_CONTACT_MM)))
}

/// A blunt rod end whose loop is wider than the rod (contact wider than body: it enters at a slant). The
/// cone keeps the rod's width (a straight frustum, as the original rod) and the written position sinks below the
/// surface along its normal until the sliced loop has the contact's area: past the tip offset the cap leans out of
/// the surface, so a deeper start fills the ellipse in, up to the whole of it (the cap wholly under the surface, as
/// the original rod's end is). A loop wider than that whole ellipse keeps the deepest placement (the rod's own loop).
/// Returns the position to write and the rod's width; the loop centre is moved back against the lean as above.
fn sunk_blunt_placement(tip: &ContactTip, n: Vec3) -> (Vec3, f32) {
    let offset = f64::from(NOMINAL_TIP_OFFSET_MM);
    let radius = f64::from(tip.body_diameter_mm) / 2.0;
    let target = std::f64::consts::PI * (f64::from(tip.contact_diameter_mm) / 2.0).powi(2);
    // `pos` stays in the surface plane while solving; the sink is applied when written.
    let mut pos = tip.contact;
    let mut sink = 0.0_f64;
    for _ in 0..4 {
        let start = pos - n * (NOMINAL_TIP_OFFSET_MM + sink as f32);
        let axis = tip.socket - start;
        let length = f64::from(axis.length()).max(1e-3);
        let dir = if axis.length() > 1e-4 { axis * (1.0 / axis.length()) } else { n };
        let cos = f64::from(dir.dot(n)).clamp(-1.0, 1.0);
        let lean = cos.acos();
        // Past about 75 degrees the rod runs along the surface: no sinking, the rod's width at the contact.
        if lean > 75f64.to_radians() {
            return (tip.contact, tip.body_diameter_mm);
        }
        let along = dir - n * dir.dot(n);
        // Deep enough for the whole cap: the ellipse is complete and no deeper start widens it.
        let full = (radius * lean.sin() - offset).max(0.0) + 0.002;
        let (mut lo, mut hi) = (0.0_f64, full);
        if sliced_contact(radius, radius, length, lean, offset + full).0 < target {
            lo = full;
        } else {
            for _ in 0..40 {
                let mid = (lo + hi) / 2.0;
                if sliced_contact(radius, radius, length, lean, offset + mid).0 < target {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
        }
        sink = (lo + hi) / 2.0;
        let (_, centroid) = sliced_contact(radius, radius, length, lean, offset + sink);
        if along.length() < 1e-4 || !centroid.is_finite() {
            break;
        }
        let shift = (centroid as f32).clamp(-tip.contact_diameter_mm, tip.contact_diameter_mm);
        let next = tip.contact - along.normalized() * shift;
        let moved = (next - pos).length();
        pos = next;
        if moved < 1e-4 {
            break;
        }
    }
    (pos - n * sink as f32, tip.body_diameter_mm.max(MIN_WRITTEN_CONTACT_MM))
}

/// The tip as a DragonFruit contact cone: a disk standing off the surface along its normal,
/// then a cone from the disk to the socket. Written at the position and diameter that make the
/// sliced cone's loop on the surface the tip's own ([`sliced_contact_placement`]).
fn contact_cone(
    tip: &ContactTip,
    sphere_tips: bool,
    socket_id: &str,
    place: &dyn Fn(Vec3) -> [f64; 3],
    turn: &dyn Fn(Vec3) -> [f64; 3],
) -> Value {
    let (pos, diameter) = match written_tip(tip, sphere_tips) {
        WrittenTip::Sphere { contact_diameter_mm, radius_ratio, depth_mm } => {
            return sphere_cone(tip, (contact_diameter_mm, radius_ratio, depth_mm), socket_id, place, turn);
        }
        WrittenTip::Cone { pos, diameter_mm } => (pos, diameter_mm),
    };
    let disk = (tip.contact_diameter_mm / 2.0).max(TIP_DISK_THICKNESS_MM);
    let axis = tip.socket - (pos + tip.surface_normal * disk);
    let length = axis.length();
    let direction = if length > 1e-4 { axis * (1.0 / length) } else { tip.surface_normal };
    json!({
        "id": Uuid::new_v4().to_string(),
        "pos": point(place(pos)),
        "normal": point(turn(direction)),
        "surfaceNormal": point(turn(tip.surface_normal)),
        "diskLengthOverride": round(disk),
        "profile": {
            "type": "disk",
            "contactDiameterMm": round(diameter),
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

/// The contact a tip is written as: what DragonFruit gets to build the tip from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WrittenTip {
    /// A contact cone written at `pos` (in model coordinates, before placement) and `diameter_mm`
    /// wide there ([`sliced_contact_placement`]; a blunt end wider than its rod is sunk below the
    /// surface). DragonFruit slices it as one frustum from `pos - surface_normal * tip offset`,
    /// `diameter_mm` wide, to the socket at the body width.
    Cone { pos: Vec3, diameter_mm: f32 },
    /// A sphere tip at the tip's contact ([`SPHERE_TIPS_ENABLED`]): the cone's narrow end
    /// `contact_diameter_mm` wide at the ball's centre, the ball `radius_ratio` times that end's
    /// radius, its centre `depth_mm` below the surface before the tip offset, or DragonFruit's own
    /// seat (`sqrt(R^2 - r^2)` outside the surface, before the offset) when None.
    Sphere { contact_diameter_mm: f64, radius_ratio: f64, depth_mm: Option<f64> },
}

/// How the writer writes a tip's contact, `sphere_tips` saying whether ball tips are sphere tips
/// (the writer uses [`SPHERE_TIPS_ENABLED`]). The one place tip placement is decided, so that what
/// is written and what checks model from it (the synthetic suite's nominal build) agree.
pub fn written_tip(tip: &ContactTip, sphere_tips: bool) -> WrittenTip {
    if let Some((contact_diameter_mm, radius_ratio, depth_mm)) = sphere_tip(tip, sphere_tips) {
        return WrittenTip::Sphere { contact_diameter_mm, radius_ratio, depth_mm };
    }
    let (pos, diameter_mm) = sliced_contact_placement(tip);
    WrittenTip::Cone { pos, diameter_mm }
}

/// Whether tips whose original has a ball at the contact (a separate ball sunk at a cone's end, or
/// a rounded rod end) are written as DragonFruit sphere tips. Off: they are written as every other
/// tip (a cone sized from the contact loop, a blunt end at the rod's width). Needs DragonFruit's
/// sphere-tip support with `sphereDepthMm`, which is not released yet: released DragonFruit builds
/// sphere tips with undersized contacts.
pub const SPHERE_TIPS_ENABLED: bool = false;

/// DragonFruit's sphere tip for a tip whose original has a ball at the contact, when
/// `enabled`: the ball at the original's own depth (`sphereDepthMm`), the cone from the original
/// cone's end at the ball's centre. Where the reader found that a ball seated there would cross
/// the model a second time (no depth), DragonFruit's own seat instead (the centre
/// `sqrt(R^2 - r^2)` outside the surface, so it crosses at the contact width). Either way the
/// footprint at the nominal tip offset is the original loop (`tip.contact_diameter_mm`). Returns
/// (contact diameter, sphere radius ratio, depth of the centre below the surface before the offset).
fn sphere_tip(tip: &ContactTip, enabled: bool) -> Option<(f64, f64, Option<f64>)> {
    let ball = tip.ball.filter(|b| enabled && b.diameter_mm > 0.0)?;
    let p = f64::from(NOMINAL_TIP_OFFSET_MM);
    let f = f64::from(tip.contact_diameter_mm) / 2.0;
    let big = f64::from(ball.diameter_mm) / 2.0;
    let centred = || (2.0 * (f * f + p * p).sqrt(), 1.0, None);
    match ball.depth_mm {
        None => {
            // Ball centre at lift = sqrt(R^2 - r^2) - p off the surface; footprint^2 = R^2 - lift^2 = f^2.
            let s = p + (big * big - f * f).max(0.0).sqrt();
            if big <= f || s >= big {
                return Some(centred());
            }
            let r = (big * big - s * s).sqrt();
            Some((2.0 * r, big / r, None))
        }
        Some(seated) => {
            // The centre on the original's side of the surface, where the ball crosses it in the loop
            // once sunk by the offset: |d + p| = sqrt(R^2 - f^2), so d = +-sqrt(R^2 - f^2) - p.
            let seated = f64::from(seated);
            let big = big.max(f);
            let side = if seated >= 0.0 { 1.0 } else { -1.0 };
            let end = f64::from(ball.cone_end_mm);
            let end = if end > 0.0 && end <= 2.0 * big + 1e-6 { end } else { 2.0 * big * 0.75 };
            // A rounded rod end (cone end as wide as the ball: a rod) crosses the surface on its rod
            // once the dome's centre is inside, whatever its depth: there the original's depth is kept.
            let geo = (big * big - f * f).max(0.0).sqrt();
            let d = if end >= 2.0 * big * 0.99 && seated > geo { seated } else { side * geo };
            Some((end, 2.0 * big / end, Some(d - p)))
        }
    }
}

/// The sphere tip's contact cone: no nib, the cone from the ball's centre to the socket at pos + normal x length.
fn sphere_cone(
    tip: &ContactTip,
    (diameter, ratio, depth): (f64, f64, Option<f64>),
    socket_id: &str,
    place: &dyn Fn(Vec3) -> [f64; 3],
    turn: &dyn Fn(Vec3) -> [f64; 3],
) -> Value {
    let pos = tip.contact;
    let axis = tip.socket - pos;
    let length = axis.length();
    let direction = if length > 1e-4 { axis * (1.0 / length) } else { tip.surface_normal };
    let mut cone = json!({
        "id": Uuid::new_v4().to_string(),
        "pos": point(place(pos)),
        "normal": point(turn(direction)),
        "surfaceNormal": point(turn(tip.surface_normal)),
        "profile": {
            "type": "sphere",
            "contactDiameterMm": round(diameter as f32),
            "bodyDiameterMm": round(tip.body_diameter_mm.max(diameter as f32)),
            "lengthMm": round(length),
            "penetrationMm": round(TIP_PENETRATION_MM),
            "sphereRadiusRatio": (ratio * 10000.0).round() / 10000.0,
        },
        "socketJointId": socket_id,
    });
    if let Some(depth) = depth {
        cone["profile"]["sphereDepthMm"] = json!(round(depth as f32));
    }
    cone
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
/// The span of `spans` nearest `at` (as [`knot_on`] picks it).
fn nearest_span(spans: &[Span], at: [f64; 3]) -> &Span {
    let miss = |span: &Span| {
        let d: Vec<f64> = (0..3).map(|k| span.end[k] - span.start[k]).collect();
        let length_sq: f64 = d.iter().map(|v| v * v).sum();
        let t = if length_sq > 0.0 {
            ((0..3).map(|k| (at[k] - span.start[k]) * d[k]).sum::<f64>() / length_sq).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (0..3).map(|k| (span.start[k] + d[k] * t - at[k]).powi(2)).sum::<f64>()
    };
    spans.iter().min_by(|a, b| miss(a).total_cmp(&miss(b))).expect("a support has at least one segment")
}

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
    write_compressed_chunk(out, tag, index, &zlib_compress(data)?, data.len())
}

/// Writes a chunk already compressed by [`zlib_compress`] from `uncompressed_size` bytes.
fn write_compressed_chunk<W: Write + Seek>(
    out: &mut W,
    tag: [u8; 4],
    index: u16,
    compressed: &[u8],
    uncompressed_size: usize,
) -> Result<ChunkEntry> {
    let offset = out.stream_position()?;
    out.write_all(compressed)?;
    Ok(ChunkEntry {
        tag,
        index,
        offset,
        compressed_size: compressed.len() as u64,
        uncompressed_size: uncompressed_size as u64,
    })
}

/// Compresses `data` into one zlib stream, deflating [`COMPRESSION_PIECE_BYTES`] pieces of it
/// in parallel (as pigz does).
fn zlib_compress(data: &[u8]) -> Result<Vec<u8>> {
    let pieces: Vec<&[u8]> = if data.is_empty() { vec![data] } else { data.chunks(COMPRESSION_PIECE_BYTES).collect() };
    let last = pieces.len() - 1;
    let (deflated, checksum) = rayon::join(
        || pieces.par_iter().enumerate().map(|(i, piece)| deflate_piece(piece, i == last)).collect::<Result<Vec<_>>>(),
        || {
            let mut adler = adler2::Adler32::new();
            adler.write_slice(data);
            adler.checksum()
        },
    );
    let deflated = deflated?;
    let mut out = Vec::with_capacity(ZLIB_HEADER.len() + deflated.iter().map(Vec::len).sum::<usize>() + 4);
    out.extend_from_slice(&ZLIB_HEADER);
    for piece in &deflated {
        out.extend_from_slice(piece);
    }
    out.extend_from_slice(&checksum.to_be_bytes());
    Ok(out)
}

/// Raw deflate of one piece, ended by a sync flush (byte-aligned, not final) unless it is the
/// last piece of the stream.
fn deflate_piece(piece: &[u8], last: bool) -> Result<Vec<u8>> {
    let mut compress = Compress::new(Compression::default(), false);
    // Room for deflate's worst case (stored blocks), so one call takes the whole piece and
    // flushes it.
    let mut out = Vec::with_capacity(piece.len() + piece.len() / 1024 + 1024);
    let flush = if last { FlushCompress::Finish } else { FlushCompress::Sync };
    let status = compress.compress_vec(piece, &mut out, flush)?;
    let complete = compress.total_in() == piece.len() as u64
        && out.len() < out.capacity()
        && (!last || status == Status::StreamEnd);
    if !complete {
        bail!("Compressing a VOXL chunk stopped early");
    }
    Ok(out)
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
mod tests_branch_on_post {
    use super::super::support_layout::PostLayout;
    use super::*;

    #[test]
    fn a_branch_can_hang_from_a_post() {
        let post = PostLayout {
            foot: Vec3::new(5.0, 0.0, 2.0),
            joints: Vec::new(),
            top: Vec3::new(5.0, 0.0, 30.0),
            root_diameter_mm: 3.0,
            diameter_mm: 1.0,
        };
        let branch = SupportPath {
            tip: ContactTip {
                contact: Vec3::new(9.0, 0.0, 33.0),
                surface_normal: Vec3::new(0.0, 0.0, -1.0),
                from_outside: true,
                socket: Vec3::new(9.0, 0.0, 28.0),
                contact_diameter_mm: 0.3,
                body_diameter_mm: 0.6,
                ball: None,
            },
            base: SupportBase::Shaft { parent: 1, point: Vec3::new(5.0, 0.0, 20.0) },
            joints: Vec::new(),
            shaft_diameter_mm: 0.6,
        };
        let layout = SupportLayout { supports: vec![branch], posts: vec![post], ..SupportLayout::default() };
        let triangles = [Triangle::new(
            Vec3::new(0.0, 0.0, 33.0),
            Vec3::new(10.0, 0.0, 33.0),
            Vec3::new(0.0, 10.0, 33.0),
            Vec3::new(0.0, 0.0, 1.0),
        )];
        let mesh = VoxlMesh { name: "m", triangles: &triangles, supports: Some(&layout) };
        let placement = VoxlPlacement { mesh: 0, rotation_z: 0.0, translation: [0.0; 3] };
        let bytes = write_voxl(std::io::Cursor::new(Vec::new()), &[mesh], &[placement], None).unwrap().into_inner();
        let (_, chunks) = read_voxl(&bytes);
        let supports: Value = serde_json::from_slice(&chunks.iter().find(|c| &c.0 == b"SUPP").unwrap().2).unwrap();
        let knot = &supports["knots"][0];
        let trunk = &supports["trunks"][0];
        assert_eq!(knot["parentShaftId"], trunk["segments"][0]["id"]);
        assert_eq!(supports["branches"][0]["parentKnotId"], knot["id"]);
        assert!((knot["pos"]["z"].as_f64().unwrap() - 20.0).abs() < 1e-3);
        assert!((knot["diameter"].as_f64().unwrap() - (1.0 + f64::from(JOINT_DIAMETER_OFFSET_MM))).abs() < 1e-3);
    }
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
            from_outside: true,
            socket,
            contact_diameter_mm: 0.3,
            body_diameter_mm: 0.8,
            ball: None,
        };
        let layout = SupportLayout {
            braces: vec![],
            posts: vec![],
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
            ..Default::default()
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
        // The contact written is the buried base's, so the cone is 0.3 mm wide at the surface once the
        // slicer has run it on by the default tip offset.
        let (contact, body) = (
            cone["profile"]["contactDiameterMm"].as_f64().unwrap(),
            cone["profile"]["bodyDiameterMm"].as_f64().unwrap(),
        );
        let reach = f64::from((Vec3::new(1.0, 2.0, 10.0) - Vec3::new(1.0, 2.0, 8.0)).length());
        let offset = f64::from(NOMINAL_TIP_OFFSET_MM);
        let at_surface = contact + (body - contact) * offset / (reach + offset);
        assert!((at_surface - 0.3).abs() < 1e-3, "{contact} {body}");
        assert!(contact < 0.3);
        assert_eq!(body, 0.8, "the body stays as read");
        assert_eq!(cone["profile"]["penetrationMm"], 0.1);

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
            from_outside: true,
            socket,
            contact_diameter_mm: 0.4,
            body_diameter_mm: 0.5,
            ball: None,
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
                from_outside: true,
                socket: Vec3::new(x, 0.0, 8.0),
                contact_diameter_mm: 0.3,
                body_diameter_mm: 0.8,
                ball: None,
            },
            base: SupportBase::Plate { x, y: 0.0, diameter_mm: 3.0 },
            joints: vec![],
            shaft_diameter_mm: 0.8,
        };
        let layout = SupportLayout {
            supports: vec![trunk(0.0), trunk(3.0)],
            braces: vec![BraceLayout {
                lower: BraceEnd { support: 1, kickstand: None, point: Vec3::new(3.0, 0.0, 4.0) },
                upper: BraceEnd { support: 0, kickstand: None, point: Vec3::new(0.0, 0.0, 6.0) },
                diameter_mm: 0.6,
            }],
            posts: vec![],
            kickstands: vec![],
            ..Default::default()
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
    fn braces_to_a_kickstand_hang_from_a_knot_on_its_column() {
        use crate::geometry::support_layout::{
            BraceEnd, BraceLayout, ContactTip, KickstandLayout, ROOT_TOP_MM, SupportBase, SupportPath,
        };

        let layout = SupportLayout {
            supports: vec![SupportPath {
                tip: ContactTip {
                    contact: Vec3::new(0.0, 0.0, 12.0),
                    surface_normal: Vec3::new(0.0, 0.0, -1.0),
                    from_outside: true,
                    socket: Vec3::new(0.0, 0.0, 10.0),
                    contact_diameter_mm: 0.3,
                    body_diameter_mm: 0.8,
                    ball: None,
                },
                base: SupportBase::Plate { x: 0.0, y: 0.0, diameter_mm: 3.0 },
                joints: vec![],
                shaft_diameter_mm: 0.8,
            }],
            posts: vec![],
            // A post 4 mm from the trunk leaning onto it from 8 mm up, braced to it lower down.
            kickstands: vec![KickstandLayout {
                support: 0,
                knot: Vec3::new(0.0, 0.0, 10.0),
                joints: vec![Vec3::new(4.0, 0.0, 8.0)],
                foot: Vec3::new(4.0, 0.0, ROOT_TOP_MM),
                root_diameter_mm: 2.0,
                diameter_mm: 0.6,
            }],
            braces: vec![BraceLayout {
                lower: BraceEnd { support: 0, kickstand: Some(0), point: Vec3::new(4.0, 0.0, 4.0) },
                upper: BraceEnd { support: 0, kickstand: None, point: Vec3::new(0.0, 0.0, 5.5) },
                diameter_mm: 0.5,
            }],
            ..Default::default()
        };
        let a = [tri([0.0, 0.0, 12.0], [4.0, 0.0, 12.0], [0.0, 4.0, 14.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: Some(&layout) }];
        let (_, chunks) = read_voxl(&write(&meshes, &[placement(0, 0.0)], None));
        let supp = json_chunk(&chunks, b"SUPP");

        let brace = &supp["braces"][0];
        let knot = |id: &Value| supp["knots"].as_array().unwrap().iter().find(|k| k["id"] == *id).unwrap().clone();
        let (start, end) = (knot(&brace["startKnotId"]), knot(&brace["endKnotId"]));
        let column = &supp["kickstands"][0]["kickstand"]["segments"][0]["id"];
        assert_eq!(&start["parentShaftId"], column);
        assert!(near(xyz(&start["pos"]), [4.0, 0.0, 4.0]), "{}", start["pos"]);
        assert_eq!(end["parentShaftId"], supp["trunks"][0]["segments"][0]["id"]);
        assert!(near(xyz(&end["pos"]), [0.0, 0.0, 5.5]), "{}", end["pos"]);
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
                from_outside: true,
                socket: Vec3::new(x, 0.0, 8.0),
                contact_diameter_mm: 0.3,
                body_diameter_mm: 0.8,
                ball: None,
            },
            base: SupportBase::Plate { x, y: 0.0, diameter_mm: 3.0 },
            joints: vec![Vec3::new(x, 0.0, 5.0)],
            shaft_diameter_mm: 0.8,
        };
        let layout = SupportLayout {
            supports: vec![trunk(0.0), trunk(3.0)],
            braces: vec![BraceLayout {
                lower: BraceEnd { support: 0, kickstand: None, point: Vec3::new(0.0, 0.0, 5.0) },
                upper: BraceEnd { support: 1, kickstand: None, point: Vec3::new(3.0, 0.0, 1.0) },
                diameter_mm: 0.6,
            }],
            posts: vec![],
            kickstands: vec![],
            ..Default::default()
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
            from_outside: true,
            socket: Vec3::new(x, 0.0, z - 2.0),
            contact_diameter_mm: 0.3,
            body_diameter_mm: 0.8,
            ball: None,
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
                lower: BraceEnd { support: 0, kickstand: None, point: Vec3::new(0.0, 0.0, 7.0) },
                upper: BraceEnd { support: 1, kickstand: None, point: Vec3::new(2.0, 0.0, 6.0) },
                diameter_mm: 0.5,
            }],
            posts: vec![],
            kickstands: vec![],
            ..Default::default()
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
                    from_outside: true,
                    socket: Vec3::new(0.0, 0.0, 10.0),
                    contact_diameter_mm: 0.3,
                    body_diameter_mm: 0.8,
                    ball: None,
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
    fn posts_are_trunks_without_a_contact_cone_that_kickstands_and_braces_hang_from() {
        use crate::geometry::support_layout::{
            BraceEnd, BraceLayout, ContactTip, KickstandLayout, PostLayout, ROOT_TOP_MM, SupportBase, SupportPath,
        };

        let layout = SupportLayout {
            supports: vec![SupportPath {
                tip: ContactTip {
                    contact: Vec3::new(0.0, 0.0, 12.0),
                    surface_normal: Vec3::new(0.0, 0.0, -1.0),
                    from_outside: true,
                    socket: Vec3::new(0.0, 0.0, 10.0),
                    contact_diameter_mm: 0.3,
                    body_diameter_mm: 0.8,
                    ball: None,
                },
                base: SupportBase::Plate { x: 0.0, y: 0.0, diameter_mm: 3.0 },
                joints: vec![],
                shaft_diameter_mm: 0.8,
            }],
            // A post 4 mm from the trunk, bending at 5 mm and ending 7 mm up, held up by a leg and
            // braced to the trunk.
            posts: vec![PostLayout {
                foot: Vec3::new(4.0, 0.0, ROOT_TOP_MM),
                joints: vec![Vec3::new(4.0, 0.0, 5.0)],
                top: Vec3::new(4.5, 0.0, 7.0),
                root_diameter_mm: 2.0,
                diameter_mm: 0.6,
            }],
            kickstands: vec![KickstandLayout {
                support: 1,
                knot: Vec3::new(4.0, 0.0, 4.0),
                joints: vec![],
                foot: Vec3::new(7.0, 0.0, ROOT_TOP_MM),
                root_diameter_mm: 2.0,
                diameter_mm: 0.6,
            }],
            braces: vec![BraceLayout {
                lower: BraceEnd { support: 1, kickstand: None, point: Vec3::new(4.25, 0.0, 6.0) },
                upper: BraceEnd { support: 0, kickstand: None, point: Vec3::new(0.0, 0.0, 9.0) },
                diameter_mm: 0.5,
            }],
            ..Default::default()
        };
        let a = [tri([0.0, 0.0, 12.0], [4.0, 0.0, 12.0], [0.0, 4.0, 14.0])];
        let meshes = [VoxlMesh { name: "a.stl", triangles: &a, supports: Some(&layout) }];
        let (_, chunks) = read_voxl(&write(&meshes, &[placement(0, 0.0)], None));
        let supp = json_chunk(&chunks, b"SUPP");

        let trunks = supp["trunks"].as_array().unwrap();
        assert_eq!(trunks.len(), 2);
        let post = &trunks[1];
        assert!(post.get("contactCone").is_none(), "{post}");
        assert_eq!(post["typeId"], "trunk");
        assert_eq!(post["baseDiameterMm"], 0.6);
        let root = supp["roots"].as_array().unwrap().iter().find(|r| r["id"] == post["rootId"]).expect("its root");
        assert!(near(xyz(&root["transform"]["pos"]), [4.0, 0.0, 0.0]), "{}", root["transform"]["pos"]);
        assert_eq!(root["diameter"], 2.0);
        // Segment 0 leaves the root top; every segment ends at a joint, the last at the top.
        let segments = post["segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2);
        assert!(segments[0].get("bottomJoint").is_none());
        assert_eq!(segments[0]["topJoint"]["id"], segments[1]["bottomJoint"]["id"]);
        assert!(near(xyz(&segments[0]["topJoint"]["pos"]), [4.0, 0.0, 5.0]));
        assert!(near(xyz(&segments[1]["topJoint"]["pos"]), [4.5, 0.0, 7.0]));
        assert_eq!(segments[1]["topJoint"]["diameter"], 0.7);
        assert_eq!(segments[1]["diameter"], 0.6);

        // The leg hangs from a knot on the post's lower segment, the brace from its upper one.
        let kickstand = &supp["kickstands"][0];
        assert_eq!(kickstand["hostKnot"]["parentShaftId"], segments[0]["id"]);
        assert!(near(xyz(&kickstand["hostKnot"]["pos"]), [4.0, 0.0, 4.0]), "{}", kickstand["hostKnot"]["pos"]);
        let brace = &supp["braces"][0];
        let knot = |id: &Value| supp["knots"].as_array().unwrap().iter().find(|k| k["id"] == *id).unwrap().clone();
        let (start, end) = (knot(&brace["startKnotId"]), knot(&brace["endKnotId"]));
        assert_eq!(start["parentShaftId"], segments[1]["id"]);
        assert!(near(xyz(&start["pos"]), [4.25, 0.0, 6.0]), "{}", start["pos"]);
        assert_eq!(end["parentShaftId"], trunks[0]["segments"][0]["id"]);
    }

    #[test]
    fn unknown_mesh_index_is_rejected() {
        let result = write_voxl(std::io::Cursor::new(Vec::new()), &[], &[placement(0, 0.0)], None);
        assert!(result.is_err());
    }

    #[test]
    fn chunks_compressed_in_pieces_inflate_as_one_zlib_stream() {
        use std::io::Read;

        // Half repetitive, half noisy, so pieces hold both compressed and stored blocks.
        let mut seed = 0x2545_f491_u32;
        let mut noise = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        };
        let piece = COMPRESSION_PIECE_BYTES;
        for len in [0, 1, 1000, piece - 1, piece, piece + 1, 3 * piece + 17] {
            let data: Vec<u8> = (0..len).map(|i| if i % 2 == 0 { (i / 7) as u8 } else { noise() }).collect();
            let compressed = zlib_compress(&data).unwrap();
            let mut inflated = Vec::new();
            flate2::read::ZlibDecoder::new(compressed.as_slice()).read_to_end(&mut inflated).unwrap();
            assert_eq!(inflated, data, "{len} bytes");
            if len > 2 * piece {
                assert!(compressed.len() < data.len(), "{len} bytes compress");
            }
        }
    }
}

#[cfg(test)]
mod tests_reader_layouts {
    use super::super::support_layout::BallTip;
    use super::*;

    fn tip(contact: Vec3, socket: Vec3, d: f32, body: f32) -> ContactTip {
        ContactTip {
            contact,
            surface_normal: Vec3::new(0.0, 0.0, -1.0),
            from_outside: true,
            socket,
            contact_diameter_mm: d,
            body_diameter_mm: body,
            ball: None,
        }
    }

    fn supp(layout: &SupportLayout, sphere: bool) -> Value {
        let triangles = [Triangle::new(
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(10.0, 0.0, 10.0),
            Vec3::new(0.0, 10.0, 10.0),
            Vec3::new(0.0, 0.0, 1.0),
        )];
        let mesh = VoxlMesh { name: "m", triangles: &triangles, supports: Some(layout) };
        let placement = VoxlPlacement { mesh: 0, rotation_z: 0.0, translation: [0.0; 3] };
        let bytes = write_voxl_with(std::io::Cursor::new(Vec::new()), &[mesh], &[placement], None, sphere)
            .unwrap()
            .into_inner();
        let (_, chunks) = read_voxl(&bytes);
        serde_json::from_slice(&chunks.iter().find(|c| &c.0 == b"SUPP").unwrap().2).unwrap()
    }

    fn trunk(t: ContactTip) -> SupportLayout {
        let path = SupportPath {
            tip: t,
            base: SupportBase::Plate { x: 0.0, y: 0.0, diameter_mm: 3.0 },
            joints: vec![],
            shaft_diameter_mm: 0.8,
        };
        SupportLayout { supports: vec![path], ..SupportLayout::default() }
    }

    #[test]
    fn sphere_tips_are_off_by_default() {
        const { assert!(!SPHERE_TIPS_ENABLED) };
    }

    #[test]
    fn a_ball_tip_is_a_sphere_tip_only_when_sphere_tips_are_on() {
        let mut t = tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 8.0), 0.5, 0.8);
        t.ball = Some(BallTip { diameter_mm: 0.6, cone_end_mm: 0.4, depth_mm: Some(0.15) });
        let layout = trunk(t);
        let on = supp(&layout, true);
        let cone = &on["trunks"][0]["contactCone"];
        assert_eq!(cone["profile"]["type"], "sphere");
        assert!(cone["profile"]["sphereDepthMm"].is_number());
        assert!(cone["profile"]["sphereRadiusRatio"].as_f64().unwrap() > 1.0);
        let off = supp(&layout, false);
        let cone = &off["trunks"][0]["contactCone"];
        assert_eq!(cone["profile"]["type"], "disk", "off: the ball tip is written as any other tip");
        let plain = supp(&trunk(tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 8.0), 0.5, 0.8)), false);
        assert_eq!(cone["profile"], plain["trunks"][0]["contactCone"]["profile"]);
    }

    #[test]
    fn a_ball_with_no_depth_takes_dragonfruits_own_seat() {
        let mut t = tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 8.0), 0.5, 0.8);
        t.ball = Some(BallTip { diameter_mm: 0.8, cone_end_mm: 0.0, depth_mm: None });
        let s = supp(&trunk(t), true);
        let profile = &s["trunks"][0]["contactCone"]["profile"];
        assert_eq!(profile["type"], "sphere");
        assert!(profile.get("sphereDepthMm").is_none());
    }

    #[test]
    fn a_stepped_shaft_writes_one_width_per_segment_and_joints_as_wide_as_the_wider() {
        let t = tip(Vec3::new(0.0, 0.0, 20.0), Vec3::new(0.0, 0.0, 18.0), 0.3, 0.8);
        let mut layout = trunk(t);
        layout.supports[0].joints = vec![Vec3::new(0.0, 0.0, 10.0)];
        layout.segment_diameters.insert(0, vec![1.6, 1.0]);
        let s = supp(&layout, false);
        let segs = s["trunks"][0]["segments"].as_array().unwrap();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0]["diameter"], 1.6);
        assert_eq!(segs[1]["diameter"], 1.0);
        assert!(
            (segs[0]["topJoint"]["diameter"].as_f64().unwrap() - (1.6 + f64::from(JOINT_DIAMETER_OFFSET_MM))).abs()
                < 1e-6
        );
        assert_eq!(s["trunks"][0]["baseDiameterMm"], 1.6);
    }

    #[test]
    fn strut_kind_flags_override_the_span_rule() {
        let lower = ContactTip {
            surface_normal: Vec3::new(0.0, 0.0, 1.0),
            ..tip(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 6.0), 0.3, 0.6)
        };
        let strut = SupportPath {
            tip: tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 9.0), 0.3, 0.6),
            base: SupportBase::Model(lower),
            joints: vec![],
            shaft_diameter_mm: 0.6,
        };
        // 5 mm apart: short of DragonFruit's stick span, so a twig unless marked a stick.
        let short = |stick: bool, twig: bool| {
            let mut l = SupportLayout { supports: vec![strut.clone()], ..SupportLayout::default() };
            if stick {
                l.sticks.insert(0);
            }
            if twig {
                l.twigs.insert(0);
            }
            let s = supp(&l, false);
            (s["sticks"].as_array().unwrap().len(), s["twigs"].as_array().unwrap().len())
        };
        assert_eq!(short(true, false), (1, 0));
        assert_eq!(short(false, true), (0, 1));
    }

    #[test]
    fn a_leaning_cone_is_written_wider_and_moved_against_its_lean() {
        // A cone leaning 45 degrees: its sliced loop at the tip offset is the contact's area.
        let straight = tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 8.0), 0.4, 0.8);
        let leaning = tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(2.0, 0.0, 8.0), 0.4, 0.8);
        let (p0, d0) = sliced_contact_placement(&straight);
        let (p1, d1) = sliced_contact_placement(&leaning);
        assert!((p0.x).abs() < 1e-6 && d0 < 0.4, "a straight tip only loses the taper over the offset: {d0}");
        assert!(d1 < d0, "a leaning cone's cut is an ellipse wider than its section: {d1} {d0}");
        assert!(p1.x < -1e-3, "the loop's centre moves along the lean, so the written one moves back: {p1}");
    }

    #[test]
    fn a_blunt_end_wider_than_its_rod_keeps_the_rod_and_sinks() {
        // A 1 mm rod ending at a slant leaves a 1.3 mm loop.
        let t = tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(1.0, 0.0, 8.0), 1.3, 1.0);
        let (p, d) = sliced_contact_placement(&t);
        assert!((d - 1.0).abs() < 1e-6, "{d}");
        assert!(p.z > 10.0 - 1e-6, "sunk into the model (along the surface normal, which faces down): {p}");
    }

    #[test]
    fn the_written_tip_is_the_contact_cone_written() {
        let leaning = tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(2.0, 0.0, 8.0), 0.4, 0.8);
        let WrittenTip::Cone { pos, diameter_mm } = written_tip(&leaning, false) else { panic!("a cone") };
        let cone = &supp(&trunk(leaning.clone()), false)["trunks"][0]["contactCone"];
        let at = |k: &str| cone["pos"][k].as_f64().unwrap();
        assert!((at("x") - f64::from(pos.x)).abs() < 1e-4 && (at("z") - f64::from(pos.z)).abs() < 1e-4, "{cone} {pos}");
        assert!((cone["profile"]["contactDiameterMm"].as_f64().unwrap() - f64::from(diameter_mm)).abs() < 1e-4);
        let mut ball = leaning;
        ball.ball = Some(BallTip { diameter_mm: 0.6, cone_end_mm: 0.4, depth_mm: Some(0.15) });
        assert!(matches!(written_tip(&ball, true), WrittenTip::Sphere { depth_mm: Some(_), .. }));
        assert!(matches!(written_tip(&ball, false), WrittenTip::Cone { .. }));
    }

    #[test]
    fn strut_ends_follow_the_end_kind_then_the_span() {
        let lower = tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 10.4), 1.0, 1.0);
        let upper = tip(Vec3::new(0.0, 0.0, 12.0), Vec3::new(0.0, 0.0, 11.6), 1.0, 1.0);
        let short = strut_ends(&lower, &upper, 1.0, false, false, false);
        assert!(short.twig && short.standoff_mm > 0.0);
        assert!((short.end(&lower).z - (10.0 - short.standoff_mm)).abs() < 1e-6, "off the disk along the normal");
        let stick = strut_ends(&lower, &upper, 1.0, true, false, false);
        assert!(!stick.twig && stick.end(&upper) == upper.socket);
        assert!(strut_ends(&lower, &upper, 1.0, false, true, false).twig);
    }

    #[test]
    fn a_cone_ended_micro_support_shorter_than_its_built_cones_is_a_twig() {
        // Cones 0.06 mm long on a 0.05 mm shaft: each built cone reaches 0.06 + 0.075 mm (the socket joint's radius).
        let at = |z: f32, dz: f32, nz: f32| {
            let mut t = tip(Vec3::new(0.0, 0.0, z), Vec3::new(0.0, 0.0, z + dz), 0.05, 0.05);
            t.surface_normal = Vec3::new(0.0, 0.0, nz);
            t
        };
        let short = strut_ends(&at(10.0, 0.06, 1.0), &at(10.2, -0.06, -1.0), 0.05, true, false, true);
        assert!(short.twig, "0.2 mm < 2 x 0.135 mm: {short:?}");
        let long = strut_ends(&at(10.0, 0.06, 1.0), &at(10.4, -0.06, -1.0), 0.05, true, false, true);
        assert!(!long.twig, "0.4 mm is longer than its cones: {long:?}");
        let strut = strut_ends(&at(10.0, 0.06, 1.0), &at(10.2, -0.06, -1.0), 0.05, true, false, false);
        assert!(!strut.twig, "struts with cone ends stay sticks: {strut:?}");
    }

    #[test]
    fn a_micro_twig_stands_off_by_its_disk_ball_radius() {
        let lower = tip(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 10.05), 0.05, 0.05);
        let upper = tip(Vec3::new(0.0, 0.0, 10.2), Vec3::new(0.0, 0.0, 10.15), 0.05, 0.05);
        let micro = strut_ends(&lower, &upper, 0.05, false, true, true);
        assert!(micro.twig && (micro.standoff_mm - 0.025).abs() < 1e-6, "{micro:?}");
        let plain = strut_ends(&lower, &upper, 0.05, false, true, false);
        assert!(plain.standoff_mm >= TIP_DISK_THICKNESS_MM, "other twigs keep their disks: {plain:?}");
        // Written: the micro twig's disks are as long as the stand-off.
        let path = SupportPath { tip: upper, base: SupportBase::Model(lower), joints: vec![], shaft_diameter_mm: 0.05 };
        let mut layout = SupportLayout { supports: vec![path], ..SupportLayout::default() };
        layout.twigs.insert(0);
        layout.micro.insert(0);
        assert_eq!(support_strut_ends(&layout, 0), Some(micro));
        let twig = &supp(&layout, false)["twigs"][0];
        assert_eq!(twig["contactDiskA"]["diskLengthOverride"], 0.025);
    }
}
