//! Closed 2D profiles (lines and circular arcs) used by extrusions

use glam::DVec2;
use nova_core::{NovaError, Result};
use serde::{Deserialize, Serialize};

/// One segment of a loop. It starts at `start` and ends at the start of the
/// next segment in the loop (the last one wraps to the first).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    Line { start: DVec2 },
    /// Circular arc through `through`, a point strictly inside the arc
    Arc { start: DVec2, through: DVec2 },
}

impl Segment {
    pub fn start(&self) -> DVec2 {
        match *self {
            Segment::Line { start } | Segment::Arc { start, .. } => start,
        }
    }
}

/// A closed loop of segments
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    pub segments: Vec<Segment>,
}

impl Loop {
    pub fn new(segments: Vec<Segment>) -> Self {
        Loop { segments }
    }

    /// Closed polygon through the given corners
    pub fn polygon(points: &[DVec2]) -> Self {
        Loop::new(points.iter().map(|&start| Segment::Line { start }).collect())
    }

    /// Axis-aligned rectangle
    pub fn rectangle(min: DVec2, max: DVec2) -> Self {
        Loop::polygon(&[min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)])
    }

    /// Full circle, stored as two half arcs
    pub fn circle(center: DVec2, radius: f64) -> Self {
        Loop::new(vec![
            Segment::Arc {
                start: center + DVec2::new(radius, 0.0),
                through: center + DVec2::new(0.0, radius),
            },
            Segment::Arc {
                start: center - DVec2::new(radius, 0.0),
                through: center - DVec2::new(0.0, radius),
            },
        ])
    }

    /// End point of segment `i`
    pub fn end_of(&self, i: usize) -> DVec2 {
        self.segments[(i + 1) % self.segments.len()].start()
    }

    /// Polyline approximation (closed, last point not repeated)
    pub fn sample(&self, segments_per_arc: usize) -> Vec<DVec2> {
        let mut pts = Vec::new();
        for (i, seg) in self.segments.iter().enumerate() {
            match *seg {
                Segment::Line { start } => pts.push(start),
                Segment::Arc { start, through } => {
                    let end = self.end_of(i);
                    if let Some(arc) = ArcGeom::from_three_points(start, through, end) {
                        for k in 0..segments_per_arc {
                            pts.push(arc.point_at(k as f64 / segments_per_arc as f64));
                        }
                    } else {
                        pts.push(start);
                    }
                }
            }
        }
        pts
    }

    /// Signed area (positive when counter-clockwise)
    pub fn signed_area(&self) -> f64 {
        let pts = self.sample(32);
        let n = pts.len();
        (0..n).map(|i| pts[i].perp_dot(pts[(i + 1) % n])).sum::<f64>() / 2.0
    }

    /// Same loop traversed the other way
    pub fn reversed(&self) -> Self {
        let n = self.segments.len();
        let segments = (0..n)
            .rev()
            .map(|i| {
                let end = self.end_of(i);
                match self.segments[i] {
                    Segment::Line { .. } => Segment::Line { start: end },
                    Segment::Arc { through, .. } => Segment::Arc { start: end, through },
                }
            })
            .collect();
        Loop { segments }
    }

    /// Check the loop can be turned into geometry
    pub fn validate(&self) -> Result<()> {
        let n = self.segments.len();
        let has_arc = self.segments.iter().any(|s| matches!(s, Segment::Arc { .. }));
        if n < 2 || (n < 3 && !has_arc) {
            return Err(NovaError::InvalidSketch("profile loop needs at least 3 edges (or 2 arcs)".into()));
        }
        for i in 0..n {
            let start = self.segments[i].start();
            let end = self.end_of(i);
            if (end - start).length() < 1e-9 {
                return Err(NovaError::InvalidSketch(format!("profile edge {} has zero length", i)));
            }
            if let Segment::Arc { through, .. } = self.segments[i] {
                if ArcGeom::from_three_points(start, through, end).is_none() {
                    return Err(NovaError::InvalidSketch(format!("profile arc {} is degenerate", i)));
                }
            }
        }
        if self.signed_area().abs() < 1e-12 {
            return Err(NovaError::InvalidSketch("profile loop has no area".into()));
        }
        Ok(())
    }
}

/// A region to extrude: one outer boundary and any number of holes
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

impl Profile {
    pub fn new(outer: Loop, holes: Vec<Loop>) -> Self {
        Profile { outer, holes }
    }

