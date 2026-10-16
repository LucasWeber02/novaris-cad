//! Assembly mode: components placed by mates, interference detection and
//! bill of materials, laid out like SolidWorks' assembly environment.

use super::*;
use glam::{DMat4, DQuat};
use nova_assembly::{AssemblyDocument, ComponentState, FaceKind, FaceRef, Interference, MateKind, MateState};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum MateChoice {
    Coincident,
    Parallel,
    Perpendicular,
    Distance,
    Angle,
    Concentric,
}

impl MateChoice {
    const ALL: [MateChoice; 6] = [
        MateChoice::Coincident,
        MateChoice::Parallel,
        MateChoice::Perpendicular,
        MateChoice::Distance,
        MateChoice::Angle,
        MateChoice::Concentric,
    ];

    fn label(&self) -> &'static str {
        match self {
            MateChoice::Coincident => "Coïncidente",
            MateChoice::Parallel => "Parallèle",
            MateChoice::Perpendicular => "Perpendiculaire",
            MateChoice::Distance => "Distance",
            MateChoice::Angle => "Angle",
            MateChoice::Concentric => "Coaxiale",
        }
    }
}

/// Mate PropertyManager state
pub(super) struct MateEditor {
    choice: MateChoice,
    distance: f64,
    angle: f64,
    aligned: bool,
    faces: Vec<FaceRef>,
    /// Mate being edited, or None for new mates
    editing: Option<Id>,
}

impl MateEditor {
    fn kind(&self) -> MateKind {
        match self.choice {
            MateChoice::Coincident => MateKind::Coincident,
            MateChoice::Parallel => MateKind::Parallel,
            MateChoice::Perpendicular => MateKind::Perpendicular,
            MateChoice::Distance => MateKind::Distance { value: self.distance },
            MateChoice::Angle => MateKind::Angle { degrees: self.angle },
            MateChoice::Concentric => MateKind::Concentric,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum AsmSelection {
    None,
    Component(Id),
    Mate(Id),
    Faces(Vec<FaceRef>),
}

/// An open assembly and its view state
pub(super) struct AssemblyView {
    pub doc: AssemblyDocument,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<AssemblyDocument>,
    redo: Vec<AssemblyDocument>,
    /// Tessellation of each part file, in part coordinates
    part_meshes: HashMap<PathBuf, RenderMesh>,
    /// All components placed, face ids = component index << 16 | face
    pub scene: Option<RenderMesh>,
    order: Vec<Id>,
    pub selection: AsmSelection,
    mate_editor: Option<MateEditor>,
    interferences: Option<Vec<Interference>>,
    interference_edges: Vec<Vec<[f32; 3]>>,
    show_bom: bool,
    drag: Option<(Id, bool)>,
    pub hover_face: Option<u32>,
}

impl AssemblyView {
    pub fn new(doc: AssemblyDocument, path: Option<PathBuf>) -> Self {
        let mut view = AssemblyView {
            doc,
            path,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            part_meshes: HashMap::new(),
            scene: None,
            order: Vec::new(),
            selection: AsmSelection::None,
            mate_editor: None,
            interferences: None,
            interference_edges: Vec::new(),
            show_bom: false,
            drag: None,
            hover_face: None,
        };
        view.rebuild_scene();
        view
    }

    /// Merge the placed component meshes into one scene
    pub fn rebuild_scene(&mut self) {
        let mut scene = RenderMesh::default();
        self.order = self.doc.components.iter().map(|c| c.id).collect();
        for (k, c) in self.doc.components.iter().enumerate() {
            if c.suppressed {
                continue;
            }
            let path = self.doc.part_file(c);
            let Some(solid) = self.doc.part(c.id).and_then(|p| p.solid.as_ref()) else { continue };
            let mesh = self
                .part_meshes
                .entry(path)
                .or_insert_with(|| solid.tessellate((solid.size() * 1e-3).max(0.01)));
            let m = c.placement.matrix();
            let point = |p: [f32; 3]| m.transform_point3(DVec3::from(p.map(f64::from))).as_vec3().to_array();
            let base = scene.positions.len() as u32;
            scene.positions.extend(mesh.positions.iter().map(|&p| point(p)));
            scene.normals.extend(
                mesh.normals.iter().map(|&n| m.transform_vector3(DVec3::from(n.map(f64::from))).as_vec3().to_array()),
            );
            scene.indices.extend(mesh.indices.iter().map(|i| i + base));
            scene.triangle_faces.extend(mesh.triangle_faces.iter().map(|f| ((k as u32) << 16) | f));
            scene.edges.extend(mesh.edges.iter().map(|e| e.iter().map(|&p| point(p)).collect()));
        }
        self.scene = (!scene.indices.is_empty()).then_some(scene);
    }

    /// The assembly changed: mark dirty, drop stale results, redraw
    fn changed(&mut self) {
        self.dirty = true;
        self.interferences = None;
        self.interference_edges.clear();
        self.rebuild_scene();
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.doc.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(d) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, d));
            self.selection = AsmSelection::None;
            self.changed();
        }
    }

