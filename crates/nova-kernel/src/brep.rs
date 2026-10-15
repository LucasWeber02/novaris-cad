//! B-Rep (Boundary Representation) solid

use nova_geometry::Mesh;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BRep {
    pub vertices: Vec<glam::DVec3>,
    pub faces: Vec<Face>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Face {
    pub vertices: Vec<usize>, // Indices into BRep.vertices
}

impl BRep {
    /// Create an empty B-Rep
    pub fn new() -> Self {
        BRep {
            vertices: Vec::new(),
            faces: Vec::new(),
        }
    }

    /// Add a vertex
    pub fn add_vertex(&mut self, v: glam::DVec3) -> usize {
        let idx = self.vertices.len();
        self.vertices.push(v);
        idx
    }

    /// Add a face (polygon)
    pub fn add_face(&mut self, vertex_indices: Vec<usize>) {
        if vertex_indices.len() >= 3 {
            self.faces.push(Face {
                vertices: vertex_indices,
            });
        }
    }

    /// Convert to render mesh
    pub fn to_mesh(&self) -> Mesh {
        let mut indices = Vec::new();

        // Triangulate each face
        for face in &self.faces {
            if face.vertices.len() < 3 {
                continue;
            }

            // Fan triangulation: use first vertex as center
            let v0 = face.vertices[0];
            for i in 1..face.vertices.len() - 1 {
                indices.push(v0 as u32);
                indices.push(face.vertices[i] as u32);
                indices.push(face.vertices[i + 1] as u32);
            }
        }

        Mesh {
            vertices: self.vertices.clone(),
            indices,
            normals: vec![glam::DVec3::ZERO; self.vertices.len()],
        }
    }
}

impl Default for BRep {
    fn default() -> Self {
        Self::new()
    }
}

use glam;
