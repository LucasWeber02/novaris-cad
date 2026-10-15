//! Exact B-Rep solids, built on OpenCascade through `nova-occt`

use crate::plane::Plane;
use crate::profile::{Profile, Segment};
use glam::{DMat3, DMat4, DVec2, DVec3};
use nova_core::{NovaError, Result};
use nova_occt::{Seg, Shape, P3};

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

impl From<Revolve> for Tool {
    fn from(r: Revolve) -> Self {
        Tool { shape: ToolShape::Revolve(r), transform: DMat4::IDENTITY }
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

impl Tool {
    /// The same tool moved by `m` (applied after the current transform)
    pub fn transformed(&self, m: DMat4) -> Tool {
        Tool { shape: self.shape.clone(), transform: m * self.transform }
    }

    /// The shape with its transform applied to the sketch plane, so the
    /// kernel only ever builds untransformed shapes. Mirrors flip the
    /// handedness, which is compensated on the extent or angle.
    pub fn resolved(&self) -> ToolShape {
        let m = self.transform;
        if m == DMat4::IDENTITY {
            return self.shape.clone();
        }
        let linear = DMat3::from_mat4(m);
        let mirrored = linear.determinant() < 0.0;
        let move_plane = |p: &Plane| Plane {
            origin: m.transform_point3(p.origin),
            x_axis: (linear * p.x_axis).normalize(),
            y_axis: (linear * p.y_axis).normalize(),
        };
        match &self.shape {
            ToolShape::Prism(p) => {
                let (start, end) = if mirrored { (-p.end, -p.start) } else { (p.start, p.end) };
                ToolShape::Prism(Prism { profile: p.profile.clone(), plane: move_plane(&p.plane), start, end })
            }
            ToolShape::Revolve(r) => {
                let start_angle = if mirrored { -(r.start_angle + r.angle) } else { r.start_angle };
                ToolShape::Revolve(Revolve { plane: move_plane(&r.plane), start_angle, ..r.clone() })
            }
        }
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
    let r = DMat3::IDENTITY - 2.0 * DMat3::from_cols(n * n.x, n * n.y, n * n.z);
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
    inner: Shape,
}

/// An edge of a solid; the index in `Solid::edges` identifies it for fillets
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeInfo {
    pub mid: DVec3,
    pub start: DVec3,
    pub end: DVec3,
}

/// Triangle mesh ready for display, with B-Rep face ids per triangle
#[derive(Clone, Debug, Default)]
pub struct RenderMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// B-Rep face index of each triangle
    pub triangle_faces: Vec<u32>,
    /// Model edges as polylines, indexed like `Solid::edges`
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

fn kernel_err(e: nova_occt::Error) -> NovaError {
    NovaError::GeometryError(e.to_string())
}

fn p3(v: DVec3) -> P3 {
    P3::new(v.x, v.y, v.z)
}

fn dv(p: P3) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}

impl Solid {
    /// Build a solid prism
    pub fn prism(prism: &Prism) -> Result<Solid> {
        build_prism(prism)
    }

    /// Build a tool's solid
    pub fn from_tool(tool: &Tool) -> Result<Solid> {
        match tool.resolved() {
            ToolShape::Prism(p) => build_prism(&p),
            ToolShape::Revolve(r) => build_revolve(&r),
        }
    }

    /// Combine this body with a tool
    pub fn boolean(&self, op: BooleanOp, tool: impl Into<Tool>) -> Result<Solid> {
        let tool = Solid::from_tool(&tool.into())?;
        let result = match op {
            BooleanOp::Union => self.inner.fuse(&tool.inner),
            BooleanOp::Subtract => self.inner.cut(&tool.inner),
        };
        result.map(|inner| Solid { inner }).map_err(|e| {
            NovaError::GeometryError(match op {
                BooleanOp::Union => format!("boss could not be merged with the body: {e}"),
                BooleanOp::Subtract if e.0.contains("no material") => "cut removed all material".into(),
                BooleanOp::Subtract => format!("cut failed: {e}"),
            })
        })
    }

    /// Round edges (indices from `edges`)
    pub fn fillet(&self, edges: &[u32], radius: f64) -> Result<Solid> {
        if !(radius > 0.0 && radius.is_finite()) {
            return Err(NovaError::InvalidFeature("fillet radius must be positive".into()));
        }
        self.inner.fillet(edges, radius).map(|inner| Solid { inner }).map_err(kernel_err)
    }

    /// Bevel edges (indices from `edges`)
    pub fn chamfer(&self, edges: &[u32], distance: f64) -> Result<Solid> {
        if !(distance > 0.0 && distance.is_finite()) {
            return Err(NovaError::InvalidFeature("chamfer distance must be positive".into()));
        }
        self.inner.chamfer(edges, distance).map(|inner| Solid { inner }).map_err(kernel_err)
    }

    /// Edges in a stable order (degenerate ones included, with no extent)
    pub fn edges(&self) -> Vec<EdgeInfo> {
        self.inner
            .edges()
            .unwrap_or_default()
            .into_iter()
            .map(|e| EdgeInfo { mid: dv(e.mid), start: dv(e.start), end: dv(e.end) })
            .collect()
    }

    /// Number of B-Rep faces
    pub fn face_count(&self) -> usize {
        self.inner.face_count()
    }

    /// Number of disjoint lumps
    pub fn lump_count(&self) -> usize {
        self.inner.solid_count()
    }

    /// Triangulate for display; `tol` is the chord tolerance in model units
    pub fn tessellate(&self, tol: f64) -> RenderMesh {
        let Ok(m) = self.inner.triangulate(tol.max(1e-4)) else { return RenderMesh::default() };
        let triples = |v: &[f32]| v.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect::<Vec<_>>();
        let points = triples(&m.edge_points);
        let edges = m
            .edge_offsets
            .windows(2)
            .map(|w| points[w[0] as usize..w[1] as usize].to_vec())
            .collect();
        RenderMesh {
            positions: triples(&m.positions),
            normals: triples(&m.normals),
            indices: m.indices,
            triangle_faces: m.tri_faces,
            edges,
        }
    }

    /// Exact volume
    pub fn volume(&self) -> f64 {
        self.inner.volume().unwrap_or(0.0)
    }

    /// Diagonal of the bounding box
    pub fn size(&self) -> f64 {
        self.bounds().map(|(lo, hi)| (hi - lo).length()).unwrap_or(0.0)
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        self.inner.bounds().ok().flatten().map(|(lo, hi)| (dv(lo), dv(hi)))
    }

    /// Write a STEP (AP214) file
    pub fn write_step(&self, path: &std::path::Path) -> Result<()> {
        self.inner.write_step(path).map_err(kernel_err)
    }

    /// STEP file contents as a string
    pub fn to_step(&self, name: &str) -> String {
        let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_').collect();
        let path = std::env::temp_dir().join(format!("nova-{}-{}-{}", std::process::id(), unique(), safe));
        let text = self
            .write_step(&path)
            .ok()
            .and_then(|_| std::fs::read_to_string(&path).ok())
            .unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        text
    }
}

fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Planar face for a profile placed on a plane
fn profile_face(profile: &Profile, plane: &Plane) -> Result<Shape> {
    let profile = profile.normalized();
    profile.outer.validate()?;
    for h in &profile.holes {
        h.validate()?;
    }
    let world = |p: DVec2| p3(plane.to_world(p));
    let loops: Vec<Vec<Seg>> = std::iter::once(&profile.outer)
        .chain(profile.holes.iter())
        .map(|lp| {
            lp.segments
                .iter()
                .enumerate()
                .map(|(i, seg)| match *seg {
                    Segment::Line { start } => Seg::line(world(start), world(lp.end_of(i))),
                    Segment::Arc { start, through } => Seg::arc(world(start), world(lp.end_of(i)), world(through)),
                })
                .collect()
        })
        .collect();
    Shape::planar_face(&loops).map_err(kernel_err)
}

fn build_prism(prism: &Prism) -> Result<Solid> {
    if !(prism.end - prism.start > 1e-9) {
        return Err(NovaError::InvalidFeature("extrusion depth must be positive".into()));
    }
    let face = profile_face(&prism.profile, &prism.plane.offset(prism.start))?;
    let inner = face.prism(p3(prism.plane.normal() * (prism.end - prism.start))).map_err(kernel_err)?;
    Ok(Solid { inner })
}

fn build_revolve(rev: &Revolve) -> Result<Solid> {
    if !(rev.angle > 1e-9 && rev.angle <= std::f64::consts::TAU + 1e-9) {
        return Err(NovaError::InvalidFeature("revolve angle must be between 0 and 360°".into()));
    }
    if rev.axis_dir.length() < 1e-12 {
        return Err(NovaError::InvalidFeature("revolve axis has no direction".into()));
    }
    // The profile must stay on one side of the axis
    let dir = rev.axis_dir.normalize();
    let samples = rev.profile.outer.sample(24);
    let size = {
        let (lo, hi) = rev.profile.bounds();
        (hi - lo).length().max(1e-9)
    };
    let side = |p: DVec2| dir.perp_dot(p - rev.axis_origin);
    let (min, max) = samples.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(side(*p)), b.max(side(*p))));
    if min < -1e-7 * size && max > 1e-7 * size {
        return Err(NovaError::InvalidFeature("the profile crosses the revolve axis".into()));
    }
    let (origin, axis) = rev.world_axis();
    // Start angle: rotate the sketch plane about the (in-plane) axis
    let plane = if rev.start_angle.abs() > 1e-12 {
        let r = rotation_about(origin, axis, rev.start_angle);
        let lin = DMat3::from_mat4(r);
        Plane { origin: r.transform_point3(rev.plane.origin), x_axis: lin * rev.plane.x_axis, y_axis: lin * rev.plane.y_axis }
    } else {
        rev.plane
    };
    let face = profile_face(&rev.profile, &plane)?;
    let inner = face.revolve(p3(origin), p3(axis), rev.angle.min(std::f64::consts::TAU)).map_err(kernel_err)?;
    Ok(Solid { inner })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Loop;
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

