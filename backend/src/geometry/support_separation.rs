//! Separates a pre-supported mesh into model body and support components using
//! connected-component analysis (union-find on shared quantised edges).
//!
//! Heuristic: connected components whose triangle count is below
//! [`SUPPORT_SIZE_THRESHOLD`] x (largest component size) are classified as supports.
//! This works well for pre-supported STL/OBJ files where supports are physically
//! detached from the main body (no shared edges) or touch only at tiny break-away
//! contact points.

use rustc_hash::FxHashMap;

use super::{Triangle, Vec3};

/// Components smaller than this fraction of the largest component are supports.
const SUPPORT_SIZE_THRESHOLD: f32 = 0.20;

/// Minimum fraction of total triangles that must be classified as supports before the
/// separation is considered meaningful (avoids false positives on models with small
/// decorative detachments).
const MIN_SUPPORT_FRACTION: f32 = 0.01;

/// Vertex quantisation grid size in mm. Vertices within this distance are treated as
/// coincident, which handles floating-point rounding at shared edges.
const VERTEX_GRID_SIZE: f32 = 0.01;

type QuantizedVertex = (i32, i32, i32);

/// Partitions the triangles into model body and support geometry. `supports` is `None`
/// when no clear support components are found.
pub fn separate(triangles: &[Triangle]) -> (Vec<Triangle>, Option<Vec<Triangle>>) {
    let labels = classify(triangles);
    let Some(is_support) = labels else {
        return (triangles.to_vec(), None);
    };

    let support_count = is_support.iter().filter(|s| **s).count();
    let mut model = Vec::with_capacity(triangles.len() - support_count);
    let mut supports = Vec::with_capacity(support_count);
    for (tri, support) in triangles.iter().zip(is_support) {
        if support {
            supports.push(*tri);
        } else {
            model.push(*tri);
        }
    }
    (model, Some(supports))
}

/// Returns a per-triangle support flag, or `None` when the mesh has no meaningful
/// support components.
pub fn classify(triangles: &[Triangle]) -> Option<Vec<bool>> {
    if triangles.is_empty() {
        return None;
    }

    // Edge-based connectivity keeps point-contact tips detached from the body.
    let mut parent: Vec<u32> = (0..triangles.len() as u32).collect();
    let mut edge_owner: FxHashMap<(QuantizedVertex, QuantizedVertex), u32> = FxHashMap::default();
    edge_owner.reserve(triangles.len() * 3 / 2);

    for (i, tri) in triangles.iter().enumerate() {
        let i = i as u32;
        let q0 = quantize(tri.v0);
        let q1 = quantize(tri.v1);
        let q2 = quantize(tri.v2);
        for edge in [ordered(q0, q1), ordered(q1, q2), ordered(q2, q0)] {
            let owner = *edge_owner.entry(edge).or_insert(i);
            union(&mut parent, i, owner);
        }
    }

    let mut component_size = vec![0u32; triangles.len()];
    let mut roots = Vec::with_capacity(triangles.len());
    for i in 0..triangles.len() as u32 {
        let root = find(&mut parent, i);
        component_size[root as usize] += 1;
        roots.push(root);
    }

    let max_size = component_size.iter().copied().max().unwrap_or(0);
    let size_threshold = (max_size as f32 * SUPPORT_SIZE_THRESHOLD) as u32;

    let is_support: Vec<bool> = roots.iter().map(|r| component_size[*r as usize] < size_threshold).collect();
    let support_count = is_support.iter().filter(|s| **s).count();

    // Discard if the support fraction is negligible (likely not a supported model).
    if (support_count as f32) < triangles.len() as f32 * MIN_SUPPORT_FRACTION {
        return None;
    }

    Some(is_support)
}

fn quantize(v: Vec3) -> QuantizedVertex {
    (
        (v.x / VERTEX_GRID_SIZE).round_ties_even() as i32,
        (v.y / VERTEX_GRID_SIZE).round_ties_even() as i32,
        (v.z / VERTEX_GRID_SIZE).round_ties_even() as i32,
    )
}

