//! Basic geometry utilities

use nova_core::{Vector3, Point3};

/// Calculate the length of a vector
pub fn length(v: Vector3) -> f64 {
    v.length()
}

/// Normalize a vector
pub fn normalize(v: Vector3) -> Vector3 {
    v.normalize()
}

/// Dot product
pub fn dot(a: Vector3, b: Vector3) -> f64 {
    a.dot(b)
}

/// Cross product
pub fn cross(a: Vector3, b: Vector3) -> Vector3 {
    a.cross(b)
}

/// Distance between two points
pub fn distance(a: Point3, b: Point3) -> f64 {
    (b - a).length()
}
