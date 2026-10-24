//! Camera framing and lighting shared by the CPU and GPU renderers. Matches the frontend
//! viewer (ModelViewer.tsx): Y-up, 45 degree vertical FOV, view direction (1, 0.8, -1),
//! 1.15x framing padding, ambient 0.4 plus three directional lights.

use crate::geometry::{Triangle, Vec3};

pub const FRAMING_PADDING: f32 = 1.15;
pub const FOV_Y: f32 = std::f32::consts::FRAC_PI_4;

pub fn camera_direction() -> Vec3 {
    Vec3::new(1.0, 0.8, -1.0).normalized()
}

/// (direction, intensity) pairs; directions are normalised by consumers.
pub const LIGHTS: [(Vec3, f32); 3] =
    [(Vec3::new(5.0, 8.0, 5.0), 1.20), (Vec3::new(-4.0, 2.0, -2.0), 0.40), (Vec3::new(0.0, -3.0, -5.0), 0.25)];

pub const AMBIENT: f32 = 0.40;
pub const DIFFUSE: f32 = 0.70;
pub const SPECULAR: f32 = 0.15;
pub const SHININESS: f32 = 20.0;

/// Bounding box (centre, half extents) over all triangle sets combined.
pub fn bounding_box<'a>(sets: impl IntoIterator<Item = &'a [Triangle]>) -> (Vec3, Vec3) {
    let (mut min_x, mut min_y, mut min_z) = (f32::MAX, f32::MAX, f32::MAX);
    let (mut max_x, mut max_y, mut max_z) = (f32::MIN, f32::MIN, f32::MIN);
    for set in sets {
        for t in set {
            for v in [t.v0, t.v1, t.v2] {
                min_x = min_x.min(v.x);
                max_x = max_x.max(v.x);
                min_y = min_y.min(v.y);
                max_y = max_y.max(v.y);
                min_z = min_z.min(v.z);
                max_z = max_z.max(v.z);
            }
        }
    }
    let centre = Vec3::new((min_x + max_x) * 0.5, (min_y + max_y) * 0.5, (min_z + max_z) * 0.5);
    let half = Vec3::new((max_x - min_x) * 0.5, (max_y - min_y) * 0.5, (max_z - min_z) * 0.5);
    (centre, half)
}

/// Port of calculateCameraDistanceForBox in ModelViewer.tsx: the distance at which every
/// corner of the box fits inside the frustum, with padding. `direction` points from the
/// target towards the camera.
pub fn camera_distance_for_box(half_extents: Vec3, direction: Vec3, fov_y: f32, aspect: f32) -> f32 {
    let half_vert = fov_y / 2.0;
    let half_horz = (half_vert.tan() * aspect).atan();
    let tan_vert = half_vert.tan();
    let tan_horz = half_horz.tan();

    let to_camera = direction.normalized();
    let world_up = if to_camera.dot(Vec3::UP).abs() > 0.999 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::UP };
    let right = world_up.cross(to_camera).normalized();
    let up = to_camera.cross(right).normalized();

    let mut required = 0f32;
    for sx in [-1.0f32, 1.0] {
        for sy in [-1.0f32, 1.0] {
            for sz in [-1.0f32, 1.0] {
                let corner = Vec3::new(sx * half_extents.x, sy * half_extents.y, sz * half_extents.z);
                let cx = corner.dot(right).abs();
                let cy = corner.dot(up).abs();
                let cz = corner.dot(to_camera);
                required = required.max((cz + cx / tan_horz).max(cz + cy / tan_vert));
            }
        }
    }
    required.max(0.001) * FRAMING_PADDING
}

/// Eye position and distance framing the given triangle sets.
pub fn frame<'a>(sets: impl IntoIterator<Item = &'a [Triangle]>, aspect: f32) -> (Vec3, Vec3, f32) {
    let (centre, half) = bounding_box(sets);
    let dir = camera_direction();
    let dist = camera_distance_for_box(half, dir, FOV_Y, aspect);
    (centre, centre + dir * dist, dist)
}

/// Blinn-free Phong shading used by the CPU renderer (matches the GPU fragment shader).
pub fn shade(normal: Vec3, position: Vec3, eye: Vec3, albedo: Vec3) -> Vec3 {
    let mut colour = albedo * AMBIENT;
    let view_dir = (eye - position).normalized();
    for (dir, intensity) in LIGHTS {
        let light_dir = dir.normalized();
        let diff = normal.dot(light_dir).max(0.0);
        if diff <= 0.0 {
            continue;
        }
        colour = colour + albedo * (DIFFUSE * diff * intensity);
        let reflected = (normal * (2.0 * normal.dot(light_dir)) - light_dir).normalized();
        let spec = reflected.dot(view_dir).max(0.0).powf(SHININESS);
        if spec > 0.0 {
            let s = SPECULAR * spec * intensity;
            colour = colour + Vec3::new(s, s, s);
        }
    }
    colour
}