    fn shaft() -> Solid {
        Solid::from_tool(&Revolve::new(Profile::simple(rect(0.0, 0.0, 10.0, 40.0)), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI).into())
            .unwrap()
    }

    #[test]
    fn rectangular_block() {
        let b = block();
        assert!(close(b.volume(), 120_000.0, 1e-12), "{}", b.volume());
        assert_eq!(b.face_count(), 6);
        let (lo, hi) = b.bounds().unwrap();
        assert!((hi - lo - DVec3::new(100.0, 60.0, 20.0)).length() < 1e-6);
    }

    #[test]
    fn clockwise_profile_still_extrudes_upward() {
        let p = Prism::new(Profile::simple(rect(0.0, 0.0, 10.0, 10.0).reversed()), Plane::front(), 0.0, 5.0);
        let s = Solid::prism(&p).unwrap();
        assert!(close(s.volume(), 500.0, 1e-12));
        assert!(s.bounds().unwrap().0.z > -1e-6);
    }

    #[test]
    fn profile_with_hole() {
        let p = Profile::new(rect(0.0, 0.0, 100.0, 60.0), vec![Loop::circle(DVec2::new(20.0, 20.0), 5.0)]);
        let s = Solid::prism(&Prism::new(p, Plane::front(), 0.0, 20.0)).unwrap();
        assert!(close(s.volume(), 120_000.0 - PI * 25.0 * 20.0, 1e-9), "{}", s.volume());
    }

