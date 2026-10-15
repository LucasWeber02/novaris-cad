//! Basic mathematical types for NOVARIS CAD

use serde::{Deserialize, Serialize};

/// 3D vector with f64 precision
pub type Vector3 = glam::DVec3;

/// 3D point with f64 precision
pub type Point3 = glam::DVec3;

/// 4x4 transformation matrix
pub type Matrix4 = glam::DMat4;

/// A general 3D transformation (translation, rotation, scale)
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Transform {
    pub matrix: Matrix4,
}

impl Transform {
    /// Identity transform
    pub fn identity() -> Self {
        Transform {
            matrix: Matrix4::IDENTITY,
        }
    }

    /// Translation only
    pub fn translation(x: f64, y: f64, z: f64) -> Self {
        Transform {
            matrix: Matrix4::from_translation(Vector3::new(x, y, z)),
        }
    }

    /// Get position
    pub fn position(&self) -> Vector3 {
        self.matrix.w_axis.truncate()
    }

    /// Apply to point
    pub fn transform_point(&self, p: Point3) -> Point3 {
        self.matrix.transform_point3(p)
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

/// Tolerance for geometric comparisons (mm)
pub const GEOMETRIC_TOLERANCE: f64 = 1e-6;
