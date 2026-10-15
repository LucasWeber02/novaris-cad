//! Minimal bindings to OpenCascade (OCCT 7.8) for NOVARIS CAD.
//!
//! Only the operations the kernel needs are exposed. Every call is wrapped
//! on the C++ side so OCCT exceptions become `Err` values instead of
//! terminating the process.

#[cxx::bridge(namespace = "nova")]
mod ffi {
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct P3 {
        x: f64,
        y: f64,
        z: f64,
    }

    /// Profile segment in world coordinates: kind 0 = line a→b,
    /// kind 1 = circular arc a→b through c
    #[derive(Clone, Copy, Debug)]
    struct Seg {
        kind: u8,
        a: P3,
        b: P3,
        c: P3,
    }

    /// Triangles grouped by face plus edge polylines
    struct MeshOut {
        positions: Vec<f32>,
        normals: Vec<f32>,
        indices: Vec<u32>,
        tri_faces: Vec<u32>,
        /// Edge i spans points edge_offsets[i]..edge_offsets[i+1]
        edge_offsets: Vec<u32>,
        edge_points: Vec<f32>,
    }

    #[derive(Clone, Copy, Debug)]
    struct EdgeData {
        mid: P3,
        start: P3,
        end: P3,
        degenerated: bool,
    }

    #[derive(Clone, Copy, Debug)]
    struct FacePlane {
        planar: bool,
        origin: P3,
        normal: P3,
    }

    #[derive(Clone, Copy, Debug)]
    struct Bounds {
        min: P3,
        max: P3,
        empty: bool,
    }

    unsafe extern "C++" {
        include!("nova-occt/include/shim.hxx");

        type Shape;

        fn planar_face(segs: &[Seg], loop_lens: &[u32]) -> Result<UniquePtr<Shape>>;
        fn prism(face: &Shape, dir: P3) -> Result<UniquePtr<Shape>>;
        fn revolve(face: &Shape, origin: P3, axis: P3, angle: f64) -> Result<UniquePtr<Shape>>;
        fn fuse(a: &Shape, b: &Shape) -> Result<UniquePtr<Shape>>;
        fn cut(a: &Shape, b: &Shape) -> Result<UniquePtr<Shape>>;
        fn fillet(s: &Shape, edges: &[u32], radius: f64) -> Result<UniquePtr<Shape>>;
        fn chamfer(s: &Shape, edges: &[u32], distance: f64) -> Result<UniquePtr<Shape>>;
        fn triangulate(s: &Shape, tolerance: f64) -> Result<MeshOut>;
        fn volume(s: &Shape) -> Result<f64>;
        fn bounds(s: &Shape) -> Result<Bounds>;
        fn face_count(s: &Shape) -> usize;
        fn solid_count(s: &Shape) -> usize;
        fn edges(s: &Shape) -> Result<Vec<EdgeData>>;
        fn face_planes(s: &Shape) -> Result<Vec<FacePlane>>;
        fn write_step(s: &Shape, path: &str) -> Result<()>;
        fn clone_shape(s: &Shape) -> UniquePtr<Shape>;
        fn is_valid(s: &Shape) -> bool;
    }
}

pub use ffi::{Bounds, EdgeData, FacePlane, MeshOut, Seg, P3};

/// Error from an OCCT operation
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn err(e: cxx::Exception) -> Error {
    Error(e.what().to_string())
}

impl P3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        P3 { x, y, z }
    }
}

impl Seg {
    pub fn line(a: P3, b: P3) -> Self {
        Seg { kind: 0, a, b, c: a }
    }

    /// Arc from `a` to `b` passing through `through`
    pub fn arc(a: P3, b: P3, through: P3) -> Self {
        Seg { kind: 1, a, b, c: through }
    }
}

/// An OCCT shape (face or solid). Cloning shares the underlying geometry.
pub struct Shape(cxx::UniquePtr<ffi::Shape>);

impl Clone for Shape {
    fn clone(&self) -> Self {
        Shape(ffi::clone_shape(self.inner()))
    }
}

impl std::fmt::Debug for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Shape({} faces, {} solids)", self.face_count(), self.solid_count())
    }
}

impl Shape {
    fn inner(&self) -> &ffi::Shape {
        self.0.as_ref().expect("shapes are never null")
    }

    fn wrap(r: std::result::Result<cxx::UniquePtr<ffi::Shape>, cxx::Exception>) -> Result<Shape> {
        let p = r.map_err(err)?;
        if p.is_null() {
            return Err(Error("OCCT returned no shape".into()));
        }
        Ok(Shape(p))
    }

    /// Planar face from loops: first the outer boundary, then holes.
    /// `loops[i]` lists the segments of loop i in order.
    pub fn planar_face(loops: &[Vec<Seg>]) -> Result<Shape> {
        let segs: Vec<Seg> = loops.iter().flatten().copied().collect();
        let lens: Vec<u32> = loops.iter().map(|l| l.len() as u32).collect();
        Self::wrap(ffi::planar_face(&segs, &lens))
    }

    pub fn prism(&self, dir: P3) -> Result<Shape> {
        Self::wrap(ffi::prism(self.inner(), dir))
    }

    pub fn revolve(&self, origin: P3, axis: P3, angle: f64) -> Result<Shape> {
        Self::wrap(ffi::revolve(self.inner(), origin, axis, angle))
    }

    pub fn fuse(&self, other: &Shape) -> Result<Shape> {
        Self::wrap(ffi::fuse(self.inner(), other.inner()))
    }

