//! Extrusion feature implementation

use crate::{FeatureOp, FeatureContext};
use nova_core::{Id, Result};
use nova_kernel::BRep;
use nova_sketch::Sketch;
use serde::{Deserialize, Serialize};

/// Extrusion feature: takes a 2D sketch and creates a 3D solid
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtrusionFeature {
    pub sketch_id: Id,
    pub length: f64,
    pub direction: ExtrDir,
}

#[derive(Clone, Debug, Serialize, Deserialize, Copy, PartialEq)]
pub enum ExtrDir {
    Forward,
    Backward,
    Symmetric,
}

impl ExtrusionFeature {
    pub fn new(sketch_id: Id, length: f64) -> Self {
        ExtrusionFeature {
            sketch_id,
            length,
            direction: ExtrDir::Forward,
        }
    }

    /// Evaluate: convert sketch to solid
    pub fn evaluate_sketch(&self, sketch: &Sketch) -> Result<BRep> {
        // Extract 2D profile from sketch
        let profile = self.extract_profile(sketch)?;

        // Use kernel to extrude
        nova_kernel::extrude_profile(&profile, self.length)
    }

    /// Extract 2D points from sketch geometry
    fn extract_profile(&self, sketch: &Sketch) -> Result<Vec<[f64; 2]>> {
        let mut points = Vec::new();

        // For now, extract all points from the sketch
        for (_, elem) in &sketch.elements {
            if let nova_sketch::SketchElement::Point(p) = elem {
                points.push([p.x, p.y]);
            }
        }

        if points.len() < 3 {
            return Err(nova_core::NovaError::InvalidSketch(
                "Sketch must have at least 3 points".into(),
            ));
        }

        // Sort points counterclockwise to form a closed profile
        // (Simplified for now; real implementation would compute convex hull)
        points.sort_by(|a, b| {
            let angle_a = a[1].atan2(a[0]);
            let angle_b = b[1].atan2(b[0]);
            angle_a.partial_cmp(&angle_b).unwrap()
        });

        Ok(points)
    }
}

impl FeatureOp for ExtrusionFeature {
    fn evaluate(&self, context: &FeatureContext) -> Result<BRep> {
        // Get the sketch from context
        let sketch = context.get_sketch(self.sketch_id)?;
        self.evaluate_sketch(&sketch)
    }

    fn feature_type(&self) -> &str {
        "Extrusion"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extrusion_creation() {
        let sketch_id = Id::new();
        let extr = ExtrusionFeature::new(sketch_id, 20.0);

        assert_eq!(extr.length, 20.0);
        assert_eq!(extr.direction, ExtrDir::Forward);
    }

    #[test]
    fn test_extrusion_with_sketch() {
        let mut sketch = Sketch::new("Test");
        sketch.add_rectangle(0.0, 0.0, 100.0, 60.0).unwrap();

        let extr = ExtrusionFeature::new(sketch.id, 20.0);
        let profile = extr.extract_profile(&sketch).unwrap();

        assert_eq!(profile.len(), 4); // Rectangle has 4 points
    }
}
