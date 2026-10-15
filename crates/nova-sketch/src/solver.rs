//! Sketch constraint solver
//!
//! Every constraint is turned into one or more residual equations `r(x) = 0`
//! over the sketch variables (point coordinates and circle radii). The system
//! is solved in the least-squares sense with Levenberg-Marquardt, which copes
//! with under-constrained sketches (it moves geometry as little as needed) and
//! with redundant constraints.
//!
//! Degrees of freedom are computed from the rank of the Jacobian at the
//! solution, so redundant constraints (e.g. two `Horizontal` on the same line)
//! are not counted twice.

use crate::{ConstraintType, Sketch, SketchElement};
use nalgebra::{DMatrix, DVector};
use nova_core::{Id, NovaError, Result};
use std::collections::HashMap;

/// Residual magnitude under which a constraint counts as satisfied
const TOLERANCE: f64 = 1e-9;
const MAX_ITERATIONS: usize = 200;
/// Relative threshold on singular values when computing the Jacobian rank
const RANK_TOLERANCE: f64 = 1e-8;

/// Overall state of a sketch after solving
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveStatus {
    /// Geometry can still move along `dof` independent directions
    UnderConstrained { dof: usize },
    /// Every variable is determined by the constraints
    FullyConstrained,
    /// All constraints hold, but `redundant` of them add no information
    OverConstrained { redundant: usize },
    /// Constraints contradict each other; no geometry satisfies them all
    Inconsistent,
}

/// Outcome of a solve
#[derive(Clone, Copy, Debug)]
pub struct SolveReport {
    pub status: SolveStatus,
    pub dof: usize,
    pub iterations: usize,
    /// Euclidean norm of all residuals at the end of the solve
    pub residual: f64,
}

/// Where a variable lives in the sketch
#[derive(Clone, Copy)]
enum Var {
    X(Id),
    Y(Id),
    Radius(Id),
}

/// Solves sketch constraints
pub struct SketchSolver<'a> {
    sketch: &'a mut Sketch,
    vars: Vec<Var>,
    point_index: HashMap<Id, usize>,
    circle_index: HashMap<Id, usize>,
    /// Arcs, whose end points must stay at equal distance from the center
    arcs: Vec<(Id, Id, Id)>,
    report: Option<SolveReport>,
}

impl<'a> SketchSolver<'a> {
    pub fn new(sketch: &'a mut Sketch) -> Self {
        let mut points: Vec<Id> = Vec::new();
        let mut circles: Vec<Id> = Vec::new();
        let mut arcs = Vec::new();
        for (id, element) in &sketch.elements {
            match element {
                SketchElement::Point(_) => points.push(*id),
                SketchElement::Circle(_) => circles.push(*id),
                SketchElement::Arc(a) => arcs.push((a.id, a.center, a.start, a.end)),
                SketchElement::Line(_) => {}
            }
        }
        arcs.sort_by_key(|a| a.0.uuid());
        let arcs = arcs.into_iter().map(|(_, c, s, e)| (c, s, e)).collect();
        // Stable ordering so results don't depend on HashMap iteration order
        points.sort_by_key(|id| id.uuid());
        circles.sort_by_key(|id| id.uuid());

        let mut vars = Vec::with_capacity(points.len() * 2 + circles.len());
        let mut point_index = HashMap::new();
        let mut circle_index = HashMap::new();
        for id in points {
            point_index.insert(id, vars.len());
            vars.push(Var::X(id));
            vars.push(Var::Y(id));
        }
        for id in circles {
            circle_index.insert(id, vars.len());
            vars.push(Var::Radius(id));
        }

        SketchSolver {
            sketch,
            vars,
            point_index,
            circle_index,
            arcs,
            report: None,
        }
    }

