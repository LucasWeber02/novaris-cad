//! Exact B-Rep solids built on the Truck kernel

use crate::plane::Plane;
use crate::profile::{Loop, Profile, Segment};
use glam::{DVec2, DVec3};
use nova_core::{NovaError, Result};
use std::panic::{catch_unwind, AssertUnwindSafe};
use truck_meshalgo::prelude::*;
use truck_modeling::{builder, Point3, Vector3, Wire};

type TruckSolid = truck_modeling::Solid;

/// Tolerance passed to Truck's boolean operations
const BOOLEAN_TOLERANCE: f64 = 0.05;

/// A prism: a profile swept along its plane normal from `start` to `end`
/// (distances measured from the plane, `start < end`)
#[derive(Clone, Debug)]
pub struct Prism {
    pub profile: Profile,
    pub plane: Plane,
    pub start: f64,
    pub end: f64,
}

impl Prism {
    pub fn new(profile: Profile, plane: Plane, start: f64, end: f64) -> Self {
        Prism { profile, plane, start, end }
    }
}

/// Boolean operation between a body and a tool
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    Subtract,
}

/// A closed solid body (possibly made of several disjoint lumps)
#[derive(Clone, Debug)]
pub struct Solid {
    inner: TruckSolid,
}

/// Triangle mesh ready for display, with B-Rep face ids per triangle
#[derive(Clone, Debug, Default)]
pub struct RenderMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// B-Rep face index of each triangle
    pub triangle_faces: Vec<u32>,
    /// Model edges as polylines
    pub edges: Vec<Vec<[f32; 3]>>,
}

impl RenderMesh {
    /// Enclosed volume (divergence theorem)
    pub fn volume(&self) -> f64 {
        self.indices
            .chunks_exact(3)
            .map(|t| {
                let p = |i: u32| DVec3::from(self.positions[i as usize].map(f64::from));
                p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0
            })
            .sum()
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut it = self.positions.iter().map(|p| DVec3::from(p.map(f64::from)));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }
}

impl Solid {
    /// Build a solid prism
    pub fn prism(prism: &Prism) -> Result<Solid> {
        build_prism(prism, 0.0, 0.0)
    }

    /// Combine this body with a prism tool.
    ///
    /// Truck's booleans fail on coplanar faces, which are the normal case in
    /// CAD (a boss sketched on a face). When the exact operation fails, the
    /// tool is extended along its axis, then also grown in its plane, by a
    /// few microns — far below manufacturing tolerances.
    pub fn boolean(&self, op: BooleanOp, tool: &Prism) -> Result<Solid> {
        let size = self.size().max(tool_size(tool));
        // Must exceed Truck's internal geometric tolerance to break coplanarity
        let eps = (size * 1e-6).max(1e-3);
        let base_volume = self.quick_volume();
        // Cuts start on a face of the body almost every time, so the exact
        // attempt would only fail slowly
        let attempts: &[(f64, f64)] = match op {
            BooleanOp::Union => &[(0.0, 0.0), (eps, 0.0), (eps, eps), (10.0 * eps, 10.0 * eps)],
            BooleanOp::Subtract => &[(eps, 0.0), (eps, eps), (10.0 * eps, 10.0 * eps), (0.0, 0.0)],
        };
        for &(extend, grow) in attempts {
            let Ok(tool_solid) = build_prism(tool, extend, grow) else { continue };
            let tool_volume = tool_solid.quick_volume();
            let base = self.inner.clone();
            let mut t = tool_solid.inner;
            let result = catch_unwind(AssertUnwindSafe(|| match op {
                BooleanOp::Union => truck_shapeops::or(&base, &t, BOOLEAN_TOLERANCE),
                BooleanOp::Subtract => {
                    t.not();
                    truck_shapeops::and(&base, &t, BOOLEAN_TOLERANCE)
                }
            }));
            let Ok(Some(solid)) = result else { continue };
            if solid.boundaries().is_empty() {
                continue;
            }
            let solid = Solid { inner: solid };
            // Truck occasionally returns a wrong shell instead of failing:
            // reject results whose volume is impossible for the operation.
            let v = solid.quick_volume();
            let slack = 1e-3 * (base_volume + tool_volume);
            let plausible = match op {
                BooleanOp::Union => v + slack >= base_volume.max(tool_volume) && v <= base_volume + tool_volume + slack,
                BooleanOp::Subtract => v <= base_volume + slack && v + slack >= base_volume - tool_volume && v > slack,
            };
            if plausible {
                return Ok(solid);
            }
        }
        Err(NovaError::GeometryError(match op {
            BooleanOp::Union => "boss could not be merged with the body".into(),
            BooleanOp::Subtract => "cut failed or removed all material".into(),
        }))
    }

