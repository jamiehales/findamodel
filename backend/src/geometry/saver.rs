//! Serialises geometry into model file formats: binary STL, 3MF (with instancing) and GLB.
//! Triangles are written as-is; callers handle any coordinate-system conversion.

use std::io::{Seek, Write};

use anyhow::Result;
use rustc_hash::FxHashMap;
use serde_json::json;
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use super::{Triangle, Vec3};

/// Writes a binary STL: 80-byte header, u32 triangle count, then 50 bytes per triangle
/// (f32x3 normal, f32x3 v0, v1, v2, u16 attribute = 0).
pub fn write_stl<W: Write>(
    writer: &mut W,
    triangle_count: u32,
    triangles: impl IntoIterator<Item = Triangle>,
    header_text: Option<&str>,
) -> Result<()> {
    let mut header = [0u8; 84];
    if let Some(text) = header_text {
        let bytes: Vec<u8> = text.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).collect();
        let len = bytes.len().min(79);
        header[..len].copy_from_slice(&bytes[..len]);
    }
    header[80..84].copy_from_slice(&triangle_count.to_le_bytes());
    writer.write_all(&header)?;

    let mut buffer = Vec::with_capacity(50 * 4096);
    for tri in triangles {
        for v in [tri.normal, tri.v0, tri.v1, tri.v2] {
            buffer.extend_from_slice(&v.x.to_le_bytes());
            buffer.extend_from_slice(&v.y.to_le_bytes());
            buffer.extend_from_slice(&v.z.to_le_bytes());
        }
        buffer.extend_from_slice(&[0, 0]);
        if buffer.len() >= 50 * 4096 {
            writer.write_all(&buffer)?;
            buffer.clear();
        }
    }
    writer.write_all(&buffer)?;
    Ok(())
}

pub fn save_stl(triangles: &[Triangle], header_text: Option<&str>) -> Vec<u8> {
    let mut out = Vec::with_capacity(84 + triangles.len() * 50);
    write_stl(&mut out, triangles.len() as u32, triangles.iter().copied(), header_text).expect("writing to memory");
    out
}

type VertexKey = (u32, u32, u32);

/// Hash key for exact vertex deduplication (treats -0.0 and 0.0 as equal).
fn vertex_key(v: Vec3) -> VertexKey {
    let bits = |f: f32| if f == 0.0 { 0 } else { f.to_bits() };
    (bits(v.x), bits(v.y), bits(v.z))
}

/// One mesh resource for 3MF/GLB output, referenced by `id` from placements.
pub struct MeshObject<'a> {
    pub id: u32,
    pub triangles: &'a [Triangle],
}

/// Writes a 3MF package. Each object is one mesh resource (no placement transform); each
/// item references an object and carries the 3MF transform string positioning it on the
/// plate. Multiple items may reference the same object: the core spec forbids repeating
/// an objectid in `<build>`, so every placement gets its own component wrapper object.
pub fn write_3mf<W: Write + Seek>(writer: W, objects: &[MeshObject<'_>], items: &[(u32, String)]) -> Result<W> {
    let mut zip = zip::ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // Alphabetical by extension, matching lib3mf's default registration set.
    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
            "<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>",
            "<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>",
            "<Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>",
            "<Default Extension=\"png\" ContentType=\"image/png\"/>",
            "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
            "<Default Extension=\"texture\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodeltexture\"/>",
            "</Types>"
        )
        .as_bytes(),
    )?;

    zip.start_file("_rels/.rels", options)?;
    zip.write_all(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            "<Relationship Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\" Target=\"/3D/3dmodel.model\" Id=\"rel0\"/>",
            "</Relationships>"
        )
        .as_bytes(),
    )?;

    zip.start_file("3D/3dmodel.model", options)?;
    {
        let mut out = std::io::BufWriter::with_capacity(1 << 16, &mut zip);
        write_3d_model_xml(&mut out, objects, items)?;
        out.flush()?;
    }

    Ok(zip.finish()?)
}

