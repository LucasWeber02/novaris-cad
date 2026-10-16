//! NOVARIS CAD — Drawings (.nvdrw)
//!
//! A drawing shows a part or an assembly in 2D views on a sheet with a
//! title block, like a SolidWorks drawing. Views are hidden-line projections
//! computed from the model each time it changes, so the drawing stays
//! associative. Dimensions are driven: they measure the model, and point at
//! model vertices (or circles) by geometry, so they follow its edits.
//!
//! The sheet is rendered once into 2D primitives ([`render::Sheet2D`]),
//! shared by the screen, the PDF and the DXF exports.

pub mod dxf;
pub mod pdf;
pub mod render;

use glam::{DVec2, DVec3};
use nova_assembly::{AssemblyDocument, BomRow};
use nova_core::{Id, NovaError, Result};
use nova_io::PartDocument;
use nova_kernel::{ProjCurve, ProjectedEdge, Solid};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "nvcad-drawing";
pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION: &str = "nvdrw";

/// Margin between the paper edge and the frame, in mm
pub const MARGIN: f64 = 10.0;
/// Title block size, in mm
pub const TITLE_W: f64 = 180.0;
pub const TITLE_H: f64 = 42.0;

/// Standard drawing scales, largest first
pub const SCALES: [f64; 13] = [10.0, 5.0, 2.0, 1.0, 0.5, 0.4, 0.25, 0.2, 0.1, 0.05, 0.02, 0.01, 0.005];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Paper {
    A4,
    A3,
    A2,
}

impl Paper {
    pub const ALL: [Paper; 3] = [Paper::A4, Paper::A3, Paper::A2];

    /// Landscape size in mm
    pub fn size(&self) -> DVec2 {
        match self {
            Paper::A4 => DVec2::new(297.0, 210.0),
            Paper::A3 => DVec2::new(420.0, 297.0),
            Paper::A2 => DVec2::new(594.0, 420.0),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Paper::A4 => "A4",
            Paper::A3 => "A3",
            Paper::A2 => "A2",
        }
    }
}

/// Where projected views go (ISO first angle is the European default)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Projection {
    FirstAngle,
    ThirdAngle,
}

impl Projection {
    pub fn label(&self) -> &'static str {
        match self {
            Projection::FirstAngle => "Premier dièdre (européenne)",
            Projection::ThirdAngle => "Troisième dièdre (américaine)",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TitleBlock {
    pub title: String,
    #[serde(default)]
    pub drawn_by: String,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub date: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub paper: Paper,
    /// Sheet mm per model mm (0.5 = scale 1:2)
    pub scale: f64,
    pub projection: Projection,
    pub title_block: TitleBlock,
}

/// Model orientation of a view
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    Iso,
}

impl Orientation {
    pub const ALL: [Orientation; 7] = [
        Orientation::Front,
        Orientation::Top,
        Orientation::Right,
        Orientation::Left,
        Orientation::Back,
        Orientation::Bottom,
        Orientation::Iso,
    ];

    /// (towards the viewer, right) as in SolidWorks' standard views
    pub fn frame(&self) -> Frame {
        let (n, x) = match self {
            Orientation::Front => (DVec3::Z, DVec3::X),
            Orientation::Back => (-DVec3::Z, -DVec3::X),
            Orientation::Top => (DVec3::Y, DVec3::X),
            Orientation::Bottom => (-DVec3::Y, DVec3::X),
            Orientation::Right => (DVec3::X, -DVec3::Z),
            Orientation::Left => (-DVec3::X, DVec3::Z),
            Orientation::Iso => (DVec3::ONE.normalize(), DVec3::new(1.0, 0.0, -1.0).normalize()),
        };
        Frame::new(n, x)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Orientation::Front => "Vue de face",
            Orientation::Back => "Vue arrière",
            Orientation::Top => "Vue de dessus",
            Orientation::Bottom => "Vue de dessous",
            Orientation::Left => "Vue de gauche",
            Orientation::Right => "Vue de droite",
            Orientation::Iso => "Vue isométrique",
        }
    }
}

/// View axes: `normal` points to the viewer, `x` right, `up` = normal × x
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub normal: DVec3,
    pub x: DVec3,
    pub up: DVec3,
}

impl Frame {
    pub fn new(normal: DVec3, x: DVec3) -> Self {
        let normal = normal.normalize();
        let x = x.normalize();
        Frame { normal, x, up: normal.cross(x) }
    }

