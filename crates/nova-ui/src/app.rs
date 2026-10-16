//! The NOVARIS CAD desktop application (egui + wgpu), laid out like
//! SolidWorks: CommandManager on top, FeatureManager tree on the left,
//! PropertyManager when editing, graphics area in the middle.

mod assembly;

use crate::camera::{Camera, StandardView};
use crate::render::{FrameParams, Viewport};
use crate::sketcher::{self, Preview, SelItem, Sketcher, Tool};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use glam::{DVec2, DVec3};
use nova_core::Id;
use nova_features::{
    Axis, EdgeFeature, EdgeRef, EndCondition, Extrude, ExtrudeOp, FeatureId, FeatureKind, FeatureState, FeatureTree, Hole,
    HoleKind, Pattern, PatternKind, PlaneBase, RefPlane, RevolveFeature,
};
use nova_io::PartDocument;
use nova_kernel::{Plane, RenderMesh};
use nova_sketch::{ConstraintType, Sketch, SketchElement, SolveStatus};
use std::path::PathBuf;

const SNAP_PX: f32 = 9.0;
const PICK_PX: f32 = 6.0;
const UNDER_DEFINED: Color32 = Color32::from_rgb(20, 40, 230);
const DEFINED: Color32 = Color32::from_rgb(10, 10, 10);
const CONFLICT: Color32 = Color32::from_rgb(220, 20, 20);
const SELECTED: Color32 = Color32::from_rgb(0, 160, 255);
const PREVIEW: Color32 = Color32::from_rgb(230, 120, 0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Features,
    Sketch,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum StdPlane {
    Front,
    Top,
    Right,
}

impl StdPlane {
    fn plane(&self) -> Plane {
        match self {
            StdPlane::Front => Plane::front(),
            StdPlane::Top => Plane::top(),
            StdPlane::Right => Plane::right(),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            StdPlane::Front => "Plan de face",
            StdPlane::Top => "Plan de dessus",
            StdPlane::Right => "Plan de droite",
        }
    }
}

/// What is selected in part mode
#[derive(Clone, Debug, PartialEq)]
enum Selection {
    None,
    Plane(StdPlane),
    Face { face: u32, plane: Plane },
    /// Model edges (indices into the body's edges), for fillets and chamfers
    Edges(Vec<u32>),
    Feature(FeatureId),
}

/// PropertyManager state for a feature being created or edited
struct FeatureEditor {
    feature: FeatureId,
    /// Tree before the edit, restored on cancel
    before: FeatureTree,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileAction {
    Open,
    SaveAs,
    ExportStep,
    ExportStl,
    InsertComponent,
}

struct FileDialog {
    action: FileAction,
    path: String,
    /// Give the path field keyboard focus on the first frame
    fresh: bool,
}

/// A dimension being typed in the Modify box
struct DimEdit {
    index: usize,
    text: String,
    at: Pos2,
}

pub struct NovarisApp {
    doc: PartDocument,
    path: Option<PathBuf>,
    dirty: bool,
    undo: Vec<FeatureTree>,
    redo: Vec<FeatureTree>,

    camera: Camera,
    viewport: Option<Viewport>,
    mesh: Option<RenderMesh>,
    /// Volume of the current body, computed once per rebuild
    volume: Option<f64>,
    mesh_dirty: bool,
    fit_pending: bool,

    tab: Tab,
    selection: Selection,
    hover_face: Option<u32>,
    hover_edge: Option<u32>,
    sketcher: Option<Sketcher>,
    /// Rollback position to restore after editing a sketch
    saved_rollback: Option<Option<usize>>,
    feature_editor: Option<FeatureEditor>,
    file_dialog: Option<FileDialog>,
    dim_edit: Option<DimEdit>,
    rename: Option<(FeatureId, String)>,
    dragging_point: Option<Id>,
    message: Option<(String, bool)>,
    cursor_sketch: Option<DVec2>,
    /// Frames to wait before the first rebuild, so the window shows first
    startup_frames: u8,
    /// Open assembly (kept while one of its parts is edited)
    assembly: Option<assembly::AssemblyView>,
    /// Showing the assembly rather than the part
    in_assembly: bool,
}

impl NovarisApp {
    pub fn new(cc: &eframe::CreationContext<'_>, doc: PartDocument, path: Option<PathBuf>) -> Self {
        let viewport = cc.wgpu_render_state.as_ref().map(Viewport::new);
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = Color32::from_rgb(240, 241, 244);
        cc.egui_ctx.set_visuals(visuals);
        let mut app = NovarisApp {
            doc,
            path,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            camera: Camera::default(),
            viewport,
            mesh: None,
            volume: None,
            mesh_dirty: true,
            fit_pending: true,
            tab: Tab::Features,
            selection: Selection::None,
            hover_face: None,
            hover_edge: None,
            sketcher: None,
            saved_rollback: None,
            feature_editor: None,
            file_dialog: None,
            dim_edit: None,
            rename: None,
            dragging_point: None,
            message: None,
            cursor_sketch: None,
            startup_frames: 2,
            assembly: None,
            in_assembly: false,
        };
        if app.doc.tree.is_empty() {
            app.startup_frames = 0;
            app.rebuild();
        }
        app
    }

    // ── Document operations ──────────────────────────────────────

    /// Open a part (.nvcad) or assembly (.nvasm) file
    pub fn open(&mut self, path: PathBuf) {
        self.run_file_action(FileAction::Open, path);
    }

    fn rebuild(&mut self) {
        self.doc.tree.rebuild();
        self.mesh = self.doc.tree.body().map(|b| b.tessellate((b.size() * 1e-3).max(0.01)));
        self.volume = self.doc.tree.body().map(|b| b.volume());
        self.mesh_dirty = true;
    }

    /// Snapshot for undo before a modification
    fn checkpoint(&mut self) {
        self.undo.push(self.doc.tree.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    fn undo(&mut self) {
        if let Some(t) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc.tree, t));
            self.after_history_change();
        }
    }

    fn redo(&mut self) {
        if let Some(t) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc.tree, t));
            self.after_history_change();
        }
    }

    fn after_history_change(&mut self) {
        self.selection = Selection::None;
        self.feature_editor = None;
        self.dirty = true;
        self.rebuild();
    }

    fn info(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), false));
    }

    fn error(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), true));
    }

    fn new_document(&mut self) {
        self.in_assembly = false;
        self.doc = PartDocument::new("Pièce1");
        self.path = None;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.sketcher = None;
        self.feature_editor = None;
        self.selection = Selection::None;
        self.fit_pending = true;
        self.camera = Camera::default();
        self.rebuild();
    }

    fn save(&mut self) {
        if self.in_assembly {
            self.asm_save();
            return;
        }
        match self.path.clone() {
            Some(p) => match self.doc.save(&p) {
                Ok(()) => {
                    self.dirty = false;
                    self.info(format!("Enregistré : {}", p.display()));
                }
                Err(e) => self.error(format!("Échec de l'enregistrement : {e}")),
            },
            None => self.open_file_dialog(FileAction::SaveAs),
        }
    }

    fn open_file_dialog(&mut self, action: FileAction) {
        let dir = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
        let (current, name) = match self.asm() {
            Some(a) => (a.path.clone(), a.doc.name.clone()),
            None => (self.path.clone(), self.doc.name.clone()),
        };
        let stem = current.as_ref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or(&name).to_string();
        let ext = match action {
            FileAction::Open | FileAction::SaveAs | FileAction::InsertComponent if self.in_assembly => {
                if action == FileAction::SaveAs { "nvasm" } else { "nvcad" }
            }
            FileAction::Open | FileAction::SaveAs | FileAction::InsertComponent => "nvcad",
            FileAction::ExportStep => "step",
            FileAction::ExportStl => "stl",
        };
        let path = match (&current, action) {
            (Some(p), FileAction::Open | FileAction::SaveAs) => p.display().to_string(),
            (Some(p), FileAction::InsertComponent) => p.with_file_name("piece.nvcad").display().to_string(),
            (Some(p), _) => p.with_extension(ext).display().to_string(),
            (None, FileAction::InsertComponent) => dir.join("piece.nvcad").display().to_string(),
            (None, _) => dir.join(format!("{stem}.{ext}")).display().to_string(),
        };
        self.file_dialog = Some(FileDialog { action, path, fresh: true });
    }

    fn run_file_action(&mut self, action: FileAction, path: PathBuf) {
        let body = self.doc.tree.body().cloned();
        let is_asm_file = path.extension().is_some_and(|e| e == nova_assembly::EXTENSION);
        let result = match action {
            FileAction::Open if is_asm_file => self.open_assembly(&path),
            FileAction::InsertComponent => self.asm_insert(&path),
            FileAction::SaveAs if self.in_assembly => self.asm_save_as(&path),
            FileAction::ExportStep if self.in_assembly => self.asm_export_step(&path),
            FileAction::ExportStl if self.in_assembly => Err("l'export STL d'un assemblage n'est pas encore disponible".into()),
            FileAction::Open => match PartDocument::load(&path) {
                Ok(doc) => {
                    self.new_document();
                    self.doc = doc;
                    self.path = Some(path.clone());
                    self.startup_frames = 2;
                    Ok(format!("Ouvert : {}", path.display()))
                }
                Err(e) => Err(e.to_string()),
            },
            FileAction::SaveAs => {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    self.doc.name = name.to_string();
                }
                self.doc.save(&path).map(|_| {
                    self.path = Some(path.clone());
                    self.dirty = false;
                    format!("Enregistré : {}", path.display())
                })
                .map_err(|e| e.to_string())
            }
            FileAction::ExportStep | FileAction::ExportStl => match body {
                None => Err("aucun corps à exporter".into()),
                Some(b) => {
                    let r = if action == FileAction::ExportStep {
                        nova_io::export_step(&b, &path)
                    } else {
                        nova_io::export_stl(&b, &path, (b.size() * 2e-4).max(0.005))
                    };
                    r.map(|_| format!("Exporté : {}", path.display())).map_err(|e| e.to_string())
                }
            },
        };
        match result {
            Ok(m) => self.info(m),
            Err(e) => self.error(e),
        }
    }

    // ── Sketch and feature commands ──────────────────────────────

    fn selected_plane(&self) -> Option<Plane> {
        self.selected_plane_base().and_then(|(b, _)| self.plane_of(b))
    }

    /// The selection as a plane to build on, with the reference plane
    /// feature it comes from (if any)
    fn selected_plane_base(&self) -> Option<(PlaneBase, Option<FeatureId>)> {
        match &self.selection {
            Selection::Plane(StdPlane::Front) => Some((PlaneBase::Front, None)),
            Selection::Plane(StdPlane::Top) => Some((PlaneBase::Top, None)),
            Selection::Plane(StdPlane::Right) => Some((PlaneBase::Right, None)),
            Selection::Face { plane, .. } => Some((PlaneBase::Fixed(*plane), None)),
            Selection::Feature(id) if matches!(self.doc.tree.get(*id).map(|f| &f.kind), Some(FeatureKind::RefPlane(_))) => {
                Some((PlaneBase::Feature(*id), Some(*id)))
            }
            _ => None,
        }
    }

    fn plane_of(&self, base: PlaneBase) -> Option<Plane> {
        match base {
            PlaneBase::Front => Some(Plane::front()),
            PlaneBase::Top => Some(Plane::top()),
            PlaneBase::Right => Some(Plane::right()),
            PlaneBase::Fixed(p) => Some(p),
            PlaneBase::Feature(id) => self.doc.tree.feature_plane(id),
        }
    }

    fn start_new_sketch(&mut self) {
        let Some((base, support)) = self.selected_plane_base() else {
            self.error("Sélectionnez un plan (dans l'arbre) ou une face plane, puis cliquez Esquisse.");
            return;
        };
        let Some(plane) = self.plane_of(base) else {
            self.error("Ce plan a une erreur de reconstruction.");
            return;
        };
        self.camera.normal_to(&plane);
        let mut sketch = Sketch::on_plane("", plane);
        sketch.support = support;
        self.sketcher = Some(Sketcher::new(None, sketch));
        self.saved_rollback = None;
        self.tab = Tab::Sketch;
        self.selection = Selection::None;
        self.info("Esquisse : choisissez un outil (Ligne, Rectangle, Cercle, Arc).");
    }

    fn edit_sketch(&mut self, id: FeatureId) {
        let Some(FeatureKind::Sketch(s)) = self.doc.tree.get(id).map(|f| f.kind.clone()) else { return };
        // Roll back to just before the sketch, as SolidWorks does
        self.saved_rollback = Some(self.doc.tree.rollback());
        let index = self.doc.tree.index_of(id).unwrap_or(0);
        self.doc.tree.set_rollback(Some(index));
        self.rebuild();
        self.camera.normal_to(&s.plane);
        self.sketcher = Some(Sketcher::new(Some(id), s));
        self.tab = Tab::Sketch;
    }

    fn exit_sketch(&mut self, keep: bool) {
        let Some(sk) = self.sketcher.take() else { return };
        self.dim_edit = None;
        if let Some(r) = self.saved_rollback.take() {
            self.doc.tree.set_rollback(r);
        }
        if keep {
            // Lone points count: they position holes
            let has_geometry = !sk.sketch.elements.is_empty();
            match sk.feature {
                Some(id) => {
                    self.checkpoint();
                    if let Some(s) = self.doc.tree.sketch_mut(id) {
                        *s = sk.sketch;
                    }
                    self.selection = Selection::Feature(id);
                }
                None if has_geometry => {
                    self.checkpoint();
                    let id = self.doc.tree.add_sketch(sk.sketch);
                    self.selection = Selection::Feature(id);
                }
                None => {}
            }
        }
        self.tab = Tab::Features;
        self.message = None;
        self.rebuild();
    }

    /// The sketch an extrusion command should use
    fn sketch_for_extrude(&self) -> Option<FeatureId> {
        if let Selection::Feature(id) = self.selection {
            if matches!(self.doc.tree.get(id).map(|f| &f.kind), Some(FeatureKind::Sketch(_))) {
                return Some(id);
            }
        }
        // Otherwise the last sketch not yet used by a feature
        self.doc
            .tree
            .features()
            .iter()
            .rev()
            .find(|f| matches!(f.kind, FeatureKind::Sketch(_)) && self.doc.tree.dependents(f.id).is_empty())
            .map(|f| f.id)
    }

    fn start_extrude(&mut self, op: ExtrudeOp) {
        if self.sketcher.is_some() {
            self.exit_sketch(true);
        }
        let Some(sketch) = self.sketch_for_extrude() else {
            self.error("Sélectionnez d'abord une esquisse fermée à extruder.");
            return;
        };
        let before = self.doc.tree.clone();
        let end = if op == ExtrudeOp::Cut && self.doc.tree.body().is_some() {
            EndCondition::ThroughAll
        } else {
            EndCondition::Blind { depth: 10.0 }
        };
        // A cut sketched on a face goes into the material
        let reverse = op == ExtrudeOp::Cut && self.sketch_points_into_body(sketch);
        match self.doc.tree.add_extrude(Extrude { sketch, end, reverse, op }) {
            Ok(feature) => {
                self.checkpoint_from(before.clone());
                self.feature_editor = Some(FeatureEditor { feature, before });
                self.selection = Selection::Feature(feature);
                self.rebuild();
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    /// Add a feature and open its PropertyManager
    fn start_feature(&mut self, kind: FeatureKind) {
        if self.sketcher.is_some() {
            self.exit_sketch(true);
        }
        let before = self.doc.tree.clone();
        match self.doc.tree.add_feature(kind) {
            Ok(feature) => {
                self.checkpoint_from(before.clone());
                self.feature_editor = Some(FeatureEditor { feature, before });
                self.selection = Selection::Feature(feature);
                self.rebuild();
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    fn start_revolve(&mut self, op: ExtrudeOp) {
        if self.sketcher.is_some() {
            self.exit_sketch(true);
        }
        let Some(sketch) = self.sketch_for_extrude() else {
            self.error("Sélectionnez une esquisse avec un profil fermé et un axe (ligne de construction).");
            return;
        };
        self.start_feature(FeatureKind::Revolve(RevolveFeature { sketch, axis: None, angle_deg: 360.0, mid_plane: false, reverse: false, op }));
    }

    fn start_hole(&mut self) {
        if self.sketcher.is_some() {
            self.exit_sketch(true);
        }
        let Some(sketch) = self.sketch_for_extrude() else {
            self.error("Esquissez des points (outil Point) sur une face, puis cliquez Perçage.");
            return;
        };
        let end = if self.doc.tree.body().is_some() { EndCondition::ThroughAll } else { EndCondition::Blind { depth: 10.0 } };
        let reverse = !self.sketch_points_into_body_at_points(sketch);
        self.start_feature(FeatureKind::Hole(Hole { sketch, kind: HoleKind::Simple { diameter: 6.0 }, end, reverse }));
    }

    /// Features a pattern command repeats: the selected one, else the last
    fn pattern_seed(&self) -> Option<FeatureId> {
        let repeatable = |k: &FeatureKind| matches!(k, FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Hole(_) | FeatureKind::Pattern(_));
        if let Selection::Feature(id) = self.selection {
            if self.doc.tree.get(id).is_some_and(|f| repeatable(&f.kind)) {
                return Some(id);
            }
        }
        self.doc.tree.features().iter().rev().find(|f| repeatable(&f.kind)).map(|f| f.id)
    }

    fn start_pattern(&mut self, kind: PatternKind) {
        let Some(seed) = self.pattern_seed() else {
            self.error("Sélectionnez la fonction à répéter dans l'arbre.");
            return;
        };
        self.start_feature(FeatureKind::Pattern(Pattern { seeds: vec![seed], kind }));
    }

    fn start_ref_plane(&mut self) {
        let base = self.selected_plane_base().map(|(b, _)| b).unwrap_or(PlaneBase::Front);
        self.start_feature(FeatureKind::RefPlane(RefPlane { base, offset: 10.0 }));
    }

    /// Like `sketch_points_into_body`, for sketches made of lone points
    fn sketch_points_into_body_at_points(&self, sketch: FeatureId) -> bool {
        let (Some(FeatureKind::Sketch(s)), Some(mesh)) = (self.doc.tree.get(sketch).map(|f| &f.kind), self.mesh.as_ref())
        else {
            return true;
        };
        let Some(p) = s.free_points().into_iter().next() else { return true };
        let c = s.plane.to_world(DVec2::new(p.x, p.y));
        let n = s.plane.normal();
        let eps = mesh.bounds().map(|(lo, hi)| (hi - lo).length() * 1e-4).unwrap_or(1e-3).max(1e-4);
        point_in_mesh(mesh, c - n * eps) || !point_in_mesh(mesh, c + n * eps)
    }

    fn checkpoint_from(&mut self, before: FeatureTree) {
        self.undo.push(before);
        self.redo.clear();
        self.dirty = true;
    }

    /// Whether material lies just behind the sketch (so a cut must go
    /// against the plane normal, into the part)
    fn sketch_points_into_body(&self, sketch: FeatureId) -> bool {
        let (Some(FeatureKind::Sketch(s)), Some(mesh)) = (self.doc.tree.get(sketch).map(|f| &f.kind), self.mesh.as_ref())
        else {
            return false;
        };
        let Some(profile) = s.profiles().ok().and_then(|p| p.into_iter().next()) else { return false };
        // A point inside the profile (centroid of its outline samples)
        let pts = profile.outer.sample(16);
        let c2 = pts.iter().fold(DVec2::ZERO, |a, p| a + *p) / pts.len() as f64;
        let c = s.plane.to_world(c2);
        let n = s.plane.normal();
        let eps = mesh.bounds().map(|(lo, hi)| (hi - lo).length() * 1e-4).unwrap_or(1e-3).max(1e-4);
        if point_in_mesh(mesh, c - n * eps) {
            true
        } else if point_in_mesh(mesh, c + n * eps) {
            false
        } else {
            // Fall back to the side holding most of the part
            mesh.bounds().is_some_and(|(lo, hi)| ((lo + hi) / 2.0 - s.plane.origin).dot(n) < 0.0)
        }
    }

    fn edit_feature(&mut self, id: FeatureId) {
        match self.doc.tree.get(id).map(|f| &f.kind) {
            Some(FeatureKind::Sketch(_)) => self.edit_sketch(id),
            Some(_) => {
                self.feature_editor = Some(FeatureEditor { feature: id, before: self.doc.tree.clone() });
                self.selection = Selection::Feature(id);
            }
            None => {}
        }
    }

    fn delete_feature(&mut self, id: FeatureId) {
        let before = self.doc.tree.clone();
        match self.doc.tree.remove(id) {
            Ok(_) => {
                self.checkpoint_from(before);
                self.selection = Selection::None;
                self.rebuild();
            }
            Err(e) => self.error(format!("Suppression impossible : {e}")),
        }
    }

    // ── UI ───────────────────────────────────────────────────────

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if std::env::var_os("NOVARIS_DEBUG_KEYS").is_some() {
            ctx.input(|i| {
                for e in &i.events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                        eprintln!("key {:?} {:?}", key, modifiers);
                    }
                }
            });
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        use egui::{Key, Modifiers};
        // Modifiers are taken from each key event, not the end-of-frame state
        let key = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        let cmd = Modifiers::COMMAND;
        if key(cmd | Modifiers::SHIFT, Key::Z) || key(cmd, Key::Y) {
            if let Some(a) = self.assembly.as_mut().filter(|_| self.in_assembly) {
                a.redo();
                self.mesh_dirty = true;
            } else if self.sketcher.is_none() {
                self.redo();
            }
        } else if key(cmd, Key::Z) {
            if let Some(a) = self.assembly.as_mut().filter(|_| self.in_assembly) {
                a.undo();
                self.mesh_dirty = true;
            } else if self.sketcher.is_none() {
                self.undo();
            }
        } else if key(cmd, Key::S) {
            self.save();
        } else if key(cmd, Key::O) {
            self.open_file_dialog(FileAction::Open);
        } else if key(cmd, Key::N) {
            self.new_document();
        } else if key(cmd, Key::Num1) {
            self.camera.set_view(StandardView::Front);
        } else if key(cmd, Key::Num4) {
            self.camera.set_view(StandardView::Right);
        } else if key(cmd, Key::Num5) {
            self.camera.set_view(StandardView::Top);
        } else if key(cmd, Key::Num7) {
            self.camera.set_view(StandardView::Isometric);
        } else if key(cmd, Key::Num8) {
            if let Some(p) = self.sketcher.as_ref().map(|s| s.sketch.plane).or_else(|| self.selected_plane()) {
                self.camera.normal_to(&p);
            }
        } else if key(Modifiers::NONE, Key::F) {
            self.fit_pending = true;
        } else if self.in_assembly && key(Modifiers::NONE, Key::Escape) {
            self.asm_escape();
        } else if self.in_assembly && key(Modifiers::NONE, Key::Delete) {
            self.asm_delete_selection();
        } else if key(Modifiers::NONE, Key::Escape) {
            if let Some(s) = self.sketcher.as_mut() {
                if s.stage.is_empty() && s.tool != Tool::Select {
                    s.set_tool(Tool::Select);
                } else {
                    s.cancel();
                    s.selection.clear();
                }
            } else {
                self.selection = Selection::None;
            }
        } else if key(Modifiers::NONE, Key::Delete) {
            if let Some(s) = self.sketcher.as_mut() {
                s.delete_selection();
            } else if let Selection::Feature(id) = self.selection {
                self.delete_feature(id);
            }
        } else if self.sketcher.is_some() && key(Modifiers::NONE, Key::L) {
            self.sketcher.as_mut().unwrap().set_tool(Tool::Line);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Fichier", |ui| {
                if ui.button("Nouveau        Ctrl+N").clicked() {
                    self.new_document();
                    ui.close_menu();
                }
                if ui.button("Nouvel assemblage").clicked() {
                    self.new_assembly();
                    ui.close_menu();
                }
                if ui.button("Ouvrir…        Ctrl+O").clicked() {
                    self.open_file_dialog(FileAction::Open);
                    ui.close_menu();
                }
                if ui.button("Enregistrer    Ctrl+S").clicked() {
                    self.save();
                    ui.close_menu();
                }
                if ui.button("Enregistrer sous…").clicked() {
                    self.open_file_dialog(FileAction::SaveAs);
                    ui.close_menu();
                }
                if self.in_assembly && ui.button("Insérer des composants…").clicked() {
                    self.open_file_dialog(FileAction::InsertComponent);
                    ui.close_menu();
                }
                if self.assembly.is_some() && !self.in_assembly && ui.button("Retour à l'assemblage").clicked() {
                    self.back_to_assembly();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Exporter STEP…").clicked() {
                    self.open_file_dialog(FileAction::ExportStep);
                    ui.close_menu();
                }
                if ui.button("Exporter STL…").clicked() {
                    self.open_file_dialog(FileAction::ExportStl);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Quitter").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Édition", |ui| {
                if let Some(a) = self.assembly.as_mut().filter(|_| self.in_assembly) {
                    if ui.add_enabled(a.can_undo(), egui::Button::new("Annuler   Ctrl+Z")).clicked() {
                        a.undo();
                        self.mesh_dirty = true;
                        ui.close_menu();
                    }
                    if ui.add_enabled(a.can_redo(), egui::Button::new("Rétablir   Ctrl+Y")).clicked() {
                        a.redo();
                        self.mesh_dirty = true;
                        ui.close_menu();
                    }
                    return;
                }
                let in_sketch = self.sketcher.is_some();
                if ui.add_enabled(!in_sketch && !self.undo.is_empty(), egui::Button::new("Annuler   Ctrl+Z")).clicked() {
                    self.undo();
                    ui.close_menu();
                }
                if ui.add_enabled(!in_sketch && !self.redo.is_empty(), egui::Button::new("Rétablir   Ctrl+Y")).clicked() {
                    self.redo();
                    ui.close_menu();
                }
            });
            ui.menu_button("Affichage", |ui| {
                for (label, view) in [
                    ("Face          Ctrl+1", StandardView::Front),
                    ("Droite        Ctrl+4", StandardView::Right),
                    ("Dessus        Ctrl+5", StandardView::Top),
                    ("Isométrique   Ctrl+7", StandardView::Isometric),
                ] {
                    if ui.button(label).clicked() {
                        self.camera.set_view(view);
                        ui.close_menu();
                    }
                }
                if ui.button("Zoom au mieux   F").clicked() {
                    self.fit_pending = true;
                    ui.close_menu();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (name, dirty) = match self.asm() {
                    Some(a) => (a.doc.name.clone(), a.dirty),
                    None => (self.doc.name.clone(), self.dirty),
                };
                let title = format!("{name}{} — NOVARIS CAD", if dirty { " *" } else { "" });
                ui.label(egui::RichText::new(title).strong());
            });
        });
    }

    fn command_manager(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Features, "Fonctions");
            ui.selectable_value(&mut self.tab, Tab::Sketch, "Esquisse");
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| match self.tab {
            Tab::Features => {
                if big_button(ui, "Bossage/Base\nextrudé").clicked() {
                    self.start_extrude(ExtrudeOp::Boss);
                }
                if big_button(ui, "Enlèvement de\nmatière extrudé").clicked() {
                    self.start_extrude(ExtrudeOp::Cut);
                }
                if big_button(ui, "Bossage avec\nrévolution").clicked() {
                    self.start_revolve(ExtrudeOp::Boss);
                }
                if big_button(ui, "Enlèvement avec\nrévolution").clicked() {
                    self.start_revolve(ExtrudeOp::Cut);
                }
                if big_button(ui, "Perçage").clicked() {
                    self.start_hole();
                }
                ui.separator();
                if big_button(ui, "Répétition\nlinéaire").clicked() {
                    self.start_pattern(PatternKind::Linear { axis: Axis::X, spacing: 20.0, count: 3, reverse: false });
                }
                if big_button(ui, "Répétition\ncirculaire").clicked() {
                    self.start_pattern(PatternKind::Circular { axis: Axis::Z, angle_deg: 360.0, count: 4 });
                }
                if big_button(ui, "Symétrie").clicked() {
                    self.start_pattern(PatternKind::Mirror { plane: PlaneBase::Right });
                }
                ui.separator();
                if big_button(ui, "Congé").clicked() {
                    self.start_edge_feature(true);
                }
                if big_button(ui, "Chanfrein").clicked() {
                    self.start_edge_feature(false);
                }
                ui.separator();
                if big_button(ui, "Plan").clicked() {
                    self.start_ref_plane();
                }
                if big_button(ui, "Esquisse").clicked() {
                    self.start_new_sketch();
                }
            }
            Tab::Sketch => {
                if self.sketcher.is_none() {
                    if big_button(ui, "Esquisse").clicked() {
                        self.start_new_sketch();
                    }
                    ui.label("Sélectionnez un plan ou une face plane puis cliquez Esquisse.");
                    return;
                }
                if big_button(ui, "Quitter\nl'esquisse").clicked() {
                    self.exit_sketch(true);
                    return;
                }
                ui.separator();
                let sk = self.sketcher.as_mut().unwrap();
                for tool in [Tool::Select, Tool::SmartDimension, Tool::Line, Tool::Rectangle, Tool::Circle, Tool::Arc, Tool::Centerline, Tool::Point] {
                    let selected = sk.tool == tool;
                    if ui
                        .add_sized([96.0, 40.0], egui::SelectableLabel::new(selected, tool.label()))
                        .clicked()
                    {
                        sk.set_tool(tool);
                    }
                }
            }
        });
    }

    fn feature_manager(&mut self, ui: &mut egui::Ui) {
        if self.assembly.is_some() && ui.button("↩ Enregistrer et revenir à l'assemblage").clicked() {
            self.back_to_assembly();
            return;
        }
        ui.label(egui::RichText::new(&self.doc.name).strong());
        ui.separator();
        for plane in [StdPlane::Front, StdPlane::Top, StdPlane::Right] {
            let selected = self.selection == Selection::Plane(plane);
            if ui.selectable_label(selected, plane.label()).clicked() {
                self.selection = Selection::Plane(plane);
            }
        }
        ui.label("Origine");
        ui.separator();

        let features: Vec<(FeatureId, String, bool, bool, &'static str)> = self
            .doc
            .tree
            .features()
            .iter()
            .map(|f| (f.id, f.name.clone(), matches!(f.kind, FeatureKind::Sketch(_)), f.suppressed, feature_icon(&f.kind)))
            .collect();
        let rollback = self.doc.tree.rollback();
        let mut action: Option<TreeAction> = None;
        for (i, (id, name, is_sketch, suppressed, icon)) in features.iter().enumerate() {
            if rollback == Some(i) {
                rollback_bar(ui);
            }
            let state = self.doc.tree.state(*id).cloned();
            let prefix = match (&state, is_sketch) {
                (Some(FeatureState::UnderDefined { .. }), _) => "(-) ",
                _ => "",
            };
            let mut text = egui::RichText::new(format!("{icon} {prefix}{name}"));
            match &state {
                Some(FeatureState::Error(_)) => text = text.color(CONFLICT),
                Some(FeatureState::Suppressed) | Some(FeatureState::RolledBack) => text = text.color(Color32::GRAY),
                _ => {}
            }
            if *suppressed {
                text = text.italics();
            }
            if let Some((rid, buf)) = self.rename.as_mut().filter(|(rid, _)| rid == id) {
                let r = ui.text_edit_singleline(buf);
                if r.lost_focus() {
                    let (rid, buf) = (*rid, buf.clone());
                    action = Some(Box::new(move |app: &mut Self| {
                        app.checkpoint();
                        let _ = app.doc.tree.rename(rid, buf);
                        app.rename = None;
                    }));
                } else {
                    r.request_focus();
                }
                continue;
            }
            let selected = self.selection == Selection::Feature(*id);
            let resp = ui.selectable_label(selected, text);
            if let Some(FeatureState::Error(msg)) = &state {
                resp.clone().on_hover_text(format!("Erreur de reconstruction : {msg}"));
            }
            if resp.clicked() {
                self.selection = Selection::Feature(*id);
            }
            if resp.double_clicked() {
                let id = *id;
                action = Some(Box::new(move |app: &mut Self| app.edit_feature(id)));
            }
            let id = *id;
            let suppressed = *suppressed;
            let is_sketch = *is_sketch;
            resp.context_menu(|ui| {
                if ui.button(if is_sketch { "Éditer l'esquisse" } else { "Éditer la fonction" }).clicked() {
                    action = Some(Box::new(move |app: &mut Self| app.edit_feature(id)));
                    ui.close_menu();
                }
                if ui.button(if suppressed { "Réactiver" } else { "Supprimer (suppress)" }).clicked() {
                    action = Some(Box::new(move |app: &mut Self| {
                        app.checkpoint();
                        let _ = app.doc.tree.set_suppressed(id, !suppressed);
                        app.rebuild();
                    }));
                    ui.close_menu();
                }
                if ui.button("Renommer").clicked() {
                    action = Some(Box::new(move |app: &mut Self| {
                        let name = app.doc.tree.get(id).map(|f| f.name.clone()).unwrap_or_default();
                        app.rename = Some((id, name));
                    }));
                    ui.close_menu();
                }
                if ui.button("Reculer la barre ici").clicked() {
                    action = Some(Box::new(move |app: &mut Self| {
                        let i = app.doc.tree.index_of(id).unwrap_or(0);
                        app.doc.tree.set_rollback(Some(i));
                        app.rebuild();
                    }));
                    ui.close_menu();
                }
                if ui.button("Effacer").clicked() {
                    action = Some(Box::new(move |app: &mut Self| app.delete_feature(id)));
                    ui.close_menu();
                }
            });
        }
        if rollback.is_none() {
            rollback_bar(ui);
        } else if ui.small_button("Avancer la barre à la fin").clicked() {
            action = Some(Box::new(|app: &mut Self| {
                app.doc.tree.set_rollback(None);
                app.rebuild();
            }));
        }
        if let Some(a) = action {
            a(self);
        }
    }

    fn feature_property_manager(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = self.feature_editor.as_ref() else { return };
        let id = editor.feature;
        let name = self.doc.tree.get(id).map(|f| f.name.clone()).unwrap_or_default();
        ui.heading(name);
        ui.horizontal(|ui| {
            if ui.button(egui::RichText::new("✔ Valider").color(Color32::from_rgb(0, 140, 0))).clicked() {
                self.feature_editor = None;
                return;
            }
            if ui.button(egui::RichText::new("✖ Annuler").color(CONFLICT)).clicked() {
                if let Some(ed) = self.feature_editor.take() {
                    self.doc.tree = ed.before;
                    self.undo.pop();
                    self.rebuild();
                }
            }
        });
        if self.feature_editor.is_none() {
            return;
        }
        ui.separator();
        let Some(original) = self.doc.tree.get(id).map(|f| f.kind.clone()) else { return };
        let mut kind = original.clone();
        let candidates: Vec<(FeatureId, String)> = self
            .doc
            .tree
            .features()
            .iter()
            .take_while(|f| f.id != id)
            .map(|f| (f.id, f.name.clone()))
            .collect();
        let ref_planes: Vec<(FeatureId, String)> = self
            .doc
            .tree
            .features()
            .iter()
            .take_while(|f| f.id != id)
            .filter(|f| matches!(f.kind, FeatureKind::RefPlane(_)))
            .map(|f| (f.id, f.name.clone()))
            .collect();
        match &mut kind {
            FeatureKind::Extrude(e) => {
                ui.label("Opération");
                ui.radio_value(&mut e.op, ExtrudeOp::Boss, "Bossage (ajouter)");
                ui.radio_value(&mut e.op, ExtrudeOp::Cut, "Enlèvement de matière");
                ui.add_space(6.0);
                ui.label("Direction 1");
                end_condition_ui(ui, "end", &mut e.end, true);
                ui.checkbox(&mut e.reverse, "Inverser la direction");
            }
            FeatureKind::Revolve(r) => {
                ui.label("Opération");
                ui.radio_value(&mut r.op, ExtrudeOp::Boss, "Bossage (ajouter)");
                ui.radio_value(&mut r.op, ExtrudeOp::Cut, "Enlèvement de matière");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Angle");
                    ui.add(egui::DragValue::new(&mut r.angle_deg).speed(1.0).range(0.1..=360.0).suffix(" °"));
                });
                ui.checkbox(&mut r.mid_plane, "Plan milieu");
                ui.add_enabled(!r.mid_plane, egui::Checkbox::new(&mut r.reverse, "Inverser la direction"));
                ui.weak("Axe : première ligne de construction de l'esquisse.");
            }
            FeatureKind::Hole(h) => {
                ui.label("Type de perçage");
                let mut cb = matches!(h.kind, HoleKind::Counterbore { .. });
                let d = match h.kind {
                    HoleKind::Simple { diameter } | HoleKind::Counterbore { diameter, .. } => diameter,
                };
                ui.horizontal(|ui| {
                    ui.radio_value(&mut cb, false, "Simple");
                    ui.radio_value(&mut cb, true, "Lamé");
                });
                let mut dia = d;
                ui.horizontal(|ui| {
                    ui.label("Diamètre");
                    ui.add(egui::DragValue::new(&mut dia).speed(0.1).range(0.1..=1e4).suffix(" mm"));
                });
                h.kind = if cb {
                    let (mut cbd, mut cbh) = match h.kind {
                        HoleKind::Counterbore { cb_diameter, cb_depth, .. } => (cb_diameter, cb_depth),
                        _ => (dia * 1.8, dia * 0.6),
                    };
                    ui.horizontal(|ui| {
                        ui.label("Ø lamage");
                        ui.add(egui::DragValue::new(&mut cbd).speed(0.1).range(dia + 0.01..=1e4).suffix(" mm"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Prof. lamage");
                        ui.add(egui::DragValue::new(&mut cbh).speed(0.1).range(0.01..=1e4).suffix(" mm"));
                    });
                    HoleKind::Counterbore { diameter: dia, cb_diameter: cbd.max(dia + 0.01), cb_depth: cbh }
                } else {
                    HoleKind::Simple { diameter: dia }
                };
                ui.add_space(6.0);
                ui.label("Condition de fin");
                end_condition_ui(ui, "hole_end", &mut h.end, false);
                ui.checkbox(&mut h.reverse, "Inverser la direction");
                ui.weak("Un perçage à chaque point isolé de l'esquisse.");
            }
            FeatureKind::RefPlane(p) => {
                ui.label("Plan de base");
                plane_base_ui(ui, "base", &mut p.base, &ref_planes);
                ui.horizontal(|ui| {
                    ui.label("Décalage");
                    ui.add(egui::DragValue::new(&mut p.offset).speed(0.5).suffix(" mm"));
                });
            }
            FeatureKind::Pattern(pat) => {
                match &mut pat.kind {
                    PatternKind::Linear { axis, spacing, count, reverse } => {
                        axis_ui(ui, "lin_axis", axis);
                        ui.checkbox(reverse, "Inverser la direction");
                        ui.horizontal(|ui| {
                            ui.label("Espacement");
                            ui.add(egui::DragValue::new(spacing).speed(0.5).range(0.01..=1e5).suffix(" mm"));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Occurrences");
                            ui.add(egui::DragValue::new(count).range(2..=200));
                        });
                    }
                    PatternKind::Circular { axis, angle_deg, count } => {
                        axis_ui(ui, "circ_axis", axis);
                        ui.horizontal(|ui| {
                            ui.label("Angle total");
                            ui.add(egui::DragValue::new(angle_deg).speed(1.0).range(0.1..=360.0).suffix(" °"));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Occurrences");
                            ui.add(egui::DragValue::new(count).range(2..=200));
                        });
                    }
                    PatternKind::Mirror { plane } => {
                        ui.label("Plan de symétrie");
                        plane_base_ui(ui, "mirror", plane, &ref_planes);
                    }
                }
                ui.add_space(6.0);
                ui.label("Fonctions à répéter");
                for (fid, fname) in &candidates {
                    let repeatable = matches!(
                        self.doc.tree.get(*fid).map(|f| &f.kind),
                        Some(FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Hole(_) | FeatureKind::Pattern(_))
                    );
                    if !repeatable {
                        continue;
                    }
                    let mut on = pat.seeds.contains(fid);
                    if ui.checkbox(&mut on, fname).changed() {
                        if on {
                            pat.seeds.push(*fid);
                        } else if pat.seeds.len() > 1 {
                            pat.seeds.retain(|s| s != fid);
                        }
                    }
                }
            }
            FeatureKind::Fillet(e) | FeatureKind::Chamfer(e) => {
                let is_fillet = matches!(original, FeatureKind::Fillet(_));
                ui.horizontal(|ui| {
                    ui.label(if is_fillet { "Rayon" } else { "Distance" });
                    ui.add(egui::DragValue::new(&mut e.size).speed(0.1).range(0.01..=1e4).suffix(" mm"));
                });
                ui.label(format!("{} arête(s)", e.edges.len()));
                if let (Selection::Edges(sel), Some(body)) = (&self.selection, self.doc.tree.body()) {
                    if ui.button("Ajouter les arêtes sélectionnées").clicked() {
                        let infos = body.edges();
                        for &i in sel {
                            if let Some(info) = infos.get(i as usize) {
                                let r = EdgeRef { midpoint: info.mid };
                                if !e.edges.contains(&r) {
                                    e.edges.push(r);
                                }
                            }
                        }
                    }
                }
                if e.edges.len() > 1 && ui.button("Retirer la dernière arête").clicked() {
                    e.edges.pop();
                }
                ui.weak("Maj+clic dans la vue pour sélectionner des arêtes.");
            }
            FeatureKind::Sketch(_) => {}
        }

        if format!("{:?}", kind) != format!("{:?}", original) {
            if let Some(f) = self.doc.tree.get_mut(id) {
                f.kind = kind;
            }
            self.dirty = true;
            self.rebuild();
        }
        if let Some(FeatureState::Error(m)) = self.doc.tree.state(id) {
            ui.add_space(8.0);
            ui.colored_label(CONFLICT, format!("⚠ {m}"));
        }
    }

    fn sketch_property_manager(&mut self, ui: &mut egui::Ui) {
        let Some(sk) = self.sketcher.as_mut() else { return };
        ui.heading(if sk.feature.is_some() { "Édition de l'esquisse" } else { "Nouvelle esquisse" });
        let mut exit = None;
        ui.horizontal(|ui| {
            if ui.button(egui::RichText::new("✔ Valider").color(Color32::from_rgb(0, 140, 0))).clicked() {
                exit = Some(true);
            }
            if ui.button(egui::RichText::new("✖ Abandonner").color(CONFLICT)).clicked() {
                exit = Some(false);
            }
        });
        if let Some(keep) = exit {
            self.exit_sketch(keep);
            return;
        }
        let sk = self.sketcher.as_mut().unwrap();
        ui.separator();
        ui.label(egui::RichText::new("Ajouter des relations").strong());
        let rels = sk.available_relations();
        if rels.is_empty() {
            ui.weak("Sélectionnez des entités (Maj+clic pour en ajouter).");
        }
        ui.horizontal_wrapped(|ui| {
            for r in rels {
                if ui.button(r.label()).clicked() {
                    sk.apply_relation(r);
                }
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("Relations et cotes").strong());
        let mut remove = None;
        let mut set_dim = None;
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (i, c) in sk.sketch.constraints.iter().enumerate() {
                ui.horizontal(|ui| {
                    let color = if c.is_satisfied { Color32::DARK_GRAY } else { CONFLICT };
                    if let Some(v) = sketcher::dimension_value(&c.constraint_type) {
                        let label = sketcher::constraint_label(&c.constraint_type);
                        ui.colored_label(color, label.split(' ').next().unwrap_or(""));
                        let mut value = v;
                        if ui.add(egui::DragValue::new(&mut value).speed(0.1).range(0.001..=1e6)).changed() {
                            set_dim = Some((i, value));
                        }
                    } else {
                        let selected = sk.selection.contains(&SelItem::Constraint(i));
                        if ui
                            .selectable_label(selected, egui::RichText::new(sketcher::constraint_label(&c.constraint_type)).color(color))
                            .clicked()
                        {
                            sk.selection = vec![SelItem::Constraint(i)];
                        }
                    }
                    if ui.small_button("✖").on_hover_text("Supprimer").clicked() {
                        remove = Some(i);
                    }
                });
            }
        });
        if let Some((i, v)) = set_dim {
            sk.set_dimension(i, v);
        }
        if let Some(i) = remove {
            sk.selection = vec![SelItem::Constraint(i)];
            sk.delete_selection();
        }
        if let Some(e) = &sk.error {
            ui.colored_label(CONFLICT, format!("⚠ {e}"));
        }
        if let Err(e) = sk.sketch.profiles() {
            if sk.sketch.elements.values().any(|e| !matches!(e, SketchElement::Point(_))) {
                ui.add_space(6.0);
                ui.colored_label(Color32::from_rgb(170, 90, 0), format!("Profil : {e}"));
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some((m, err)) = &self.message {
                ui.colored_label(if *err { CONFLICT } else { Color32::DARK_GRAY }, m);
            } else {
                ui.label("NOVARIS CAD");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label("MMGS");
                ui.separator();
                if self.in_assembly {
                    self.asm_status(ui);
                } else if let Some(sk) = &self.sketcher {
                    let empty = sk.sketch.elements.is_empty();
                    let (text, color) = match sk.report.map(|r| r.status) {
                        _ if empty => ("Esquisse vide".to_string(), Color32::DARK_GRAY),
                        Some(SolveStatus::FullyConstrained) => ("Entièrement contrainte".to_string(), DEFINED),
                        Some(SolveStatus::UnderConstrained { dof }) => (format!("Sous-contrainte ({dof} DDL)"), UNDER_DEFINED),
                        Some(SolveStatus::OverConstrained { redundant }) => (format!("Sur-contrainte ({redundant} redondante(s))"), CONFLICT),
                        Some(SolveStatus::Inconsistent) => ("Relations contradictoires".to_string(), CONFLICT),
                        None => (String::new(), DEFINED),
                    };
                    ui.colored_label(color, text);
                    ui.separator();
                    if let Some(p) = self.cursor_sketch {
                        ui.label(format!("{:.2} mm, {:.2} mm", p.x, p.y));
                    }
                } else if let Some(v) = self.volume {
                    ui.label(format!("Volume : {:.1} mm³", v));
                }
            });
        });
    }

    fn file_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.file_dialog.as_mut() else { return };
        let title = match dialog.action {
            FileAction::Open => "Ouvrir une pièce",
            FileAction::SaveAs => "Enregistrer sous",
            FileAction::ExportStep => "Exporter en STEP",
            FileAction::ExportStl => "Exporter en STL",
            FileAction::InsertComponent => "Insérer des composants",
        };
        let mut run = None;
        let mut close = false;
        egui::Window::new(title).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label("Chemin du fichier :");
            let r = ui.add(egui::TextEdit::singleline(&mut dialog.path).desired_width(420.0));
            if dialog.fresh {
                dialog.fresh = false;
                r.request_focus();
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                run = Some((dialog.action, PathBuf::from(&dialog.path)));
            }
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    run = Some((dialog.action, PathBuf::from(&dialog.path)));
                }
                if ui.button("Annuler").clicked() {
                    close = true;
                }
            });
        });
        if let Some((action, path)) = run {
            self.file_dialog = None;
            self.run_file_action(action, path);
        } else if close {
            self.file_dialog = None;
        }
    }

    // ── Graphics area ────────────────────────────────────────────

    fn graphics_area(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let aspect = (rect.width() / rect.height().max(1.0)) as f64;

        if self.fit_pending {
            self.fit_pending = false;
            let bounds = self.display_mesh().and_then(|m| m.bounds());
            let (lo, hi) = bounds.unwrap_or((DVec3::splat(-60.0), DVec3::splat(60.0)));
            self.camera.fit(lo, hi, aspect);
        }

        let to_ndc = |p: Pos2| DVec2::new(
            ((p.x - rect.left()) / rect.width() * 2.0 - 1.0) as f64,
            (1.0 - (p.y - rect.top()) / rect.height() * 2.0) as f64,
        );
        let camera = self.camera;
        let to_screen = move |w: DVec3| {
            let n = camera.project(w, aspect);
            Pos2::new(
                rect.left() + ((n.x + 1.0) / 2.0) as f32 * rect.width(),
                rect.top() + ((1.0 - n.y) / 2.0) as f32 * rect.height(),
            )
        };

        // Camera: middle/right drag rotates, Ctrl+middle or Shift+right pans
        let (mods, scroll) = ui.input(|i| (i.modifiers, i.raw_scroll_delta.y));
        let delta = response.drag_delta();
        let d = DVec2::new(delta.x as f64, delta.y as f64);
        if response.dragged_by(egui::PointerButton::Middle) || response.dragged_by(egui::PointerButton::Secondary) {
            if mods.ctrl || mods.shift {
                self.camera.pan(d, rect.height() as f64);
            } else {
                self.camera.orbit(d);
            }
        }
        if response.hovered() && scroll != 0.0 {
            if let Some(p) = response.hover_pos() {
                let factor = (-(scroll as f64) * 0.0015).exp();
                self.camera.zoom(factor, to_ndc(p), aspect);
            }
        }

        // Render the 3D scene
        let radius = self.display_mesh().and_then(|m| m.bounds()).map(|(lo, hi)| (hi - lo).length()).unwrap_or(200.0);
        let asm_highlight = self.asm().map(|a| (a.highlighted_faces(), a.hover_face, a.highlighted_component()));
        if let (Some(vp), Some(rs)) = (self.viewport.as_mut(), frame.wgpu_render_state()) {
            if self.mesh_dirty {
                let mesh = if self.in_assembly { self.assembly.as_ref().and_then(|a| a.scene.as_ref()) } else { self.mesh.as_ref() };
                vp.set_mesh(rs, mesh);
                self.mesh_dirty = false;
            }
            let ppp = ui.ctx().pixels_per_point();
            let size = [(rect.width() * ppp) as u32, (rect.height() * ppp) as u32];
            let light = (self.camera.back() * 1.0 + self.camera.up() * 0.35 - self.camera.right() * 0.25).normalize();
            let mut params = FrameParams {
                view_proj: self.camera.view_proj(aspect, radius + (self.camera.target.length())),
                light_dir: light.as_vec3(),
                selected_face: match self.selection {
                    Selection::Face { face, .. } => Some(face),
                    _ => None,
                },
                hover_face: self.hover_face,
                selected_face2: None,
                selected_component: None,
                tint_components: false,
            };
            if let Some((faces, hover, component)) = asm_highlight {
                params.selected_face = faces.first().copied();
                params.selected_face2 = faces.get(1).copied();
                params.hover_face = hover;
                params.selected_component = component;
                params.tint_components = true;
            }
            let tex = vp.paint(rs, size, &params);
            ui.painter().image(tex, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        } else {
            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(200, 205, 215));
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Rendu wgpu indisponible", egui::FontId::proportional(16.0), Color32::BLACK);
        }

        let painter = ui.painter_at(rect);
        if !self.in_assembly {
            self.draw_reference_planes(&painter, &to_screen);
        }
        self.draw_triad(&painter, rect);

        if self.in_assembly {
            self.asm_interaction(ui, &response, rect, aspect, &to_ndc, &to_screen);
        } else if self.sketcher.is_some() {
            self.sketch_interaction(ui, &response, &painter, rect, aspect, &to_screen, &to_ndc);
        } else {
            let shift = ui.input(|i| i.modifiers.shift || i.modifiers.ctrl);
            self.part_interaction(&response, aspect, &to_ndc, &to_screen, shift);
            self.draw_edge_selection(&painter, &to_screen);
        }
    }

    fn draw_reference_planes(&self, painter: &egui::Painter, to_screen: &dyn Fn(DVec3) -> Pos2) {
        // User planes are always shown, like SolidWorks' visible planes
        for f in self.doc.tree.features() {
            let (FeatureKind::RefPlane(_), Some(plane)) = (&f.kind, self.doc.tree.feature_plane(f.id)) else { continue };
            let selected = self.selection == Selection::Feature(f.id);
            let s = 40.0;
            let corners: Vec<Pos2> = [(-s, -s), (s, -s), (s, s), (-s, s)]
                .iter()
                .map(|&(u, v)| to_screen(plane.to_world(DVec2::new(u, v))))
                .collect();
            let color = if selected { SELECTED } else { Color32::from_rgb(120, 90, 160) };
            painter.add(egui::Shape::convex_polygon(corners.clone(), color.gamma_multiply(if selected { 0.18 } else { 0.05 }), Stroke::new(1.0_f32, color)));
            painter.text(corners[3], egui::Align2::LEFT_BOTTOM, &f.name, egui::FontId::proportional(12.0), color);
        }
        let show_all = self.doc.tree.body().is_none() && self.sketcher.is_none();
        for p in [StdPlane::Front, StdPlane::Top, StdPlane::Right] {
            let selected = self.selection == Selection::Plane(p);
            if !(show_all || selected) {
                continue;
            }
            let plane = p.plane();
            let s = 50.0;
            let corners: Vec<Pos2> = [(-s, -s), (s, -s), (s, s), (-s, s)]
                .iter()
                .map(|&(u, v)| to_screen(plane.to_world(DVec2::new(u, v))))
                .collect();
            let color = if selected { SELECTED } else { Color32::from_rgb(90, 110, 160) };
            painter.add(egui::Shape::convex_polygon(
                corners.clone(),
                color.gamma_multiply(if selected { 0.18 } else { 0.06 }),
                Stroke::new(1.0_f32, color),
            ));
            painter.text(corners[3], egui::Align2::LEFT_BOTTOM, p.label(), egui::FontId::proportional(12.0), color);
        }
    }

    fn draw_triad(&self, painter: &egui::Painter, rect: Rect) {
        let origin = rect.left_bottom() + Vec2::new(40.0, -40.0);
        for (axis, color, label) in [
            (DVec3::X, Color32::from_rgb(210, 40, 40), "X"),
            (DVec3::Y, Color32::from_rgb(40, 160, 40), "Y"),
            (DVec3::Z, Color32::from_rgb(40, 70, 220), "Z"),
        ] {
            let v = Vec2::new(axis.dot(self.camera.right()) as f32, -axis.dot(self.camera.up()) as f32) * 28.0;
            painter.line_segment([origin, origin + v], Stroke::new(2.0_f32, color));
            painter.text(origin + v * 1.2, egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.0), color);
        }
    }

    /// What the viewport shows: the part, or the assembly scene
    fn display_mesh(&self) -> Option<&RenderMesh> {
        if self.in_assembly {
            self.assembly.as_ref().and_then(|a| a.scene.as_ref())
        } else {
            self.mesh.as_ref()
        }
    }

    /// Face under the cursor, with its plane when the face is planar
    fn pick_face(&self, ndc: DVec2, aspect: f64) -> Option<(u32, Option<Plane>)> {
        let mesh = self.mesh.as_ref()?;
        let ray = self.camera.ray(ndc, aspect);
        let p = |i: u32| DVec3::from(mesh.positions[i as usize].map(f64::from));
        let mut best: Option<(f64, usize)> = None;
        for (t, tri) in mesh.indices.chunks_exact(3).enumerate() {
            if let Some(d) = ray.hit_triangle(p(tri[0]), p(tri[1]), p(tri[2])) {
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, t));
                }
            }
        }
        let (_, t) = best?;
        let face = mesh.triangle_faces[t];
        let tri_normal = |k: usize| {
            let tri = &mesh.indices[k * 3..k * 3 + 3];
            (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0])).normalize_or_zero()
        };
        let n = tri_normal(t);
        let planar = mesh
            .triangle_faces
            .iter()
            .enumerate()
            .filter(|(_, f)| **f == face)
            .all(|(k, _)| tri_normal(k).dot(n) > 1.0 - 1e-6);
        if !planar {
            return Some((face, None));
        }
        // Origin: the world origin projected onto the face's plane
        let origin = n * p(mesh.indices[t * 3]).dot(n);
        let x_axis = if n.z.abs() > 0.9 || n.y.abs() > 0.9 { DVec3::X } else { DVec3::NEG_Z * n.x.signum() };
        Some((face, Some(Plane::new(origin, x_axis, n))))
    }

    /// Model edge under the cursor (screen distance to its polyline)
    fn pick_edge(&self, at: Pos2, to_screen: &dyn Fn(DVec3) -> Pos2) -> Option<u32> {
        let mesh = self.mesh.as_ref()?;
        let mut best: Option<(f32, u32)> = None;
        for (i, poly) in mesh.edges.iter().enumerate() {
            let pts: Vec<Pos2> = poly.iter().map(|p| to_screen(DVec3::from(p.map(f64::from)))).collect();
            for w in pts.windows(2) {
                let d = dist_to_segment(at, w[0], w[1]);
                if d < PICK_PX && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, i as u32));
                }
            }
        }
        best.map(|(_, i)| i)
    }

    fn draw_edge_selection(&self, painter: &egui::Painter, to_screen: &dyn Fn(DVec3) -> Pos2) {
        let Some(mesh) = self.mesh.as_ref() else { return };
        let draw = |i: u32, color: Color32| {
            if let Some(poly) = mesh.edges.get(i as usize) {
                let pts: Vec<Pos2> = poly.iter().map(|p| to_screen(DVec3::from(p.map(f64::from)))).collect();
                painter.add(egui::Shape::line(pts, Stroke::new(3.0_f32, color)));
            }
        };
        if let Selection::Edges(edges) = &self.selection {
            for &e in edges {
                draw(e, SELECTED);
            }
        }
        if let Some(e) = self.hover_edge {
            draw(e, PREVIEW);
        }
    }

    /// Fillet (true) or chamfer of the selected edges
    fn start_edge_feature(&mut self, fillet: bool) {
        let (Selection::Edges(edges), Some(body)) = (&self.selection, self.doc.tree.body()) else {
            self.error("Sélectionnez des arêtes dans la vue (Maj+clic pour en ajouter), puis cliquez Congé ou Chanfrein.");
            return;
        };
        let infos = body.edges();
        let refs: Vec<EdgeRef> = edges.iter().filter_map(|&i| infos.get(i as usize)).map(|e| EdgeRef { midpoint: e.mid }).collect();
        let size = (body.size() * 0.02).clamp(0.5, 5.0).round().max(1.0);
        let feature = EdgeFeature { edges: refs, size };
        self.start_feature(if fillet { FeatureKind::Fillet(feature) } else { FeatureKind::Chamfer(feature) });
    }

    fn part_interaction(
        &mut self,
        response: &egui::Response,
        aspect: f64,
        to_ndc: &dyn Fn(Pos2) -> DVec2,
        to_screen: &dyn Fn(DVec3) -> Pos2,
        additive: bool,
    ) {
        self.cursor_sketch = None;
        self.hover_edge = response.hover_pos().and_then(|p| self.pick_edge(p, to_screen));
        self.hover_face = if self.hover_edge.is_some() {
            None
        } else {
            response.hover_pos().and_then(|p| self.pick_face(to_ndc(p), aspect)).map(|(f, _)| f)
        };
        if response.clicked_by(egui::PointerButton::Primary) {
            if let Some(edge) = response.interact_pointer_pos().and_then(|p| self.pick_edge(p, to_screen)) {
                let mut edges = match (&self.selection, additive) {
                    (Selection::Edges(e), true) => e.clone(),
                    _ => Vec::new(),
                };
                if let Some(i) = edges.iter().position(|&e| e == edge) {
                    edges.remove(i);
                } else {
                    edges.push(edge);
                }
                self.selection = if edges.is_empty() { Selection::None } else { Selection::Edges(edges) };
                return;
            }
            if let Some(p) = response.interact_pointer_pos() {
                self.selection = match self.pick_face(to_ndc(p), aspect) {
                    Some((face, Some(plane))) => Selection::Face { face, plane },
                    Some((_, None)) => {
                        self.info("Face non plane : une esquisse demande une face plane.");
                        Selection::None
                    }
                    None => Selection::None,
                };
            }
        }
        if response.double_clicked() {
            if let Selection::Face { .. } = self.selection {
                self.start_new_sketch();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sketch_interaction(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        rect: Rect,
        aspect: f64,
        to_screen: &dyn Fn(DVec3) -> Pos2,
        to_ndc: &dyn Fn(Pos2) -> DVec2,
    ) {
        self.hover_face = None;
        let camera = self.camera;
        let sk = self.sketcher.as_mut().unwrap();
        let plane = sk.sketch.plane;
        let s2 = |p: DVec2| to_screen(plane.to_world(p));
        let to_sketch = |p: Pos2| camera.ray(to_ndc(p), aspect).hit_plane(&plane);
        let px_per_unit = rect.height() as f64 / (2.0 * camera.half_height);

        let hover = response.hover_pos();
        let cursor = hover.and_then(to_sketch);
        self.cursor_sketch = cursor;

        // Hit testing in screen space
        let mut snap_point: Option<Id> = None;
        let mut hit: Option<SelItem> = None;
        if let Some(h) = hover {
            let mut best = SNAP_PX;
            for e in sk.sketch.elements.values() {
                if let SketchElement::Point(p) = e {
                    if sk.stage.contains(&p.id) && sk.tool != Tool::Line && sk.tool != Tool::SmartDimension {
                        continue;
                    }
                    let d = s2(DVec2::new(p.x, p.y)).distance(h);
                    if d < best {
                        best = d;
                        snap_point = Some(p.id);
                    }
                }
            }
            hit = snap_point.map(SelItem::Point);
            if hit.is_none() {
                let mut best = PICK_PX;
                for e in sk.sketch.elements.values() {
                    let d = match e {
                        SketchElement::Line(l) => match (sk.point(l.p1), sk.point(l.p2)) {
                            (Some(a), Some(b)) => dist_to_segment(h, s2(a), s2(b)),
                            _ => f32::MAX,
                        },
                        SketchElement::Circle(c) => sk
                            .point(c.center)
                            .map(|ctr| ((s2(ctr).distance(h)) - (c.radius * px_per_unit) as f32).abs())
                            .unwrap_or(f32::MAX),
                        SketchElement::Arc(a) => sk
                            .arc_geom(a.center, a.start, a.end)
                            .map(|g| {
                                let n = 48;
                                (0..n)
                                    .map(|k| dist_to_segment(h, s2(g.point_at(k as f64 / n as f64)), s2(g.point_at((k + 1) as f64 / n as f64))))
                                    .fold(f32::MAX, f32::min)
                            })
                            .unwrap_or(f32::MAX),
                        SketchElement::Point(_) => f32::MAX,
                    };
                    if d < best {
                        best = d;
                        hit = Some(SelItem::Entity(e.id()));
                    }
                }
            }
        }

        // Dimension labels (also clickable)
        let mut dim_rects: Vec<(usize, Rect)> = Vec::new();
        for (i, c) in sk.sketch.constraints.iter().enumerate() {
            let Some(v) = sketcher::dimension_value(&c.constraint_type) else { continue };
            let anchor = match c.constraint_type {
                ConstraintType::Distance { p1, p2, .. } => sk.point(p1).zip(sk.point(p2)).map(|(a, b)| (a + b) / 2.0),
                ConstraintType::Length { line_id, .. } => match sk.sketch.elements.get(&line_id) {
                    Some(SketchElement::Line(l)) => sk.point(l.p1).zip(sk.point(l.p2)).map(|(a, b)| (a + b) / 2.0),
                    _ => None,
                },
                ConstraintType::Radius { circle_id, .. } => match sk.sketch.elements.get(&circle_id) {
                    Some(SketchElement::Circle(ci)) => sk.point(ci.center).map(|ctr| ctr + DVec2::new(ci.radius, 0.0) * 0.7),
                    Some(SketchElement::Arc(a)) => sk.arc_geom(a.center, a.start, a.end).map(|g| g.point_at(0.5)),
                    _ => None,
                },
                _ => None,
            };
            let Some(anchor) = anchor else { continue };
            let text = match c.constraint_type {
                ConstraintType::Radius { .. } => format!("R{:.2}", v),
                _ => format!("{:.2}", v),
            };
            let pos = s2(anchor) + Vec2::new(0.0, -14.0);
            let color = if c.is_satisfied { Color32::from_rgb(20, 20, 20) } else { CONFLICT };
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(13.0), color);
            let r = Rect::from_center_size(pos, galley.size() + Vec2::splat(4.0));
            let selected = sk.selection.contains(&SelItem::Constraint(i));
            painter.rect_filled(r, 2.0, if selected { SELECTED.gamma_multiply(0.35) } else { Color32::from_white_alpha(200) });
            painter.galley(r.min + Vec2::splat(2.0), galley, color);
            dim_rects.push((i, r));
        }
        if let Some(h) = hover {
            if let Some((i, _)) = dim_rects.iter().find(|(_, r)| r.contains(h)) {
                hit = Some(SelItem::Constraint(*i));
                snap_point = None;
            }
        }

        // Draw sketch geometry
        let inconsistent = matches!(sk.report.map(|r| r.status), Some(SolveStatus::Inconsistent));
        let color_of = |sk: &Sketcher, id: Id| -> Color32 {
            if sk.selection.iter().any(|s| matches!(s, SelItem::Point(x) | SelItem::Entity(x) if *x == id)) {
                SELECTED
            } else if hit.is_some_and(|h| matches!(h, SelItem::Point(x) | SelItem::Entity(x) if x == id)) {
                PREVIEW
            } else if inconsistent {
                CONFLICT
            } else if sk.sketch.is_defined(id) {
                DEFINED
            } else {
                UNDER_DEFINED
            }
        };
        for e in sk.sketch.elements.values() {
            let color = color_of(sk, e.id());
            let stroke = Stroke::new(1.6_f32, color);
            match e {
                SketchElement::Line(l) => {
                    if let (Some(a), Some(b)) = (sk.point(l.p1), sk.point(l.p2)) {
                        if l.construction {
                            painter.extend(egui::Shape::dashed_line(&[s2(a), s2(b)], Stroke::new(1.2_f32, color), 10.0, 4.0));
                        } else {
                            painter.line_segment([s2(a), s2(b)], stroke);
                        }
                    }
                }
                SketchElement::Circle(c) => {
                    if let Some(ctr) = sk.point(c.center) {
                        let pts: Vec<Pos2> = (0..=64)
                            .map(|k| {
                                let t = k as f64 / 64.0 * std::f64::consts::TAU;
                                s2(ctr + DVec2::new(t.cos(), t.sin()) * c.radius)
                            })
                            .collect();
                        painter.add(egui::Shape::line(pts, stroke));
                    }
                }
                SketchElement::Arc(a) => {
                    if let Some(g) = sk.arc_geom(a.center, a.start, a.end) {
                        let pts: Vec<Pos2> = (0..=48).map(|k| s2(g.point_at(k as f64 / 48.0))).collect();
                        painter.add(egui::Shape::line(pts, stroke));
                    }
                }
                SketchElement::Point(_) => {}
            }
        }
        for e in sk.sketch.elements.values() {
            if let SketchElement::Point(p) = e {
                let c = color_of(sk, p.id);
                painter.circle_filled(s2(DVec2::new(p.x, p.y)), 2.6, c);
            }
        }
        // Sketch origin
        let o = s2(DVec2::ZERO);
        painter.line_segment([o, o + Vec2::new(18.0, 0.0)], Stroke::new(1.2_f32, CONFLICT));
        painter.line_segment([o, o + Vec2::new(0.0, -18.0)], Stroke::new(1.2_f32, CONFLICT));

        // Tool preview
        if let Some(c) = cursor {
            let c = snap_point.and_then(|id| sk.point(id)).unwrap_or(c);
            for shape in sk.preview(c) {
                let stroke = Stroke::new(1.2_f32, PREVIEW);
                match shape {
                    Preview::Line(a, b) => {
                        painter.line_segment([s2(a), s2(b)], stroke);
                    }
                    Preview::Circle(ctr, r) => {
                        let pts: Vec<Pos2> = (0..=64)
                            .map(|k| {
                                let t = k as f64 / 64.0 * std::f64::consts::TAU;
                                s2(ctr + DVec2::new(t.cos(), t.sin()) * r)
                            })
                            .collect();
                        painter.add(egui::Shape::line(pts, stroke));
                    }
                    Preview::Arc { center, start, end } => {
                        let r = (start - center).length();
                        let a0 = (start - center).y.atan2((start - center).x);
                        let a1 = (end - center).y.atan2((end - center).x);
                        let sweep = (a1 - a0).rem_euclid(std::f64::consts::TAU);
                        let pts: Vec<Pos2> = (0..=48)
                            .map(|k| {
                                let t = a0 + sweep * k as f64 / 48.0;
                                s2(center + DVec2::new(t.cos(), t.sin()) * r)
                            })
                            .collect();
                        painter.add(egui::Shape::line(pts, stroke));
                    }
                }
            }
            if snap_point.is_some() && sk.tool != Tool::Select {
                painter.circle_stroke(s2(c), 5.0, Stroke::new(1.5_f32, PREVIEW));
            }
        }

        // Mouse actions
        let additive = ui.input(|i| i.modifiers.shift || i.modifiers.ctrl);
        if response.drag_started_by(egui::PointerButton::Primary) && sk.tool == Tool::Select {
            if let Some(SelItem::Point(id)) = hit {
                self.dragging_point = Some(id);
            }
        }
        if let Some(id) = self.dragging_point {
            if response.dragged_by(egui::PointerButton::Primary) {
                if let Some(c) = cursor {
                    sk.drag_point(id, c);
                }
            } else {
                self.dragging_point = None;
            }
        }
        if response.clicked_by(egui::PointerButton::Primary) {
            if let Some(c) = cursor {
                let c = snap_point.and_then(|id| sk.point(id)).unwrap_or(c);
                if let Some(index) = sk.click(c, snap_point, hit, additive) {
                    let at = hover.unwrap_or(rect.center());
                    let value = sk.sketch.constraints.get(index).and_then(|c| sketcher::dimension_value(&c.constraint_type)).unwrap_or(0.0);
                    self.dim_edit = Some(DimEdit { index, text: format!("{:.2}", value), at });
                }
            }
        }
        if response.double_clicked() {
            if let Some(SelItem::Constraint(i)) = hit {
                if let Some(v) = sk.sketch.constraints.get(i).and_then(|c| sketcher::dimension_value(&c.constraint_type)) {
                    self.dim_edit = Some(DimEdit { index: i, text: format!("{:.2}", v), at: hover.unwrap_or(rect.center()) });
                }
            } else if sk.tool == Tool::Line {
                sk.cancel();
            }
        }
        self.dim_edit_window(ui.ctx());
    }

    /// SolidWorks' "Modify" box for a dimension value
    fn dim_edit_window(&mut self, ctx: &egui::Context) {
        let Some(edit) = self.dim_edit.as_mut() else { return };
        let mut done = None;
        egui::Area::new(egui::Id::new("modify-dimension"))
            .fixed_pos(edit.at + Vec2::new(12.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label("Modifier");
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut edit.text).desired_width(90.0));
                        r.request_focus();
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.button("✔").clicked() || enter {
                            done = Some(true);
                        }
                        if ui.button("✖").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            done = Some(false);
                        }
                    });
                });
            });
        if let Some(apply) = done {
            let edit = self.dim_edit.take().unwrap();
            if apply {
                match edit.text.replace(',', ".").trim().parse::<f64>() {
                    Ok(v) if v > 0.0 => {
                        if let Some(sk) = self.sketcher.as_mut() {
                            sk.set_dimension(edit.index, v);
                        }
                    }
                    _ => self.error("Valeur de cote invalide"),
                }
            }
        }
    }
}