pub fn save_3mf(objects: &[MeshObject<'_>], items: &[(u32, String)]) -> Vec<u8> {
    write_3mf(std::io::Cursor::new(Vec::new()), objects, items).expect("writing to memory").into_inner()
}

fn write_3d_model_xml<W: Write>(out: &mut W, objects: &[MeshObject<'_>], items: &[(u32, String)]) -> Result<()> {
    out.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
    // xmlns first, then unit / xml:lang, then all extension namespaces, matching lib3mf order.
    out.write_all(
        concat!(
            "<model",
            " xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\"",
            " unit=\"millimeter\" xml:lang=\"en-US\"",
            " xmlns:m=\"http://schemas.microsoft.com/3dmanufacturing/material/2015/02\"",
            " xmlns:p=\"http://schemas.microsoft.com/3dmanufacturing/production/2015/06\"",
            " xmlns:b=\"http://schemas.microsoft.com/3dmanufacturing/beamlattice/2017/02\"",
            " xmlns:s=\"http://schemas.microsoft.com/3dmanufacturing/slice/2015/07\"",
            " xmlns:t=\"http://schemas.microsoft.com/3dmanufacturing/trianglesets/2021/07\"",
            " xmlns:sc=\"http://schemas.microsoft.com/3dmanufacturing/securecontent/2019/04\"",
            " xmlns:v=\"http://schemas.3mf.io/3dmanufacturing/volumetric/2022/01\"",
            " xmlns:i=\"http://schemas.3mf.io/3dmanufacturing/implicit/2023/12\">",
            "<resources>"
        )
        .as_bytes(),
    )?;

    // Base mesh objects; not referenced directly in <build>.
    for object in objects {
        let mut vertices: Vec<Vec3> = Vec::new();
        let mut vertex_map: FxHashMap<(u32, u32, u32), u32> = FxHashMap::default();
        let mut tri_indices = Vec::with_capacity(object.triangles.len());
        for tri in object.triangles {
            let mut idx = [0u32; 3];
            for (slot, v) in idx.iter_mut().zip([tri.v0, tri.v1, tri.v2]) {
                *slot = *vertex_map.entry(vertex_key(v)).or_insert_with(|| {
                    vertices.push(v);
                    (vertices.len() - 1) as u32
                });
            }
            tri_indices.push(idx);
        }

        write!(out, "<object id=\"{}\" type=\"model\" p:UUID=\"{}\"><mesh><vertices>", object.id, Uuid::new_v4())?;
        for v in &vertices {
            write!(
                out,
                "<vertex x=\"{}\" y=\"{}\" z=\"{}\"/>",
                format_coord(v.x),
                format_coord(v.y),
                format_coord(v.z)
            )?;
        }
        out.write_all(b"</vertices><triangles>")?;
        for [v1, v2, v3] in &tri_indices {
            write!(out, "<triangle v1=\"{v1}\" v2=\"{v2}\" v3=\"{v3}\"/>")?;
        }
        out.write_all(b"</triangles></mesh></object>")?;
    }

    let first_instance_id = objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
    let mut instance_ids = Vec::with_capacity(items.len());
    for (instance_id, (object_id, transform)) in (first_instance_id..).zip(items) {
        write!(
            out,
            "<object id=\"{instance_id}\" type=\"model\" p:UUID=\"{}\"><components><component objectid=\"{object_id}\" transform=\"{transform}\" p:UUID=\"{}\"/></components></object>",
            Uuid::new_v4(),
            Uuid::new_v4()
        )?;
        instance_ids.push(instance_id);
    }

    write!(out, "</resources><build p:UUID=\"{}\">", Uuid::new_v4())?;
    for id in instance_ids {
        write!(out, "<item objectid=\"{id}\" p:UUID=\"{}\"/>", Uuid::new_v4())?;
    }
    out.write_all(b"</build></model>")?;
    Ok(())
}

/// Six decimal places, matching lib3mf's output precision; values that round to zero are
/// emitted as "0".
fn format_coord(v: f32) -> String {
    let r = (v as f64 * 1e6).round_ties_even() / 1e6;
    if r == 0.0 { "0".to_string() } else { format!("{r:.6}") }
}

/// Formats a value like .NET's "G9" round-trip float formatting closely enough for 3MF
/// transforms: shortest representation that round-trips an f32.
pub fn format_transform_value(v: f32) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let s = format!("{v}");
    if s.contains('e') || s.contains('E') { format!("{v:.9}") } else { s }
}

/// Writes a GLB (binary glTF 2.0). Geometry must be Y-up, glTF's native convention.
/// Each object becomes one mesh stored once; each item becomes a node referencing it with
/// a column-major `matrix`, which is how glTF instancing works. Meshes are flat shaded:
/// each triangle's normal is replicated to its three vertices.
pub fn save_glb(objects: &[MeshObject<'_>], items: &[(u32, [f32; 16])]) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut buffer_views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut mesh_index_by_object: FxHashMap<u32, usize> = FxHashMap::default();

    for object in objects {
        let mut positions: Vec<Vec3> = Vec::new();
        let mut normals: Vec<Vec3> = Vec::new();
        let mut map: FxHashMap<(VertexKey, VertexKey), u32> = FxHashMap::default();
        let mut indices: Vec<u32> = Vec::with_capacity(object.triangles.len() * 3);
        for tri in object.triangles {
            for v in [tri.v0, tri.v1, tri.v2] {
                let index = *map.entry((vertex_key(v), vertex_key(tri.normal))).or_insert_with(|| {
                    positions.push(v);
                    normals.push(tri.normal);
                    (positions.len() - 1) as u32
                });
                indices.push(index);
            }
        }

        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for p in &positions {
            for (i, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                min[i] = min[i].min(c);
                max[i] = max[i].max(c);
            }
        }
        if positions.is_empty() {
            min = [0.0; 3];
            max = [0.0; 3];
        }

        let mut push_view = |data: &[u8], target: u32| -> usize {
            while !bin.len().is_multiple_of(4) {
                bin.push(0);
            }
            let offset = bin.len();
            bin.extend_from_slice(data);
            buffer_views.push(json!({ "buffer": 0, "byteOffset": offset, "byteLength": data.len(), "target": target }));
            buffer_views.len() - 1
        };

        let position_view = push_view(bytemuck::cast_slice(&positions), 34962);
        let normal_view = push_view(bytemuck::cast_slice(&normals), 34962);
        let index_view = push_view(bytemuck::cast_slice(&indices), 34963);

        accessors.push(json!({
            "bufferView": position_view, "componentType": 5126, "count": positions.len(), "type": "VEC3",
            "min": min, "max": max,
        }));
        let position_accessor = accessors.len() - 1;
        accessors
            .push(json!({ "bufferView": normal_view, "componentType": 5126, "count": normals.len(), "type": "VEC3" }));
        let normal_accessor = accessors.len() - 1;
        accessors
            .push(json!({ "bufferView": index_view, "componentType": 5125, "count": indices.len(), "type": "SCALAR" }));
        let index_accessor = accessors.len() - 1;

        meshes.push(json!({
            "name": format!("mesh_{}", object.id),
            "primitives": [{
                "attributes": { "POSITION": position_accessor, "NORMAL": normal_accessor },
                "indices": index_accessor,
                "material": 0,
                "mode": 4,
            }],
        }));
        mesh_index_by_object.insert(object.id, meshes.len() - 1);
    }

    let nodes: Vec<serde_json::Value> = items
        .iter()
        .filter_map(|(object_id, matrix)| {
            mesh_index_by_object.get(object_id).map(|mesh| json!({ "mesh": mesh, "matrix": matrix }))
        })
        .collect();
    let node_indices: Vec<usize> = (0..nodes.len()).collect();

    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }

    let mut document = json!({
        "asset": { "version": "2.0", "generator": "findamodel" },
        "scene": 0,
        "scenes": [{ "nodes": node_indices }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": [{ "name": "Default", "pbrMetallicRoughness": { "baseColorFactor": [1.0, 1.0, 1.0, 1.0], "metallicFactor": 0.0, "roughnessFactor": 1.0 } }],
        "accessors": accessors,
        "bufferViews": buffer_views,
    });
    if !bin.is_empty() {
        document["buffers"] = json!([{ "byteLength": bin.len() }]);
    }

    let mut json_bytes = serde_json::to_vec(&document).expect("serialising glTF json");
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }

    let total = 12 + 8 + json_bytes.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_triangles() -> Vec<Triangle> {
        vec![
            Triangle::new(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(0.0, 10.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ),
            Triangle::new(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 10.0, 0.0),
                Vec3::new(0.0, 0.0, 10.0),
                Vec3::new(1.0, 0.0, 0.0),
            ),
        ]
    }

    #[test]
    fn stl_streaming_writer_matches_slice_output() {
        let triangles = sample_triangles();
        let expected = save_stl(&triangles, Some("stream test"));

        let mut streamed = Vec::new();
        write_stl(&mut streamed, triangles.len() as u32, triangles.iter().copied(), Some("stream test")).unwrap();

        assert_eq!(expected, streamed);
        assert_eq!(expected.len(), 84 + 2 * 50);
        assert_eq!(&expected[..11], b"stream test");
    }

    #[test]
    fn stl_header_text_is_truncated_to_79_bytes() {
        let long = "x".repeat(200);
        let bytes = save_stl(&[], Some(&long));
        assert_eq!(bytes[78], b'x');
        assert_eq!(bytes[79], 0);
    }

    #[test]
    fn format_coord_matches_lib3mf_precision() {
        assert_eq!(format_coord(0.0), "0");
        assert_eq!(format_coord(-0.0000001), "0");
        assert_eq!(format_coord(1.5), "1.500000");
        assert_eq!(format_coord(-2.25), "-2.250000");
    }

    // ── 3MF conformance (3MF Core Specification v1.3) ───────────────────────

    const NS_3MF: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";

    fn tetrahedron() -> Vec<Triangle> {
        vec![
            Triangle::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 10.0), Vec3::UP),
            Triangle::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 10.0), Vec3::new(5.0, 10.0, 5.0), Vec3::UP),
            Triangle::new(Vec3::new(10.0, 0.0, 0.0), Vec3::new(5.0, 10.0, 5.0), Vec3::new(5.0, 0.0, 10.0), Vec3::UP),
            Triangle::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(5.0, 10.0, 5.0), Vec3::new(10.0, 0.0, 0.0), Vec3::UP),
        ]
    }

    fn simple_3mf(items: &[(u32, &str)]) -> Vec<u8> {
        let tris = tetrahedron();
        let items: Vec<(u32, String)> = if items.is_empty() {
            vec![(1, "1 0 0 0 1 0 0 0 1 0 0 0".to_string())]
        } else {
            items.iter().map(|(id, t)| (*id, t.to_string())).collect()
        };
        save_3mf(&[MeshObject { id: 1, triangles: &tris }], &items)
    }

    fn zip_entries(bytes: &[u8]) -> std::collections::HashMap<String, String> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        (0..archive.len())
            .map(|i| {
                let mut entry = archive.by_index(i).unwrap();
                let mut text = String::new();
                std::io::Read::read_to_string(&mut entry, &mut text).unwrap();
                (entry.name().to_string(), text)
            })
            .collect()
    }

    fn with_model<T>(bytes: &[u8], f: impl FnOnce(roxmltree::Document<'_>) -> T) -> T {
        let entries = zip_entries(bytes);
        let xml = entries.get("3D/3dmodel.model").expect("3D model part");
        f(roxmltree::Document::parse(xml).expect("model xml"))
    }

    fn elements<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Vec<roxmltree::Node<'a, 'i>> {
        node.descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(NS_3MF))
            .collect()
    }

    fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
        node.children().find(|n| n.is_element() && n.tag_name().name() == name)
    }

    #[test]
    fn package_contains_opc_parts_with_correct_types_and_relationship() {
        let entries = zip_entries(&simple_3mf(&[]));
        for part in ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"] {
            assert!(entries.contains_key(part), "missing {part}");
        }

        let types = roxmltree::Document::parse(&entries["[Content_Types].xml"]).unwrap();
        let content_type = |ext: &str| {
            types
                .descendants()
                .find(|n| n.attribute("Extension") == Some(ext))
                .and_then(|n| n.attribute("ContentType"))
                .map(str::to_string)
        };
        assert_eq!(content_type("model").as_deref(), Some("application/vnd.ms-package.3dmanufacturing-3dmodel+xml"));
        assert_eq!(content_type("rels").as_deref(), Some("application/vnd.openxmlformats-package.relationships+xml"));

        let rels = roxmltree::Document::parse(&entries["_rels/.rels"]).unwrap();
        let rel = rels
            .descendants()
            .find(|n| n.attribute("Type") == Some("http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"))
            .expect("3MF relationship");
        assert!(rel.attribute("Target").unwrap().trim_start_matches('/').eq_ignore_ascii_case("3D/3dmodel.model"));
    }

    #[test]
    fn model_root_has_namespace_unit_resources_and_build() {
        with_model(&simple_3mf(&[]), |doc| {
            let root = doc.root_element();
            assert_eq!(root.tag_name().namespace(), Some(NS_3MF));
            assert!(
                ["micron", "millimeter", "centimeter", "inch", "foot", "meter"]
                    .contains(&root.attribute("unit").unwrap())
            );
            assert!(child(root, "resources").is_some());
            assert!(child(root, "build").is_some());
        });
    }

    #[test]
    fn objects_have_unique_positive_ids_and_mesh_xor_components() {
        with_model(&simple_3mf(&[]), |doc| {
            let objects = elements(doc.root_element(), "object");
            let ids: Vec<u32> = objects.iter().map(|o| o.attribute("id").unwrap().parse().unwrap()).collect();
            assert!(ids.iter().all(|id| *id >= 1));
            let unique: std::collections::HashSet<_> = ids.iter().collect();
            assert_eq!(unique.len(), ids.len());
            for o in objects {
                assert!(child(o, "mesh").is_some() ^ child(o, "components").is_some());
            }
        });
    }

    #[test]
    fn meshes_are_deduplicated_with_valid_distinct_indices() {
        with_model(&simple_3mf(&[]), |doc| {
            for mesh in elements(doc.root_element(), "mesh") {
                let vertex_count = elements(mesh, "vertex").len();
                let triangles = elements(mesh, "triangle");
                assert_eq!(vertex_count, 4);
                assert!(!triangles.is_empty());
                for t in triangles {
                    let idx: Vec<usize> =
                        ["v1", "v2", "v3"].iter().map(|a| t.attribute(*a).unwrap().parse().unwrap()).collect();
                    assert!(idx.iter().all(|i| *i < vertex_count));
                    assert!(idx[0] != idx[1] && idx[0] != idx[2] && idx[1] != idx[2]);
                }
            }
        });
    }

    #[test]
    fn build_items_reference_unique_wrapper_objects() {
        with_model(&simple_3mf(&[]), |doc| {
            let root = doc.root_element();
            let object_ids: Vec<&str> = elements(root, "object").iter().map(|o| o.attribute("id").unwrap()).collect();
            let mesh_ids: Vec<&str> = elements(root, "object")
                .iter()
                .filter(|o| child(**o, "mesh").is_some())
                .map(|o| o.attribute("id").unwrap())
                .collect();
            let items: Vec<&str> = elements(child(root, "build").unwrap(), "item")
                .iter()
                .map(|i| i.attribute("objectid").unwrap())
                .collect();
            assert!(!items.is_empty());
            for item in &items {
                assert!(object_ids.contains(item));
                assert!(!mesh_ids.contains(item), "build items must reference component wrappers");
            }
            let unique: std::collections::HashSet<_> = items.iter().collect();
            assert_eq!(unique.len(), items.len());

            for component in elements(root, "component") {
                assert!(object_ids.contains(&component.attribute("objectid").unwrap()));
                let parts: Vec<&str> =
                    component.attribute("transform").unwrap().split(' ').filter(|p| !p.is_empty()).collect();
                assert_eq!(parts.len(), 12);
                assert!(parts.iter().all(|p| p.parse::<f64>().is_ok()));
            }
        });
    }

    #[test]
    fn instancing_shares_one_mesh_across_placements() {
        let bytes = simple_3mf(&[
            (1, "1 0 0 0 1 0 0 0 1 0 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 50 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 100 0 0"),
        ]);
        with_model(&bytes, |doc| {
            let root = doc.root_element();
            assert_eq!(elements(root, "object").iter().filter(|o| child(**o, "mesh").is_some()).count(), 1);
            let items: Vec<&str> = elements(child(root, "build").unwrap(), "item")
                .iter()
                .map(|i| i.attribute("objectid").unwrap())
                .collect();
            assert_eq!(items.len(), 3);
            assert_eq!(items.iter().collect::<std::collections::HashSet<_>>().len(), 3);
        });
    }

    #[test]
    fn thousands_of_placements_remain_valid() {
        let transforms: Vec<String> = (0..5000).map(|i| format!("1 0 0 0 1 0 0 0 1 {i} 0 0")).collect();
        let items: Vec<(u32, &str)> = transforms.iter().map(|t| (1, t.as_str())).collect();
        with_model(&simple_3mf(&items), |doc| {
            let root = doc.root_element();
            assert_eq!(elements(root, "object").iter().filter(|o| child(**o, "mesh").is_some()).count(), 1);
            assert_eq!(elements(child(root, "build").unwrap(), "item").len(), 5000);
        });
    }

    #[test]
    fn two_meshes_with_twenty_instances_round_trip_translations() {
        let cube: Vec<Triangle> = {
            let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
            let quads = [
                [p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.)],
                [p(0., 0., 1.), p(0., 1., 1.), p(1., 1., 1.), p(1., 0., 1.)],
                [p(0., 0., 0.), p(0., 0., 1.), p(1., 0., 1.), p(1., 0., 0.)],
                [p(0., 1., 0.), p(1., 1., 0.), p(1., 1., 1.), p(0., 1., 1.)],
                [p(0., 0., 0.), p(0., 1., 0.), p(0., 1., 1.), p(0., 0., 1.)],
                [p(1., 0., 0.), p(1., 0., 1.), p(1., 1., 1.), p(1., 1., 0.)],
            ];
            quads
                .iter()
                .flat_map(|[a, b, c, d]| [Triangle::new(*a, *b, *c, Vec3::UP), Triangle::new(*a, *c, *d, Vec3::UP)])
                .collect()
        };
        let tetra = tetrahedron();
        let positions = |from: (f32, f32, f32), to: (f32, f32, f32)| -> Vec<(f32, f32, f32)> {
            (0..20)
                .map(|i| {
                    let t = i as f32 / 19.0;
                    (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t, from.2 + (to.2 - from.2) * t)
                })
                .collect()
        };
        let cube_positions = positions((-10.0, 10.0, 10.0), (10.0, 10.0, 10.0));
        let tetra_positions = positions((10.0, -10.0, -10.0), (-10.0, 10.0, 10.0));
        let transform = |p: &(f32, f32, f32)| format!("1 0 0 0 1 0 0 0 1 {} {} {}", p.0, p.1, p.2);
        let items: Vec<(u32, String)> = cube_positions
            .iter()
            .map(|p| (1, transform(p)))
            .chain(tetra_positions.iter().map(|p| (2, transform(p))))
            .collect();

        let bytes =
            save_3mf(&[MeshObject { id: 1, triangles: &cube }, MeshObject { id: 2, triangles: &tetra }], &items);
        with_model(&bytes, |doc| {
            let root = doc.root_element();
            let meshes: Vec<_> = elements(root, "object").into_iter().filter(|o| child(*o, "mesh").is_some()).collect();
            assert_eq!(meshes.len(), 2);
            assert_eq!(elements(child(root, "build").unwrap(), "item").len(), 40);

            for (mesh_id, tri_count, vert_count, expected) in
                [("1", 12, 8, &cube_positions), ("2", 4, 4, &tetra_positions)]
            {
                let mesh = meshes.iter().find(|m| m.attribute("id") == Some(mesh_id)).unwrap();
                assert_eq!(elements(*mesh, "triangle").len(), tri_count);
                assert_eq!(elements(*mesh, "vertex").len(), vert_count);

                let mut actual: Vec<[f32; 3]> = elements(root, "component")
                    .iter()
                    .filter(|c| c.attribute("objectid") == Some(mesh_id))
                    .map(|c| {
                        let v: Vec<f32> =
                            c.attribute("transform").unwrap().split(' ').map(|p| p.parse().unwrap()).collect();
                        [v[9], v[10], v[11]]
                    })
                    .collect();
                assert_eq!(actual.len(), 20);
                for axis in 0..3 {
                    let mut got: Vec<f32> = actual.iter_mut().map(|t| t[axis]).collect();
                    let mut want: Vec<f32> = expected.iter().map(|p| [p.0, p.1, p.2][axis]).collect();
                    got.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    want.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    for (g, w) in got.iter().zip(&want) {
                        assert!((g - w).abs() < 1e-3, "axis {axis}: {g} vs {w}");
                    }
                }
            }
        });
    }

    #[test]
    fn glb_has_valid_header_and_chunks() {
        let triangles = sample_triangles();
        let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let glb = save_glb(&[MeshObject { id: 1, triangles: &triangles }], &[(1, identity), (1, identity)]);

        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(&glb[16..20], b"JSON");
        let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        assert_eq!(doc["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(doc["meshes"].as_array().unwrap().len(), 1);
        assert_eq!(&glb[20 + json_len + 4..20 + json_len + 8], b"BIN\0");
    }
}
