//! NOVARIS CAD — 2D Sketch Editor

pub mod geometry;
pub mod constraints;
pub mod profile;
pub mod solver;

pub use geometry::{SketchArc, SketchCircle, SketchElement, SketchLine, SketchPoint};
pub use constraints::{Constraint, ConstraintType};
pub use solver::{SketchSolver, SolveReport, SolveStatus};
pub use nova_kernel::Plane;

use nova_core::{Id, NovaError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A 2D sketch containing geometry and constraints, drawn on a plane
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sketch {
    pub id: Id,
    pub name: String,
    pub plane: Plane,
    pub elements: HashMap<Id, SketchElement>,
    pub constraints: Vec<Constraint>,
    pub is_fully_constrained: bool,
}

impl Sketch {
    /// Create a new empty sketch on the Front plane
    pub fn new(name: impl Into<String>) -> Self {
        Sketch::on_plane(name, Plane::front())
    }

    /// Create a new empty sketch on the given plane
    pub fn on_plane(name: impl Into<String>, plane: Plane) -> Self {
        Sketch {
            id: Id::new(),
            name: name.into(),
            plane,
            elements: HashMap::new(),
            constraints: Vec::new(),
            is_fully_constrained: false,
        }
    }

    /// Add a point to the sketch
    pub fn add_point(&mut self, x: f64, y: f64) -> Id {
        let id = Id::new();
        self.elements.insert(id, SketchElement::Point(SketchPoint { id, x, y }));
        id
    }

    fn require_point(&self, id: Id) -> Result<()> {
        match self.elements.get(&id) {
            Some(SketchElement::Point(_)) => Ok(()),
            _ => Err(NovaError::InvalidSketch(format!("{} is not a point of this sketch", id))),
        }
    }

    /// Add a line between two points
    pub fn add_line(&mut self, p1_id: Id, p2_id: Id) -> Result<Id> {
        self.require_point(p1_id)?;
        self.require_point(p2_id)?;
        if p1_id == p2_id {
            return Err(NovaError::InvalidSketch("a line needs two distinct points".into()));
        }
        let id = Id::new();
        self.elements.insert(id, SketchElement::Line(SketchLine { id, p1: p1_id, p2: p2_id }));
        Ok(id)
    }

    /// Add a circle
    pub fn add_circle(&mut self, center_id: Id, radius: f64) -> Result<Id> {
        self.require_point(center_id)?;
        if radius <= 0.0 {
            return Err(NovaError::InvalidSketch("circle radius must be positive".into()));
        }
        let id = Id::new();
        self.elements.insert(id, SketchElement::Circle(SketchCircle { id, center: center_id, radius }));
        Ok(id)
    }

    /// Add a counter-clockwise arc from `start` to `end` around `center`
    pub fn add_arc(&mut self, center: Id, start: Id, end: Id) -> Result<Id> {
        for p in [center, start, end] {
            self.require_point(p)?;
        }
        if start == end || center == start || center == end {
            return Err(NovaError::InvalidSketch("an arc needs distinct center, start and end points".into()));
        }
        let id = Id::new();
        self.elements.insert(id, SketchElement::Arc(SketchArc { id, center, start, end }));
        Ok(id)
    }

    /// Add a rectangle (4 points + 4 lines)
    pub fn add_rectangle(&mut self, x: f64, y: f64, width: f64, height: f64) -> Result<Vec<Id>> {
        let p1 = self.add_point(x, y);
        let p2 = self.add_point(x + width, y);
        let p3 = self.add_point(x + width, y + height);
        let p4 = self.add_point(x, y + height);

        self.add_line(p1, p2)?;
        self.add_line(p2, p3)?;
        self.add_line(p3, p4)?;
        self.add_line(p4, p1)?;

        Ok(vec![p1, p2, p3, p4])
    }

    /// Add a constraint
    pub fn add_constraint(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }

    /// Remove an element, plus anything built on it and constraints using it
    pub fn remove_element(&mut self, id: Id) {
        let dependents: Vec<Id> = self
            .elements
            .values()
            .filter(|e| match e {
                SketchElement::Line(l) => l.p1 == id || l.p2 == id,
                SketchElement::Circle(c) => c.center == id,
                SketchElement::Arc(a) => a.center == id || a.start == id || a.end == id,
                SketchElement::Point(_) => false,
            })
            .map(|e| e.id())
            .collect();
        self.elements.remove(&id);
        for d in dependents {
            self.remove_element(d);
        }
        self.constraints.retain(|c| !constraints::references(&c.constraint_type, id));
    }

    /// Get a point by ID
    pub fn get_point(&self, id: Id) -> Option<SketchPoint> {
        if let Some(SketchElement::Point(p)) = self.elements.get(&id) {
            Some(*p)
        } else {
            None
        }
    }

    /// Solve constraints and update geometry
    pub fn solve(&mut self) -> Result<SolveReport> {
        let mut solver = SketchSolver::new(self);
        let report = solver.solve()?;
        let fully = solver.is_fully_constrained();
        self.is_fully_constrained = fully;
        Ok(report)
    }

    /// Remaining degrees of freedom at the current geometry
    pub fn degrees_of_freedom(&mut self) -> i32 {
        SketchSolver::new(self).degrees_of_freedom()
    }

    /// Closed regions of the sketch, ready to extrude (sketch coordinates)
    pub fn profiles(&self) -> Result<Vec<nova_kernel::Profile>> {
        profile::extract_profiles(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sketch_creation() {
        let sketch = Sketch::new("Test Sketch");
        assert_eq!(sketch.name, "Test Sketch");
        assert_eq!(sketch.elements.len(), 0);
    }

    #[test]
    fn test_add_rectangle() {
        let mut sketch = Sketch::new("Rectangle");
        let points = sketch.add_rectangle(0.0, 0.0, 100.0, 60.0).unwrap();

        assert_eq!(points.len(), 4);
        assert_eq!(sketch.elements.len(), 8); // 4 points + 4 lines
    }

    #[test]
    fn test_references_are_checked() {
        let mut sketch = Sketch::new("Bad");
        let p = sketch.add_point(0.0, 0.0);
        assert!(sketch.add_line(p, Id::new()).is_err());
        assert!(sketch.add_line(p, p).is_err());
        assert!(sketch.add_circle(p, -1.0).is_err());
    }

    #[test]
    fn test_remove_point_cascades() {
        let mut sketch = Sketch::new("Rect");
        let pts = sketch.add_rectangle(0.0, 0.0, 10.0, 10.0).unwrap();
        sketch.add_constraint(Constraint::fixed(pts[0], 0.0, 0.0));
        sketch.remove_element(pts[0]);
        // the point and its two lines are gone, as is the constraint
        assert_eq!(sketch.elements.len(), 5);
        assert!(sketch.constraints.is_empty());
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut sketch = Sketch::on_plane("S", Plane::top());
        sketch.add_rectangle(0.0, 0.0, 10.0, 5.0).unwrap();
        let json = serde_json::to_string(&sketch).unwrap();
        let back: Sketch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.elements.len(), 8);
        assert_eq!(back.plane, Plane::top());
    }
}
