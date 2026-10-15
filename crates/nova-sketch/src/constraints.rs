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
    /// Circle or arc radius
    Radius { circle_id: Id, radius: f64 },
    /// Line tangent to a circle or arc
    Tangent { line: Id, curve: Id },
    /// Point lies on the (infinite) line
    PointOnLine { point: Id, line: Id },
    /// Point lies on a circle or arc
    PointOnCircle { point: Id, curve: Id },
    /// Equal length (two lines) or equal radius (two circles/arcs)
    Equal { e1: Id, e2: Id },
    /// Point at the middle of a line
    Midpoint { point: Id, line: Id },
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
            ConstraintType::Tangent { .. } => 1,
            ConstraintType::PointOnLine { .. } => 1,
            ConstraintType::PointOnCircle { .. } => 1,
            ConstraintType::Equal { .. } => 1,
            ConstraintType::Midpoint { .. } => 2,
        }
    }
}

/// Whether a constraint refers to the given element
pub fn references(c: &ConstraintType, id: Id) -> bool {
    use ConstraintType::*;
    let ids: Vec<Id> = match *c {
        Fixed { point, .. } => vec![point],
        Horizontal { line_id } | Vertical { line_id } | Length { line_id, .. } => vec![line_id],
        Coincident { p1, p2 } | Distance { p1, p2, .. } | EqualX { p1, p2 } | EqualY { p1, p2 } => vec![p1, p2],
        Parallel { l1, l2 } | Perpendicular { l1, l2 } => vec![l1, l2],
        Radius { circle_id, .. } => vec![circle_id],
        Tangent { line, curve } => vec![line, curve],
        PointOnLine { point, line } | Midpoint { point, line } => vec![point, line],
        PointOnCircle { point, curve } => vec![point, curve],
        Equal { e1, e2 } => vec![e1, e2],
    };
    ids.contains(&id)
}