/// Ray parity test: is `p` inside the closed mesh?
fn point_in_mesh(mesh: &RenderMesh, p: DVec3) -> bool {
    // Slightly skewed direction avoids hitting edges exactly
    let ray = crate::camera::Ray { origin: p, dir: DVec3::new(0.5773, 0.5774, 0.5775).normalize() };
    let v = |i: u32| DVec3::from(mesh.positions[i as usize].map(f64::from));
    mesh.indices
        .chunks_exact(3)
        .filter(|t| ray.hit_triangle(v(t[0]), v(t[1]), v(t[2])).is_some())
        .count()
        % 2
        == 1
}

fn feature_icon(kind: &FeatureKind) -> &'static str {
    // Only glyphs present in egui's bundled fonts
    match kind {
        FeatureKind::Sketch(_) => "✏",
        FeatureKind::Extrude(_) => "⬛",
        FeatureKind::Revolve(_) => "🔁",
        FeatureKind::Hole(_) => "○",
        FeatureKind::RefPlane(_) => "■",
        FeatureKind::Pattern(p) => match p.kind {
            PatternKind::Linear { .. } => "📏",
            PatternKind::Circular { .. } => "⚙",
            PatternKind::Mirror { .. } => "◑",
        },
        FeatureKind::Fillet(_) => "⭕",
        FeatureKind::Chamfer(_) => "✂",
    }
}

