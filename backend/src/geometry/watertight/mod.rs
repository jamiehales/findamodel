//! Watertight export meshes (the remeshed VOXL download and plate export): checks that say whether a
//! mesh is closed, manifold, consistently oriented and free of self-intersections, the shells'
//! nesting (sealed voids, inner skins facing the wrong way, buried parts), and two repairs: the
//! boolean self-union ([`union`]) that keeps the exact surface, and the level-set remesh
//! ([`remesh`]) that rebuilds it on a fine grid. [`measure`] compares a repair with its original.

pub mod intersect;
pub mod measure;
pub mod mesh;
pub mod remesh;
pub mod shells;
pub mod simplify;
pub mod union;

use crate::geometry::Triangle;

/// Increment when the repair's output changes, so cached repaired exports are rebuilt.
pub const CURRENT_REPAIR_VERSION: i32 = 2;

/// The repair's remesh grid (mm) and the simplification bound of its remeshed patches.
pub const REPAIR_CELL_MM: f64 = 0.05;
pub const REPAIR_TOLERANCE_MM: f64 = 0.01;

/// A model mesh made watertight for printing ([`union::hybrid`], then [`mesh::finish`]): closed,
/// manifold and consistently oriented, the exact surface kept outside the mended spots, sealed
/// voids filled and drained cavities kept. Supports are never passed in.
pub fn repair_model(triangles: &[Triangle]) -> Vec<Triangle> {
    let pts = crate::geometry::support_reader::math::points_of(triangles);
    let (repaired, _) = union::hybrid(&pts, REPAIR_CELL_MM, REPAIR_TOLERANCE_MM);
    let (finished, _) = mesh::finish(&repaired);
    in_original_order(triangles, &finished)
}

/// The repaired faces as triangles for writing: faces kept from the original come first, in the
/// original's order and starting from its first corner, then the new ones. A boolean returns its
/// faces sorted along a space-filling curve, which scatters a mesh written row by row and
/// compresses far worse. No normal is stored (readers take it from the winding, as STL allows):
/// a computed normal is 12 bytes of noise per face.
fn in_original_order(original: &[Triangle], m: &mesh::Indexed) -> Vec<Triangle> {
    use crate::geometry::Vec3;
    let key = |v: Vec3| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()];
    let mut by_corners: rustc_hash::FxHashMap<[[u32; 3]; 3], usize> = rustc_hash::FxHashMap::default();
    for (i, t) in original.iter().enumerate() {
        let mut c = [key(t.v0), key(t.v1), key(t.v2)];
        c.sort_unstable();
        by_corners.entry(c).or_insert(i);
    }
    let mut faces: Vec<(usize, usize, [Vec3; 3])> = m
        .faces
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let v = f.map(|i| {
                let p = m.verts[i as usize];
                Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
            });
            let mut c = v.map(key);
            c.sort_unstable();
            match by_corners.get(&c) {
                Some(&i) => {
                    // The original's corner order (the same winding, so a rotation).
                    let first = key(original[i].v0);
                    let r = (0..3).find(|&r| key(v[r]) == first).unwrap_or(0);
                    (i, k, [v[r], v[(r + 1) % 3], v[(r + 2) % 3]])
                }
                None => (usize::MAX, k, v),
            }
        })
        .collect();
    faces.sort_unstable_by_key(|f| (f.0, f.1));
    faces.into_iter().map(|(_, _, v)| Triangle::new(v[0], v[1], v[2], Vec3::default())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Vec3;
    use crate::geometry::voxl::{self, VoxlMesh, VoxlPlacement};

    /// A finely tessellated sphere stored with no normals, as many STL files are.
    fn sphere(r: f32, n: usize) -> Vec<Triangle> {
        let at = |i: usize, j: usize| {
            if i == 0 || i == n {
                return Vec3::new(0.0, 0.0, if i == 0 { r } else { -r });
            }
            let (t, p) =
                (std::f32::consts::PI * i as f32 / n as f32, std::f32::consts::TAU * (j % n) as f32 / n as f32);
            Vec3::new(r * t.sin() * p.cos(), r * t.sin() * p.sin(), r * t.cos())
        };
        let mut out = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                if i > 0 {
                    out.push(Triangle::new(a, b, d, Vec3::default()));
                }
                if i + 1 < n {
                    out.push(Triangle::new(b, c, d, Vec3::default()));
                }
            }
        }
        out
    }

    fn voxl_size(triangles: &[Triangle]) -> usize {
        let mesh = VoxlMesh { name: "ball.stl", triangles, supports: None };
        let placement = VoxlPlacement { mesh: 0, rotation_z: 0.0, translation: [0.0; 3] };
        voxl::write_voxl(std::io::Cursor::new(Vec::new()), &[mesh], &[placement], None).unwrap().into_inner().len()
    }

    #[test]
    fn a_repaired_mesh_is_written_about_as_compactly_as_the_original() {
        let original = sphere(10.0, 60);
        let repaired = repair_model(&original);
        assert_eq!(repaired.len(), original.len(), "a closed sphere comes through as it is");
        assert!(repaired.iter().all(|t| t.normal == Vec3::default()), "no stored normals");
        let (before, after) = (voxl_size(&original), voxl_size(&repaired));
        assert!(after * 10 < before * 11, "{after} bytes against {before}");
    }
}
