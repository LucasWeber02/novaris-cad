//! Mathematical utilities for NOVARIS CAD

pub mod geometry;

pub use geometry::*;

pub const EPSILON: f64 = 1e-10;

/// 3D vector (double precision)
pub type Vec3 = glam::DVec3;
/// 4x4 transform matrix (double precision)
pub type Matrix4 = glam::DMat4;
