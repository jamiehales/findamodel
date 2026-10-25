//! Mesh geometry primitives and processing: loading, encoding, hulls, support separation
//! and export formats.

pub mod hull;
pub mod loader;
pub mod mesh_transfer;
pub mod saver;
pub mod support_separation;
pub mod voxl;

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const UP: Vec3 = Vec3 { x: 0.0, y: 1.0, z: 0.0 };

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, b: Vec3) -> f32 {
        self.x * b.x + self.y * b.y + self.z * b.z
    }

    pub fn cross(self, b: Vec3) -> Vec3 {
        Vec3::new(self.y * b.z - self.z * b.y, self.z * b.x - self.x * b.z, self.x * b.y - self.y * b.x)
    }

    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn length(self) -> f32 {
        self.length_sq().sqrt()
    }

    /// Unit vector in the same direction; degenerate vectors normalise to `UP`.
    pub fn normalized(self) -> Vec3 {
        let l = self.length();
        if l < 1e-8 { Vec3::UP } else { self * (1.0 / l) }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, b: Vec3) -> Vec3 {
        Vec3::new(self.x + b.x, self.y + b.y, self.z + b.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, b: Vec3) -> Vec3 {
        Vec3::new(self.x - b.x, self.y - b.y, self.z - b.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

impl std::fmt::Display for Vec3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:.3}, {:.3}, {:.3})", self.x, self.y, self.z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Triangle {
    pub v0: Vec3,
    pub v1: Vec3,
    pub v2: Vec3,
    pub normal: Vec3,
}

impl Triangle {
    pub const fn new(v0: Vec3, v1: Vec3, v2: Vec3, normal: Vec3) -> Self {
        Self { v0, v1, v2, normal }
    }

    /// Normal used for preview shading: the geometric face normal, falling back to the
    /// stored normal for degenerate triangles.
    pub fn preview_normal(&self) -> Vec3 {
        let face = (self.v1 - self.v0).cross(self.v2 - self.v0);
        if face.length_sq() >= 1e-12 { face.normalized() } else { self.normal.normalized() }
    }
}

/// Result of loading a model file.
///
/// Coordinate system: Y-up (Z-up source files are rotated on load).
/// Units: millimetres (STL/OBJ carry no unit metadata; scale factor = 1.0).
/// Origin: X/Z centred at 0, base face (minimum Y) sitting at Y = 0.
#[derive(Debug, Clone)]
pub struct LoadedGeometry {
    pub triangles: Vec<Triangle>,
    /// Centre of the bounding sphere (= AABB centre = (0, dimY/2, 0)).
    pub sphere_centre: Vec3,
    /// Radius of the bounding sphere in mm (max vertex distance from `sphere_centre`).
    pub sphere_radius: f32,
    pub dimension_x_mm: f32,
    pub dimension_y_mm: f32,
    pub dimension_z_mm: f32,
}

impl LoadedGeometry {
    /// Copy of the bounds metadata with a different triangle set.
    pub fn with_triangles(&self, triangles: Vec<Triangle>) -> LoadedGeometry {
        LoadedGeometry {
            triangles,
            sphere_centre: self.sphere_centre,
            sphere_radius: self.sphere_radius,
            dimension_x_mm: self.dimension_x_mm,
            dimension_y_mm: self.dimension_y_mm,
            dimension_z_mm: self.dimension_z_mm,
        }
    }
}

/// Converts between the Z-up convention of CAD/slicer files and the Y-up convention used
/// internally. The mapping `(x, y, z) -> (-x, z, y)` is its own inverse.
pub fn swap_up_axis(v: Vec3) -> Vec3 {
    Vec3::new(-v.x, v.z, v.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_normal_prefers_face_normal_over_stored_triangle_normal() {
        let tri = Triangle::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let n = tri.preview_normal();
        assert!((n.x - 0.0).abs() < 1e-4);
        assert!((n.y - 0.0).abs() < 1e-4);
        assert!((n.z - 1.0).abs() < 1e-4);
    }

    #[test]
    fn preview_normal_falls_back_to_stored_normal_for_degenerate_triangles() {
        let tri = Triangle::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        );
        let n = tri.preview_normal();
        assert!((n.x - 0.0).abs() < 1e-4);
        assert!((n.y - 0.0).abs() < 1e-4);
        assert!((n.z - 1.0).abs() < 1e-4);
    }

    #[test]
    fn vec3_display_uses_three_decimals() {
        assert_eq!(Vec3::new(1.0, 2.5, -3.25).to_string(), "(1.000, 2.500, -3.250)");
    }
}
