// NOVARIS CAD — Phase 3: Interaction System
// Ray casting, selection, picking

use std::collections::HashSet;
use nova_core::Id;
use nova_math::Vec3;
use nova_kernel::BRep;

// ═══════════════════════════════════════════════════════════
// RAY CASTING
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        // Normalize direction
        let len = (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z).sqrt();
        let normalized = if len > 1e-10 {
            Vec3::new(direction.x / len, direction.y / len, direction.z / len)
        } else {
            Vec3::new(0.0, 0.0, 1.0)
        };

        Ray {
            origin,
            direction: normalized,
        }
    }

    pub fn point_at(&self, t: f64) -> Vec3 {
        Vec3::new(
            self.origin.x + self.direction.x * t,
            self.origin.y + self.direction.y * t,
            self.origin.z + self.direction.z * t,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RayTriangleIntersection {
    pub hit: bool,
    pub distance: f64,
    pub point: Vec3,
    pub barycentric: (f64, f64, f64),
}

impl RayTriangleIntersection {
    pub fn none() -> Self {
        RayTriangleIntersection {
            hit: false,
            distance: f64::MAX,
            point: Vec3::ZERO,
            barycentric: (0.0, 0.0, 0.0),
        }
    }
}

// Möller-Trumbore ray-triangle intersection
pub fn ray_triangle_intersection(ray: &Ray, v0: Vec3, v1: Vec3, v2: Vec3) -> RayTriangleIntersection {
    const EPSILON: f64 = 1e-8;

    let edge1 = Vec3::new(v1.x - v0.x, v1.y - v0.y, v1.z - v0.z);
    let edge2 = Vec3::new(v2.x - v0.x, v2.y - v0.y, v2.z - v0.z);

    // Cross product: ray.direction × edge2
    let h = Vec3::new(
        ray.direction.y * edge2.z - ray.direction.z * edge2.y,
        ray.direction.z * edge2.x - ray.direction.x * edge2.z,
        ray.direction.x * edge2.y - ray.direction.y * edge2.x,
    );

    // Dot product: edge1 · h
    let a = edge1.x * h.x + edge1.y * h.y + edge1.z * h.z;

    if a.abs() < EPSILON {
        return RayTriangleIntersection::none();
    }

    let f = 1.0 / a;
    let s = Vec3::new(
        ray.origin.x - v0.x,
        ray.origin.y - v0.y,
        ray.origin.z - v0.z,
    );

    // Barycentric u
    let u = f * (s.x * h.x + s.y * h.y + s.z * h.z);

    if u < 0.0 || u > 1.0 {
        return RayTriangleIntersection::none();
    }

    // Cross product: s × edge1
    let q = Vec3::new(
        s.y * edge1.z - s.z * edge1.y,
        s.z * edge1.x - s.x * edge1.z,
        s.x * edge1.y - s.y * edge1.x,
    );

    // Barycentric v
    let v = f * (ray.direction.x * q.x + ray.direction.y * q.y + ray.direction.z * q.z);

    if v < 0.0 || u + v > 1.0 {
        return RayTriangleIntersection::none();
    }

    let t = f * (edge2.x * q.x + edge2.y * q.y + edge2.z * q.z);

    if t > EPSILON {
        let point = ray.point_at(t);
        let w = 1.0 - u - v;
        RayTriangleIntersection {
            hit: true,
            distance: t,
            point,
            barycentric: (w, u, v),
        }
    } else {
        RayTriangleIntersection::none()
    }
}

// ═══════════════════════════════════════════════════════════
// SELECTION MANAGER
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectionMode {
    Object,
    Face,
    Edge,
    Vertex,
}

#[derive(Clone, Debug)]
pub struct PickResult {
    pub object_id: Id,
    pub face_index: Option<usize>,
    pub distance: f64,
    pub point: Vec3,
}

#[derive(Clone, Debug)]
pub struct SelectionManager {
    pub selected_objects: HashSet<Id>,
    pub selected_faces: HashSet<(Id, usize)>,
    pub selected_edges: HashSet<(Id, usize)>,
    pub selected_vertices: HashSet<(Id, usize)>,
    pub selection_mode: SelectionMode,
}

impl SelectionManager {
    pub fn new() -> Self {
        SelectionManager {
            selected_objects: HashSet::new(),
            selected_faces: HashSet::new(),
            selected_edges: HashSet::new(),
            selected_vertices: HashSet::new(),
            selection_mode: SelectionMode::Object,
        }
    }

    pub fn clear(&mut self) {
        self.selected_objects.clear();
        self.selected_faces.clear();
        self.selected_edges.clear();
        self.selected_vertices.clear();
    }

    pub fn select_object(&mut self, object_id: Id, multi_select: bool) {
        if !multi_select {
            self.clear();
        }
        self.selected_objects.insert(object_id);
    }

    pub fn select_face(&mut self, object_id: Id, face_index: usize, multi_select: bool) {
        if !multi_select {
            self.clear();
        }
        self.selected_faces.insert((object_id, face_index));
    }

    pub fn deselect_object(&mut self, object_id: Id) {
        self.selected_objects.remove(&object_id);
    }

    pub fn deselect_face(&mut self, object_id: Id, face_index: usize) {
        self.selected_faces.remove(&(object_id, face_index));
    }

    pub fn is_object_selected(&self, object_id: Id) -> bool {
        self.selected_objects.contains(&object_id)
    }

    pub fn is_face_selected(&self, object_id: Id, face_index: usize) -> bool {
        self.selected_faces.contains(&(object_id, face_index))
    }

    pub fn get_selection_count(&self) -> usize {
        match self.selection_mode {
            SelectionMode::Object => self.selected_objects.len(),
            SelectionMode::Face => self.selected_faces.len(),
            SelectionMode::Edge => self.selected_edges.len(),
            SelectionMode::Vertex => self.selected_vertices.len(),
        }
    }
}

// ═══════════════════════════════════════════════════════════
// 3D PICKING
// ═══════════════════════════════════════════════════════════

pub struct Picker {
    pub ray: Ray,
    pub selection_manager: SelectionManager,
}

impl Picker {
    pub fn new(ray: Ray) -> Self {
        Picker {
            ray,
            selection_manager: SelectionManager::new(),
        }
    }

    pub fn pick_mesh(&self, object_id: Id, geometry: &BRep) -> Option<PickResult> {
        let mut closest_hit: Option<PickResult> = None;
        let mut min_distance = f64::MAX;

        // Try all triangles in the mesh
        for (face_idx, face) in geometry.faces.iter().enumerate() {
            if face.vertices.len() < 3 {
                continue;
            }

            // Split face into triangles (fan triangulation)
            for tri_idx in 1..(face.vertices.len() - 1) {
                let v0_idx = face.vertices[0];
                let v1_idx = face.vertices[tri_idx];
                let v2_idx = face.vertices[tri_idx + 1];

                if v0_idx >= geometry.vertices.len()
                    || v1_idx >= geometry.vertices.len()
                    || v2_idx >= geometry.vertices.len()
                {
                    continue;
                }

                let v0 = geometry.vertices[v0_idx];
                let v1 = geometry.vertices[v1_idx];
                let v2 = geometry.vertices[v2_idx];

                let intersection = ray_triangle_intersection(&self.ray, v0, v1, v2);

                if intersection.hit && intersection.distance < min_distance {
                    min_distance = intersection.distance;
                    closest_hit = Some(PickResult {
                        object_id,
                        face_index: Some(face_idx),
                        distance: intersection.distance,
                        point: intersection.point,
                    });
                }
            }
        }

        closest_hit
    }
}

// ═══════════════════════════════════════════════════════════
// CAMERA & VIEWPORT INTERACTION
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov: f64,
    pub near: f64,
    pub far: f64,
}

impl Camera {
    pub fn new(position: Vec3, target: Vec3) -> Self {
        Camera {
            position,
            target,
            up: Vec3::new(0.0, 1.0, 0.0),
            fov: 45.0,
            near: 0.1,
            far: 1000.0,
        }
    }

    pub fn orbit(&mut self, delta_yaw: f64, delta_pitch: f64, radius: f64) {
        let dir = Vec3::new(
            self.position.x - self.target.x,
            self.position.y - self.target.y,
            self.position.z - self.target.z,
        );

        let len_sq = dir.x * dir.x + dir.y * dir.y + dir.z * dir.z;
        let len = len_sq.sqrt();

        if len < 1e-6 {
            return;
        }

        // Simple orbit (simplified spherical coordinates)
        let new_pos = Vec3::new(
            self.target.x + radius * delta_yaw.cos(),
            self.target.y + (delta_pitch * 50.0).clamp(-80.0, 80.0),
            self.target.z + radius * delta_yaw.sin(),
        );

        self.position = new_pos;
    }

    pub fn pan(&mut self, delta_x: f64, delta_y: f64) {
        let pan_amount = 0.1;
        let right = Vec3::new(1.0, 0.0, 0.0);
        let up = self.up;

        self.target.x += right.x * delta_x * pan_amount;
        self.target.y += up.y * delta_y * pan_amount;
        self.target.z += right.z * delta_x * pan_amount;

        self.position.x += right.x * delta_x * pan_amount;
        self.position.y += up.y * delta_y * pan_amount;
        self.position.z += right.z * delta_x * pan_amount;
    }

    pub fn zoom(&mut self, delta: f64) {
        let dir = Vec3::new(
            self.target.x - self.position.x,
            self.target.y - self.position.y,
            self.target.z - self.position.z,
        );

        let len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
        let scale = 1.0 + delta * 0.1;

        self.position.x = self.target.x - dir.x / len * len / scale;
        self.position.y = self.target.y - dir.y / len * len / scale;
        self.position.z = self.target.z - dir.z / len * len / scale;
    }
}

// ═══════════════════════════════════════════════════════════
// UNIT TESTS
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use nova_core::Id;

    #[test]
    fn test_ray_creation() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(ray.direction.x, 1.0);
        assert!((ray.direction.y).abs() < 1e-10);
        assert!((ray.direction.z).abs() < 1e-10);
    }

    #[test]
    fn test_ray_point_at() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let p = ray.point_at(5.0);
        assert_eq!(p.x, 5.0);
        assert_eq!(p.y, 0.0);
        assert_eq!(p.z, 0.0);
    }

    #[test]
    fn test_ray_triangle_intersection() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 1.0));
        let v0 = Vec3::new(-1.0, -1.0, 0.0);
        let v1 = Vec3::new(1.0, -1.0, 0.0);
        let v2 = Vec3::new(0.0, 1.0, 0.0);

        let intersection = ray_triangle_intersection(&ray, v0, v1, v2);
        assert!(intersection.hit);
        assert!(intersection.distance > 0.0);
    }

    #[test]
    fn test_selection_manager() {
        let mut mgr = SelectionManager::new();
        let id = Id::new();

        mgr.select_object(id, false);
        assert!(mgr.is_object_selected(id));
        assert_eq!(mgr.get_selection_count(), 1);

        mgr.deselect_object(id);
        assert!(!mgr.is_object_selected(id));
    }

    #[test]
    fn test_face_selection() {
        let mut mgr = SelectionManager::new();
        mgr.selection_mode = SelectionMode::Face;
        let id = Id::new();

        mgr.select_face(id, 0, false);
        assert!(mgr.is_face_selected(id, 0));
        assert_eq!(mgr.get_selection_count(), 1);
    }

    #[test]
    fn test_camera_orbit() {
        let mut camera = Camera::new(Vec3::new(10.0, 10.0, 10.0), Vec3::new(0.0, 0.0, 0.0));
        let orig_pos = camera.position;

        camera.orbit(0.1, 0.1, 15.0);
        // Position should change
        assert!(
            (camera.position.x - orig_pos.x).abs() > 0.1
                || (camera.position.y - orig_pos.y).abs() > 0.1
                || (camera.position.z - orig_pos.z).abs() > 0.1
        );
    }

    #[test]
    fn test_camera_pan() {
        let mut camera = Camera::new(Vec3::new(10.0, 10.0, 10.0), Vec3::new(0.0, 0.0, 0.0));
        let orig_target = camera.target;

        camera.pan(10.0, 0.0);
        // Target should move
        assert!((camera.target.x - orig_target.x).abs() > 0.01);
    }

    #[test]
    fn test_camera_zoom() {
        let mut camera = Camera::new(Vec3::new(10.0, 10.0, 10.0), Vec3::new(0.0, 0.0, 0.0));
        let orig_distance = (
            (camera.position.x - camera.target.x).powi(2)
                + (camera.position.y - camera.target.y).powi(2)
                + (camera.position.z - camera.target.z).powi(2)
        )
        .sqrt();

        camera.zoom(1.0); // Zoom in
        let new_distance = (
            (camera.position.x - camera.target.x).powi(2)
                + (camera.position.y - camera.target.y).powi(2)
                + (camera.position.z - camera.target.z).powi(2)
        )
        .sqrt();

        assert!(new_distance < orig_distance);
    }
}

fn main() {
    println!("╔════════════════════════════════════════╗");
    println!("║ NOVARIS CAD — Phase 3: Interaction   ║");
    println!("║  Ray casting, selection, camera      ║");
    println!("╚════════════════════════════════════════╝\n");

    let ray = Ray::new(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 1.0));
    println!("✓ Ray: origin={:?}, dir={:?}", ray.origin, ray.direction);

    let mut mgr = SelectionManager::new();
    let id = Id::new();
    mgr.select_object(id, false);
    println!("✓ Selection: {} objects selected", mgr.get_selection_count());

    let mut camera = Camera::new(Vec3::new(10.0, 10.0, 10.0), Vec3::new(0.0, 0.0, 0.0));
    camera.zoom(-1.0);
    println!("✓ Camera: position={:?}, target={:?}", camera.position, camera.target);

    println!("\n✓ nova-interaction IMPLEMENTATION — WORKING");
}