    /// Solve all constraints, moving sketch geometry to satisfy them
    pub fn solve(&mut self) -> Result<SolveReport> {
        let mut x = self.read_vars();
        let (iterations, residual) = self.levenberg_marquardt(&mut x)?;
        self.write_vars(&x);

        // Per-constraint satisfaction flags
        for i in 0..self.sketch.constraints.len() {
            let r = self.constraint_residuals(&self.sketch.constraints[i].constraint_type, &x)?;
            let satisfied = r.iter().all(|v| v.abs() < TOLERANCE.sqrt());
            self.sketch.constraints[i].is_satisfied = satisfied;
        }

        let report = self.analyze(&x, iterations, residual)?;
        self.report = Some(report);
        Ok(report)
    }

    /// Check if sketch is fully constrained (after `solve`)
    pub fn is_fully_constrained(&self) -> bool {
        matches!(
            self.report.map(|r| r.status),
            Some(SolveStatus::FullyConstrained) | Some(SolveStatus::OverConstrained { .. })
        )
    }

    /// Remaining degrees of freedom at the current geometry
    pub fn degrees_of_freedom(&self) -> i32 {
        if let Some(report) = self.report {
            return report.dof as i32;
        }
        let x = self.read_vars();
        match self.jacobian(&x) {
            Ok(j) => (self.vars.len() - rank(&j)) as i32,
            Err(_) => self.vars.len() as i32,
        }
    }

    /// Human-readable constraint status
    pub fn constraint_status(&self) -> String {
        match self.report.map(|r| r.status) {
            None => format!("Not solved ({} DOF)", self.degrees_of_freedom()),
            Some(SolveStatus::UnderConstrained { dof }) => format!("Under-constrained ({} DOF)", dof),
            Some(SolveStatus::FullyConstrained) => "Fully constrained".to_string(),
            Some(SolveStatus::OverConstrained { redundant }) => {
                format!("Over-constrained ({} redundant)", redundant)
            }
            Some(SolveStatus::Inconsistent) => "Inconsistent constraints".to_string(),
        }
    }

    // ── Variables ────────────────────────────────────────────────

    fn read_vars(&self) -> DVector<f64> {
        DVector::from_iterator(
            self.vars.len(),
            self.vars.iter().map(|v| match *v {
                Var::X(id) => self.point(id).map(|p| p.0).unwrap_or(0.0),
                Var::Y(id) => self.point(id).map(|p| p.1).unwrap_or(0.0),
                Var::Radius(id) => match self.sketch.elements.get(&id) {
                    Some(SketchElement::Circle(c)) => c.radius,
                    _ => 0.0,
                },
            }),
        )
    }

    fn write_vars(&mut self, x: &DVector<f64>) {
        for (i, v) in self.vars.iter().enumerate() {
            match (*v, self.sketch.elements.get_mut(&var_id(*v))) {
                (Var::X(_), Some(SketchElement::Point(p))) => p.x = x[i],
                (Var::Y(_), Some(SketchElement::Point(p))) => p.y = x[i],
                (Var::Radius(_), Some(SketchElement::Circle(c))) => c.radius = x[i],
                _ => {}
            }
        }
    }

    fn point(&self, id: Id) -> Option<(f64, f64)> {
        match self.sketch.elements.get(&id) {
            Some(SketchElement::Point(p)) => Some((p.x, p.y)),
            _ => None,
        }
    }

    // ── Residuals ────────────────────────────────────────────────

    fn point_index(&self, id: Id) -> Result<usize> {
        self.point_index
            .get(&id)
            .copied()
            .ok_or_else(|| NovaError::InvalidSketch(format!("constraint references unknown point {}", id)))
    }

    fn pt(&self, id: Id, x: &DVector<f64>) -> Result<(f64, f64)> {
        let i = self.point_index(id)?;
        Ok((x[i], x[i + 1]))
    }

    fn line(&self, id: Id, x: &DVector<f64>) -> Result<((f64, f64), (f64, f64))> {
        match self.sketch.elements.get(&id) {
            Some(SketchElement::Line(l)) => Ok((self.pt(l.p1, x)?, self.pt(l.p2, x)?)),
            _ => Err(NovaError::InvalidSketch(format!("constraint references unknown line {}", id))),
        }
    }

    fn line_dir(&self, id: Id, x: &DVector<f64>) -> Result<(f64, f64)> {
        let (a, b) = self.line(id, x)?;
        Ok((b.0 - a.0, b.1 - a.1))
    }

