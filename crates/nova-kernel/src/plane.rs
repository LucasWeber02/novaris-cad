//! Sketch planes: a 2D frame placed in 3D space

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

/// A plane with an orthonormal frame. Sketch coordinates `(u, v)` map to
/// `origin + u * x_axis + v * y_axis`; `normal = x_axis × y_axis`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub origin: DVec3,
    pub x_axis: DVec3,
    pub y_axis: DVec3,
}

impl Plane {
    /// Build a plane from an origin, an in-plane X direction and a normal
    pub fn new(origin: DVec3, x_axis: DVec3, normal: DVec3) -> Self {
        let normal = normal.normalize();
        // Remove any normal component so the frame stays orthonormal
        let x_axis = (x_axis - normal * x_axis.dot(normal)).normalize();
        Plane {
            origin,
            x_axis,
            y_axis: normal.cross(x_axis),
        }
    }

    /// Front plane (XY), normal +Z — same convention as SolidWorks
    pub fn front() -> Self {
        Plane::new(DVec3::ZERO, DVec3::X, DVec3::Z)
    }

    /// Top plane (XZ), normal +Y, sketch Y pointing to -Z
    pub fn top() -> Self {
        Plane::new(DVec3::ZERO, DVec3::X, DVec3::Y)
    }

    /// Right plane (YZ), normal +X, sketch X pointing to -Z
    pub fn right() -> Self {
        Plane::new(DVec3::ZERO, DVec3::NEG_Z, DVec3::X)
    }

    pub fn normal(&self) -> DVec3 {
        self.x_axis.cross(self.y_axis)
    }

    /// Sketch coordinates → world point
    pub fn to_world(&self, p: DVec2) -> DVec3 {
        self.origin + self.x_axis * p.x + self.y_axis * p.y
    }

    /// World point → sketch coordinates (projected onto the plane)
    pub fn to_local(&self, p: DVec3) -> DVec2 {
        let d = p - self.origin;
        DVec2::new(d.dot(self.x_axis), d.dot(self.y_axis))
    }

    /// Same plane moved along its normal
    pub fn offset(&self, distance: f64) -> Self {
        Plane {
            origin: self.origin + self.normal() * distance,
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_planes_are_right_handed() {
        assert!((Plane::front().normal() - DVec3::Z).length() < 1e-12);
        assert!((Plane::top().normal() - DVec3::Y).length() < 1e-12);
        assert!((Plane::right().normal() - DVec3::X).length() < 1e-12);
        assert!((Plane::top().y_axis - DVec3::NEG_Z).length() < 1e-12);
    }

    #[test]
    fn local_world_roundtrip() {
        let plane = Plane::new(DVec3::new(1.0, 2.0, 3.0), DVec3::new(1.0, 1.0, 0.0), DVec3::new(0.0, 0.0, 2.0));
        let p = DVec2::new(4.0, -7.5);
        assert!((plane.to_local(plane.to_world(p)) - p).length() < 1e-12);
    }
}