fn ordered(a: QuantizedVertex, b: QuantizedVertex) -> (QuantizedVertex, QuantizedVertex) {
    if a <= b { (a, b) } else { (b, a) }
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize]; // path halving
        x = parent[x as usize];
    }
    x
}

fn union(parent: &mut [u32], a: u32, b: u32) {
    let a = find(parent, a);
    let b = find(parent, b);
    if a != b {
        parent[b as usize] = a;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x_offset: f32, z_offset: f32) -> Vec<Triangle> {
        let v0 = Vec3::new(x_offset, 0.0, z_offset);
        let v1 = Vec3::new(x_offset + 1.0, 0.0, z_offset);
        let v2 = Vec3::new(x_offset + 1.0, 0.0, z_offset + 1.0);
        let v3 = Vec3::new(x_offset, 0.0, z_offset + 1.0);
        vec![Triangle::new(v0, v1, v2, Vec3::UP), Triangle::new(v0, v2, v3, Vec3::UP)]
    }

    fn body_and_supports(body_squares: usize, support_squares: usize) -> Vec<Triangle> {
        let mut triangles = Vec::new();
        for i in 0..body_squares {
            triangles.extend(square(i as f32, 0.0));
        }
        for i in 0..support_squares {
            triangles.extend(square(i as f32, 1000.0));
        }
        triangles
    }

    #[test]
    fn empty_input_returns_empty_model_and_no_supports() {
        let (model, supports) = separate(&[]);
        assert!(model.is_empty());
        assert!(supports.is_none());
    }

    #[test]
    fn single_connected_component_is_all_model() {
        let (model, supports) = separate(&square(0.0, 0.0));
        assert_eq!(model.len(), 2);
        assert!(supports.is_none());
    }

    #[test]
    fn large_connected_mesh_is_all_model() {
        let (model, supports) = separate(&body_and_supports(50, 0));
        assert_eq!(model.len(), 100);
        assert!(supports.is_none());
    }

    #[test]
    fn small_detached_component_is_classified_as_supports() {
        let (model, supports) = separate(&body_and_supports(20, 1));
        assert_eq!(supports.expect("supports").len(), 2);
        assert_eq!(model.len(), 40);
    }

    #[test]
    fn support_below_min_fraction_returns_no_supports() {
        let (_, supports) = separate(&body_and_supports(200, 1));
        assert!(supports.is_none());
    }

    #[test]
    fn two_equal_components_are_not_supports() {
        let (_, supports) = separate(&body_and_supports(10, 10));
        assert!(supports.is_none());
    }

    #[test]
    fn multiple_small_supports_are_all_classified() {
        let mut triangles: Vec<Triangle> = (0..30).flat_map(|i| square(i as f32, 0.0)).collect();
        triangles.extend(square(0.0, 1000.0));
        triangles.extend(square(0.0, 2000.0));
        triangles.extend(square(0.0, 3000.0));

        let (model, supports) = separate(&triangles);
        assert_eq!(supports.expect("supports").len(), 6);
        assert_eq!(model.len(), 60);
    }

    #[test]
    fn point_contact_support_tip_is_classified_as_support() {
        let mut triangles: Vec<Triangle> = (0..20).flat_map(|i| square(i as f32, 0.0)).collect();
        triangles.push(Triangle::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(-0.2, 0.0, 0.0),
            Vec3::new(0.0, 0.2, 0.0),
            Vec3::UP,
        ));
        triangles.push(Triangle::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.2, 0.0),
            Vec3::new(0.0, 0.0, -0.2),
            Vec3::UP,
        ));

        let (model, supports) = separate(&triangles);
        assert_eq!(supports.expect("supports").len(), 2);
        assert_eq!(model.len(), 40);
    }

    #[test]
    fn shared_edge_within_quantisation_tolerance_is_connected() {
        let triangles = vec![
            Triangle::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), Vec3::UP),
            Triangle::new(Vec3::new(0.0, 0.0, 1.001), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0), Vec3::UP),
        ];
        let (model, supports) = separate(&triangles);
        assert_eq!(model.len(), 2);
        assert!(supports.is_none());
    }
}