    /// Model point → view coordinates (same as the projection's)
    pub fn project(&self, p: DVec3) -> DVec2 {
        DVec2::new(p.dot(self.x), p.dot(self.up))
    }

    fn same(&self, o: &Frame) -> bool {
        (self.normal - o.normal).length() < 1e-9 && (self.x - o.x).length() < 1e-9
    }
}

/// Side of the parent view a projected view is placed on
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

impl Side {
    pub fn vector(&self) -> DVec2 {
        match self {
            Side::Left => DVec2::NEG_X,
            Side::Right => DVec2::X,
            Side::Up => DVec2::Y,
            Side::Down => DVec2::NEG_Y,
        }
    }

    pub fn horizontal(&self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewKind {
    Model(Orientation),
    /// Aligned with `parent`, on one of its sides
    Projected { parent: Id, side: Side },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingView {
    pub id: Id,
    pub name: String,
    pub kind: ViewKind,
    /// Sheet position of the view center (model views)
    pub position: DVec2,
    /// Distance from the parent's center (projected views), in sheet mm
    #[serde(default)]
    pub distance: f64,
    pub show_hidden: bool,
    /// Own scale instead of the sheet's (model views only; projected views
    /// use their parent's)
    #[serde(default)]
    pub scale: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DimDir {
    Horizontal,
    Vertical,
    Aligned,
}

/// What a dimension measures. Points are model coordinates; circles are
/// in view coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DimKind {
    Linear { a: DVec3, b: DVec3, dir: DimDir },
    Diameter { center: DVec2, radius: f64 },
    Radius { center: DVec2, radius: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub id: Id,
    pub name: String,
    pub view: Id,
    pub kind: DimKind,
    /// Where the dimension line / text goes, in view coordinates
    pub place: DVec2,
}

/// A circle (or arc) found in a view
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle2 {
    pub center: DVec2,
    pub radius: f64,
    /// The arcs found cover the whole circle
    pub full: bool,
    pub visible: bool,
}

/// Projection of the model in one view, cached
#[derive(Clone, Debug)]
pub struct ViewGeom {
    pub frame: Frame,
    pub hidden: bool,
    pub edges: Vec<ProjectedEdge>,
    pub circles: Vec<Circle2>,
    /// Pickable vertices: view position and the model vertex there
    pub vertices: Vec<(DVec2, DVec3)>,
    /// View coordinates shown at the view's center on the sheet
    pub center: DVec2,
    pub lo: DVec2,
    pub hi: DVec2,
}

/// The model a drawing documents, rebuilt from its file
#[derive(Clone, Debug)]
pub struct Model {
    pub name: String,
    pub solid: Solid,
    pub is_assembly: bool,
    pub bom: Vec<BomRow>,
    pub vertices: Vec<DVec3>,
    pub size: f64,
    pub center: DVec3,
}

/// Something under the cursor on the sheet
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Vertex { view: Id, point: DVec3 },
    Circle { view: Id, circle: Circle2 },
    /// A straight edge between two model vertices
    Edge { view: Id, a: DVec3, b: DVec3 },
    Dimension(Id),
    View(Id),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DrawingDocument {
    pub name: String,
    /// Part (.nvcad) or assembly (.nvasm), relative to the drawing file when possible
    pub source: PathBuf,
    pub sheet: Sheet,
    pub views: Vec<DrawingView>,
    pub dimensions: Vec<Dimension>,
    /// Show the assembly's bill of materials above the title block
    #[serde(default)]
    pub show_bom: bool,
    #[serde(skip)]
    base_dir: PathBuf,
    #[serde(skip)]
    model: Option<Model>,
    #[serde(skip)]
    source_error: Option<String>,
    #[serde(skip)]
    geometry: HashMap<Id, ViewGeom>,
    #[serde(skip)]
    orphans: Vec<Id>,
}

impl Default for Sheet {
    fn default() -> Self {
        Sheet { paper: Paper::A4, scale: 1.0, projection: Projection::FirstAngle, title_block: TitleBlock::default() }
    }
}

#[derive(Serialize, Deserialize)]
struct DrawingFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    drawing: DrawingDocument,
}

/// "12,5" — French decimal comma, no trailing zeros
pub fn format_mm(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    let mut s = format!("{r:.2}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" {
        s = "0".into();
    }
    s.replace('.', ",")
}

/// "1:2", "2:1"
pub fn format_scale(s: f64) -> String {
    if s >= 1.0 {
        format!("{}:1", format_mm(s))
    } else {
        format!("1:{}", format_mm(1.0 / s))
    }
}

impl DrawingDocument {
    /// A drawing of `source` (a .nvcad or .nvasm file); call [`Self::update`]
    pub fn new(name: impl Into<String>, source: &Path) -> Self {
        let name = name.into();
        DrawingDocument {
            sheet: Sheet { title_block: TitleBlock { title: name.clone(), ..Default::default() }, ..Default::default() },
            name,
            source: nova_assembly::normalize(&nova_assembly::absolute(source)),
            ..Default::default()
        }
    }

    pub fn set_base_dir(&mut self, dir: impl Into<PathBuf>) {
        self.base_dir = dir.into();
    }

    pub fn source_file(&self) -> PathBuf {
        if self.source.is_absolute() {
            nova_assembly::normalize(&self.source)
        } else {
            nova_assembly::normalize(&nova_assembly::absolute(&self.base_dir.join(&self.source)))
        }
    }

    pub fn source_is_assembly(&self) -> bool {
        self.source.extension().is_some_and(|e| e == nova_assembly::EXTENSION)
    }

    pub fn model(&self) -> Option<&Model> {
        self.model.as_ref()
    }

    pub fn source_error(&self) -> Option<&str> {
        self.source_error.as_deref()
    }

    pub fn view(&self, id: Id) -> Option<&DrawingView> {
        self.views.iter().find(|v| v.id == id)
    }

    pub fn view_mut(&mut self, id: Id) -> Option<&mut DrawingView> {
        self.views.iter_mut().find(|v| v.id == id)
    }

    pub fn geometry(&self, view: Id) -> Option<&ViewGeom> {
        self.geometry.get(&view)
    }

    pub fn is_orphan(&self, dim: Id) -> bool {
        self.orphans.contains(&dim)
    }

    // ---------------------------------------------------------------- model

    fn load_model(&self) -> Result<Model> {
        let path = self.source_file();
        let (name, solid, bom, is_assembly) = if self.source_is_assembly() {
            let asm = AssemblyDocument::open(&path)?;
            let solids: Vec<Solid> = asm
                .components
                .iter()
                .filter(|c| !c.suppressed)
                .filter_map(|c| asm.world_solid(c.id))
                .collect();
            let refs: Vec<&Solid> = solids.iter().collect();
            if refs.is_empty() {
                return Err(NovaError::DocumentError("l'assemblage est vide".into()));
            }
            (asm.name.clone(), Solid::compound(&refs)?, asm.bom(), true)
        } else {
            let doc = PartDocument::open(&path)?;
            let solid = doc
                .tree
                .body()
                .cloned()
                .ok_or_else(|| NovaError::DocumentError("la pièce ne contient aucun corps".into()))?;
            (doc.name.clone(), solid, Vec::new(), false)
        };
        let (lo, hi) = solid.bounds().unwrap_or((DVec3::ZERO, DVec3::ZERO));
        Ok(Model {
            name,
            vertices: solid.vertices(),
            size: (hi - lo).length().max(1.0),
            center: (lo + hi) / 2.0,
            solid,
            bom,
            is_assembly,
        })
    }

    /// Re-read the model, re-project every view and re-attach the dimensions
    pub fn update(&mut self) {
        self.geometry.clear();
        match self.load_model() {
            Ok(m) => {
                self.model = Some(m);
                self.source_error = None;
            }
            Err(e) => {
                self.model = None;
                self.source_error = Some(e.to_string());
            }
        }
        self.refresh();
    }

    /// Project views whose orientation or settings changed, re-attach dimensions
    pub fn refresh(&mut self) {
        let ids: Vec<Id> = self.views.iter().map(|v| v.id).collect();
        self.geometry.retain(|id, _| ids.contains(id));
        for id in ids {
            let Some(frame) = self.frame(id) else { continue };
            let hidden = self.view(id).map(|v| v.show_hidden).unwrap_or(false);
            let fresh = self.geometry.get(&id).is_some_and(|g| g.frame.same(&frame) && g.hidden == hidden);
            if fresh {
                continue;
            }
            if let Some(g) = self.model.as_ref().and_then(|m| project_view(m, frame, hidden)) {
                self.geometry.insert(id, g);
            } else {
                self.geometry.remove(&id);
            }
        }
        self.resolve_dimensions();
    }

    // ---------------------------------------------------------------- views

    /// Axes of a view (projected views follow their parent)
    pub fn frame(&self, id: Id) -> Option<Frame> {
        self.frame_depth(id, 0)
    }

    fn frame_depth(&self, id: Id, depth: usize) -> Option<Frame> {
        if depth > 32 {
            return None;
        }
        match self.view(id)?.kind {
            ViewKind::Model(o) => Some(o.frame()),
            ViewKind::Projected { parent, side } => {
                let p = self.frame_depth(parent, depth + 1)?;
                // Third angle: the view on the right shows the right side
                let sign = match self.sheet.projection {
                    Projection::ThirdAngle => 1.0,
                    Projection::FirstAngle => -1.0,
                };
                let d = side.vector();
                let look = (p.x * d.x + p.up * d.y) * sign;
                Some(if side.horizontal() {
                    Frame::new(look, p.up.cross(look))
                } else {
                    Frame::new(look, p.x)
                })
            }
        }
    }

    /// Sheet position of a view's center
    pub fn view_center(&self, id: Id) -> Option<DVec2> {
        self.view_center_depth(id, 0)
    }

    fn view_center_depth(&self, id: Id, depth: usize) -> Option<DVec2> {
        if depth > 32 {
            return None;
        }
        let v = self.view(id)?;
        match v.kind {
            ViewKind::Model(_) => Some(v.position),
            ViewKind::Projected { parent, side } => {
                Some(self.view_center_depth(parent, depth + 1)? + side.vector() * v.distance)
            }
        }
    }

    /// Sheet mm per model mm in a view
    pub fn view_scale(&self, id: Id) -> f64 {
        let mut id = id;
        for _ in 0..32 {
            match self.view(id) {
                Some(DrawingView { kind: ViewKind::Projected { parent, .. }, .. }) => id = *parent,
                Some(v) => return v.scale.unwrap_or(self.sheet.scale),
                None => break,
            }
        }
        self.sheet.scale
    }

    /// View coordinates → sheet mm
    pub fn to_sheet(&self, view: Id, p: DVec2) -> Option<DVec2> {
        let c = self.geometry.get(&view)?.center;
        Some(self.view_center(view)? + (p - c) * self.view_scale(view))
    }

    /// Sheet mm → view coordinates
    pub fn from_sheet(&self, view: Id, p: DVec2) -> Option<DVec2> {
        let c = self.geometry.get(&view)?.center;
        Some(c + (p - self.view_center(view)?) / self.view_scale(view))
    }

    /// Sheet rectangle covered by a view
    pub fn view_rect(&self, view: Id) -> Option<(DVec2, DVec2)> {
        let g = self.geometry.get(&view)?;
        Some((self.to_sheet(view, g.lo)?, self.to_sheet(view, g.hi)?))
    }

    /// Size of the model seen in an orientation, in model mm
    fn extent(&self, frame: Frame) -> DVec2 {
        let Some(m) = &self.model else { return DVec2::splat(50.0) };
        let (lo, hi) = m.solid.bounds().unwrap_or((DVec3::ZERO, DVec3::ZERO));
        let mut a = DVec2::splat(f64::MAX);
        let mut b = DVec2::splat(f64::MIN);
        for i in 0..8 {
            let c = DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let p = frame.project(c);
            a = a.min(p);
            b = b.max(p);
        }
        b - a
    }

    fn view_name(&self, frame: &Frame) -> String {
        let base = Orientation::ALL
            .iter()
            .find(|o| o.frame().same(frame))
            .map(|o| o.label().to_string())
            .unwrap_or_else(|| "Vue projetée".to_string());
        if self.views.iter().all(|v| v.name != base) {
            return base;
        }
        (2..).map(|n| format!("{base} {n}")).find(|n| self.views.iter().all(|v| &v.name != n)).expect("unbounded")
    }

    /// Add a view of the model in a standard orientation
    pub fn add_model_view(&mut self, orientation: Orientation, position: DVec2) -> Id {
        let id = Id::new();
        let name = self.view_name(&orientation.frame());
        let hidden = orientation != Orientation::Iso;
        self.views.push(DrawingView {
            id,
            name,
            kind: ViewKind::Model(orientation),
            position,
            distance: 0.0,
            show_hidden: hidden,
            scale: None,
        });
        self.refresh();
        id
    }

    /// Add a view projected from `parent` on one of its sides
    pub fn add_projected_view(&mut self, parent: Id, side: Side) -> Result<Id> {
        let pf = self.frame(parent).ok_or_else(|| NovaError::DocumentError("vue introuvable".into()))?;
        let id = Id::new();
        self.views.push(DrawingView {
            id,
            name: String::new(),
            kind: ViewKind::Projected { parent, side },
            position: DVec2::ZERO,
            distance: 0.0,
            show_hidden: true,
            scale: None,
        });
        let frame = self.frame(id).expect("parent exists");
        let name = self.view_name(&frame);
        let s = self.view_scale(parent);
        let gap = 15.0;
        let (pe, ce) = (self.extent(pf), self.extent(frame));
        let distance = if side.horizontal() { (pe.x + ce.x) / 2.0 * s + gap } else { (pe.y + ce.y) / 2.0 * s + gap };
        if let Some(v) = self.view_mut(id) {
            v.name = name;
            v.distance = distance;
        }
        self.refresh();
        Ok(id)
    }

    /// Delete a view, the views projected from it and their dimensions
    pub fn remove_view(&mut self, id: Id) {
        let mut doomed = vec![id];
        let mut i = 0;
        while i < doomed.len() {
            let p = doomed[i];
            for v in &self.views {
                if matches!(v.kind, ViewKind::Projected { parent, .. } if parent == p) && !doomed.contains(&v.id) {
                    doomed.push(v.id);
                }
            }
            i += 1;
        }
        self.views.retain(|v| !doomed.contains(&v.id));
        self.dimensions.retain(|d| !doomed.contains(&d.view));
        self.refresh();
    }

    /// Move a view by `delta` sheet mm. Projected views only slide along
    /// their alignment, and views projected from this one follow it.
    pub fn move_view(&mut self, id: Id, delta: DVec2) {
        let Some(v) = self.view_mut(id) else { return };
        match v.kind {
            ViewKind::Model(_) => v.position += delta,
            ViewKind::Projected { side, .. } => v.distance = (v.distance + delta.dot(side.vector())).max(5.0),
        }
    }

    /// Switch between first and third angle; projected views change side so
    /// they keep showing the same face of the model
    pub fn set_projection(&mut self, p: Projection) {
        if self.sheet.projection == p {
            return;
        }
        self.sheet.projection = p;
        for v in &mut self.views {
            if let ViewKind::Projected { side, .. } = &mut v.kind {
                *side = match side {
                    Side::Left => Side::Right,
                    Side::Right => Side::Left,
                    Side::Up => Side::Down,
                    Side::Down => Side::Up,
                };
            }
        }
        self.refresh();
    }

    /// Replace the views by front, top and side views (as the projection
    /// convention places them) plus an isometric view, at a standard scale
    /// that fits the sheet
    pub fn standard_views(&mut self, iso: bool) {
        self.views.clear();
        self.dimensions.clear();
        self.geometry.clear();
        let front = Orientation::Front.frame();
        let (side_o, vertical) = match self.sheet.projection {
            Projection::FirstAngle => (Orientation::Left, Side::Down),
            Projection::ThirdAngle => (Orientation::Right, Side::Up),
        };
        let ef = self.extent(front);
        let es = self.extent(side_o.frame());
        let et = self.extent(Orientation::Top.frame());
        let ei = if iso { self.extent(Orientation::Iso.frame()) } else { DVec2::ZERO };
        let gap = 15.0;
        let paper = self.sheet.paper.size();
        let avail = DVec2::new(paper.x - 2.0 * MARGIN, paper.y - 2.0 * MARGIN - TITLE_H);
        let need = DVec2::new(ef.x + es.x, ef.y + et.y);
        let s = SCALES
            .iter()
            .copied()
            .find(|s| need.x * s + 3.0 * gap <= avail.x && need.y * s + 3.0 * gap <= avail.y)
            .unwrap_or(*SCALES.last().expect("non-empty"));
        self.sheet.scale = s;
        let top_edge = paper.y - MARGIN;
        let left = MARGIN + gap + ef.x * s / 2.0;
        let front_y = match vertical {
            Side::Down => top_edge - gap - ef.y * s / 2.0,
            _ => top_edge - gap - et.y * s - gap - ef.y * s / 2.0,
        };
        let f = self.add_model_view(Orientation::Front, DVec2::new(left, front_y));
        let _ = self.add_projected_view(f, vertical);
        let _ = self.add_projected_view(f, Side::Right);
        if iso {
            // In the free area right of the side view, above the title block;
            // smaller than the other views if needed, like SolidWorks
            let x0 = left + ef.x * s / 2.0 + gap + es.x * s + gap;
            let free = DVec2::new(paper.x - MARGIN - gap - x0, top_edge - gap - (MARGIN + TITLE_H + gap));
            let si = SCALES.iter().copied().filter(|k| *k <= s).find(|k| ei.x * k <= free.x && ei.y * k <= free.y);
            if let Some(si) = si {
                let at = DVec2::new(x0 + free.x / 2.0, top_edge - gap - free.y / 2.0);
                let id = self.add_model_view(Orientation::Iso, at);
                if si != s {
                    if let Some(v) = self.view_mut(id) {
                        v.scale = Some(si);
                    }
                }
            }
        }
        self.refresh();
    }

    // ---------------------------------------------------------------- dimensions

    fn next_dim_name(&self) -> String {
        (1..).map(|n| format!("D{n}")).find(|n| self.dimensions.iter().all(|d| &d.name != n)).expect("unbounded")
    }

    pub fn add_dimension(&mut self, view: Id, kind: DimKind, place: DVec2) -> Id {
        let id = Id::new();
        let name = self.next_dim_name();
        self.dimensions.push(Dimension { id, name, view, kind, place });
        self.resolve_dimensions();
        id
    }

    pub fn remove_dimension(&mut self, id: Id) {
        self.dimensions.retain(|d| d.id != id);
        self.orphans.retain(|d| *d != id);
    }

    /// Re-attach dimensions to the current model: each point goes to the
    /// nearest model vertex, each circle to the nearest circle of its view
    fn resolve_dimensions(&mut self) {
        self.orphans.clear();
        let Some(model) = &self.model else {
            self.orphans = self.dimensions.iter().map(|d| d.id).collect();
            return;
        };
        let tol = model.size * 0.25;
        let near_vertex = |p: DVec3| -> Option<DVec3> {
            model
                .vertices
                .iter()
                .map(|v| (*v, (*v - p).length()))
                .filter(|(_, d)| *d <= tol)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(v, _)| v)
        };
        for d in &mut self.dimensions {
            let ok = match &mut d.kind {
                DimKind::Linear { a, b, .. } => match (near_vertex(*a), near_vertex(*b)) {
                    (Some(na), Some(nb)) if (na - nb).length() > 1e-9 => {
                        *a = na;
                        *b = nb;
                        true
                    }
                    _ => false,
                },
                DimKind::Diameter { center, radius } | DimKind::Radius { center, radius } => {
                    let found = self.geometry.get(&d.view).and_then(|g| {
                        g.circles
                            .iter()
                            .filter(|c| (c.radius - *radius).abs() <= radius.max(1.0) * 0.5)
                            .map(|c| (*c, (c.center - *center).length()))
                            .filter(|(_, dist)| *dist <= tol)
                            .min_by(|x, y| x.1.total_cmp(&y.1))
                    });
                    match found {
                        Some((c, _)) => {
                            *center = c.center;
                            *radius = c.radius;
                            true
                        }
                        None => false,
                    }
                }
            };
            if !ok || !self.geometry.contains_key(&d.view) {
                self.orphans.push(d.id);
            }
        }
    }

    /// Measured value of a dimension (model mm)
    pub fn value(&self, d: &Dimension) -> Option<f64> {
        let frame = self.frame(d.view)?;
        Some(match d.kind {
            DimKind::Linear { a, b, dir } => {
                let v = b - a;
                match dir {
                    DimDir::Horizontal => v.dot(frame.x).abs(),
                    DimDir::Vertical => v.dot(frame.up).abs(),
                    DimDir::Aligned if self.is_iso(d.view) => v.length(),
                    DimDir::Aligned => frame.project(v).length(),
                }
            }
            DimKind::Diameter { radius, .. } => 2.0 * radius,
            DimKind::Radius { radius, .. } => radius,
        })
    }

    fn is_iso(&self, view: Id) -> bool {
        self.frame(view).is_some_and(|f| [f.normal.x, f.normal.y, f.normal.z].iter().filter(|c| c.abs() > 1e-6).count() > 1)
    }

    /// Text of a dimension, e.g. "100", "Ø20", "R5"
    pub fn label(&self, d: &Dimension) -> String {
        let v = self.value(d).unwrap_or(0.0);
        match d.kind {
            DimKind::Linear { .. } => format_mm(v),
            DimKind::Diameter { .. } => format!("Ø{}", format_mm(v)),
            DimKind::Radius { .. } => format!("R{}", format_mm(v)),
        }
    }

    /// Dimension direction from where the user drops it, like SolidWorks:
    /// beside the span → vertical, above/below → horizontal, else aligned
    pub fn infer_dir(&self, view: Id, a: DVec3, b: DVec3, place: DVec2) -> DimDir {
        let Some(f) = self.frame(view) else { return DimDir::Aligned };
        let (pa, pb) = (f.project(a), f.project(b));
        let (lo, hi) = (pa.min(pb), pa.max(pb));
        let inside_x = place.x >= lo.x && place.x <= hi.x;
        let inside_y = place.y >= lo.y && place.y <= hi.y;
        if (hi.x - lo.x) < 1e-6 {
            return DimDir::Vertical;
        }
        if (hi.y - lo.y) < 1e-6 {
            return DimDir::Horizontal;
        }
        match (inside_x, inside_y) {
            (true, false) => DimDir::Horizontal,
            (false, true) => DimDir::Vertical,
            _ => DimDir::Aligned,
        }
    }

    // ---------------------------------------------------------------- picking

    /// What is under a sheet point; `tol` in sheet mm
    pub fn pick(&self, at: DVec2, tol: f64) -> Option<Pick> {
        for d in &self.dimensions {
            if let Some(t) = render::dimension_text_pos(self, d) {
                if (t - at).length() <= tol.max(3.0) {
                    return Some(Pick::Dimension(d.id));
                }
            }
        }
        let mut inside = None;
        for v in &self.views {
            let (Some(g), Some(p)) = (self.geometry.get(&v.id), self.from_sheet(v.id, at)) else { continue };
            let t = tol / self.view_scale(v.id);
            if let Some((_, point)) =
                g.vertices.iter().filter(|(q, _)| (*q - p).length() <= t).min_by(|a, b| (a.0 - p).length().total_cmp(&(b.0 - p).length()))
            {
                return Some(Pick::Vertex { view: v.id, point: *point });
            }
            if let Some(c) = g.circles.iter().find(|c| ((c.center - p).length() - c.radius).abs() <= t) {
                return Some(Pick::Circle { view: v.id, circle: *c });
            }
            for e in g.edges.iter().filter(|e| e.curve == ProjCurve::Line) {
                if dist_to_segment(p, e.a, e.b) <= t {
                    let a = g.vertices.iter().find(|(q, _)| (*q - e.a).length() <= 1e-6 * (1.0 + q.length()));
                    let b = g.vertices.iter().find(|(q, _)| (*q - e.b).length() <= 1e-6 * (1.0 + q.length()));
                    if let (Some(a), Some(b)) = (a, b) {
                        return Some(Pick::Edge { view: v.id, a: a.1, b: b.1 });
                    }
                }
            }
            let m = 5.0 / self.view_scale(v.id);
            if inside.is_none() && p.x >= g.lo.x - m && p.x <= g.hi.x + m && p.y >= g.lo.y - m && p.y <= g.hi.y + m {
                inside = Some(Pick::View(v.id));
            }
        }
        inside
    }

    // ---------------------------------------------------------------- files

    pub fn to_json(&self) -> Result<String> {
        let file = DrawingFile { format: FORMAT.into(), version: FORMAT_VERSION, drawing: self.clone() };
        serde_json::to_string_pretty(&file).map_err(|e| NovaError::DocumentError(e.to_string()))
    }

    /// Parse a drawing; a relative source starts from `base_dir`. The model
    /// is loaded and the views projected.
    pub fn from_json(text: &str, base_dir: &Path) -> Result<Self> {
        let header: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| NovaError::DocumentError(format!("not a valid drawing file: {e}")))?;
        if header.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
            return Err(NovaError::DocumentError("not a NOVARIS drawing file".into()));
        }
        let version = header.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
        if version > FORMAT_VERSION as u64 {
            return Err(NovaError::DocumentError(format!(
                "drawing file version {version} is newer than this program supports ({FORMAT_VERSION})"
            )));
        }
        let file: DrawingFile = serde_json::from_value(header)
            .map_err(|e| NovaError::DocumentError(format!("corrupt drawing file: {e}")))?;
        let mut d = file.drawing;
        d.base_dir = base_dir.to_path_buf();
        d.update();
        Ok(d)
    }

    /// Save, storing the source path relative to the drawing file
    pub fn save(&mut self, path: &Path) -> Result<()> {
        let base = nova_assembly::absolute(path).parent().map(nova_assembly::normalize).unwrap_or_default();
        let src = self.source_file();
        self.source = nova_assembly::relative_to(&src, &base);
        self.base_dir = base;
        std::fs::write(path, self.to_json()?)?;
        Ok(())
    }

    pub fn open(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let base = nova_assembly::absolute(path).parent().map(nova_assembly::normalize).unwrap_or_default();
        Self::from_json(&text, &base)
    }

    /// Write the sheet as a PDF file
    pub fn export_pdf(&self, path: &Path) -> Result<()> {
        std::fs::write(path, pdf::write(&render::render(self)))?;
        Ok(())
    }

    /// Write the sheet as a DXF (R12) file
    pub fn export_dxf(&self, path: &Path) -> Result<()> {
        std::fs::write(path, dxf::write(&render::render(self)))?;
        Ok(())
    }
}

fn dist_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
}