    /// Number of B-Rep faces
    pub fn face_count(&self) -> usize {
        self.inner.boundaries().iter().map(|s| s.len()).sum()
    }

    /// Number of disjoint lumps
    pub fn lump_count(&self) -> usize {
        self.inner.boundaries().len()
    }

    /// Triangulate for display; `tol` is the chord tolerance in model units
    pub fn tessellate(&self, tol: f64) -> RenderMesh {
        let meshed = self.inner.triangulation(tol.max(1e-5));
        let mut mesh = RenderMesh::default();
        let mut face_index = 0u32;
        let mut seen_edges = std::collections::HashSet::new();
        for shell in meshed.boundaries() {
            for face in shell.face_iter() {
                if let Some(mut poly) = face.surface() {
                    if !face.orientation() {
                        poly.invert();
                    }
                    let positions = poly.positions();
                    let normals = poly.normals();
                    for tri in poly.faces().triangle_iter() {
                        let a = positions[tri[0].pos];
                        let b = positions[tri[1].pos];
                        let c = positions[tri[2].pos];
                        let flat = (b - a).cross(c - a);
                        for v in tri {
                            let p = positions[v.pos];
                            let n = v
                                .nor
                                .and_then(|i| normals.get(i))
                                .unwrap_or(flat)
                                .normalize();
                            mesh.indices.push(mesh.positions.len() as u32);
                            mesh.positions.push([p.x as f32, p.y as f32, p.z as f32]);
                            mesh.normals.push([n.x as f32, n.y as f32, n.z as f32]);
                        }
                        mesh.triangle_faces.push(face_index);
                    }
                }
                face_index += 1;
            }
            for edge in shell.edge_iter() {
                if seen_edges.insert(edge.id()) {
                    let curve = edge.curve();
                    mesh.edges.push(curve.iter().map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect());
                }
            }
        }
        mesh
    }

    /// Volume from a fine tessellation
    pub fn volume(&self) -> f64 {
        self.tessellate(self.size() * 1e-4).volume()
    }

    /// Coarser volume, good enough for sanity checks
    fn quick_volume(&self) -> f64 {
        self.tessellate(self.size() * 2e-3).volume()
    }

    /// Diagonal of the bounding box (from a coarse tessellation)
    pub fn size(&self) -> f64 {
        self.bounds().map(|(lo, hi)| (hi - lo).length()).unwrap_or(0.0)
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        // Vertices bound polyhedral solids exactly; the tessellation also
        // covers curved faces.
        self.inner
            .triangulation(1.0)
            .boundaries()
            .iter()
            .flat_map(|s| s.face_iter())
            .filter_map(|f| f.surface())
            .flat_map(|poly| poly.positions().clone())
            .map(|p| DVec3::new(p.x, p.y, p.z))
            .fold(None, |acc, p| match acc {
                None => Some((p, p)),
                Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
            })
    }

    /// Export as an ISO 10303-21 (STEP AP203) string
    pub fn to_step(&self, name: &str) -> String {
        use truck_stepio::out;
        out::CompleteStepDisplay::new(
            out::StepModel::from(&self.inner.compress()),
            out::StepHeaderDescriptor {
                file_name: name.to_owned(),
                organization_system: "NOVARIS CAD".to_owned(),
                ..Default::default()
            },
        )
        .to_string()
    }
}

fn tool_size(tool: &Prism) -> f64 {
    let (lo, hi) = tool.profile.bounds();
    (hi - lo).length().hypot(tool.end - tool.start)
}

