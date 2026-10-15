//! Document model - the container for a complete CAD project

use crate::{Id, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A CAD document containing sketches, features, and properties
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    /// Unique identifier for this document
    pub id: Id,

    /// User-facing name
    pub name: String,

    /// Document version (for compatibility)
    pub version: u32,

    /// All sketches in the document, indexed by ID
    pub sketches: HashMap<Id, Sketch>,

    /// All features (extrusions, revolutions, etc.), in order
    pub features: Vec<Feature>,

    /// Undo/redo stack
    pub history: History,
}

/// A 2D sketch in a plane
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sketch {
    pub id: Id,
    pub name: String,
    pub plane: SketchPlane,
    pub elements: Vec<SketchElement>,
    pub constraints: Vec<Constraint>,
}

/// Standard sketch planes
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum SketchPlane {
    XY,
    XZ,
    YZ,
}

/// Element in a sketch (point, line, circle, etc.)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SketchElement {
    Point { id: Id, x: f64, y: f64 },
    Line { id: Id, p1: Id, p2: Id },
    Circle { id: Id, center: Id, radius: f64 },
    Rectangle { id: Id, p1: Id, p2: Id },
}

/// Constraint on sketch geometry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Constraint {
    Horizontal { element: Id },
    Vertical { element: Id },
    Coincident { el1: Id, el2: Id },
    Distance { el1: Id, el2: Id, distance: f64 },
    Radius { element: Id, radius: f64 },
    Diameter { element: Id, diameter: f64 },
}

/// A 3D feature (extrusion, revolution, etc.)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feature {
    pub id: Id,
    pub name: String,
    pub kind: FeatureKind,
    pub visible: bool,
}

/// Types of features
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FeatureKind {
    Extrusion {
        sketch_id: Id,
        length: f64,
        reversed: bool,
    },
    Revolution {
        sketch_id: Id,
        axis: String, // "X", "Y", "Z"
        angle: f64,
    },
}

/// Undo/redo history
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct History {
    pub undo_stack: Vec<String>,
    pub redo_stack: Vec<String>,
}

impl Document {
    /// Create a new empty document
    pub fn new(name: impl Into<String>) -> Self {
        Document {
            id: Id::new(),
            name: name.into(),
            version: 1,
            sketches: HashMap::new(),
            features: Vec::new(),
            history: History {
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
            },
        }
    }

    /// Add a sketch to the document
    pub fn add_sketch(&mut self, sketch: Sketch) {
        self.sketches.insert(sketch.id, sketch);
    }

    /// Add a feature to the document
    pub fn add_feature(&mut self, feature: Feature) {
        self.features.push(feature);
    }

    /// Get total number of entities
    pub fn entity_count(&self) -> usize {
        self.sketches.len() + self.features.len()
    }
}
