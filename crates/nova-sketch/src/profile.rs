//! Turn sketch geometry into closed profiles for features
//!
//! Lines and arcs are joined end to end into loops (end points closer than a
//! small tolerance count as joined). Circles are loops on their own. Loops
//! nested inside another loop become holes of it; a loop inside a hole is a
//! new island, as in SolidWorks.

use crate::{Sketch, SketchElement};
use glam::DVec2;
use nova_core::{Id, NovaError, Result};
use nova_kernel::{Loop, Profile, Segment};
use std::collections::HashMap;

enum EdgeKind {
    Line,
    Arc { through: DVec2 },
}

struct Edge {
    a: usize,
    b: usize,
    kind: EdgeKind,
}

/// Extract the closed regions of a sketch, largest first
pub fn extract_profiles(sketch: &Sketch) -> Result<Vec<Profile>> {
    let points: HashMap<Id, DVec2> = sketch
        .elements
        .values()
        .filter_map(|e| match e {
            SketchElement::Point(p) => Some((p.id, DVec2::new(p.x, p.y))),
            _ => None,
        })
        .collect();
    let pos = |id: &Id| -> Result<DVec2> {
        points
            .get(id)
            .copied()
            .ok_or_else(|| NovaError::InvalidSketch(format!("unknown point {}", id)))
    };

    // Deterministic element order
    let mut elements: Vec<&SketchElement> = sketch.elements.values().collect();
    elements.sort_by_key(|e| e.id().uuid());

    let size = points.values().fold(0.0f64, |m, p| m.max(p.abs().max_element())).max(1.0);
    let merge_tol = size * 1e-7;

    // Nodes: merged end points
    let mut nodes: Vec<DVec2> = Vec::new();
    let mut node_of = |p: DVec2| -> usize {
        if let Some(i) = nodes.iter().position(|n| (*n - p).length() <= merge_tol) {
            i
        } else {
            nodes.push(p);
            nodes.len() - 1
        }
    };

    let mut edges = Vec::new();
    let mut loops = Vec::new();
    for e in elements {
        match e {
            SketchElement::Line(l) if l.construction => {}
            SketchElement::Line(l) => {
                let (a, b) = (pos(&l.p1)?, pos(&l.p2)?);
                edges.push(Edge { a: node_of(a), b: node_of(b), kind: EdgeKind::Line });
            }
            SketchElement::Arc(arc) => {
                let (c, s, t) = (pos(&arc.center)?, pos(&arc.start)?, pos(&arc.end)?);
                let r = (s - c).length();
                let a0 = (s - c).y.atan2((s - c).x);
                let a1 = (t - c).y.atan2((t - c).x);
                let sweep = (a1 - a0).rem_euclid(std::f64::consts::TAU);
                if r < merge_tol || sweep < 1e-9 {
                    return Err(NovaError::InvalidSketch("degenerate arc in sketch".into()));
                }
                let mid = a0 + sweep / 2.0;
                let through = c + DVec2::new(mid.cos(), mid.sin()) * r;
                edges.push(Edge { a: node_of(s), b: node_of(t), kind: EdgeKind::Arc { through } });
            }
            SketchElement::Circle(circle) => {
                loops.push(Loop::circle(pos(&circle.center)?, circle.radius));
            }
            SketchElement::Point(_) => {}
        }
    }

    // Every end point must join exactly two edges
    let mut incident: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, e) in edges.iter().enumerate() {
        if e.a == e.b {
            return Err(NovaError::InvalidSketch("sketch contains a zero-length edge".into()));
        }
        incident[e.a].push(i);
        incident[e.b].push(i);
    }
    for (n, inc) in incident.iter().enumerate() {
        match inc.len() {
            0 | 2 => {}
            1 => {
                return Err(NovaError::InvalidSketch(format!(
                    "open contour: end point at ({:.3}, {:.3}) is not connected",
                    nodes[n].x, nodes[n].y
                )))
            }
            k => {
                return Err(NovaError::InvalidSketch(format!(
                    "{} edges meet at ({:.3}, {:.3}); contours must not branch",
                    k, nodes[n].x, nodes[n].y
                )))
            }
        }
    }

    // Walk the cycles
    let mut used = vec![false; edges.len()];
    for first in 0..edges.len() {
        if used[first] {
            continue;
        }
        let mut segments = Vec::new();
        let (mut edge, mut at) = (first, edges[first].a);
        loop {
            used[edge] = true;
            let e = &edges[edge];
            let next = if e.a == at { e.b } else { e.a };
            segments.push(match e.kind {
                EdgeKind::Line => Segment::Line { start: nodes[at] },
                EdgeKind::Arc { through } => Segment::Arc { start: nodes[at], through },
            });
            at = next;
            match incident[at].iter().find(|&&i| !used[i]) {
                Some(&i) => edge = i,
                None => break,
            }
        }
        loops.push(Loop::new(segments));
    }

    if loops.is_empty() {
        return Err(NovaError::InvalidSketch("sketch has no closed contour".into()));
    }
    for l in &loops {
        l.validate()?;
    }

    let polys: Vec<Vec<DVec2>> = loops.iter().map(|l| l.sample(24)).collect();
    check_intersections(&polys)?;

    // Nesting: parent = smallest loop containing this one
    let areas: Vec<f64> = loops.iter().map(|l| l.signed_area().abs()).collect();
    let parent: Vec<Option<usize>> = (0..loops.len())
        .map(|i| {
            let probe = polys[i][0];
            (0..loops.len())
                .filter(|&j| j != i && areas[j] > areas[i] && point_in_polygon(probe, &polys[j]))
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
        })
        .collect();
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(p) = parent[i] {
            d += 1;
            i = p;
        }
        d
    };

    let mut order: Vec<usize> = (0..loops.len()).filter(|&i| depth(i) % 2 == 0).collect();
    order.sort_by(|&a, &b| areas[b].total_cmp(&areas[a]));
    let profiles = order
        .into_iter()
        .map(|i| {
            let holes = (0..loops.len()).filter(|&j| parent[j] == Some(i)).map(|j| loops[j].clone()).collect();
            Profile::new(loops[i].clone(), holes).normalized()
        })
        .collect();
    Ok(profiles)
}

fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Strict crossing of segments ab and cd (touching end points do not count)
fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let o = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    let scale = (b - a).length().max((d - c).length()).max(1e-12);
    let eps = 1e-9 * scale * scale;
    let (d1, d2, d3, d4) = (o(a, b, c), o(a, b, d), o(c, d, a), o(c, d, b));
    ((d1 > eps && d2 < -eps) || (d1 < -eps && d2 > eps)) && ((d3 > eps && d4 < -eps) || (d3 < -eps && d4 > eps))
}

fn check_intersections(polys: &[Vec<DVec2>]) -> Result<()> {
    let segs: Vec<(usize, usize, DVec2, DVec2)> = polys
        .iter()
        .enumerate()
        .flat_map(|(li, p)| (0..p.len()).map(move |i| (li, i, p[i], p[(i + 1) % p.len()])))
        .collect();
    for (k, s) in segs.iter().enumerate() {
        for t in &segs[k + 1..] {
            if s.0 == t.0 {
                let n = polys[s.0].len();
                if (s.1 + 1) % n == t.1 || (t.1 + 1) % n == s.1 {
                    continue; // neighbours share an end point
                }
            }
            if segments_cross(s.2, s.3, t.2, t.3) {
                return Err(NovaError::InvalidSketch(format!(
                    "sketch contours intersect near ({:.3}, {:.3})",
                    s.2.x, s.2.y
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::Sketch;
    use std::f64::consts::PI;

    #[test]
    fn rectangle_with_circular_hole() {
        let mut s = Sketch::new("Plate");
        s.add_rectangle(0.0, 0.0, 100.0, 60.0).unwrap();
        let c = s.add_point(30.0, 30.0);
        s.add_circle(c, 10.0).unwrap();
        let profiles = s.profiles().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].holes.len(), 1);
        let area = profiles[0].area();
        assert!((area - (6000.0 - PI * 100.0)).abs() < 2.0, "{}", area);
    }

    #[test]
    fn island_inside_hole_is_separate_profile() {
        let mut s = Sketch::new("Rings");
        s.add_rectangle(0.0, 0.0, 100.0, 100.0).unwrap();
        s.add_rectangle(10.0, 10.0, 80.0, 80.0).unwrap();
        s.add_rectangle(30.0, 30.0, 40.0, 40.0).unwrap();
        let profiles = s.profiles().unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].holes.len(), 1);
        assert!(profiles[1].holes.is_empty());
    }

    #[test]
    fn open_contour_is_rejected() {
        let mut s = Sketch::new("Open");
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(10.0, 0.0);
        let c = s.add_point(10.0, 10.0);
        s.add_line(a, b).unwrap();
        s.add_line(b, c).unwrap();
        let err = s.profiles().unwrap_err().to_string();
        assert!(err.contains("open contour"), "{}", err);
    }

    #[test]
    fn separate_points_at_same_location_are_joined() {
        // Each line has its own end points, as after drawing then snapping
        let mut s = Sketch::new("Tri");
        let corners = [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)];
        for i in 0..3 {
            let (x0, y0) = corners[i];
            let (x1, y1) = corners[(i + 1) % 3];
            let a = s.add_point(x0, y0);
            let b = s.add_point(x1, y1);
            s.add_line(a, b).unwrap();
        }
        let p = s.profiles().unwrap();
        assert!((p[0].area() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn slot_with_arcs() {
        // Obround: two lines and two half arcs
        let mut s = Sketch::new("Slot");
        let p1 = s.add_point(0.0, -5.0);
        let p2 = s.add_point(20.0, -5.0);
        let p3 = s.add_point(20.0, 5.0);
        let p4 = s.add_point(0.0, 5.0);
        let c1 = s.add_point(20.0, 0.0);
        let c2 = s.add_point(0.0, 0.0);
        s.add_line(p1, p2).unwrap();
        s.add_arc(c1, p2, p3).unwrap();
        s.add_line(p3, p4).unwrap();
        s.add_arc(c2, p4, p1).unwrap();
        let p = s.profiles().unwrap();
        assert_eq!(p.len(), 1);
        let exact = 200.0 + PI * 25.0;
        assert!((p[0].area() - exact).abs() / exact < 1e-2, "{}", p[0].area());
    }

    #[test]
    fn crossing_contours_are_rejected() {
        let mut s = Sketch::new("X");
        s.add_rectangle(0.0, 0.0, 10.0, 10.0).unwrap();
        s.add_rectangle(5.0, 5.0, 10.0, 10.0).unwrap();
        assert!(s.profiles().unwrap_err().to_string().contains("intersect"));
    }

    #[test]
    fn centerlines_are_not_part_of_profiles() {
        let mut s = Sketch::new("Rev");
        s.add_rectangle(5.0, 0.0, 10.0, 20.0).unwrap();
        let a = s.add_point(0.0, -5.0);
        let b = s.add_point(0.0, 25.0);
        s.add_centerline(a, b).unwrap();
        let p = s.profiles().unwrap();
        assert_eq!(p.len(), 1);
        assert!((p[0].area() - 200.0).abs() < 1e-9);
        assert_eq!(s.centerlines().len(), 1);
        assert!(s.free_points().is_empty());
        s.add_point(1.0, 1.0);
        assert_eq!(s.free_points().len(), 1);
    }

    #[test]
    fn empty_sketch_has_no_profile() {
        assert!(Sketch::new("E").profiles().is_err());
    }
}