fn project_view(model: &Model, frame: Frame, hidden: bool) -> Option<ViewGeom> {
    let mut edges = model.solid.project(frame.normal, frame.x, hidden).ok()?;
    let tol = model.size * 1e-7 + 1e-7;
    // A hidden line lying on a visible one (stacked parts) adds nothing
    let on_visible = |p: DVec2, edges: &[ProjectedEdge]| {
        edges
            .iter()
            .filter(|e| e.visible && e.curve == ProjCurve::Line)
            .any(|e| dist_to_segment(p, e.a, e.b) <= tol * 100.0)
    };
    let covered: Vec<bool> = edges
        .iter()
        .map(|e| !e.visible && e.curve == ProjCurve::Line && on_visible(e.a, &edges) && on_visible(e.b, &edges) && on_visible((e.a + e.b) / 2.0, &edges))
        .collect();
    let mut k = 0;
    edges.retain(|_| {
        k += 1;
        !covered[k - 1]
    });

    // Circles: arcs grouped by center and radius
    let mut circles: Vec<(Circle2, f64)> = Vec::new();
    for e in &edges {
        let ProjCurve::Arc { center, radius } = e.curve else { continue };
        let span = arc_span(e, center);
        if let Some((c, total)) = circles
            .iter_mut()
            .find(|(c, _)| (c.center - center).length() <= tol * 10.0 && (c.radius - radius).abs() <= tol * 10.0)
        {
            *total += span;
            c.visible |= e.visible;
        } else {
            circles.push((Circle2 { center, radius, full: false, visible: e.visible }, span));
        }
    }
    let circles = circles
        .into_iter()
        .map(|(mut c, span)| {
            c.full = span >= std::f64::consts::TAU - 1e-3;
            c
        })
        .collect();

    // Vertices: model vertices landing on an edge end, the nearest one to
    // the viewer when several project to the same point
    let mut vertices: Vec<(DVec2, DVec3)> = Vec::new();
    let ends: Vec<DVec2> = edges.iter().flat_map(|e| [e.a, e.b]).collect();
    for v in &model.vertices {
        let p = frame.project(*v);
        if !ends.iter().any(|q| (*q - p).length() <= tol * 10.0) {
            continue;
        }
        match vertices.iter_mut().find(|(q, _)| (*q - p).length() <= tol * 10.0) {
            Some(slot) if v.dot(frame.normal) > slot.1.dot(frame.normal) => slot.1 = *v,
            Some(_) => {}
            None => vertices.push((p, *v)),
        }
    }

    let mut lo = DVec2::splat(f64::MAX);
    let mut hi = DVec2::splat(f64::MIN);
    for p in edges.iter().flat_map(|e| e.points.iter()) {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if lo.x > hi.x {
        lo = DVec2::ZERO;
        hi = DVec2::ZERO;
    }
    Some(ViewGeom { frame, hidden, edges, circles, vertices, center: frame.project(model.center), lo, hi })
}

/// Angle covered by an arc edge (from its samples)
fn arc_span(e: &ProjectedEdge, center: DVec2) -> f64 {
    let mut total = 0.0;
    for w in e.points.windows(2) {
        let (a, b) = (w[0] - center, w[1] - center);
        total += a.perp_dot(b).atan2(a.dot(b)).abs();
    }
    total
}

#[cfg(test)]
mod tests;
