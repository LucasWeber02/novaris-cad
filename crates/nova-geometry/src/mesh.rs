//! Triangle mesh representation for rendering

use nova_core::Point3;

/// Tessellated mesh for display
#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<Point3>,
    pub indices: Vec<u32>,
    pub normals: Vec<glam::DVec3>,
}

impl Mesh {
    /// Create an empty mesh
    pub fn new() -> Self {
        Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            normals: Vec::new(),
        }
    }

    /// Create a simple cube mesh
    pub fn cube(size: f64) -> Self {
        let s = size / 2.0;
        let vertices = vec![
            Point3::new(-s, -s, -s), Point3::new(s, -s, -s),
            Point3::new(s, s, -s),   Point3::new(-s, s, -s),
            Point3::new(-s, -s, s),  Point3::new(s, -s, s),
            Point3::new(s, s, s),    Point3::new(-s, s, s),
        ];

        let indices = vec![
            0, 1, 2, 2, 3, 0, // front
            4, 6, 5, 4, 7, 6, // back
            0, 4, 5, 5, 1, 0, // bottom
            2, 6, 7, 7, 3, 2, // top
            0, 3, 7, 7, 4, 0, // left
            1, 5, 6, 6, 2, 1, // right
        ];

        Mesh {
            vertices,
            indices,
            normals: vec![glam::DVec3::ZERO; 8],
        }
    }
}

impl Default for Mesh {
    fn default() -> Self {
        Self::new()
    }
}

impl Mesh {
    pub fn empty() -> Self {
        Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            normals: Vec::new(),
        }
    }
}
