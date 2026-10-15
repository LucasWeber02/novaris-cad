//! Base feature types

use nova_core::Id;
use serde::{Deserialize, Serialize};

pub type FeatureId = Id;

/// A parametric feature (extrusion, revolution, pocket, etc.)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    pub kind: FeatureKind,
    pub visible: bool,
    pub suppressed: bool,
}

/// Feature types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FeatureKind {
    /// Extrude a sketch along an axis
    Extrusion {
        sketch_id: Id,
        length: f64,
        direction: ExtrusionDirection,
    },
    /// Revolve a sketch around an axis
    Revolution {
        sketch_id: Id,
        axis: String, // "X", "Y", "Z"
        angle: f64,
    },
    /// Remove material (subtract)
    Pocket {
        sketch_id: Id,
        depth: f64,
    },
    /// Fillet edges
    Fillet {
        edge_ids: Vec<Id>,
        radius: f64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, Copy)]
pub enum ExtrusionDirection {
    Forward,
    Backward,
    Symmetric,
}

impl Feature {
    /// Create a new extrusion feature
    pub fn extrusion(
        name: impl Into<String>,
        sketch_id: Id,
        length: f64,
    ) -> Self {
        Feature {
            id: Id::new(),
            name: name.into(),
            kind: FeatureKind::Extrusion {
                sketch_id,
                length,
                direction: ExtrusionDirection::Forward,
            },
            visible: true,
            suppressed: false,
        }
    }

    /// Create a new pocket feature
    pub fn pocket(name: impl Into<String>, sketch_id: Id, depth: f64) -> Self {
        Feature {
            id: Id::new(),
            name: name.into(),
            kind: FeatureKind::Pocket { sketch_id, depth },
            visible: true,
            suppressed: false,
        }
    }

    /// Get the sketch ID this feature depends on (if any)
    pub fn get_sketch_dependency(&self) -> Option<Id> {
        match &self.kind {
            FeatureKind::Extrusion { sketch_id, .. } => Some(*sketch_id),
            FeatureKind::Revolution { sketch_id, .. } => Some(*sketch_id),
            FeatureKind::Pocket { sketch_id, .. } => Some(*sketch_id),
            FeatureKind::Fillet { .. } => None,
        }
    }

    /// Get the previous feature this depends on (for sequential evaluation)
    pub fn get_parent_feature(&self) -> Option<FeatureId> {
        // Features depend on the previous feature in the tree
        // This is set by the FeatureTree
        None
    }
}