    /// Center and radius of a circle or arc
    fn curve(&self, id: Id, x: &DVector<f64>) -> Result<((f64, f64), f64)> {
        match self.sketch.elements.get(&id) {
            Some(SketchElement::Circle(c)) => {
                let i = self.circle_index[&id];
                Ok((self.pt(c.center, x)?, x[i]))
            }
            Some(SketchElement::Arc(a)) => {
                let (c, s) = (self.pt(a.center, x)?, self.pt(a.start, x)?);
                Ok((c, norm((s.0 - c.0, s.1 - c.1))))
            }
            _ => Err(NovaError::InvalidSketch(format!("constraint references unknown circle or arc {}", id))),
        }
    }

    fn is_line(&self, id: Id) -> bool {
        matches!(self.sketch.elements.get(&id), Some(SketchElement::Line(_)))
    }

    fn constraint_residuals(&self, c: &ConstraintType, x: &DVector<f64>) -> Result<Vec<f64>> {
        Ok(match *c {
            ConstraintType::Fixed { point, x: fx, y: fy } => {
                let p = self.pt(point, x)?;
                vec![p.0 - fx, p.1 - fy]
            }
            ConstraintType::Horizontal { line_id } => {
                let (a, b) = self.line(line_id, x)?;
                vec![b.1 - a.1]
            }
            ConstraintType::Vertical { line_id } => {
                let (a, b) = self.line(line_id, x)?;
                vec![b.0 - a.0]
            }
            ConstraintType::Coincident { p1, p2 } => {
                let (a, b) = (self.pt(p1, x)?, self.pt(p2, x)?);
                vec![b.0 - a.0, b.1 - a.1]
            }
            ConstraintType::Parallel { l1, l2 } => {
                let (d1, d2) = (self.line_dir(l1, x)?, self.line_dir(l2, x)?);
                // sin of the angle between the lines
                vec![(d1.0 * d2.1 - d1.1 * d2.0) / (norm(d1) * norm(d2)).max(f64::EPSILON)]
            }
            ConstraintType::Perpendicular { l1, l2 } => {
                let (d1, d2) = (self.line_dir(l1, x)?, self.line_dir(l2, x)?);
                // cos of the angle between the lines
                vec![(d1.0 * d2.0 + d1.1 * d2.1) / (norm(d1) * norm(d2)).max(f64::EPSILON)]
            }
            ConstraintType::Distance { p1, p2, distance } => {
                let (a, b) = (self.pt(p1, x)?, self.pt(p2, x)?);
                vec![norm((b.0 - a.0, b.1 - a.1)) - distance]
            }
            ConstraintType::Length { line_id, length } => {
                vec![norm(self.line_dir(line_id, x)?) - length]
            }
            ConstraintType::Radius { circle_id, radius } => vec![self.curve(circle_id, x)?.1 - radius],
            ConstraintType::Tangent { line, curve } => {
                let (a, b) = self.line(line, x)?;
                let (c, r) = self.curve(curve, x)?;
                let d = (b.0 - a.0, b.1 - a.1);
                let dist = (d.0 * (c.1 - a.1) - d.1 * (c.0 - a.0)).abs() / norm(d).max(f64::EPSILON);
                vec![dist - r]
            }
            ConstraintType::PointOnLine { point, line } => {
                let (a, b) = self.line(line, x)?;
                let p = self.pt(point, x)?;
                let d = (b.0 - a.0, b.1 - a.1);
                vec![(d.0 * (p.1 - a.1) - d.1 * (p.0 - a.0)) / norm(d).max(f64::EPSILON)]
            }
            ConstraintType::PointOnCircle { point, curve } => {
                let p = self.pt(point, x)?;
                let (c, r) = self.curve(curve, x)?;
                vec![norm((p.0 - c.0, p.1 - c.1)) - r]
            }
            ConstraintType::Equal { e1, e2 } => match (self.is_line(e1), self.is_line(e2)) {
                (true, true) => vec![norm(self.line_dir(e1, x)?) - norm(self.line_dir(e2, x)?)],
                (false, false) => vec![self.curve(e1, x)?.1 - self.curve(e2, x)?.1],
                _ => return Err(NovaError::InvalidSketch("equal relation needs two lines or two circles/arcs".into())),
            },
            ConstraintType::Midpoint { point, line } => {
                let (a, b) = self.line(line, x)?;
                let p = self.pt(point, x)?;
                vec![p.0 - (a.0 + b.0) / 2.0, p.1 - (a.1 + b.1) / 2.0]
            }
            ConstraintType::EqualX { p1, p2 } => vec![self.pt(p1, x)?.0 - self.pt(p2, x)?.0],
            ConstraintType::EqualY { p1, p2 } => vec![self.pt(p1, x)?.1 - self.pt(p2, x)?.1],
        })
    }

