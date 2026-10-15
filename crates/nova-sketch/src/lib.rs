//! NOVARIS CAD — 2D Sketch Editor

pub mod geometry;
pub mod constraints;
pub mod solver;

pub use geometry::{SketchPoint, SketchLine, SketchCircle, SketchElement};
pub use constraints::{Constraint, ConstraintType};
pub use solver::{SketchSolver, SolveReport, SolveStatus};

use nova_core::{Id, Result};
use std::collections::HashMap;

/// A 2D sketch containing geometry and constraints
#[derive(Clone, Debug)]
pub struct Sketch {
    pub id: Id,
    pub name: String,
    pub elements: HashMap<Id, SketchElement>,
    pub constraints: Vec<Constraint>,
    pub is_fully_constrained: bool,
}

impl Sketch {
    /// Create a new empty sketch
    pub fn new(name: impl Into<String>) -> Self {
        Sketch {
            id: Id::new(),
            name: name.into(),
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

    /// Add a line between two points
    pub fn add_line(&mut self, p1_id: Id, p2_id: Id) -> Result<Id> {
        let id = Id::new();
        self.elements.insert(id, SketchElement::Line(SketchLine { id, p1: p1_id, p2: p2_id }));
        Ok(id)
    }

    /// Add a circle
    pub fn add_circle(&mut self, center_id: Id, radius: f64) -> Result<Id> {
        let id = Id::new();
        self.elements.insert(id, SketchElement::Circle(SketchCircle { id, center: center_id, radius }));
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
}