    pub fn redo(&mut self) {
        if let Some(d) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, d));
            self.selection = AsmSelection::None;
            self.changed();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Re-read every part file (after one was edited)
    pub fn reload(&mut self) {
        self.part_meshes.clear();
        self.doc.reload_parts();
        self.changed();
    }

    fn component_index(&self, id: Id) -> Option<usize> {
        self.order.iter().position(|c| *c == id)
    }

    fn decode(&self, global: u32) -> Option<(Id, usize)> {
        let comp = *self.order.get((global >> 16) as usize)?;
        Some((comp, (global & 0xffff) as usize))
    }

    fn global_face(&self, r: &FaceRef) -> Option<u32> {
        let k = self.component_index(r.component)? as u32;
        Some((k << 16) | self.doc.resolve(r)? as u32)
    }

    fn component_name(&self, id: Id) -> String {
        self.doc.component(id).map(|c| c.name.clone()).unwrap_or_else(|| "?".into())
    }

    fn face_label(&self, r: &FaceRef) -> String {
        let kind = match r.kind {
            FaceKind::Plane => "Face plane",
            FaceKind::Cylinder => "Face cylindrique",
        };
        format!("{kind} @ {}", self.component_name(r.component))
    }

    /// Faces highlighted in the view
    pub fn highlighted_faces(&self) -> Vec<u32> {
        let refs: &[FaceRef] = match (&self.mate_editor, &self.selection) {
            (Some(ed), _) => &ed.faces,
            (None, AsmSelection::Faces(f)) => f,
            _ => &[],
        };
        refs.iter().filter_map(|r| self.global_face(r)).collect()
    }

    pub fn highlighted_component(&self) -> Option<u32> {
        match self.selection {
            AsmSelection::Component(id) => self.component_index(id).map(|k| k as u32),
            _ => None,
        }
    }
}

/// Triangle of `mesh` hit first by the ray
fn pick_triangle(mesh: &RenderMesh, ray: &crate::camera::Ray) -> Option<usize> {
    let p = |i: u32| DVec3::from(mesh.positions[i as usize].map(f64::from));
    let mut best: Option<(f64, usize)> = None;
    for (t, tri) in mesh.indices.chunks_exact(3).enumerate() {
        if let Some(d) = ray.hit_triangle(p(tri[0]), p(tri[1]), p(tri[2])) {
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, t));
            }
        }
    }
    best.map(|(_, t)| t)
}

impl NovarisApp {
    pub(super) fn asm(&self) -> Option<&AssemblyView> {
        self.assembly.as_ref().filter(|_| self.active == Active::Assembly)
    }

    fn asm_mut(&mut self) -> Option<&mut AssemblyView> {
        self.assembly.as_mut().filter(|_| self.active == Active::Assembly)
    }

    pub(super) fn new_assembly(&mut self) {
        self.assembly = Some(AssemblyView::new(AssemblyDocument::new("Assemblage1"), None));
        self.active = Active::Assembly;
        self.mesh_dirty = true;
        self.fit_pending = true;
        self.info("Insérez une première pièce : elle sera fixe, comme dans SolidWorks.");
        self.open_file_dialog(FileAction::InsertComponent);
    }

