//! Exact B-Rep solids built on the Truck kernel

use crate::plane::Plane;
use crate::profile::{Loop, Profile, Segment};
use glam::{DMat4, DVec2, DVec3};
use nova_core::{NovaError, Result};
use std::panic::{catch_unwind, AssertUnwindSafe};
use truck_meshalgo::prelude::*;
use truck_modeling::{builder, Matrix4, Point3, Rad, Vector3, Wire};

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

/// A profile revolved around an axis lying in its sketch plane
#[derive(Clone, Debug)]
pub struct Revolve {
    pub profile: Profile,
    pub plane: Plane,
    /// Axis point and direction, in sketch coordinates
    pub axis_origin: DVec2,
    pub axis_dir: DVec2,
    /// Angle where the sweep starts, measured from the sketch plane (radians)
    pub start_angle: f64,
    /// Swept angle in radians (0 < angle ≤ 2π)
    pub angle: f64,
}

impl Revolve {
    pub fn new(profile: Profile, plane: Plane, axis_origin: DVec2, axis_dir: DVec2, angle: f64) -> Self {
        Revolve { profile, plane, axis_origin, axis_dir, start_angle: 0.0, angle }
    }

    /// Axis in world space (point, unit direction)
    pub fn world_axis(&self) -> (DVec3, DVec3) {
        let o = self.plane.to_world(self.axis_origin);
        let d = (self.plane.x_axis * self.axis_dir.x + self.plane.y_axis * self.axis_dir.y).normalize();
        (o, d)
    }
}

#[derive(Clone, Debug)]
pub enum ToolShape {
    Prism(Prism),
    Revolve(Revolve),
}

/// A feature's material: a shape placed by a rigid transform (or a mirror)
#[derive(Clone, Debug)]
pub struct Tool {
    pub shape: ToolShape,
    pub transform: DMat4,
}

impl From<Prism> for Tool {
    fn from(p: Prism) -> Self {
        Tool { shape: ToolShape::Prism(p), transform: DMat4::IDENTITY }
    }
}

impl From<&Prism> for Tool {
    fn from(p: &Prism) -> Self {
        p.clone().into()
    }
}

impl From<&Revolve> for Tool {
    fn from(r: &Revolve) -> Self {
        r.clone().into()
    }
}

impl From<&Tool> for Tool {
    fn from(t: &Tool) -> Self {
        t.clone()
    }
}

impl From<Revolve> for Tool {
    fn from(r: Revolve) -> Self {
        Tool { shape: ToolShape::Revolve(r), transform: DMat4::IDENTITY }
    }
}

impl Tool {
    /// The same tool moved by `m` (applied after the current transform)
    pub fn transformed(&self, m: DMat4) -> Tool {
        Tool { shape: self.shape.clone(), transform: m * self.transform }
    }