fn to_point(p: DVec3) -> Point3 {
    Point3::new(p.x, p.y, p.z)
}

/// Build a prism, optionally extended by `extend` at both ends and grown by
/// `grow` (absolute, in plane) around the profile centre.
fn build_prism(prism: &Prism, extend: f64, grow: f64) -> Result<Solid> {
    if prism.end - prism.start < 1e-9 {
        return Err(NovaError::InvalidFeature("extrusion depth must be positive".into()));
    }
    let profile = prism.profile.normalized();
    profile.outer.validate()?;
    for h in &profile.holes {
        h.validate()?;
    }

    let (lo, hi) = profile.bounds();
    let center = (lo + hi) / 2.0;
    let half = ((hi - lo) / 2.0).max_element().max(1e-9);
    let factor = 1.0 + grow / half;
    let map = |p: DVec2| center + (p - center) * factor;
    let start = prism.start - extend;
    let base_plane = prism.plane.offset(start);
    // Holes shrink when the outer boundary grows, keeping walls in place
    let hole_map = |p: DVec2| center + (p - center) / factor;

    let mut wires = vec![loop_to_wire(&profile.outer, &base_plane, &map)?];
    for h in &profile.holes {
        wires.push(loop_to_wire(h, &base_plane, &hole_map)?);
    }

    let face = catch_unwind(AssertUnwindSafe(|| builder::try_attach_plane(&wires)))
        .map_err(|_| NovaError::GeometryError("profile is not a valid planar region".into()))?
        .map_err(|e| NovaError::GeometryError(format!("profile is not a valid planar region: {e}")))?;
    let depth = prism.end - prism.start + 2.0 * extend;
    let n = prism.plane.normal() * depth;
    let mut solid: TruckSolid = builder::tsweep(&face, Vector3::new(n.x, n.y, n.z));

    // The face orientation chosen by Truck may oppose the sweep direction
    let mut s = Solid { inner: solid.clone() };
    if s.tessellate(1.0).volume() < 0.0 {
        solid.not();
        s = Solid { inner: solid };
    }
    Ok(s)
}

