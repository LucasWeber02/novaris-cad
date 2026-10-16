//! Mate solver
//!
//! Each free component gets six variables: a small translation and a
//! rotation vector, applied about the component's own center on top of its
//! current placement (so the parametrisation never hits a singularity). Mates
//! become residual equations `r(x) = 0`, solved with Levenberg-Marquardt
//! like the sketch solver. A free component with no mate is left in place.
//!
//! Degrees of freedom come from a second, minimal set of equations (exactly
//! one row per removed degree of freedom), so the rank of its Jacobian tells
//! how many independent motions remain for each component and how many mates
//! are redundant.

use crate::Placement;
use glam::{DQuat, DVec3};
use nalgebra::{DMatrix, DVector};

/// Residual under which a mate counts as satisfied
pub(crate) const TOLERANCE: f64 = 1e-6;
const MAX_ITERATIONS: usize = 200;
const RANK_TOLERANCE: f64 = 1e-8;

/// Mate geometry in part coordinates
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Geom {
    Plane { point: DVec3, normal: DVec3 },
    Axis { point: DVec3, dir: DVec3 },
}

impl Geom {
    fn moved(&self, q: DQuat, t: DVec3) -> Geom {
        match *self {
            Geom::Plane { point, normal } => Geom::Plane { point: q * point + t, normal: q * normal },
            Geom::Axis { point, dir } => Geom::Axis { point: q * point + t, dir: q * dir },
        }
    }

    fn dir(&self) -> DVec3 {
        match *self {
            Geom::Plane { normal, .. } => normal,
            Geom::Axis { dir, .. } => dir,
        }
    }

    fn point(&self) -> DVec3 {
        match *self {
            Geom::Plane { point, .. } | Geom::Axis { point, .. } => point,
        }
    }
}

/// What a mate imposes
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Relation {
    Coincident,
    Parallel,
    Perpendicular,
    Distance(f64),
    Angle(f64),
    Concentric,
}

/// One mate between two bodies
#[derive(Clone, Copy, Debug)]
pub(crate) struct Equation {
    pub a: (usize, Geom),
    pub b: (usize, Geom),
    pub relation: Relation,
    pub aligned: bool,
}

/// A component as seen by the solver
#[derive(Clone, Copy, Debug)]
pub(crate) struct Body {
    pub placement: Placement,
    pub free: bool,
    /// Rotation center in part coordinates
    pub pivot: DVec3,
}

#[derive(Clone, Debug)]
pub(crate) struct Outcome {
    pub placements: Vec<Placement>,
    /// Remaining degrees of freedom per body (0 for fixed bodies)
    pub dof: Vec<usize>,
    /// Residual norm of each equation
    pub residuals: Vec<f64>,
    /// Equations rows that add no information
    pub redundant: usize,
    pub iterations: usize,
}

/// Can this relation link these two geometries?
pub(crate) fn supported(relation: Relation, a: &Geom, b: &Geom) -> bool {
    use Geom::*;
    match relation {
        Relation::Parallel | Relation::Perpendicular | Relation::Angle(_) => true,
        Relation::Concentric => matches!((a, b), (Axis { .. }, Axis { .. })),
        Relation::Coincident | Relation::Distance(_) => true,
    }
}

struct Problem<'a> {
    bodies: &'a [Body],
    eqs: &'a [Equation],
    /// Offset of each body's variables, if it moves
    offsets: Vec<Option<usize>>,
    n: usize,
}