    /// A full revolution of a profile whose edges are all parallel or
    /// perpendicular to the axis (grooves, shoulders, bores) is exactly a
    /// stack of rings extruded along the axis. Truck's booleans handle such
    /// prisms far better than revolved surfaces.
    pub fn as_axial_prisms(&self) -> Option<Vec<Tool>> {
        let ToolShape::Revolve(r) = &self.shape else { return None };
        if r.angle < std::f64::consts::TAU - 1e-9 {
            return None;
        }
        let dir = r.axis_dir.normalize();
        let profile = r.profile.normalized();
        let loops: Vec<&Loop> = std::iter::once(&profile.outer).chain(profile.holes.iter()).collect();
        // (axial t, radial r) coordinates of every segment
        let to_tr = |p: DVec2| ((p - r.axis_origin).dot(dir), dir.perp_dot(p - r.axis_origin).abs());
        let mut axial_edges = Vec::new(); // edges parallel to the axis: (t0, t1, r)
        let mut ts = Vec::new();
        for lp in &loops {
            for (i, seg) in lp.segments.iter().enumerate() {
                let Segment::Line { start } = *seg else { return None };
                let (t0, r0) = to_tr(start);
                let (t1, r1) = to_tr(lp.end_of(i));
                let scale = 1e-9 * (1.0 + t0.abs().max(r0));
                if (r0 - r1).abs() <= scale {
                    axial_edges.push((t0.min(t1), t0.max(t1), r0));
                } else if (t0 - t1).abs() > scale {
                    return None; // slanted edge (cone)
                }
                ts.push(t0);
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        let (origin, axis) = r.world_axis();
        let x = axis.any_orthonormal_vector();
        let mut tools = Vec::new();
        for w in ts.windows(2) {
            let (ta, tb) = (w[0], w[1]);
            let mid = (ta + tb) / 2.0;
            let mut radii: Vec<f64> = axial_edges.iter().filter(|e| e.0 < mid && mid < e.1).map(|e| e.2).collect();
            radii.sort_by(f64::total_cmp);
            let plane = Plane::new(origin + axis * ta, x, axis);
            for pair in radii.chunks_exact(2) {
                let (rin, rout) = (pair[0], pair[1]);
                if rout - rin < 1e-9 {
                    continue;
                }
                let holes = if rin > 1e-9 { vec![Loop::circle(DVec2::ZERO, rin)] } else { Vec::new() };
                let prism = Prism::new(Profile::new(Loop::circle(DVec2::ZERO, rout), holes), plane, 0.0, tb - ta);
                tools.push(Tool { shape: ToolShape::Prism(prism), transform: self.transform });
            }
        }
        (!tools.is_empty()).then_some(tools)
    }
}

/// Translation matrix
pub fn translation(v: DVec3) -> DMat4 {
    DMat4::from_translation(v)
}

/// Rotation by `angle` (radians) around an axis through `origin`
pub fn rotation_about(origin: DVec3, axis: DVec3, angle: f64) -> DMat4 {
    DMat4::from_translation(origin) * DMat4::from_axis_angle(axis.normalize(), angle) * DMat4::from_translation(-origin)
}

/// Reflection through a plane
pub fn mirror(plane: &Plane) -> DMat4 {
    let n = plane.normal();
    let r = glam::DMat3::IDENTITY - 2.0 * glam::DMat3::from_cols(n * n.x, n * n.y, n * n.z);
    DMat4::from_translation(plane.origin) * DMat4::from_mat3(r) * DMat4::from_translation(-plane.origin)
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
        build_prism(prism, 0.0, 0.0, 0.0)
    }

    /// Build a tool's solid
    pub fn from_tool(tool: &Tool) -> Result<Solid> {
        build_tool(tool, Perturb::NONE)
    }

    /// Combine this body with a prism tool.
    ///
    /// Truck's booleans fail on coplanar faces, which are the normal case in
    /// CAD (a boss sketched on a face). When the exact operation fails, the
    /// tool is extended along its axis, then also grown in its plane, by a
    /// few microns — far below manufacturing tolerances.
    pub fn boolean(&self, op: BooleanOp, tool: impl Into<Tool>) -> Result<Solid> {
        let tool = tool.into();
        if let Some(rings) = tool.as_axial_prisms() {
            let mut body = self.clone();
            for ring in rings {
                body = body.boolean_single(op, &ring)?;
            }
            return Ok(body);
        }
        self.boolean_single(op, &tool)
    }

    fn boolean_single(&self, op: BooleanOp, tool: &Tool) -> Result<Solid> {
        let size = self.size().max(tool_size(tool));
        // Must exceed Truck's internal geometric tolerance to break coplanarity
        let eps = (size * 1e-6).max(1e-3);
        let base_volume = self.quick_volume();
        // Cuts start on a face of the body almost every time, so the exact
        // attempt would only fail slowly
        // Third value: seam rotation for full revolutions (no geometric change,
        // but keeps the tool's seam away from the body's seams)
        let p = |start: f64, end: f64, grow: f64| Perturb { start: start * eps, end: end * eps, grow: grow * eps, seam: 0.37 };
        // Extending only one end first keeps the other (often free) face exact
        let attempts: Vec<Perturb> = match (op, &tool.shape) {
            // Revolved tools rarely succeed in Truck: fail fast
            (_, ToolShape::Revolve(_)) => vec![Perturb::NONE, p(0.0, 0.0, 1.0)],
            (BooleanOp::Union, _) => vec![
                Perturb::NONE,
                p(1.0, 0.0, 0.0),
                p(0.0, 1.0, 0.0),
                p(1.0, 1.0, 0.0),
                p(1.0, 0.0, 1.0),
                p(1.0, 1.0, 1.0),
                p(10.0, 10.0, 10.0),
            ],
            (BooleanOp::Subtract, _) => vec![
                p(1.0, 1.0, 0.0),
                p(0.0, 1.0, 0.0),
                p(1.0, 0.0, 0.0),
                p(1.0, 1.0, 1.0),
                p(10.0, 10.0, 0.0),
                p(37.0, 37.0, 0.0),
                p(10.0, 10.0, 10.0),
                Perturb::NONE,
            ],
        };
        for perturb in attempts {
            let Ok(tool_solid) = build_tool(tool, perturb) else { continue };
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
        if matches!(tool.shape, ToolShape::Revolve(_)) {
            return Err(NovaError::GeometryError(
                "the kernel cannot combine this revolved shape with the body (only full turns of profiles with edges parallel or perpendicular to the axis are supported)".into(),
            ));
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

fn tool_size(tool: &Tool) -> f64 {
    match &tool.shape {
        ToolShape::Prism(p) => {
            let (lo, hi) = p.profile.bounds();
            (hi - lo).length().hypot(p.end - p.start)
        }
        ToolShape::Revolve(r) => {
            let (lo, hi) = r.profile.bounds();
            2.0 * ((lo - r.axis_origin).length().max((hi - r.axis_origin).length()) + (hi - lo).length())
        }
    }
}

fn to_matrix(m: DMat4) -> Matrix4 {
    Matrix4::from(m.to_cols_array_2d())
}

/// Build a tool (see `build_prism` for `extend`/`grow`)
/// Tiny tool changes used to work around Truck's coplanar-face failures
#[derive(Clone, Copy, Debug)]
struct Perturb {
    /// Extension before the start / after the end of a prism
    start: f64,
    end: f64,
    /// Growth of the profile in its plane
    grow: f64,
    /// Seam rotation of full revolutions (radians, no geometric change)
    seam: f64,
}

impl Perturb {
    const NONE: Perturb = Perturb { start: 0.0, end: 0.0, grow: 0.0, seam: 0.0 };
}

fn build_tool(tool: &Tool, perturb: Perturb) -> Result<Solid> {
    let solid = match &tool.shape {
        ToolShape::Prism(p) => build_prism(p, perturb.start, perturb.end, perturb.grow)?,
        ToolShape::Revolve(r) => build_revolve(r, perturb.grow, perturb.seam)?,
    };
    if tool.transform == DMat4::IDENTITY {
        return Ok(solid);
    }
    let moved: TruckSolid = builder::transformed(&solid.inner, to_matrix(tool.transform));
    Ok(oriented(moved))
}

/// Make sure the solid's faces point outwards (mirrors and some sweeps flip them)
fn oriented(mut solid: TruckSolid) -> Solid {
    let s = Solid { inner: solid.clone() };
    if s.tessellate(s.size().max(1.0) * 1e-2).volume() < 0.0 {
        solid.not();
        return Solid { inner: solid };
    }
    s
}

fn build_revolve(rev: &Revolve, grow: f64, seam: f64) -> Result<Solid> {
    let full = rev.angle >= std::f64::consts::TAU - 1e-9;
    if !(rev.angle > 1e-9 && rev.angle <= std::f64::consts::TAU + 1e-9) {
        return Err(NovaError::InvalidFeature("revolve angle must be between 0 and 360°".into()));
    }
    if rev.axis_dir.length() < 1e-12 {
        return Err(NovaError::InvalidFeature("revolve axis has no direction".into()));
    }
    let profile = rev.profile.normalized();
    profile.outer.validate()?;
    for h in &profile.holes {
        h.validate()?;
    }
    // The profile must stay on one side of the axis
    let dir = rev.axis_dir.normalize();
    let size = {
        let (lo, hi) = profile.bounds();
        (hi - lo).length().max(1e-9)
    };
    let side = |p: DVec2| dir.perp_dot(p - rev.axis_origin);
    let samples = profile.outer.sample(24);
    let (min, max) = samples.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(side(*p)), b.max(side(*p))));
    if min < -1e-7 * size && max > 1e-7 * size {
        return Err(NovaError::InvalidFeature("the profile crosses the revolve axis".into()));
    }

    let (lo, hi) = profile.bounds();
    let center = (lo + hi) / 2.0;
    let half = ((hi - lo) / 2.0).max_element().max(1e-9);
    let factor = 1.0 + grow / half;
    let map = |p: DVec2| center + (p - center) * factor;
    let hole_map = |p: DVec2| center + (p - center) / factor;
    let mut wires = vec![loop_to_wire(&profile.outer, &rev.plane, &map)?];
    for h in &profile.holes {
        wires.push(loop_to_wire(h, &rev.plane, &hole_map)?);
    }
    let face = catch_unwind(AssertUnwindSafe(|| builder::try_attach_plane(&wires)))
        .map_err(|_| NovaError::GeometryError("profile is not a valid planar region".into()))?
        .map_err(|e| NovaError::GeometryError(format!("profile is not a valid planar region: {e}")))?;
    let (origin, axis) = rev.world_axis();
    let (o, a) = (to_point(origin), Vector3::new(axis.x, axis.y, axis.z));
    let start = rev.start_angle + if full { seam } else { 0.0 };
    let face = if start.abs() > 1e-12 {
        builder::rotated(&face, o, a, Rad(start))
    } else {
        face
    };
    let solid = catch_unwind(AssertUnwindSafe(|| -> TruckSolid { builder::rsweep(&face, o, a, Rad(rev.angle)) }))
        .map_err(|_| NovaError::GeometryError("revolve failed".into()))?;
    Ok(oriented(solid))
}

fn to_point(p: DVec3) -> Point3 {
    Point3::new(p.x, p.y, p.z)
}

/// Build a prism, optionally extended before its start / after its end and
/// grown by `grow` (absolute, in plane) around the profile centre.
fn build_prism(prism: &Prism, extend_start: f64, extend_end: f64, grow: f64) -> Result<Solid> {
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
    let start = prism.start - extend_start;
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
    let depth = prism.end - prism.start + extend_start + extend_end;
    let n = prism.plane.normal() * depth;
    let solid: TruckSolid = builder::tsweep(&face, Vector3::new(n.x, n.y, n.z));
    // The face orientation chosen by Truck may oppose the sweep direction
    Ok(oriented(solid))
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
    fn revolve_full_turn_and_partial() {
        // Tube: r 5..10, height 20, around the sketch Y axis
        let p = Profile::simple(rect(5.0, 0.0, 5.0, 20.0));
        let tube = Solid::from_tool(&Revolve::new(p.clone(), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI).into()).unwrap();
        assert!(close(tube.volume(), PI * 75.0 * 20.0, 1e-3), "{}", tube.volume());
        let half = Solid::from_tool(&Revolve::new(p, Plane::front(), DVec2::ZERO, DVec2::Y, PI).into()).unwrap();
        assert!(close(half.volume(), PI * 75.0 * 10.0, 1e-3), "{}", half.volume());
        // Profile touching the axis: a plain cylinder
        let shaft = Solid::from_tool(
            &Revolve::new(Profile::simple(rect(0.0, 0.0, 10.0, 20.0)), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI).into(),
        )
        .unwrap();
        assert!(close(shaft.volume(), PI * 100.0 * 20.0, 1e-3), "{}", shaft.volume());
    }

    #[test]
    fn revolve_rejects_profile_across_axis() {
        let r = Revolve::new(Profile::simple(rect(-5.0, 0.0, 10.0, 10.0)), Plane::front(), DVec2::ZERO, DVec2::Y, PI);
        assert!(Solid::from_tool(&r.into()).is_err());
    }

    /// Known Truck limitation: cutting a ring (a tool with a hole) out of a
    /// curved body fails. It must fail cleanly, not panic or return garbage.
    #[test]
    fn revolved_groove_cut_fails_cleanly() {
        let shaft: Tool = Revolve::new(Profile::simple(rect(0.0, 0.0, 10.0, 40.0)), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI).into();
        let shaft = Solid::from_tool(&shaft).unwrap();
        let groove = Revolve::new(Profile::simple(rect(8.0, 18.0, 4.0, 4.0)), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI);
        match shaft.boolean(BooleanOp::Subtract, groove) {
            Err(e) => assert!(e.to_string().contains("failed") || e.to_string().contains("cannot"), "{e}"),
            Ok(cut) => {
                // If a future Truck manages it, the result must be right
                let expected = PI * 100.0 * 40.0 - PI * (100.0 - 64.0) * 4.0;
                assert!(close(cut.volume(), expected, 2e-3), "{}", cut.volume());
            }
        }
    }

    #[test]
    fn prism_cuts_on_a_revolved_body() {
        // Cross hole through a revolved shaft
        let shaft: Tool = Revolve::new(Profile::simple(rect(0.0, 0.0, 10.0, 40.0)), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI).into();
        let shaft = Solid::from_tool(&shaft).unwrap();
        let hole = Prism::new(Profile::simple(Loop::circle(DVec2::new(0.0, 20.0), 3.0)), Plane::front(), -15.0, 15.0);
        let cut = shaft.boolean(BooleanOp::Subtract, hole).unwrap();
        assert!(cut.volume() < shaft.volume() - 500.0 && cut.volume() > shaft.volume() - 700.0, "{}", cut.volume());
    }

    #[test]
    fn stepped_profile_decomposes_into_rings() {
        // L-shaped profile: shoulder r 0..10 for y 0..10, then r 0..5 for y 10..30
        let l = Loop::polygon(&[
            DVec2::new(0.0, 0.0),
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 10.0),
            DVec2::new(5.0, 10.0),
            DVec2::new(5.0, 30.0),
            DVec2::new(0.0, 30.0),
        ]);
        let tool: Tool = Revolve::new(Profile::simple(l), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI).into();
        let rings = tool.as_axial_prisms().unwrap();
        assert_eq!(rings.len(), 2);
        let v: f64 = rings.iter().map(|t| Solid::from_tool(t).unwrap().volume()).sum();
        assert!(close(v, PI * (100.0 * 10.0 + 25.0 * 20.0), 1e-3), "{}", v);
        // Partial turns and slanted edges are not decomposed
        let tri = Loop::polygon(&[DVec2::new(1.0, 0.0), DVec2::new(5.0, 0.0), DVec2::new(1.0, 5.0)]);
        let cone: Tool = Revolve::new(Profile::simple(tri), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI).into();
        assert!(cone.as_axial_prisms().is_none());
    }

    #[test]
    fn transformed_tools_pattern_and_mirror() {
        let hole = Prism::new(Profile::simple(Loop::circle(DVec2::new(15.0, 30.0), 4.0)), Plane::front(), 0.0, 20.0);
        let mut body = block();
        for i in 0..3 {
            let t = Tool::from(hole.clone()).transformed(translation(DVec3::new(30.0 * i as f64, 0.0, 0.0)));
            body = body.boolean(BooleanOp::Subtract, t).unwrap();
        }
        let expected = 120_000.0 - 3.0 * PI * 16.0 * 20.0;
        assert!(close(body.volume(), expected, 2e-4), "{}", body.volume());

        // Mirror of a boss across the plane x = 50 lands on the other side
        let boss = Prism::new(Profile::simple(rect(5.0, 5.0, 10.0, 10.0)), Plane::front(), 20.0, 30.0);
        let m = mirror(&Plane::new(DVec3::new(50.0, 0.0, 0.0), DVec3::Y, DVec3::X));
        let mirrored = Solid::from_tool(&Tool::from(boss).transformed(m)).unwrap();
        let (lo, hi) = mirrored.bounds().unwrap();
        assert!((lo.x - 85.0).abs() < 1e-6 && (hi.x - 95.0).abs() < 1e-6, "{:?} {:?}", lo, hi);
        assert!((lo.z - 20.0).abs() < 1e-6 && (hi.z - 30.0).abs() < 1e-6);
        assert!(close(mirrored.volume(), 1000.0, 1e-6));

        // Circular pattern: rotate a hole 90° about the block centre's Z axis
        let r = rotation_about(DVec3::new(50.0, 30.0, 0.0), DVec3::Z, std::f64::consts::FRAC_PI_2);
        let moved = Solid::from_tool(&Tool::from(hole).transformed(r)).unwrap();
        let (lo, hi) = moved.bounds().unwrap();
        let c = (lo + hi) / 2.0;
        assert!((c.x - 50.0).abs() < 1e-3 && (c.y - (30.0 - 35.0)).abs() < 1e-3, "{:?}", c);
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