/// Blind / through all / mid plane selector with its depth
fn end_condition_ui(ui: &mut egui::Ui, id: &str, end: &mut EndCondition, allow_mid: bool) {
    let mut kind = match *end {
        EndCondition::Blind { .. } => 0,
        EndCondition::ThroughAll => 1,
        EndCondition::MidPlane { .. } => 2,
    };
    let mut d = match *end {
        EndCondition::Blind { depth } | EndCondition::MidPlane { depth } => depth,
        EndCondition::ThroughAll => 10.0,
    };
    let labels = ["Borgne", "À travers tout", "Plan milieu"];
    egui::ComboBox::from_id_source(id).selected_text(labels[kind]).show_ui(ui, |ui| {
        for (k, l) in labels.iter().enumerate().take(if allow_mid { 3 } else { 2 }) {
            ui.selectable_value(&mut kind, k, *l);
        }
    });
    if kind != 1 {
        ui.horizontal(|ui| {
            ui.label("Profondeur");
            ui.add(egui::DragValue::new(&mut d).speed(0.5).range(0.01..=1e5).suffix(" mm"));
        });
    }
    *end = match kind {
        0 => EndCondition::Blind { depth: d },
        1 => EndCondition::ThroughAll,
        _ => EndCondition::MidPlane { depth: d },
    };
}