    pub fn simple(outer: Loop) -> Self {
        Profile { outer, holes: Vec::new() }
    }

    /// Outer loop counter-clockwise, holes clockwise
    pub fn normalized(&self) -> Self {
        let outer = if self.outer.signed_area() < 0.0 { self.outer.reversed() } else { self.outer.clone() };
        let holes = self
            .holes
            .iter()
            .map(|h| if h.signed_area() > 0.0 { h.reversed() } else { h.clone() })
            .collect();
        Profile { outer, holes }
    }

    /// Bounding box in sketch coordinates
    pub fn bounds(&self) -> (DVec2, DVec2) {
        let pts = self.outer.sample(16);
        let min = pts.iter().fold(DVec2::splat(f64::MAX), |a, p| a.min(*p));
        let max = pts.iter().fold(DVec2::splat(f64::MIN), |a, p| a.max(*p));
        (min, max)
    }

    /// Net area (outer minus holes)
    pub fn area(&self) -> f64 {
        self.outer.signed_area().abs() - self.holes.iter().map(|h| h.signed_area().abs()).sum::<f64>()
    }
}

/// Circle through three points, parameterised from start to end
#[derive(Clone, Copy, Debug)]
pub struct ArcGeom {
    pub center: DVec2,
    pub radius: f64,
    pub start_angle: f64,
    /// Signed sweep (positive = counter-clockwise)
    pub sweep: f64,
}

impl ArcGeom {
    pub fn from_three_points(a: DVec2, b: DVec2, c: DVec2) -> Option<Self> {
        let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
        let scale = (b - a).length().max((c - a).length()).max(1e-300);
        if d.abs() < 1e-12 * scale * scale {
            return None;
        }
        let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
        let center = DVec2::new(
            (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
            (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
        );
        let angle = |p: DVec2| (p - center).y.atan2((p - center).x);
        let tau = std::f64::consts::TAU;
        let (sa, sb, sc) = (angle(a), angle(b), angle(c));
        let ccw = |from: f64, to: f64| (to - from).rem_euclid(tau);
        // Counter-clockwise if going CCW from a reaches b before c
        let sweep = if ccw(sa, sb) < ccw(sa, sc) { ccw(sa, sc) } else { ccw(sa, sc) - tau };
        Some(ArcGeom {
            center,
            radius: (a - center).length(),
            start_angle: sa,
            sweep,
        })
    }

    pub fn point_at(&self, t: f64) -> DVec2 {
        let ang = self.start_angle + self.sweep * t;
        self.center + DVec2::new(ang.cos(), ang.sin()) * self.radius
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_area_and_orientation() {
        let r = Loop::rectangle(DVec2::ZERO, DVec2::new(100.0, 60.0));
        assert!((r.signed_area() - 6000.0).abs() < 1e-9);
        assert!((r.reversed().signed_area() + 6000.0).abs() < 1e-9);
    }

    #[test]
    fn circle_area() {
        let c = Loop::circle(DVec2::new(5.0, 5.0), 10.0);
        let exact = std::f64::consts::PI * 100.0;
        assert!((c.signed_area() - exact).abs() / exact < 1e-2);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn arc_through_three_points() {
        let arc = ArcGeom::from_three_points(DVec2::new(1.0, 0.0), DVec2::new(0.0, 1.0), DVec2::new(-1.0, 0.0)).unwrap();
        assert!(arc.center.length() < 1e-12);
        assert!((arc.sweep - std::f64::consts::PI).abs() < 1e-12);
        let cw = ArcGeom::from_three_points(DVec2::new(1.0, 0.0), DVec2::new(0.0, -1.0), DVec2::new(-1.0, 0.0)).unwrap();
        assert!((cw.sweep + std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn normalized_orients_holes_clockwise() {
        let p = Profile::new(
            Loop::rectangle(DVec2::ZERO, DVec2::new(10.0, 10.0)).reversed(),
            vec![Loop::circle(DVec2::new(5.0, 5.0), 2.0)],
        )
        .normalized();
        assert!(p.outer.signed_area() > 0.0);
        assert!(p.holes[0].signed_area() < 0.0);
    }

    #[test]
    fn degenerate_loops_rejected() {
        assert!(Loop::polygon(&[DVec2::ZERO, DVec2::X]).validate().is_err());
        assert!(Loop::polygon(&[DVec2::ZERO, DVec2::X, DVec2::new(2.0, 0.0)]).validate().is_err());
    }
}
