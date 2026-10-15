//! Sketch constraints (geometric and dimensional)

use nova_core::Id;
use serde::{Deserialize, Serialize};

/// Constraint types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConstraintType {
    /// Point is fixed at (x, y)
    Fixed { point: Id, x: f64, y: f64 },
    /// Horizontal line
    Horizontal { line_id: Id },
    /// Vertical line
    Vertical { line_id: Id },
    /// Two points coincide
    Coincident { p1: Id, p2: Id },
    /// Two lines parallel
    Parallel { l1: Id, l2: Id },
    /// Two lines perpendicular
    Perpendicular { l1: Id, l2: Id },
    /// Distance between two points
    Distance { p1: Id, p2: Id, distance: f64 },
    /// Line length
    Length { line_id: Id, length: f64 },
    /// Circle radius
    Radius { circle_id: Id, radius: f64 },
    /// Two points have same X
    EqualX { p1: Id, p2: Id },
    /// Two points have same Y
    EqualY { p1: Id, p2: Id },
}

/// A constraint on sketch geometry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Constraint {
    pub id: Id,
    pub constraint_type: ConstraintType,
    pub is_satisfied: bool,
}

impl Constraint {
    pub fn new(constraint_type: ConstraintType) -> Self {
        Constraint {
            id: Id::new(),
            constraint_type,
            is_satisfied: false,
        }
    }

    pub fn fixed(point: Id, x: f64, y: f64) -> Self {
        Constraint::new(ConstraintType::Fixed { point, x, y })
    }

    pub fn horizontal(line_id: Id) -> Self {
        Constraint::new(ConstraintType::Horizontal { line_id })
    }

    pub fn vertical(line_id: Id) -> Self {
        Constraint::new(ConstraintType::Vertical { line_id })
    }

    pub fn distance(p1: Id, p2: Id, distance: f64) -> Self {
        Constraint::new(ConstraintType::Distance { p1, p2, distance })
    }

    pub fn length(line_id: Id, length: f64) -> Self {
        Constraint::new(ConstraintType::Length { line_id, length })
    }

    pub fn radius(circle_id: Id, radius: f64) -> Self {
        Constraint::new(ConstraintType::Radius { circle_id, radius })
    }

    /// Get DOF reduction for this constraint
    pub fn degrees_of_freedom_reduction(&self) -> i32 {
        match &self.constraint_type {
            ConstraintType::Fixed { .. } => 2,
            ConstraintType::Horizontal { .. } => 1,
            ConstraintType::Vertical { .. } => 1,
            ConstraintType::Coincident { .. } => 2,
            ConstraintType::Distance { .. } => 1,
            ConstraintType::Length { .. } => 1,
            ConstraintType::Radius { .. } => 1,
            ConstraintType::EqualX { .. } => 1,
            ConstraintType::EqualY { .. } => 1,
            ConstraintType::Parallel { .. } => 1,
            ConstraintType::Perpendicular { .. } => 1,
        }
    }
}