fn axis_ui(ui: &mut egui::Ui, id: &str, axis: &mut Axis) {
    ui.horizontal(|ui| {
        ui.label("Axe");
        egui::ComboBox::from_id_source(id).selected_text(format!("{:?}", axis)).show_ui(ui, |ui| {
            for a in [Axis::X, Axis::Y, Axis::Z] {
                ui.selectable_value(axis, a, format!("{:?}", a));
            }
        });
    });
}

fn plane_base_ui(ui: &mut egui::Ui, id: &str, base: &mut PlaneBase, ref_planes: &[(FeatureId, String)]) {
    let label = |b: &PlaneBase| match b {
        PlaneBase::Front => "Plan de face".to_string(),
        PlaneBase::Top => "Plan de dessus".to_string(),
        PlaneBase::Right => "Plan de droite".to_string(),
        PlaneBase::Fixed(_) => "Face sélectionnée".to_string(),
        PlaneBase::Feature(fid) => ref_planes.iter().find(|(i, _)| i == fid).map(|(_, n)| n.clone()).unwrap_or_else(|| "?".into()),
    };
    egui::ComboBox::from_id_source(id).selected_text(label(base)).show_ui(ui, |ui| {
        let mut options = vec![PlaneBase::Front, PlaneBase::Top, PlaneBase::Right];
        if matches!(base, PlaneBase::Fixed(_)) {
            options.push(*base);
        }
        options.extend(ref_planes.iter().map(|(i, _)| PlaneBase::Feature(*i)));
        for o in options {
            let text = label(&o);
            ui.selectable_value(base, o, text);
        }
    });
}

