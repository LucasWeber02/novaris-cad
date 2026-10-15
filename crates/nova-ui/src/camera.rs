//! Orthographic orbit camera (SolidWorks' default projection)

use glam::{DMat3, DQuat, DVec2, DVec3, Mat4};
use nova_kernel::Plane;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StandardView {
    Front,
    Top,
    Right,
    Isometric,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub target: DVec3,
    /// Camera orientation: columns are screen right, screen up, towards viewer
    pub rotation: DQuat,
    /// Half of the visible height, in model units
    pub half_height: f64,
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Camera {
            target: DVec3::ZERO,
            rotation: DQuat::IDENTITY,
            half_height: 80.0,
        };
        c.set_view(StandardView::Isometric);
        c
    }
}

/// A ray in world space
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: DVec3,
    pub dir: DVec3,
}

impl Ray {
    /// Intersection with a plane, in the plane's 2D coordinates
    pub fn hit_plane(&self, plane: &Plane) -> Option<DVec2> {
        let n = plane.normal();
        let denom = self.dir.dot(n);
        if denom.abs() < 1e-12 {
            return None;
        }
        let t = (plane.origin - self.origin).dot(n) / denom;
        Some(plane.to_local(self.origin + self.dir * t))
    }

    /// Möller–Trumbore ray/triangle test, returns distance along the ray
    pub fn hit_triangle(&self, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
        let (e1, e2) = (b - a, c - a);
        let p = self.dir.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-14 {
            return None;
        }
        let inv = 1.0 / det;
        let s = self.origin - a;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = self.dir.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = e2.dot(q) * inv;
        (t > 0.0).then_some(t)
    }
}

impl Camera {
    pub fn right(&self) -> DVec3 {
        self.rotation * DVec3::X
    }

    pub fn up(&self) -> DVec3 {
        self.rotation * DVec3::Y
    }

    pub fn back(&self) -> DVec3 {
        self.rotation * DVec3::Z
    }

    pub fn set_view(&mut self, view: StandardView) {
        use std::f64::consts::FRAC_PI_2;
        self.rotation = match view {
            StandardView::Front => DQuat::IDENTITY,
            StandardView::Top => DQuat::from_rotation_x(-FRAC_PI_2),
            StandardView::Right => DQuat::from_rotation_y(FRAC_PI_2),
            StandardView::Isometric => {
                DQuat::from_rotation_y(std::f64::consts::FRAC_PI_4) * DQuat::from_rotation_x(-(1.0f64 / 3f64.sqrt()).asin())
            }
        };
    }

    /// Look straight at a plane, its sketch X axis pointing right
    pub fn normal_to(&mut self, plane: &Plane) {
        self.rotation = DQuat::from_mat3(&DMat3::from_cols(plane.x_axis, plane.y_axis, plane.normal()));
        self.target = plane.origin + plane.normal() * (self.target - plane.origin).dot(plane.normal());
    }

    /// Trackball rotation from a mouse drag (pixels)
    pub fn orbit(&mut self, delta: DVec2) {
        let k = 0.008;
        let yaw = DQuat::from_axis_angle(self.up(), -delta.x * k);
        let pitch = DQuat::from_axis_angle(self.right(), -delta.y * k);
        self.rotation = (yaw * pitch * self.rotation).normalize();
    }

    /// Pan from a mouse drag; `viewport_height` in pixels
    pub fn pan(&mut self, delta: DVec2, viewport_height: f64) {
        let units_per_px = 2.0 * self.half_height / viewport_height.max(1.0);
        self.target += (-self.right() * delta.x + self.up() * delta.y) * units_per_px;
    }

    /// Zoom by `factor` (< 1 zooms in) keeping `anchor` (NDC) fixed on screen
    pub fn zoom(&mut self, factor: f64, anchor: DVec2, aspect: f64) {
        let before = self.ndc_to_world(anchor, aspect);
        self.half_height = (self.half_height * factor).clamp(1e-3, 1e6);
        let after = self.ndc_to_world(anchor, aspect);
        self.target += before - after;
    }