fn loop_to_wire(lp: &Loop, plane: &Plane, map: &dyn Fn(DVec2) -> DVec2) -> Result<Wire> {
    let world = |p: DVec2| to_point(plane.to_world(map(p)));
    let vertices: Vec<_> = lp.segments.iter().map(|s| builder::vertex(world(s.start()))).collect();
    let n = vertices.len();
    let edges = lp
        .segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            let (a, b) = (&vertices[i], &vertices[(i + 1) % n]);
            match *seg {
                Segment::Line { .. } => builder::line(a, b),
                Segment::Arc { through, .. } => builder::circle_arc(a, b, world(through)),
            }
        })
        .collect::<Vec<_>>();
    Ok(edges.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn rect(x0: f64, y0: f64, w: f64, h: f64) -> Loop {
        Loop::rectangle(DVec2::new(x0, y0), DVec2::new(x0 + w, y0 + h))
    }

    fn block() -> Solid {
        Solid::prism(&Prism::new(Profile::simple(rect(0.0, 0.0, 100.0, 60.0)), Plane::front(), 0.0, 20.0)).unwrap()
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1.0)
    }

    #[test]
    fn rectangular_block() {
        let b = block();
        assert!(close(b.volume(), 120_000.0, 1e-9), "{}", b.volume());
        assert_eq!(b.face_count(), 6);
        let (lo, hi) = b.bounds().unwrap();
        assert!((hi - lo - DVec3::new(100.0, 60.0, 20.0)).length() < 1e-9);
    }

    #[test]
    fn clockwise_profile_still_extrudes_upward() {
        let p = Prism::new(Profile::simple(rect(0.0, 0.0, 10.0, 10.0).reversed()), Plane::front(), 0.0, 5.0);
        let s = Solid::prism(&p).unwrap();
        assert!(close(s.volume(), 500.0, 1e-9));
        assert!(s.bounds().unwrap().0.z > -1e-9);
    }

    #[test]
    fn profile_with_hole() {
        let p = Profile::new(rect(0.0, 0.0, 100.0, 60.0), vec![Loop::circle(DVec2::new(20.0, 20.0), 5.0)]);
        let s = Solid::prism(&Prism::new(p, Plane::front(), 0.0, 20.0)).unwrap();
        assert!(close(s.volume(), 120_000.0 - PI * 25.0 * 20.0, 1e-4), "{}", s.volume());
    }

    #[test]
    fn extrusion_on_other_planes() {
        for plane in [Plane::top(), Plane::right()] {
            let s = Solid::prism(&Prism::new(Profile::simple(rect(0.0, 0.0, 10.0, 20.0)), plane, 0.0, 30.0)).unwrap();
            assert!(close(s.volume(), 6000.0, 1e-9));
            let (lo, hi) = s.bounds().unwrap();
            let along_normal = (hi - lo).dot(plane.normal()).abs();
            assert!((along_normal - 30.0).abs() < 1e-9);
        }
    }

    #[test]
    fn through_hole_cut() {
        let tool = Prism::new(Profile::simple(Loop::circle(DVec2::new(50.0, 30.0), 5.0)), Plane::front(), 0.0, 20.0);
        let cut = block().boolean(BooleanOp::Subtract, &tool).unwrap();
        assert!(close(cut.volume(), 120_000.0 - PI * 25.0 * 20.0, 1e-4), "{}", cut.volume());
    }

    #[test]
    fn blind_pocket_from_top_face() {
        let tool = Prism::new(Profile::simple(rect(10.0, 10.0, 30.0, 20.0)), Plane::front(), 15.0, 20.0);
        let cut = block().boolean(BooleanOp::Subtract, &tool).unwrap();
        assert!(close(cut.volume(), 120_000.0 - 3000.0, 1e-4), "{}", cut.volume());
    }

    #[test]
    fn boss_on_top_face() {
        let tool = Prism::new(Profile::simple(rect(20.0, 20.0, 20.0, 20.0)), Plane::front(), 20.0, 40.0);
        let u = block().boolean(BooleanOp::Union, &tool).unwrap();
        assert!(close(u.volume(), 128_000.0, 1e-4), "{}", u.volume());
        assert_eq!(u.lump_count(), 1);
    }

    #[test]
    fn boss_flush_with_side() {
        let tool = Prism::new(Profile::simple(rect(0.0, 10.0, 30.0, 20.0)), Plane::front(), 20.0, 40.0);
        let u = block().boolean(BooleanOp::Union, &tool).unwrap();
        assert!(close(u.volume(), 132_000.0, 1e-4), "{}", u.volume());
    }

    #[test]
    fn disjoint_boss_makes_second_lump() {
        let tool = Prism::new(Profile::simple(rect(200.0, 0.0, 10.0, 10.0)), Plane::front(), 0.0, 10.0);
        let u = block().boolean(BooleanOp::Union, &tool).unwrap();
        assert!(close(u.volume(), 121_000.0, 1e-4));
        assert_eq!(u.lump_count(), 2);
    }

    #[test]
    fn cut_removing_everything_is_an_error() {
        let tool = Prism::new(Profile::simple(rect(-10.0, -10.0, 200.0, 200.0)), Plane::front(), -5.0, 50.0);
        assert!(block().boolean(BooleanOp::Subtract, &tool).is_err());
    }

    #[test]
    fn tessellation_has_face_ids_and_edges() {
        let mesh = block().tessellate(0.1);
        assert_eq!(mesh.triangle_faces.len() * 3, mesh.indices.len());
        assert_eq!(*mesh.triangle_faces.iter().max().unwrap(), 5);
        assert_eq!(mesh.edges.len(), 12);
    }

    #[test]
    fn step_export() {
        let step = block().to_step("block.step");
        assert!(step.starts_with("ISO-10303-21;"));
        assert!(step.contains("MANIFOLD_SOLID_BREP") || step.contains("CLOSED_SHELL"));
        let table = truck_stepio::r#in::Table::from_step(&step).expect("STEP re-parses");
        assert_eq!(table.shell.len(), 1);
    }
}