impl Problem<'_> {
    /// Pose (rotation, translation) of body k for the variables `x`
    fn pose(&self, k: usize, x: &DVector<f64>) -> (DQuat, DVec3) {
        let b = &self.bodies[k];
        let (q0, t0) = (b.placement.rotation, b.placement.translation);
        let Some(o) = self.offsets[k] else { return (q0, t0) };
        let dt = DVec3::new(x[o], x[o + 1], x[o + 2]);
        let dq = DQuat::from_scaled_axis(DVec3::new(x[o + 3], x[o + 4], x[o + 5]));
        let c = q0 * b.pivot + t0;
        (dq * q0, dq * (t0 - c) + c + dt)
    }

    fn world(&self, side: (usize, Geom), x: &DVector<f64>) -> Geom {
        let (q, t) = self.pose(side.0, x);
        side.1.moved(q, t)
    }

    fn residuals(&self, x: &DVector<f64>, minimal: bool) -> DVector<f64> {
        let mut out = Vec::new();
        for eq in self.eqs {
            equation_residuals(eq, &self.world(eq.a, x), &self.world(eq.b, x), minimal, &mut out);
        }
        DVector::from_vec(out)
    }

    fn jacobian(&self, x: &DVector<f64>, minimal: bool) -> DMatrix<f64> {
        let r0 = self.residuals(x, minimal);
        let mut j = DMatrix::zeros(r0.len(), self.n);
        for c in 0..self.n {
            let h = 1e-6;
            let mut xp = x.clone();
            xp[c] += h;
            let mut xm = x.clone();
            xm[c] -= h;
            let d = (self.residuals(&xp, minimal) - self.residuals(&xm, minimal)) / (2.0 * h);
            j.set_column(c, &d);
        }
        j
    }
}

/// Two unit vectors perpendicular to `n` and to each other
fn basis(n: DVec3) -> (DVec3, DVec3) {
    let n = n.normalize_or_zero();
    let helper = if n.x.abs() < 0.6 { DVec3::X } else { DVec3::Y };
    let u = n.cross(helper).normalize_or_zero();
    (u, n.cross(u))
}

/// `b` along `a` (sign s): 3 rows when solving, 2 for the analysis
fn same_direction(a: DVec3, b: DVec3, s: f64, minimal: bool, out: &mut Vec<f64>) {
    if minimal {
        let (u, v) = basis(a);
        out.extend([u.dot(b), v.dot(b)]);
    } else {
        out.extend((a - s * b).to_array());
    }
}

/// `b` parallel to `a` with either sign
fn parallel_lines(a: DVec3, b: DVec3, minimal: bool, out: &mut Vec<f64>) {
    if minimal {
        let (u, v) = basis(a);
        out.extend([u.dot(b), v.dot(b)]);
    } else {
        out.extend(a.cross(b).to_array());
    }
}

/// Component of `d` across the direction `axis`
fn across(d: DVec3, axis: DVec3, minimal: bool, out: &mut Vec<f64>) {
    if minimal {
        let (u, v) = basis(axis);
        out.extend([u.dot(d), v.dot(d)]);
    } else {
        out.extend((d - d.dot(axis) * axis).to_array());
    }
}

fn equation_residuals(eq: &Equation, a: &Geom, b: &Geom, minimal: bool, out: &mut Vec<f64>) {
    let s = if eq.aligned { 1.0 } else { -1.0 };
    let (da, db) = (a.dir(), b.dir());
    let gap = b.point() - a.point();
    match eq.relation {
        Relation::Parallel => match (a, b) {
            (Geom::Plane { .. }, Geom::Plane { .. }) => same_direction(da, db, s, minimal, out),
            (Geom::Plane { .. }, Geom::Axis { .. }) | (Geom::Axis { .. }, Geom::Plane { .. }) => out.push(da.dot(db)),
            _ => parallel_lines(da, db, minimal, out),
        },
        Relation::Perpendicular => match (a, b) {
            (Geom::Plane { .. }, Geom::Axis { .. }) | (Geom::Axis { .. }, Geom::Plane { .. }) => {
                parallel_lines(da, db, minimal, out)
            }
            _ => out.push(da.dot(db)),
        },
        Relation::Angle(theta) => out.push(da.dot(db) - s * theta.cos()),
        Relation::Coincident | Relation::Distance(_) => {
            let d = if let Relation::Distance(d) = eq.relation { d } else { 0.0 };
            match (a, b) {
                (Geom::Plane { .. }, Geom::Plane { .. }) => {
                    same_direction(da, db, s, minimal, out);
                    out.push(gap.dot(da) - d);
                }
                (Geom::Plane { .. }, Geom::Axis { .. }) => {
                    out.push(da.dot(db));
                    out.push(gap.dot(da) - d);
                }
                (Geom::Axis { .. }, Geom::Plane { .. }) => {
                    out.push(db.dot(da));
                    out.push(-gap.dot(db) - d);
                }
                (Geom::Axis { .. }, Geom::Axis { .. }) => {
                    parallel_lines(da, db, minimal, out);
                    if d == 0.0 {
                        across(gap, da, minimal, out);
                    } else {
                        let off = gap - gap.dot(da) * da;
                        out.push(off.length() - d.abs());
                    }
                }
            }
        }
        Relation::Concentric => {
            parallel_lines(da, db, minimal, out);
            across(gap, da, minimal, out);
        }
    }
}