/// A FeatureManager command, applied after the tree has been drawn
type TreeAction = Box<dyn FnOnce(&mut NovarisApp)>;

fn big_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized([118.0, 44.0], egui::Button::new(text))
}

fn rollback_bar(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    ui.painter().rect_filled(r.shrink2(Vec2::new(0.0, 1.5)), 1.0, Color32::from_rgb(30, 90, 200));
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

impl eframe::App for NovarisApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.startup_frames > 0 {
            self.startup_frames -= 1;
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| ui.heading(format!("Reconstruction de « {} » en cours…", self.doc.name)));
            });
            if self.startup_frames == 0 {
                self.rebuild();
                self.fit_pending = true;
            }
            ctx.request_repaint();
            return;
        }
        self.shortcuts(ctx);
        if self.in_assembly {
            egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu_bar(ui));
            egui::TopBottomPanel::top("commands").show(ctx, |ui| self.asm_command_manager(ui));
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.status_bar(ui));
            egui::SidePanel::left("tree").default_width(260.0).min_width(240.0).resizable(true).show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.asm_has_editor() {
                        self.asm_mate_property_manager(ui);
                    } else {
                        self.asm_feature_manager(ui);
                    }
                });
            });
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| self.graphics_area(ui, frame));
            self.file_dialog_window(ctx);
            self.asm_windows(ctx);
            return;
        }
        let in_sketch = self.sketcher.is_some();
        egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu_bar(ui));
        egui::TopBottomPanel::top("commands").show(ctx, |ui| self.command_manager(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.status_bar(ui));
        egui::SidePanel::left("tree").default_width(260.0).min_width(240.0).resizable(true).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if in_sketch {
                    self.sketch_property_manager(ui);
                } else if self.feature_editor.is_some() {
                    self.feature_property_manager(ui);
                } else {
                    self.feature_manager(ui);
                }
            });
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| self.graphics_area(ui, frame));
        self.file_dialog_window(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::point_in_mesh;
    use crate::sketcher::Relation;
    use glam::{DVec2, DVec3};
    use nova_kernel::{Loop, Plane, Prism, Profile, Solid};

    #[test]
    fn relation_labels_are_french() {
        assert_eq!(Relation::Perpendicular.label(), "Perpendiculaire");
    }

    #[test]
    fn point_in_mesh_parity() {
        let s = Solid::prism(&Prism::new(
            Profile::simple(Loop::rectangle(DVec2::ZERO, DVec2::new(10.0, 10.0))),
            Plane::front(),
            0.0,
            10.0,
        ))
        .unwrap();
        let m = s.tessellate(0.1);
        assert!(point_in_mesh(&m, DVec3::new(5.0, 5.0, 9.99)));
        assert!(!point_in_mesh(&m, DVec3::new(5.0, 5.0, 10.01)));
        assert!(!point_in_mesh(&m, DVec3::new(-1.0, 5.0, 5.0)));
    }
}