    pub fn cut(&self, other: &Shape) -> Result<Shape> {
        Self::wrap(ffi::cut(self.inner(), other.inner()))
    }

    /// Round the edges with the given indices (see [`Shape::edges`])
    pub fn fillet(&self, edges: &[u32], radius: f64) -> Result<Shape> {
        Self::wrap(ffi::fillet(self.inner(), edges, radius))
    }

    pub fn chamfer(&self, edges: &[u32], distance: f64) -> Result<Shape> {
        Self::wrap(ffi::chamfer(self.inner(), edges, distance))
    }

    pub fn triangulate(&self, tolerance: f64) -> Result<MeshOut> {
        ffi::triangulate(self.inner(), tolerance).map_err(err)
    }

    /// Exact volume
    pub fn volume(&self) -> Result<f64> {
        ffi::volume(self.inner()).map_err(err)
    }

    pub fn bounds(&self) -> Result<Option<(P3, P3)>> {
        let b = ffi::bounds(self.inner()).map_err(err)?;
        Ok((!b.empty).then_some((b.min, b.max)))
    }

    pub fn face_count(&self) -> usize {
        ffi::face_count(self.inner())
    }

    pub fn solid_count(&self) -> usize {
        ffi::solid_count(self.inner())
    }

    /// Edges in a stable order: index i matches `fillet`/`chamfer` and the
    /// i-th polyline of `triangulate`
    pub fn edges(&self) -> Result<Vec<EdgeData>> {
        ffi::edges(self.inner()).map_err(err)
    }

    /// Plane of each face (same order as the face ids of `triangulate`)
    pub fn face_planes(&self) -> Result<Vec<FacePlane>> {
        ffi::face_planes(self.inner()).map_err(err)
    }

    pub fn write_step(&self, path: &std::path::Path) -> Result<()> {
        let p = path.to_str().ok_or_else(|| Error("path is not valid UTF-8".into()))?;
        ffi::write_step(self.inner(), p).map_err(err)
    }

    pub fn is_valid(&self) -> bool {
        ffi::is_valid(self.inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn rect(x0: f64, y0: f64, w: f64, h: f64, z: f64) -> Vec<Seg> {
        let p = [P3::new(x0, y0, z), P3::new(x0 + w, y0, z), P3::new(x0 + w, y0 + h, z), P3::new(x0, y0 + h, z)];
        (0..4).map(|i| Seg::line(p[i], p[(i + 1) % 4])).collect()
    }

    fn circle(cx: f64, cy: f64, r: f64, z: f64) -> Vec<Seg> {
        let (a, b) = (P3::new(cx + r, cy, z), P3::new(cx - r, cy, z));
        vec![Seg::arc(a, b, P3::new(cx, cy + r, z)), Seg::arc(b, a, P3::new(cx, cy - r, z))]
    }

    fn block() -> Shape {
        Shape::planar_face(&[rect(0.0, 0.0, 100.0, 60.0, 0.0)]).unwrap().prism(P3::new(0.0, 0.0, 20.0)).unwrap()
    }

    #[test]
    fn block_and_through_hole() {
        let b = block();
        assert!((b.volume().unwrap() - 120_000.0).abs() < 1e-6);
        assert_eq!(b.face_count(), 6);
        assert_eq!(b.edges().unwrap().len(), 12);
        let hole = Shape::planar_face(&[circle(50.0, 30.0, 5.0, 0.0)]).unwrap().prism(P3::new(0.0, 0.0, 20.0)).unwrap();
        let cut = b.cut(&hole).unwrap();
        assert!((cut.volume().unwrap() - (120_000.0 - PI * 25.0 * 20.0)).abs() < 1e-6);
        assert!(cut.is_valid());
    }

    #[test]
    fn fillet_and_errors_do_not_abort() {
        let b = block();
        let edges = b.edges().unwrap();
        let vertical = edges
            .iter()
            .position(|e| (e.mid.x - 100.0).abs() < 1e-9 && (e.mid.y - 60.0).abs() < 1e-9)
            .unwrap() as u32;
        let f = b.fillet(&[vertical], 5.0).unwrap();
        let expected = 120_000.0 - (25.0 - PI * 25.0 / 4.0) * 20.0;
        assert!((f.volume().unwrap() - expected).abs() < 1e-6, "{}", f.volume().unwrap());
        // A radius larger than the part must be an error, not a crash
        assert!(b.fillet(&[vertical], 500.0).is_err());
        assert!(b.fillet(&[999], 1.0).is_err());
        let c = b.chamfer(&[vertical], 5.0).unwrap();
        assert!((c.volume().unwrap() - 119_750.0).abs() < 1e-6);
    }

    #[test]
    fn mesh_has_faces_and_edges() {
        let m = block().triangulate(0.1).unwrap();
        assert_eq!(m.positions.len(), m.normals.len());
        assert_eq!(m.indices.len(), m.tri_faces.len() * 3);
        assert_eq!(*m.tri_faces.iter().max().unwrap(), 5);
        assert_eq!(m.edge_offsets.len(), 13);
    }

    #[test]
    fn cutting_everything_is_an_error() {
        let big = Shape::planar_face(&[rect(-10.0, -10.0, 200.0, 200.0, -5.0)]).unwrap().prism(P3::new(0.0, 0.0, 50.0)).unwrap();
        assert!(block().cut(&big).is_err());
    }
}