    #[test]
    fn extrusion_on_other_planes() {
        for plane in [Plane::top(), Plane::right()] {
            let s = Solid::prism(&Prism::new(Profile::simple(rect(0.0, 0.0, 10.0, 20.0)), plane, 0.0, 30.0)).unwrap();
            assert!(close(s.volume(), 6000.0, 1e-12));
            let (lo, hi) = s.bounds().unwrap();
            let along_normal = (hi - lo).dot(plane.normal()).abs();
            assert!((along_normal - 30.0).abs() < 1e-6);
        }
    }

    #[test]
    fn booleans_with_coplanar_faces_are_exact() {
        let cut = |p: Prism| block().boolean(BooleanOp::Subtract, p).unwrap().volume();
        let add = |p: Prism| block().boolean(BooleanOp::Union, p).unwrap().volume();
        let hole = Prism::new(Profile::simple(Loop::circle(DVec2::new(50.0, 30.0), 5.0)), Plane::front(), 0.0, 20.0);
        assert!(close(cut(hole), 120_000.0 - PI * 25.0 * 20.0, 1e-9));
        let pocket = Prism::new(Profile::simple(rect(10.0, 10.0, 30.0, 20.0)), Plane::front(), 15.0, 20.0);
        assert!(close(cut(pocket), 117_000.0, 1e-12));
        let boss = Prism::new(Profile::simple(rect(20.0, 20.0, 20.0, 20.0)), Plane::front(), 20.0, 40.0);
        assert!(close(add(boss), 128_000.0, 1e-12));
        let flush = Prism::new(Profile::simple(rect(0.0, 10.0, 30.0, 20.0)), Plane::front(), 20.0, 40.0);
        assert!(close(add(flush), 132_000.0, 1e-12));
        let same = Prism::new(Profile::simple(rect(0.0, 0.0, 100.0, 60.0)), Plane::front(), 20.0, 40.0);
        let merged = block().boolean(BooleanOp::Union, same).unwrap();
        assert!(close(merged.volume(), 240_000.0, 1e-12));
        assert_eq!(merged.face_count(), 6, "coplanar faces are merged");
    }

