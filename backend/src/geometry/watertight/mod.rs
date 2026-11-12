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
pub const CURRENT_REPAIR_VERSION: i32 = 1;

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
    finished
        .tris()
        .iter()
        .map(|t| {
            let v = t.map(|p| crate::geometry::Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32));
            Triangle::new(v[0], v[1], v[2], (v[1] - v[0]).cross(v[2] - v[0]).normalized())
        })
        .collect()
}