    fn residuals(&self, x: &DVector<f64>) -> Result<DVector<f64>> {
        let mut r = Vec::new();
        for c in &self.sketch.constraints {
            r.extend(self.constraint_residuals(&c.constraint_type, x)?);
        }
        for &(c, s, e) in &self.arcs {
            let (c, s, e) = (self.pt(c, x)?, self.pt(s, x)?, self.pt(e, x)?);
            r.push(norm((s.0 - c.0, s.1 - c.1)) - norm((e.0 - c.0, e.1 - c.1)));
        }
        Ok(DVector::from_vec(r))
    }

    /// Jacobian by central differences
    fn jacobian(&self, x: &DVector<f64>) -> Result<DMatrix<f64>> {
        let m = self.residuals(x)?.len();
        let n = x.len();
        let mut j = DMatrix::zeros(m, n);
        let mut xp = x.clone();
        for col in 0..n {
            let h = 1e-6 * x[col].abs().max(1.0);
            let orig = xp[col];
            xp[col] = orig + h;
            let rp = self.residuals(&xp)?;
            xp[col] = orig - h;
            let rm = self.residuals(&xp)?;
            xp[col] = orig;
            j.set_column(col, &((rp - rm) / (2.0 * h)));
        }
        Ok(j)
    }

    // ── Solve & analysis ────────────────────────────────────────

    /// Returns (iterations, final residual norm)
    fn levenberg_marquardt(&self, x: &mut DVector<f64>) -> Result<(usize, f64)> {
        let mut r = self.residuals(x)?;
        let mut cost = r.norm_squared();
        if r.is_empty() || x.is_empty() {
            return Ok((0, cost.sqrt()));
        }

        let mut lambda = 1e-3;
        for iter in 0..MAX_ITERATIONS {
            if cost.sqrt() < TOLERANCE {
                return Ok((iter, cost.sqrt()));
            }
            let j = self.jacobian(x)?;
            let jt = j.transpose();
            let jtj = &jt * &j;
            let g = &jt * &r;

            let mut improved = false;
            while lambda < 1e12 {
                // Identity damping: keeps the system solvable when J is rank
                // deficient and favours small moves of free geometry.
                let mut a = jtj.clone();
                for d in 0..a.nrows() {
                    a[(d, d)] += lambda;
                }
                let Some(step) = a.cholesky().map(|ch| ch.solve(&(-&g))) else {
                    lambda *= 10.0;
                    continue;
                };
                let candidate = &*x + &step;
                let r_new = self.residuals(&candidate)?;
                let cost_new = r_new.norm_squared();
                if cost_new < cost {
                    let small_step = step.norm() < 1e-14 * (1.0 + x.norm());
                    *x = candidate;
                    r = r_new;
                    cost = cost_new;
                    lambda = (lambda / 10.0).max(1e-12);
                    improved = !small_step;
                    break;
                }
                lambda *= 10.0;
            }
            if !improved {
                // Local minimum (possibly non-zero for inconsistent sketches)
                return Ok((iter + 1, cost.sqrt()));
            }
        }
        Ok((MAX_ITERATIONS, cost.sqrt()))
    }