/// Bodies whose direction must flip by half a turn: Levenberg-Marquardt
/// cannot leave the exact opposite of the target orientation (zero gradient).
fn unflip(problem: &Problem, bodies: &mut [Body]) {
    for eq in problem.eqs {
        let needs_sign = matches!(eq.relation, Relation::Coincident | Relation::Distance(_) | Relation::Parallel)
            && matches!((eq.a.1, eq.b.1), (Geom::Plane { .. }, Geom::Plane { .. }));
        if !needs_sign {
            continue;
        }
        let s = if eq.aligned { 1.0 } else { -1.0 };
        let qa = bodies[eq.a.0].placement.rotation;
        let qb = bodies[eq.b.0].placement.rotation;
        let (na, nb) = (qa * eq.a.1.dir(), qb * eq.b.1.dir());
        if na.dot(s * nb) > -0.999 {
            continue;
        }
        let k = if bodies[eq.b.0].free {
            eq.b.0
        } else if bodies[eq.a.0].free {
            eq.a.0
        } else {
            continue;
        };
        let body = &mut bodies[k];
        let p = body.placement;
        let center = p.rotation * body.pivot + p.translation;
        let flip = DQuat::from_axis_angle(basis(nb).0, std::f64::consts::PI);
        body.placement = Placement { rotation: flip * p.rotation, translation: flip * (p.translation - center) + center };
    }
}

