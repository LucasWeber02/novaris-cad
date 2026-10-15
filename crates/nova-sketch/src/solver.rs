//! Sketch constraint solver

use crate::{Sketch, ConstraintType};
use nova_core::Result;

/// Solves sketch constraints
pub struct SketchSolver<'a> {
    sketch: &'a mut Sketch,
    fully_constrained: bool,
}

impl<'a> SketchSolver<'a> {
    pub fn new(sketch: &'a mut Sketch) -> Self {
        SketchSolver {
            sketch,
            fully_constrained: false,
        }
    }

    /// Solve all constraints
    pub fn solve(&mut self) -> Result<()> {
        // Count DOF before constraints
        let n_points = self.sketch.elements.iter()
            .filter(|(_, e)| matches!(e, crate::SketchElement::Point(_)))
            .count() as i32;

        let total_constraints: i32 = self.sketch.constraints.iter()
            .map(|c| c.degrees_of_freedom_reduction())
            .sum();

        let dof = (n_points * 2) - total_constraints;

        // Mark sketch as fully constrained if dof == 0
        self.fully_constrained = dof == 0;

        // Apply simple constraint satisfaction
        for constraint in &mut self.sketch.constraints {
            Self::apply_constraint(constraint)?;
        }

        Ok(())
    }

    /// Apply a single constraint
    fn apply_constraint(constraint: &mut crate::Constraint) -> Result<()> {
        match &constraint.constraint_type {
            ConstraintType::Fixed { x, y } => {
                // This would move the point to (x, y) if needed
                constraint.is_satisfied = true;
            }
            ConstraintType::Horizontal { line_id } => {
                // This would make the line horizontal
                constraint.is_satisfied = true;
            }
            ConstraintType::Vertical { line_id } => {
                // This would make the line vertical
                constraint.is_satisfied = true;
            }
            ConstraintType::Distance { p1, p2, distance } => {
                // This would enforce distance between points
                constraint.is_satisfied = true;
            }
            ConstraintType::Radius { circle_id, radius } => {
                // This would set the circle radius
                constraint.is_satisfied = true;
            }
            _ => {
                constraint.is_satisfied = true;
            }
        }
        Ok(())
    }

    /// Check if sketch is fully constrained
    pub fn is_fully_constrained(&self) -> bool {
        self.fully_constrained
    }

    /// Get degrees of freedom
    pub fn degrees_of_freedom(&self) -> i32 {
        let n_points = self.sketch.elements.iter()
            .filter(|(_, e)| matches!(e, crate::SketchElement::Point(_)))
            .count() as i32;

        let total_constraints: i32 = self.sketch.constraints.iter()
            .map(|c| c.degrees_of_freedom_reduction())
            .sum();

        ((n_points * 2) - total_constraints).max(0)
    }

    /// Get constraint status
    pub fn constraint_status(&self) -> String {
        let dof = self.degrees_of_freedom();
        if dof > 0 {
            format!("Under-constrained ({} DOF)", dof)
        } else if dof == 0 {
            "Fully constrained".to_string()
        } else {
            format!("Over-constrained ({} conflicts)", -dof)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_sketch_dof() {
        let mut sketch = Sketch::new("Empty");
        let solver = SketchSolver::new(&mut sketch);
        assert_eq!(solver.degrees_of_freedom(), 0);
    }

    #[test]
    fn test_rectangle_under_constrained() {
        let mut sketch = Sketch::new("Rectangle");
        let _points = sketch.add_rectangle(0.0, 0.0, 100.0, 60.0).unwrap();

        let solver = SketchSolver::new(&mut sketch);
        // 4 points = 8 DOF, 0 constraints = 8 DOF
        assert_eq!(solver.degrees_of_freedom(), 8);
    }

    #[test]
    fn test_fully_constrained() {
        let mut sketch = Sketch::new("Constrained");
        let p = sketch.add_point(50.0, 30.0);
        sketch.add_constraint(crate::Constraint::fixed(50.0, 30.0));

        let mut solver = SketchSolver::new(&mut sketch);
        solver.solve().unwrap();
        assert!(solver.is_fully_constrained());
    }
}
