//! 2D geometry primitives for sketches

use nova_core::Id;
use serde::{Deserialize, Serialize};

/// A point in the sketch
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct SketchPoint {
    pub id: Id,
    pub x: f64,
    pub y: f64,
}

impl SketchPoint {
    pub fn new(id: Id, x: f64, y: f64) -> Self {
        SketchPoint { id, x, y }
    }

    pub fn distance_to(&self, other: SketchPoint) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

/// A line segment between two points
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SketchLine {
    pub id: Id,
    pub p1: Id,
    pub p2: Id,
    /// Construction geometry (centerline): constrained like any line but
    /// not part of profiles; used as revolve axis or mirror line
    #[serde(default)]
    pub construction: bool,
}

/// A circle
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SketchCircle {
    pub id: Id,
    pub center: Id,
    pub radius: f64,
}

/// A circular arc going counter-clockwise from `start` to `end` around
/// `center`. The solver keeps both end points at the same radius.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SketchArc {
    pub id: Id,
    pub center: Id,
    pub start: Id,
    pub end: Id,
}

/// Any sketch element (point, line, circle, etc.)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SketchElement {
    Point(SketchPoint),
    Line(SketchLine),
    Circle(SketchCircle),
    Arc(SketchArc),
}

impl SketchElement {
    pub fn id(&self) -> Id {
        match self {
            SketchElement::Point(p) => p.id,
            SketchElement::Line(l) => l.id,
            SketchElement::Circle(c) => c.id,
            SketchElement::Arc(a) => a.id,
        }
    }
}