    pub(super) fn open_assembly(&mut self, path: &Path) -> Result<String, String> {
        let doc = AssemblyDocument::open(path).map_err(|e| e.to_string())?;
        let missing: Vec<String> = doc
            .components
            .iter()
            .filter(|c| doc.part(c.id).is_none_or(|p| p.error.is_some()))
            .map(|c| c.name.clone())
            .collect();
        self.assembly = Some(AssemblyView::new(doc, Some(path.to_path_buf())));
        self.active = Active::Assembly;
        self.mesh_dirty = true;
        self.fit_pending = true;
        if missing.is_empty() {
            Ok(format!("Ouvert : {}", path.display()))
        } else {
            Err(format!("Pièces introuvables ou en erreur : {}", missing.join(", ")))
        }
    }

    pub(super) fn asm_save_as(&mut self, path: &Path) -> Result<String, String> {
        let a = self.assembly.as_mut().ok_or("aucun assemblage")?;
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            a.doc.name = name.to_string();
        }
        a.doc.save(path).map_err(|e| e.to_string())?;
        a.path = Some(path.to_path_buf());
        a.dirty = false;
        Ok(format!("Enregistré : {}", path.display()))
    }

    pub(super) fn asm_save(&mut self) {
        match self.asm().and_then(|a| a.path.clone()) {
            Some(p) => match self.asm_save_as(&p) {
                Ok(m) => self.info(m),
                Err(e) => self.error(format!("Échec de l'enregistrement : {e}")),
            },
            None => self.open_file_dialog(FileAction::SaveAs),
        }
    }

    pub(super) fn asm_insert(&mut self, path: &Path) -> Result<String, String> {
        let a = self.asm_mut().ok_or("aucun assemblage")?;
        a.checkpoint();
        let bounds = a.doc.bounds();
        let id = match a.doc.insert_component(path, Default::default()) {
            Ok(id) => id,
            Err(e) => {
                a.undo.pop();
                return Err(e.to_string());
            }
        };
        // Drop new components beside what is already there
        if let (Some((_, hi)), Some((plo, _))) =
            (bounds, a.doc.part(id).and_then(|p| p.solid.as_ref()).and_then(|s| s.bounds()))
        {
            let gap = 0.2 * (hi.x - plo.x).abs().max(20.0);
            if let Some(c) = a.doc.component_mut(id) {
                c.placement.translation = DVec3::new(hi.x + gap - plo.x, 0.0, 0.0);
            }
        }
        a.doc.solve();
        a.selection = AsmSelection::Component(id);
        a.changed();
        let name = a.component_name(id);
        self.fit_pending = true;
        self.mesh_dirty = true;
        Ok(format!("{name} inséré"))
    }

    pub(super) fn asm_export_step(&self, path: &Path) -> Result<String, String> {
        let a = self.asm().ok_or("aucun assemblage")?;
        a.doc.export_step(path).map_err(|e| e.to_string())?;
        Ok(format!("Exporté : {}", path.display()))
    }

    /// Open a component's part in part mode; the assembly stays open
    fn open_component_part(&mut self, id: Id) {
        let Some(path) = self.asm().and_then(|a| a.doc.component(id).map(|c| a.doc.part_file(c))) else { return };
        self.active = Active::Part;
        self.run_file_action(FileAction::Open, path);
        self.mesh_dirty = true;
    }

    /// Back from part mode: save the part, re-read the parts, re-solve
    pub(super) fn back_to_assembly(&mut self) {
        if self.dirty {
            if self.path.is_none() {
                self.error("Enregistrez d'abord la pièce.");
                return;
            }
            self.save();
        }
        self.active = Active::Assembly;
        if let Some(a) = self.assembly.as_mut() {
            a.reload();
        }
        self.mesh_dirty = true;
        self.fit_pending = true;
        let failing = self.asm().map(|a| a.doc.summary().unsatisfied).unwrap_or(0);
        if failing > 0 {
            self.error(format!("{failing} contrainte(s) ne sont plus satisfaites après la modification."));
        }
    }

    /// Apply a change to the document with undo, then refresh
    fn asm_edit(&mut self, f: impl FnOnce(&mut AssemblyDocument) -> Result<(), String>) {
        let Some(a) = self.asm_mut() else { return };
        a.checkpoint();
        match f(&mut a.doc) {
            Ok(()) => {
                a.changed();
                self.mesh_dirty = true;
            }
            Err(e) => {
                a.undo.pop();
                self.error(e);
            }
        }
    }

    // ── Commands ─────────────────────────────────────────────────

    fn start_mate(&mut self) {
        let Some(a) = self.asm_mut() else { return };
        let faces = match &a.selection {
            AsmSelection::Faces(f) => f.clone(),
            _ => Vec::new(),
        };
        let mut ed = MateEditor { choice: MateChoice::Coincident, distance: 10.0, angle: 90.0, aligned: false, faces, editing: None };
        suggest(&a.doc, &mut ed);
        a.mate_editor = Some(ed);
        a.selection = AsmSelection::None;
    }

    fn edit_mate(&mut self, id: Id) {
        let Some(a) = self.asm_mut() else { return };
        let Some(m) = a.doc.mate(id).cloned() else { return };
        let mut ed = MateEditor {
            choice: MateChoice::Coincident,
            distance: 10.0,
            angle: 90.0,
            aligned: m.aligned,
            faces: vec![m.a, m.b],
            editing: Some(id),
        };
        ed.choice = match m.kind {
            MateKind::Coincident => MateChoice::Coincident,
            MateKind::Parallel => MateChoice::Parallel,
            MateKind::Perpendicular => MateChoice::Perpendicular,
            MateKind::Distance { value } => {
                ed.distance = value;
                MateChoice::Distance
            }
            MateKind::Angle { degrees } => {
                ed.angle = degrees;
                MateChoice::Angle
            }
            MateKind::Concentric => MateChoice::Concentric,
        };
        a.mate_editor = Some(ed);
    }

    fn apply_mate(&mut self) {
        let Some(a) = self.asm_mut() else { return };
        let Some(ed) = a.mate_editor.as_ref() else { return };
        if ed.faces.len() != 2 {
            self.error("Sélectionnez deux faces dans la vue.");
            return;
        }
        let (kind, aligned, faces, editing) = (ed.kind(), ed.aligned, ed.faces.clone(), ed.editing);
        a.checkpoint();
        let result = match editing {
            Some(id) => a.doc.edit_mate(id, kind, aligned).map(|_| id),
            None => a.doc.add_mate(kind, faces[0], faces[1], aligned),
        };
        match result {
            Ok(id) => {
                let name = a.doc.mate(id).map(|m| m.name.clone()).unwrap_or_default();
                a.changed();
                // Like SolidWorks, stay in the Mate PropertyManager for the next one
                if editing.is_some() {
                    a.mate_editor = None;
                } else if let Some(ed) = a.mate_editor.as_mut() {
                    ed.faces.clear();
                }
                self.mesh_dirty = true;
                self.info(format!("{name} ajoutée"));
            }
            Err(e) => {
                a.undo.pop();
                self.error(e.to_string());
            }
        }
    }

    fn run_interference(&mut self) {
        let Some(a) = self.asm_mut() else { return };
        let found = a.doc.interferences();
        a.interference_edges = found
            .iter()
            .filter_map(|i| i.solid.as_ref())
            .flat_map(|s| s.tessellate((s.size() * 2e-3).max(0.01)).edges)
            .collect();
        let n = found.len();
        a.interferences = Some(found);
        if n == 0 {
            self.info("Aucune interférence.");
        } else {
            self.error(format!("{n} interférence(s) détectée(s)."));
        }
    }

    pub(super) fn asm_delete_selection(&mut self) {
        let Some(sel) = self.asm().map(|a| a.selection.clone()) else { return };
        match sel {
            AsmSelection::Component(id) => self.asm_edit(|d| {
                d.remove_component(id);
                Ok(())
            }),
            AsmSelection::Mate(id) => self.asm_edit(|d| {
                d.remove_mate(id);
                Ok(())
            }),
            _ => {}
        }
        if let Some(a) = self.asm_mut() {
            a.selection = AsmSelection::None;
        }
    }

    pub(super) fn asm_escape(&mut self) {
        if let Some(a) = self.asm_mut() {
            if a.mate_editor.take().is_none() {
                a.selection = AsmSelection::None;
            }
        }
    }

    // ── Panels ───────────────────────────────────────────────────

    pub(super) fn asm_command_manager(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let _ = ui.selectable_label(true, "Assemblage");
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if big_button(ui, "Insérer des\ncomposants").clicked() {
                self.open_file_dialog(FileAction::InsertComponent);
            }
            if big_button(ui, "Contrainte").clicked() {
                self.start_mate();
            }
            ui.separator();
            if big_button(ui, "Détection\nd'interférences").clicked() {
                self.run_interference();
            }
            if big_button(ui, "Nomenclature").clicked() {
                if let Some(a) = self.asm_mut() {
                    a.show_bom = !a.show_bom;
                }
            }
            ui.separator();
            ui.vertical(|ui| {
                let hint = |t: &str| egui::RichText::new(t).small().color(Color32::DARK_GRAY);
                ui.label(hint("Glisser (gauche) : déplacer un composant libre"));
                ui.label(hint("Maj + glisser : le faire pivoter"));
            });
        });
    }

    pub(super) fn asm_feature_manager(&mut self, ui: &mut egui::Ui) {
        if self.drawing.is_some() && ui.button("↩ Enregistrer et revenir à la mise en plan").clicked() {
            self.back_to_drawing();
            return;
        }
        let Some(a) = self.asm() else { return };
        ui.label(egui::RichText::new(format!("🔩 {}", a.doc.name)).strong());
        ui.separator();
        let mut action: Option<TreeAction> = None;
        let comps: Vec<(Id, String, bool, bool)> =
            a.doc.components.iter().map(|c| (c.id, c.name.clone(), c.fixed, c.suppressed)).collect();
        for (id, name, fixed, suppressed) in comps {
            let state = a.doc.component_state(id);
            let broken = a.doc.part(id).is_none_or(|p| p.error.is_some());
            let mut text = egui::RichText::new(format!("⬛ {}{name}", state.prefix()));
            if broken {
                text = text.color(CONFLICT);
            } else if suppressed {
                text = text.color(Color32::GRAY).italics();
            }
            let resp = ui.selectable_label(a.selection == AsmSelection::Component(id), text);
            let hover = match state {
                ComponentState::Fixed => "Fixe".to_string(),
                ComponentState::UnderDefined(n) => format!("Sous-contraint : {n} degré(s) de liberté"),
                ComponentState::FullyDefined => "Entièrement contraint".to_string(),
            };
            let resp = resp.on_hover_text(match a.doc.part(id).and_then(|p| p.error.clone()) {
                Some(e) => format!("Pièce en erreur : {e}"),
                None => hover,
            });
            if resp.clicked() {
                action = Some(Box::new(move |app: &mut Self| {
                    if let Some(a) = app.asm_mut() {
                        a.selection = AsmSelection::Component(id);
                    }
                }));
            }
            if resp.double_clicked() {
                action = Some(Box::new(move |app: &mut Self| app.open_component_part(id)));
            }
            resp.context_menu(|ui| {
                if ui.button("Ouvrir la pièce").clicked() {
                    action = Some(Box::new(move |app: &mut Self| app.open_component_part(id)));
                    ui.close_menu();
                }
                if ui.button(if fixed { "Libérer" } else { "Fixer" }).clicked() {
                    action = Some(Box::new(move |app: &mut Self| {
                        app.asm_edit(|d| {
                            d.set_fixed(id, !fixed);
                            Ok(())
                        })
                    }));
                    ui.close_menu();
                }
                if ui.button(if suppressed { "Réactiver" } else { "Supprimer (suppress)" }).clicked() {
                    action = Some(Box::new(move |app: &mut Self| {
                        app.asm_edit(|d| {
                            d.set_component_suppressed(id, !suppressed);
                            Ok(())
                        })
                    }));
                    ui.close_menu();
                }
                if ui.button("Effacer").clicked() {
                    action = Some(Box::new(move |app: &mut Self| {
                        app.asm_edit(|d| {
                            d.remove_component(id);
                            Ok(())
                        })
                    }));
                    ui.close_menu();
                }
            });
        }
        ui.add_space(4.0);
        egui::CollapsingHeader::new("Contraintes").default_open(true).show(ui, |ui| {
            if a.doc.mates.is_empty() {
                ui.label(egui::RichText::new("(aucune)").color(Color32::GRAY));
            }
            for m in &a.doc.mates {
                let id = m.id;
                let suppressed = m.suppressed;
                let label = format!(
                    "○ {} ({}, {})",
                    m.name,
                    a.component_name(m.a.component),
                    a.component_name(m.b.component)
                );
                let mut text = egui::RichText::new(label);
                let state = a.doc.mate_state(id).cloned();
                match state {
                    Some(MateState::Unsatisfied) | Some(MateState::MissingFace) => text = text.color(CONFLICT),
                    Some(MateState::Suppressed) => text = text.color(Color32::GRAY).italics(),
                    _ => {}
                }
                let mut resp = ui.selectable_label(a.selection == AsmSelection::Mate(id), text);
                match state {
                    Some(MateState::Unsatisfied) => resp = resp.on_hover_text("Contrainte non satisfaite"),
                    Some(MateState::MissingFace) => resp = resp.on_hover_text("Une face de la contrainte a disparu"),
                    _ => {}
                }
                if resp.clicked() {
                    action = Some(Box::new(move |app: &mut Self| {
                        if let Some(a) = app.asm_mut() {
                            a.selection = AsmSelection::Mate(id);
                        }
                    }));
                }
                if resp.double_clicked() {
                    action = Some(Box::new(move |app: &mut Self| app.edit_mate(id)));
                }
                resp.context_menu(|ui| {
                    if ui.button("Éditer la contrainte").clicked() {
                        action = Some(Box::new(move |app: &mut Self| app.edit_mate(id)));
                        ui.close_menu();
                    }
                    if ui.button(if suppressed { "Réactiver" } else { "Supprimer (suppress)" }).clicked() {
                        action = Some(Box::new(move |app: &mut Self| {
                            app.asm_edit(|d| {
                                d.set_mate_suppressed(id, !suppressed);
                                Ok(())
                            })
                        }));
                        ui.close_menu();
                    }
                    if ui.button("Effacer").clicked() {
                        action = Some(Box::new(move |app: &mut Self| {
                            app.asm_edit(|d| {
                                d.remove_mate(id);
                                Ok(())
                            })
                        }));
                        ui.close_menu();
                    }
                });
            }
        });
        if let Some(f) = action {
            f(self);
        }
    }

    pub(super) fn asm_has_editor(&self) -> bool {
        self.asm().is_some_and(|a| a.mate_editor.is_some())
    }

    pub(super) fn asm_mate_property_manager(&mut self, ui: &mut egui::Ui) {
        let mut ok = false;
        let mut cancel = false;
        let mut remove: Option<usize> = None;
        {
            let Some(a) = self.assembly.as_mut() else { return };
            let labels: Vec<String> =
                a.mate_editor.as_ref().map(|ed| ed.faces.iter().map(|f| a.face_label(f)).collect()).unwrap_or_default();
            let Some(ed) = a.mate_editor.as_mut() else { return };
            ui.horizontal(|ui| {
                ui.heading(if ed.editing.is_some() { "Éditer la contrainte" } else { "Contrainte" });
            });
            ui.horizontal(|ui| {
                ok = ui.button("✔").on_hover_text("OK").clicked();
                cancel = ui.button("✖").on_hover_text("Fermer").clicked();
            });
            ui.separator();
            ui.label(egui::RichText::new("Entités à contraindre").strong());
            if labels.is_empty() {
                ui.label(egui::RichText::new("Cliquez deux faces dans la vue").color(Color32::GRAY));
            }
            for (i, l) in labels.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}. {l}", i + 1));
                    if ui.small_button("✖").clicked() {
                        remove = Some(i);
                    }
                });
            }
            ui.separator();
            ui.label(egui::RichText::new("Contraintes standard").strong());
            for c in MateChoice::ALL {
                ui.radio_value(&mut ed.choice, c, c.label());
                if ed.choice == c && c == MateChoice::Distance {
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.add(egui::DragValue::new(&mut ed.distance).speed(0.5).suffix(" mm"));
                    });
                }
                if ed.choice == c && c == MateChoice::Angle {
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.add(egui::DragValue::new(&mut ed.angle).speed(1.0).range(0.0..=180.0).suffix(" °"));
                    });
                }
            }
            if ed.choice != MateChoice::Concentric && ed.choice != MateChoice::Perpendicular {
                ui.separator();
                ui.label(egui::RichText::new("Alignement").strong());
                ui.horizontal(|ui| {
                    ui.radio_value(&mut ed.aligned, true, "Aligné");
                    ui.radio_value(&mut ed.aligned, false, "Anti-aligné");
                });
            }
        }
        if let Some(i) = remove {
            if let Some(ed) = self.assembly.as_mut().and_then(|a| a.mate_editor.as_mut()) {
                ed.faces.remove(i);
            }
        }
        if ok {
            self.apply_mate();
        }
        if cancel {
            if let Some(a) = self.assembly.as_mut() {
                a.mate_editor = None;
            }
        }
    }

    pub(super) fn asm_status(&self, ui: &mut egui::Ui) {
        let Some(a) = self.asm() else { return };
        let s = a.doc.summary();
        let under = a.doc.components.iter().any(|c| matches!(a.doc.component_state(c.id), ComponentState::UnderDefined(_)));
        let (text, color) = if s.unsatisfied > 0 {
            ("Sur-contraint".to_string(), CONFLICT)
        } else if under {
            ("Sous-contraint".to_string(), UNDER_DEFINED)
        } else {
            ("Entièrement contraint".to_string(), DEFINED)
        };
        ui.colored_label(color, text);
        ui.separator();
        ui.label(format!("{} composant(s), {} contrainte(s)", a.doc.components.len(), a.doc.mates.len()));
    }

    pub(super) fn asm_windows(&mut self, ctx: &egui::Context) {
        let Some(a) = self.assembly.as_mut().filter(|_| self.active == Active::Assembly) else { return };
        if a.show_bom {
            let rows = a.doc.bom();
            let mut open = true;
            egui::Window::new("Nomenclature").open(&mut open).resizable(false).show(ctx, |ui| {
                egui::Grid::new("bom").striped(true).num_columns(5).show(ui, |ui| {
                    for h in ["N°", "Pièce", "Fichier", "Qté", "Volume (mm³)"] {
                        ui.label(egui::RichText::new(h).strong());
                    }
                    ui.end_row();
                    for r in &rows {
                        ui.label(r.item.to_string());
                        ui.label(&r.name);
                        ui.label(r.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
                        ui.label(r.quantity.to_string());
                        ui.label(format!("{:.1}", r.volume));
                        ui.end_row();
                    }
                });
            });
            a.show_bom = open;
        }
        let mut close = false;
        if let Some(found) = &a.interferences {
            let mut open = true;
            egui::Window::new("Détection d'interférences").default_pos(egui::pos2(980.0, 120.0)).open(&mut open).resizable(false).show(ctx, |ui| {
                if found.is_empty() {
                    ui.label("Aucune interférence.");
                }
                for (i, f) in found.iter().enumerate() {
                    ui.colored_label(
                        CONFLICT,
                        format!(
                            "Interférence{} : {} – {}   {:.1} mm³",
                            i + 1,
                            a.component_name(f.a),
                            a.component_name(f.b),
                            f.volume
                        ),
                    );
                }
            });
            close = !open;
        }
        if close {
            a.interferences = None;
            a.interference_edges.clear();
        }
    }

    // ── Graphics area ────────────────────────────────────────────

    pub(super) fn asm_interaction(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        aspect: f64,
        to_ndc: &dyn Fn(Pos2) -> DVec2,
        to_screen: &dyn Fn(DVec3) -> Pos2,
    ) {
        let camera = self.camera;
        let painter = ui.painter_at(rect);
        let mods = ui.input(|i| i.modifiers);
        let Some(a) = self.assembly.as_mut() else { return };
        let pick = |a: &AssemblyView, p: Pos2| -> Option<u32> {
            let mesh = a.scene.as_ref()?;
            let t = pick_triangle(mesh, &camera.ray(to_ndc(p), aspect))?;
            Some(mesh.triangle_faces[t])
        };

        // Drag a free component in the screen plane (Shift: rotate it)
        if response.drag_started_by(egui::PointerButton::Primary) && a.mate_editor.is_none() {
            let start = response.interact_pointer_pos().map(|p| p - response.drag_delta());
            if let Some((comp, _)) = start.and_then(|p| pick(a, p)).and_then(|f| a.decode(f)) {
                if a.doc.component(comp).is_some_and(|c| c.fixed) {
                    let name = a.component_name(comp);
                    self.message = Some((format!("{name} est fixe : clic droit dans l'arbre > Libérer pour le déplacer."), true));
                    return;
                }
                a.checkpoint();
                a.drag = Some((comp, mods.shift));
                a.selection = AsmSelection::Component(comp);
            }
        }
        if let Some((comp, rotate)) = a.drag {
            if response.dragged_by(egui::PointerButton::Primary) {
                let d = response.drag_delta();
                let m = if rotate {
                    let center = a
                        .doc
                        .component(comp)
                        .zip(a.doc.part(comp))
                        .map(|(c, p)| c.placement.point(p.center))
                        .unwrap_or_default();
                    let k = 0.01;
                    let q = DQuat::from_axis_angle(camera.up(), d.x as f64 * k)
                        * DQuat::from_axis_angle(camera.right(), d.y as f64 * k);
                    DMat4::from_translation(center) * DMat4::from_quat(q) * DMat4::from_translation(-center)
                } else {
                    let upp = 2.0 * camera.half_height / rect.height().max(1.0) as f64;
                    DMat4::from_translation((camera.right() * d.x as f64 - camera.up() * d.y as f64) * upp)
                };
                if a.doc.move_component(comp, &m).is_ok() {
                    a.changed();
                    self.mesh_dirty = true;
                }
            }
            if response.drag_stopped() {
                a.drag = None;
            }
        }

        a.hover_face = response.hover_pos().and_then(|p| pick(a, p));
        if response.clicked_by(egui::PointerButton::Primary) {
            let hit = response.interact_pointer_pos().and_then(|p| pick(a, p)).and_then(|f| a.decode(f));
            let face = hit.and_then(|(c, f)| a.doc.face_ref(c, f));
            if hit.is_some() && face.is_none() {
                self.message = Some(("Seules les faces planes et cylindriques peuvent être contraintes.".into(), true));
            }
            if let Some(ed) = a.mate_editor.as_mut() {
                if let Some(f) = face {
                    if let Some(i) = ed.faces.iter().position(|x| *x == f) {
                        ed.faces.remove(i);
                    } else {
                        if ed.faces.len() == 2 {
                            ed.faces.pop();
                        }
                        ed.faces.push(f);
                    }
                    suggest(&a.doc, ed);
                }
            } else {
                a.selection = match (face, &a.selection, mods.shift || mods.ctrl) {
                    (Some(f), AsmSelection::Faces(prev), true) => {
                        let mut v = prev.clone();
                        if let Some(i) = v.iter().position(|x| *x == f) {
                            v.remove(i);
                        } else {
                            v.push(f);
                        }
                        if v.is_empty() {
                            AsmSelection::None
                        } else {
                            AsmSelection::Faces(v)
                        }
                    }
                    (Some(f), _, _) => AsmSelection::Faces(vec![f]),
                    (None, _, _) => AsmSelection::None,
                };
            }
        }
        if response.double_clicked() {
            if let Some((comp, _)) = response.interact_pointer_pos().and_then(|p| pick(a, p)).and_then(|f| a.decode(f)) {
                a.selection = AsmSelection::Component(comp);
            }
        }

        for poly in &a.interference_edges {
            let pts: Vec<Pos2> = poly.iter().map(|p| to_screen(DVec3::from(p.map(f64::from)))).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(2.5_f32, CONFLICT)));
        }
        if a.scene.is_none() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Assemblage vide : cliquez « Insérer des composants »",
                egui::FontId::proportional(16.0),
                Color32::DARK_GRAY,
            );
        }
    }
}

/// Pick a sensible mate type and alignment for the selected faces
fn suggest(doc: &AssemblyDocument, ed: &mut MateEditor) {
    if ed.faces.len() != 2 || ed.editing.is_some() {
        return;
    }
    let cylinders = ed.faces.iter().all(|f| f.kind == FaceKind::Cylinder);
    if cylinders && ed.choice == MateChoice::Coincident {
        ed.choice = MateChoice::Concentric;
    } else if !cylinders && ed.choice == MateChoice::Concentric {
        ed.choice = MateChoice::Coincident;
    }
    ed.aligned = doc.currently_aligned(&ed.faces[0], &ed.faces[1]);
}