    #[test]
    fn disjoint_boss_makes_second_lump() {
        let tool = Prism::new(Profile::simple(rect(200.0, 0.0, 10.0, 10.0)), Plane::front(), 0.0, 10.0);
        let u = block().boolean(BooleanOp::Union, tool).unwrap();
        assert!(close(u.volume(), 121_000.0, 1e-12));
        assert_eq!(u.lump_count(), 2);
    }

    #[test]
    fn cut_removing_everything_is_an_error() {
        let tool = Prism::new(Profile::simple(rect(-10.0, -10.0, 200.0, 200.0)), Plane::front(), -5.0, 50.0);
        assert!(block().boolean(BooleanOp::Subtract, tool).is_err());
    }

    #[test]
    fn tessellation_has_face_ids_and_edges() {
        let mesh = block().tessellate(0.1);
        assert_eq!(mesh.triangle_faces.len() * 3, mesh.indices.len());
        assert_eq!(*mesh.triangle_faces.iter().max().unwrap(), 5);
        assert_eq!(mesh.edges.len(), 12);
        assert!((mesh.volume() - 120_000.0).abs() < 1e-3);
    }

    #[test]
    fn revolve_full_turn_and_partial() {
        let p = Profile::simple(rect(5.0, 0.0, 5.0, 20.0));
        let tube = Solid::from_tool(&Revolve::new(p.clone(), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI).into()).unwrap();
        assert!(close(tube.volume(), PI * 75.0 * 20.0, 1e-9), "{}", tube.volume());
        let half = Solid::from_tool(&Revolve::new(p, Plane::front(), DVec2::ZERO, DVec2::Y, PI).into()).unwrap();
        assert!(close(half.volume(), PI * 75.0 * 10.0, 1e-9), "{}", half.volume());
        assert!(close(shaft().volume(), PI * 100.0 * 40.0, 1e-9), "{}", shaft().volume());
    }

    #[test]
    fn revolve_start_angle_rotates_the_sweep() {
        // Quarter turn starting at 90° about +Y: the solid lies on the -Z side
        let mut r = Revolve::new(Profile::simple(rect(5.0, 0.0, 5.0, 10.0)), Plane::front(), DVec2::ZERO, DVec2::Y, PI / 2.0);
        r.start_angle = PI / 2.0;
        let s = Solid::from_tool(&r.into()).unwrap();
        let (lo, hi) = s.bounds().unwrap();
        assert!(hi.z < 1e-6 && lo.z < -9.0 && hi.x < 1e-6, "{:?} {:?}", lo, hi);
    }

    #[test]
    fn revolve_rejects_profile_across_axis() {
        let r = Revolve::new(Profile::simple(rect(-5.0, 0.0, 10.0, 10.0)), Plane::front(), DVec2::ZERO, DVec2::Y, PI);
        assert!(Solid::from_tool(&r.into()).is_err());
    }

    #[test]
    fn revolved_groove_and_cross_hole() {
        let groove = Revolve::new(Profile::simple(rect(8.0, 18.0, 4.0, 4.0)), Plane::front(), DVec2::ZERO, DVec2::Y, 2.0 * PI);
        let cut = shaft().boolean(BooleanOp::Subtract, groove).unwrap();
        let expected = PI * 100.0 * 40.0 - PI * (100.0 - 64.0) * 4.0;
        assert!(close(cut.volume(), expected, 1e-9), "{} vs {}", cut.volume(), expected);

        // This exact extent made the previous kernel (Truck) fail
        let hole = Prism::new(Profile::simple(Loop::circle(DVec2::new(0.0, 20.0), 3.0)), Plane::front().offset(15.0), -26.48243836597223, 0.0);
        let drilled = shaft().boolean(BooleanOp::Subtract, hole).unwrap();
        let v = drilled.volume();
        assert!(v < shaft().volume() - 500.0 && v > shaft().volume() - 700.0, "{}", v);
    }

