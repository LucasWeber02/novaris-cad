//! NOVARIS CAD — Geometry Kernel Layer
//!
//! Pure Rust geometry kernel implementation.
//! Handles B-Rep solids, extrusion, and tessellation.

pub mod extrusion;
pub mod brep;
pub mod plane;
pub mod profile;
pub mod solid;

pub use extrusion::extrude_profile;
pub use brep::BRep;
pub use plane::Plane;
pub use profile::{ArcGeom, Loop, Profile, Segment};
pub use solid::{mirror, rotation_about, translation, BooleanOp, Prism, RenderMesh, Revolve, Solid, Tool, ToolShape};

use nova_core::Result;
use nova_geometry::Mesh;

impl BRep {
    /// Tessellate to a mesh for rendering
    pub fn tessellate(&self) -> Result<Mesh> {
        Ok(self.to_mesh())
    }

    /// Calculate volume using divergence theorem
    /// Volume = 1/6 * sum of signed volumes of tetrahedra
    pub fn volume(&self) -> f64 {
        let mut volume = 0.0;

        // For each face, compute contribution to volume
        for face in &self.faces {
            if face.vertices.len() < 3 {
                continue;
            }

            // Use first vertex as reference for tetrahedra
            let p0 = self.vertices[face.vertices[0]];
            for i in 1..face.vertices.len() - 1 {
                let p1 = self.vertices[face.vertices[i]];
                let p2 = self.vertices[face.vertices[i + 1]];

                // Signed volume of tetrahedron
                let v1 = p1 - p0;
                let v2 = p2 - p0;
                let cross = v1.cross(v2);
                volume += p0.dot(cross) / 6.0;
            }
        }

        volume.abs()
    }

    /// Get bounding box
    pub fn bounding_box(&self) -> ([f64; 3], [f64; 3]) {
        if self.vertices.is_empty() {
            return ([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        }

        let mut min = self.vertices[0];
        let mut max = self.vertices[0];

        for v in &self.vertices {
            min.x = min.x.min(v.x);
            min.y = min.y.min(v.y);
            min.z = min.z.min(v.z);

            max.x = max.x.max(v.x);
            max.y = max.y.max(v.y);
            max.z = max.z.max(v.z);
        }

        ([min.x, min.y, min.z], [max.x, max.y, max.z])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extrusion_volume() {
        // Create a rectangle 100 × 60 mm
        let profile = vec![
            [0.0, 0.0],
            [100.0, 0.0],
            [100.0, 60.0],
            [0.0, 60.0],
        ];

        // Extrude by 20 mm
        let brep = extrude_profile(&profile, 20.0).expect("Extrusion failed");

        // Verify volume: 100 × 60 × 20 = 120,000 mm³
        let vol = brep.volume();
        println!("Extrusion volume: {} mm³", vol);
        assert!(
            (vol - 120_000.0).abs() < 1.0,
            "Volume mismatch: expected 120000, got {}",
            vol
        );
    }

    #[test]
    fn test_bounding_box() {
        let profile = vec![
            [10.0, 20.0],
            [110.0, 20.0],
            [110.0, 80.0],
            [10.0, 80.0],
        ];

        let brep = extrude_profile(&profile, 30.0).expect("Extrusion failed");
        let (min, max) = brep.bounding_box();

        println!("Bounding box: {:?} to {:?}", min, max);
        assert!((min[0] - 10.0).abs() < 0.1);
        assert!((max[0] - 110.0).abs() < 0.1);
        assert!((max[2] - 30.0).abs() < 0.1);
    }
}
