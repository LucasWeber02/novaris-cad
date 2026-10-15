//! Geometry kernel abstraction layer

pub mod mesh;

pub use mesh::*;

/// B-Rep solid representation (stub for now)
pub struct BRep {
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    pub vertices: Vec<Vertex>,
}

pub struct Face {
    pub id: u32,
}

pub struct Edge {
    pub id: u32,
}

pub struct Vertex {
    pub id: u32,
    pub position: glam::DVec3,
}