pub(crate) fn solve(bodies: &[Body], eqs: &[Equation]) -> Outcome {
    let mut bodies = bodies.to_vec();
    let mut offsets = vec![None; bodies.len()];
    let mut n = 0;
    for eq in eqs {
        for k in [eq.a.0, eq.b.0] {
            if bodies[k].free && offsets[k].is_none() {
                offsets[k] = Some(n);
                n += 6;
            }
        }
    }
    {
        let problem = Problem { bodies: &bodies, eqs, offsets: offsets.clone(), n };
        let mut flipped = bodies.clone();
        unflip(&problem, &mut flipped);
        bodies = flipped;
    }
    let problem = Problem { bodies: &bodies, eqs, offsets, n };
    let mut x = DVector::zeros(n);
    let mut iterations = levenberg_marquardt(&problem, &mut x);
    // Where an angle sits exactly at an extremum (e.g. parallel faces asked
    // to become perpendicular) the gradient vanishes: restart from a few
    // slightly turned positions and keep the best result.
    let mut cost = problem.residuals(&x, false).norm();
    for attempt in 1..=4 {
        if cost < TOLERANCE * 1e-3 {
            break;
        }
        let mut trial = DVector::from_fn(n, |i, _| {
            let phase = (i * 7 + attempt * 13) as f64;
            if i % 6 >= 3 { 0.2 * phase.sin() } else { 0.0 }
        });
        iterations += levenberg_marquardt(&problem, &mut trial);
        let c = problem.residuals(&trial, false).norm();
        if c < cost {
            cost = c;
            x = trial;
        }
    }

    let placements: Vec<Placement> = (0..bodies.len())
        .map(|k| {
            if problem.offsets[k].is_none() {
                return bodies[k].placement;
            }
            let (rotation, translation) = problem.pose(k, &x);
            Placement { rotation: rotation.normalize(), translation }
        })
        .collect();

    let mut residuals = Vec::with_capacity(eqs.len());
    for eq in eqs {
        let mut r = Vec::new();
        equation_residuals(eq, &problem.world(eq.a, &x), &problem.world(eq.b, &x), false, &mut r);
        residuals.push(r.iter().map(|v| v * v).sum::<f64>().sqrt());
    }

    // Degrees of freedom from the minimal equations at the solution
    let mut dof: Vec<usize> = bodies.iter().map(|b| if b.free { 6 } else { 0 }).collect();
    let mut redundant = 0;
    if n > 0 {
        let j = problem.jacobian(&x, true);
        let mut padded = DMatrix::zeros(j.nrows().max(n), n);
        padded.view_mut((0, 0), (j.nrows(), n)).copy_from(&j);
        let svd = padded.svd(false, true);
        let v_t = svd.v_t.expect("V requested");
        let max = svd.singular_values.iter().cloned().fold(0.0, f64::max);
        let null: Vec<usize> = (0..svd.singular_values.len())
            .filter(|&k| max == 0.0 || svd.singular_values[k] <= max * RANK_TOLERANCE)
            .collect();
        let rank = n - null.len();
        redundant = j.nrows().saturating_sub(rank);
        for (k, offset) in problem.offsets.iter().enumerate() {
            let Some(o) = offset else { continue };
            // Motions of this body allowed by the null space
            let block = DMatrix::from_fn(6, null.len(), |r, c| v_t[(null[c], o + r)]);
            dof[k] = matrix_rank(&block);
        }
    }

    Outcome { placements, dof, residuals, redundant, iterations }
}

fn matrix_rank(m: &DMatrix<f64>) -> usize {
    if m.nrows() == 0 || m.ncols() == 0 {
        return 0;
    }
    let sv = m.clone().svd(false, false).singular_values;
    // Null-space vectors are unit vectors: an absolute threshold is right here
    sv.iter().filter(|&&s| s > 1e-6).count()
}

fn levenberg_marquardt(p: &Problem, x: &mut DVector<f64>) -> usize {
    let mut r = p.residuals(x, false);
    let mut cost = r.norm_squared();
    if r.is_empty() || x.is_empty() {
        return 0;
    }
    let mut lambda = 1e-3;
    for iter in 0..MAX_ITERATIONS {
        if cost.sqrt() < TOLERANCE * 1e-3 {
            return iter;
        }
        let j = p.jacobian(x, false);
        let jt = j.transpose();
        let jtj = &jt * &j;
        let g = &jt * &r;
        let mut improved = false;
        while lambda < 1e12 {
            let mut a = jtj.clone();
            for d in 0..a.nrows() {
                a[(d, d)] += lambda;
            }
            let Some(step) = a.cholesky().map(|ch| ch.solve(&(-&g))) else {
                lambda *= 10.0;
                continue;
            };
            let candidate = &*x + &step;
            let r_new = p.residuals(&candidate, false);
            let cost_new = r_new.norm_squared();
            if cost_new < cost {
                let small = step.norm() < 1e-14 * (1.0 + x.norm());
                *x = candidate;
                r = r_new;
                cost = cost_new;
                lambda = (lambda / 10.0).max(1e-12);
                improved = !small;
                break;
            }
            lambda *= 10.0;
        }
        if !improved {
            return iter + 1;
        }
    }
    MAX_ITERATIONS
}