    /// Frame a bounding box
    pub fn fit(&mut self, lo: DVec3, hi: DVec3, aspect: f64) {
        self.target = (lo + hi) / 2.0;
        let radius = ((hi - lo).length() / 2.0).max(1.0);
        self.half_height = radius * 1.15 / aspect.clamp(1e-3, 1.0);
    }

    fn ndc_to_world(&self, ndc: DVec2, aspect: f64) -> DVec3 {
        self.target + self.right() * ndc.x * self.half_height * aspect + self.up() * ndc.y * self.half_height
    }

    /// View-projection matrix for wgpu (depth in 0..1), `depth` = scene radius
    pub fn view_proj(&self, aspect: f64, depth: f64) -> Mat4 {
        let depth = depth.max(1.0) * 4.0;
        let eye = self.target + self.back();
        let view = glam::DMat4::look_at_rh(eye, self.target, self.up());
        let (h, w) = (self.half_height, self.half_height * aspect);
        let proj = glam::DMat4::orthographic_rh(-w, w, -h, h, -depth, depth);
        (proj * view).as_mat4()
    }

    /// Ray through a point given in NDC (-1..1, y up)
    pub fn ray(&self, ndc: DVec2, aspect: f64) -> Ray {
        Ray {
            origin: self.ndc_to_world(ndc, aspect) + self.back() * 1e6,
            dir: -self.back(),
        }
    }

    /// World point → NDC (x, y)
    pub fn project(&self, p: DVec3, aspect: f64) -> DVec2 {
        let d = p - self.target;
        DVec2::new(d.dot(self.right()) / (self.half_height * aspect), d.dot(self.up()) / self.half_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_views_look_along_axes() {
        let mut c = Camera::default();
        c.set_view(StandardView::Top);
        assert!((c.back() - DVec3::Y).length() < 1e-12);
        assert!((c.up() + DVec3::Z).length() < 1e-12);
        c.set_view(StandardView::Right);
        assert!((c.back() - DVec3::X).length() < 1e-12);
    }

    #[test]
    fn project_and_ray_agree() {
        let c = Camera { target: DVec3::new(1.0, 2.0, 3.0), ..Camera::default() };
        let p = DVec3::new(10.0, -4.0, 7.0);
        let ndc = c.project(p, 1.5);
        let ray = c.ray(ndc, 1.5);
        // the ray passes through p
        let t = (p - ray.origin).dot(ray.dir);
        assert!((ray.origin + ray.dir * t - p).length() < 1e-6);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut c = Camera::default();
        let anchor = DVec2::new(0.5, -0.3);
        let before = c.ndc_to_world(anchor, 1.3);
        c.zoom(0.5, anchor, 1.3);
        assert!((c.ndc_to_world(anchor, 1.3) - before).length() < 1e-9);
    }

    #[test]
    fn normal_to_plane_and_hit() {
        let mut c = Camera::default();
        let plane = Plane::top();
        c.normal_to(&plane);
        assert!((c.back() - plane.normal()).length() < 1e-9);
        let hit = c.ray(DVec2::ZERO, 1.0).hit_plane(&plane).unwrap();
        assert!((plane.to_world(hit) - plane.origin).dot(plane.normal()).abs() < 1e-9);
    }

    #[test]
    fn triangle_hit() {
        let r = Ray { origin: DVec3::new(0.2, 0.2, 5.0), dir: DVec3::NEG_Z };
        assert!((r.hit_triangle(DVec3::ZERO, DVec3::X, DVec3::Y).unwrap() - 5.0).abs() < 1e-12);
        let miss = Ray { origin: DVec3::new(2.0, 2.0, 5.0), dir: DVec3::NEG_Z };
        assert!(miss.hit_triangle(DVec3::ZERO, DVec3::X, DVec3::Y).is_none());
    }
}