    #[test]
    fn transformed_tools_pattern_and_mirror() {
        let hole = Prism::new(Profile::simple(Loop::circle(DVec2::new(15.0, 30.0), 4.0)), Plane::front(), 0.0, 20.0);
        let mut body = block();
        for i in 0..3 {
            let t = Tool::from(hole.clone()).transformed(translation(DVec3::new(30.0 * i as f64, 0.0, 0.0)));
            body = body.boolean(BooleanOp::Subtract, t).unwrap();
        }
        assert!(close(body.volume(), 120_000.0 - 3.0 * PI * 16.0 * 20.0, 1e-9), "{}", body.volume());

        // Mirror of a boss across the plane x = 50 lands on the other side
        let boss = Prism::new(Profile::simple(rect(5.0, 5.0, 10.0, 10.0)), Plane::front(), 20.0, 30.0);
        let m = mirror(&Plane::new(DVec3::new(50.0, 0.0, 0.0), DVec3::Y, DVec3::X));
        let mirrored = Solid::from_tool(&Tool::from(boss).transformed(m)).unwrap();
        let (lo, hi) = mirrored.bounds().unwrap();
        assert!((lo.x - 85.0).abs() < 1e-6 && (hi.x - 95.0).abs() < 1e-6, "{:?} {:?}", lo, hi);
        assert!((lo.z - 20.0).abs() < 1e-6 && (hi.z - 30.0).abs() < 1e-6);
        assert!(close(mirrored.volume(), 1000.0, 1e-12));

        // Mirrored partial revolve keeps its volume and flips side
        let mut rev = Revolve::new(Profile::simple(rect(5.0, 0.0, 5.0, 10.0)), Plane::front(), DVec2::ZERO, DVec2::Y, PI / 2.0);
        rev.start_angle = 0.3;
        let flipped = Solid::from_tool(&Tool::from(rev.clone()).transformed(mirror(&Plane::front()))).unwrap();
        let original = Solid::from_tool(&rev.into()).unwrap();
        assert!(close(flipped.volume(), original.volume(), 1e-9));
        let (olo, ohi) = original.bounds().unwrap();
        let (flo, fhi) = flipped.bounds().unwrap();
        assert!((flo.z + ohi.z).abs() < 1e-6 && (fhi.z + olo.z).abs() < 1e-6);

        // Circular pattern: rotate a hole 90° about the block centre's Z axis
        let r = rotation_about(DVec3::new(50.0, 30.0, 0.0), DVec3::Z, std::f64::consts::FRAC_PI_2);
        let moved = Solid::from_tool(&Tool::from(hole).transformed(r)).unwrap();
        let (lo, hi) = moved.bounds().unwrap();
        let c = (lo + hi) / 2.0;
        assert!((c.x - 50.0).abs() < 1e-3 && (c.y - (30.0 - 35.0)).abs() < 1e-3, "{:?}", c);
    }

    #[test]
    fn fillets_and_chamfers() {
        let b = block();
        let edges = b.edges();
        assert_eq!(edges.len(), 12);
        let corner = edges.iter().position(|e| (e.mid - DVec3::new(100.0, 60.0, 10.0)).length() < 1e-6).unwrap() as u32;
        let f = b.fillet(&[corner], 5.0).unwrap();
        assert!(close(f.volume(), 120_000.0 - (25.0 - PI * 25.0 / 4.0) * 20.0, 1e-9), "{}", f.volume());
        let c = b.chamfer(&[corner], 5.0).unwrap();
        assert!(close(c.volume(), 119_750.0, 1e-9));
        // Too large: a clean error
        assert!(b.fillet(&[corner], 500.0).is_err());
        assert!(b.fillet(&[corner], -1.0).is_err());

        // Round the top edge of a drilled hole
        let hole = Prism::new(Profile::simple(Loop::circle(DVec2::new(50.0, 30.0), 5.0)), Plane::front(), 0.0, 20.0);
        let drilled = b.boolean(BooleanOp::Subtract, hole).unwrap();
        let rim: Vec<u32> = drilled
            .edges()
            .iter()
            .enumerate()
            .filter(|(_, e)| (e.mid.z - 20.0).abs() < 1e-6 && (e.mid.truncate() - DVec2::new(50.0, 30.0)).length() < 5.1)
            .map(|(i, _)| i as u32)
            .collect();
        assert!(!rim.is_empty());
        let rounded = drilled.fillet(&rim, 1.0).unwrap();
        assert!(rounded.volume() < drilled.volume());
    }

    #[test]
    fn step_export() {
        let step = block().to_step("block.step");
        assert!(step.starts_with("ISO-10303-21;"), "{}", &step[..step.len().min(80)]);
        assert!(step.contains("MANIFOLD_SOLID_BREP") || step.contains("CLOSED_SHELL"));
    }
}