    fn analyze(&self, x: &DVector<f64>, iterations: usize, residual: f64) -> Result<SolveReport> {
        let j = self.jacobian(x)?;
        let rank = rank(&j);
        let dof = self.vars.len() - rank;
        let redundant = j.nrows() - rank;

        let status = if residual > TOLERANCE.sqrt() {
            SolveStatus::Inconsistent
        } else if redundant > 0 {
            SolveStatus::OverConstrained { redundant }
        } else if dof > 0 {
            SolveStatus::UnderConstrained { dof }
        } else {
            SolveStatus::FullyConstrained
        };

        Ok(SolveReport { status, dof, iterations, residual })
    }
}

fn var_id(v: Var) -> Id {
    match v {
        Var::X(id) | Var::Y(id) | Var::Radius(id) => id,
    }
}

fn norm(v: (f64, f64)) -> f64 {
    (v.0 * v.0 + v.1 * v.1).sqrt()
}

fn rank(m: &DMatrix<f64>) -> usize {
    if m.nrows() == 0 || m.ncols() == 0 {
        return 0;
    }
    let sv = m.clone().svd(false, false).singular_values;
    let max = sv.iter().cloned().fold(0.0, f64::max);
    if max == 0.0 {
        return 0;
    }
    sv.iter().filter(|&&s| s > max * RANK_TOLERANCE).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Constraint;

    const EPS: f64 = 1e-6;

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
        sketch.add_constraint(Constraint::fixed(p, 50.0, 30.0));

        let mut solver = SketchSolver::new(&mut sketch);
        solver.solve().unwrap();
        assert!(solver.is_fully_constrained());
    }

    #[test]
    fn test_fixed_moves_point() {
        let mut sketch = Sketch::new("Fixed");
        let p = sketch.add_point(3.0, -7.0);
        sketch.add_constraint(Constraint::fixed(p, 10.0, 20.0));
        let report = sketch.solve().unwrap();

        let pt = sketch.get_point(p).unwrap();
        assert!((pt.x - 10.0).abs() < EPS && (pt.y - 20.0).abs() < EPS);
        assert_eq!(report.status, SolveStatus::FullyConstrained);
    }

    #[test]
    fn test_horizontal_line_is_straightened() {
        let mut sketch = Sketch::new("H");
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(10.0, 3.0);
        let l = sketch.add_line(a, b).unwrap();
        sketch.add_constraint(Constraint::horizontal(l));
        let report = sketch.solve().unwrap();

        let (pa, pb) = (sketch.get_point(a).unwrap(), sketch.get_point(b).unwrap());
        assert!((pa.y - pb.y).abs() < EPS);
        assert_eq!(report.status, SolveStatus::UnderConstrained { dof: 3 });
    }

    #[test]
    fn test_distance_constraint() {
        let mut sketch = Sketch::new("D");
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(3.0, 4.0);
        sketch.add_constraint(Constraint::fixed(a, 0.0, 0.0));
        sketch.add_constraint(Constraint::distance(a, b, 10.0));
        sketch.solve().unwrap();

        let pb = sketch.get_point(b).unwrap();
        assert!(((pb.x * pb.x + pb.y * pb.y).sqrt() - 10.0).abs() < EPS);
        assert!(sketch.constraints.iter().all(|c| c.is_satisfied));
    }

    /// Classic case: a skewed quadrilateral becomes an exact 100 × 60 rectangle
    #[test]
    fn test_fully_dimensioned_rectangle() {
        let mut sketch = Sketch::new("Rect");
        let p1 = sketch.add_point(0.0, 0.0);
        let p2 = sketch.add_point(90.0, 5.0);
        let p3 = sketch.add_point(95.0, 70.0);
        let p4 = sketch.add_point(-4.0, 55.0);
        let l1 = sketch.add_line(p1, p2).unwrap();
        let l2 = sketch.add_line(p2, p3).unwrap();
        let l3 = sketch.add_line(p3, p4).unwrap();
        let l4 = sketch.add_line(p4, p1).unwrap();

        sketch.add_constraint(Constraint::fixed(p1, 0.0, 0.0));
        sketch.add_constraint(Constraint::horizontal(l1));
        sketch.add_constraint(Constraint::horizontal(l3));
        sketch.add_constraint(Constraint::vertical(l2));
        sketch.add_constraint(Constraint::vertical(l4));
        sketch.add_constraint(Constraint::length(l1, 100.0));
        sketch.add_constraint(Constraint::length(l2, 60.0));

        let report = sketch.solve().unwrap();
        assert_eq!(report.status, SolveStatus::FullyConstrained, "{:?}", report);
        assert!(sketch.is_fully_constrained);

        let expected = [(p1, 0.0, 0.0), (p2, 100.0, 0.0), (p3, 100.0, 60.0), (p4, 0.0, 60.0)];
        for (id, ex, ey) in expected {
            let p = sketch.get_point(id).unwrap();
            assert!((p.x - ex).abs() < EPS && (p.y - ey).abs() < EPS, "{:?} != ({}, {})", p, ex, ey);
        }
    }

    #[test]
    fn test_parallel_and_perpendicular() {
        let mut sketch = Sketch::new("PP");
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(10.0, 1.0);
        let c = sketch.add_point(0.0, 5.0);
        let d = sketch.add_point(8.0, 8.0);
        let e = sketch.add_point(20.0, 0.0);
        let f = sketch.add_point(22.0, 9.0);
        let l1 = sketch.add_line(a, b).unwrap();
        let l2 = sketch.add_line(c, d).unwrap();
        let l3 = sketch.add_line(e, f).unwrap();
        sketch.add_constraint(Constraint::new(ConstraintType::Parallel { l1, l2 }));
        sketch.add_constraint(Constraint::new(ConstraintType::Perpendicular { l1, l2: l3 }));
        sketch.solve().unwrap();

        let dir = |p: Id, q: Id| {
            let (p, q) = (sketch.get_point(p).unwrap(), sketch.get_point(q).unwrap());
            (q.x - p.x, q.y - p.y)
        };
        let (d1, d2, d3) = (dir(a, b), dir(c, d), dir(e, f));
        assert!((d1.0 * d2.1 - d1.1 * d2.0).abs() < 1e-5);
        assert!((d1.0 * d3.0 + d1.1 * d3.1).abs() < 1e-5);
    }

    #[test]
    fn test_circle_radius() {
        let mut sketch = Sketch::new("C");
        let center = sketch.add_point(1.0, 1.0);
        let circle = sketch.add_circle(center, 3.0).unwrap();
        sketch.add_constraint(Constraint::fixed(center, 0.0, 0.0));
        sketch.add_constraint(Constraint::radius(circle, 12.5));
        let report = sketch.solve().unwrap();

        match sketch.elements.get(&circle) {
            Some(SketchElement::Circle(c)) => assert!((c.radius - 12.5).abs() < EPS),
            _ => panic!("circle missing"),
        }
        assert_eq!(report.status, SolveStatus::FullyConstrained);
    }

    #[test]
    fn test_redundant_constraint_detected() {
        let mut sketch = Sketch::new("Redundant");
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(10.0, 2.0);
        let l = sketch.add_line(a, b).unwrap();
        sketch.add_constraint(Constraint::horizontal(l));
        sketch.add_constraint(Constraint::new(ConstraintType::EqualY { p1: a, p2: b }));
        let report = sketch.solve().unwrap();

        assert_eq!(report.status, SolveStatus::OverConstrained { redundant: 1 });
        assert_eq!(report.dof, 3);
    }

    #[test]
    fn test_inconsistent_constraints() {
        let mut sketch = Sketch::new("Conflict");
        let p = sketch.add_point(0.0, 0.0);
        sketch.add_constraint(Constraint::fixed(p, 0.0, 0.0));
        sketch.add_constraint(Constraint::fixed(p, 5.0, 0.0));
        let report = sketch.solve().unwrap();

        assert_eq!(report.status, SolveStatus::Inconsistent);
        assert!(!sketch.constraints.iter().all(|c| c.is_satisfied));
    }

    #[test]
    fn test_arc_keeps_equal_radii_and_dof() {
        let mut sketch = Sketch::new("Arc");
        let c = sketch.add_point(0.0, 0.0);
        let s = sketch.add_point(10.0, 0.0);
        let e = sketch.add_point(0.0, 7.0);
        sketch.add_arc(c, s, e).unwrap();
        let report = sketch.solve().unwrap();
        let (pc, ps, pe) = (sketch.get_point(c).unwrap(), sketch.get_point(s).unwrap(), sketch.get_point(e).unwrap());
        assert!(((ps.x - pc.x).hypot(ps.y - pc.y) - (pe.x - pc.x).hypot(pe.y - pc.y)).abs() < EPS);
        // 6 coordinates - 1 intrinsic equation
        assert_eq!(report.dof, 5);
    }

    #[test]
    fn test_tangent_line_and_circle() {
        let mut sketch = Sketch::new("T");
        let c = sketch.add_point(0.0, 0.0);
        let circle = sketch.add_circle(c, 5.0).unwrap();
        let a = sketch.add_point(-10.0, 8.0);
        let b = sketch.add_point(10.0, 7.0);
        let l = sketch.add_line(a, b).unwrap();
        sketch.add_constraint(Constraint::fixed(c, 0.0, 0.0));
        sketch.add_constraint(Constraint::radius(circle, 5.0));
        sketch.add_constraint(Constraint::horizontal(l));
        sketch.add_constraint(Constraint::new(ConstraintType::Tangent { line: l, curve: circle }));
        sketch.solve().unwrap();
        let (pa, pb) = (sketch.get_point(a).unwrap(), sketch.get_point(b).unwrap());
        assert!((pa.y - 5.0).abs() < EPS && (pb.y - 5.0).abs() < EPS, "{:?} {:?}", pa, pb);
    }

    #[test]
    fn test_point_on_line_circle_midpoint_equal() {
        let mut sketch = Sketch::new("M");
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(10.0, 0.0);
        let l = sketch.add_line(a, b).unwrap();
        let m = sketch.add_point(3.0, 2.0);
        let q = sketch.add_point(4.0, 4.0);
        let c = sketch.add_point(20.0, 0.0);
        let circle = sketch.add_circle(c, 3.0).unwrap();
        let c2 = sketch.add_point(40.0, 0.0);
        let circle2 = sketch.add_circle(c2, 1.0).unwrap();
        sketch.add_constraint(Constraint::new(ConstraintType::Midpoint { point: m, line: l }));
        sketch.add_constraint(Constraint::new(ConstraintType::PointOnCircle { point: q, curve: circle }));
        sketch.add_constraint(Constraint::new(ConstraintType::Equal { e1: circle, e2: circle2 }));
        let p = sketch.add_point(1.0, 5.0);
        sketch.add_constraint(Constraint::new(ConstraintType::PointOnLine { point: p, line: l }));
        sketch.solve().unwrap();

        let get = |id| sketch.get_point(id).unwrap();
        let (pa, pb, pm) = (get(a), get(b), get(m));
        assert!((pm.x - (pa.x + pb.x) / 2.0).abs() < EPS && (pm.y - (pa.y + pb.y) / 2.0).abs() < EPS);
        let r = |id| match sketch.elements.get(&id) {
            Some(SketchElement::Circle(c)) => c.radius,
            _ => unreachable!(),
        };
        assert!((r(circle) - r(circle2)).abs() < EPS);
        let (pq, pc) = (get(q), get(c));
        assert!(((pq.x - pc.x).hypot(pq.y - pc.y) - r(circle)).abs() < EPS);
        let pp = get(p);
        let cross = (pb.x - pa.x) * (pp.y - pa.y) - (pb.y - pa.y) * (pp.x - pa.x);
        assert!(cross.abs() < 1e-5);
    }

    #[test]
    fn test_unknown_reference_is_error() {
        let mut sketch = Sketch::new("Bad");
        sketch.add_constraint(Constraint::fixed(Id::new(), 0.0, 0.0));
        assert!(sketch.solve().is_err());
    }
}
