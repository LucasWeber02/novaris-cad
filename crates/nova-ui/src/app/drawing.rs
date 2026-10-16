//! Drawing mode: a sheet with associative views and dimensions, drawn with
//! the egui painter, like SolidWorks' drawing environment.

use super::*;
use nova_drawing::render::{self as sheet2d, Align, Item, Layer, Prim, Sheet2D, FONT_EM};
use nova_drawing::{
    DimDir, DimKind, Dimension, DrawingDocument, Orientation, Paper, Pick, Projection, Side, ViewKind, SCALES,
};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
enum DimStage {
    First,
    /// First vertex picked, waiting for the second
    Second { view: Id, a: DVec3 },
    /// Geometry chosen, the dimension follows the mouse until clicked
    Place { view: Id, kind: DimKind },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DrwTool {
    Select,
    Dimension(DimStage),
    /// Pick the parent view, then click the side to project to
    Projected(Option<Id>),
    /// Click where to put a model view
    ModelView(Orientation),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DrwSel {
    None,
    View(Id),
    Dimension(Id),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DrwDrag {
    View(Id),
    Dimension(Id),
    Pan,
}

pub(super) struct DrawingState {
    pub doc: DrawingDocument,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<DrawingDocument>,
    redo: Vec<DrawingDocument>,
    sheet: Sheet2D,
    /// Sheet point at the center of the canvas, and pixels per mm
    pan: DVec2,
    zoom: f64,
    fit_pending: bool,
    selection: DrwSel,
    tool: DrwTool,
    hover: Option<Pick>,
    drag: Option<DrwDrag>,
    cursor: Option<DVec2>,
}

impl DrawingState {
    fn new(doc: DrawingDocument, path: Option<PathBuf>) -> Self {
        let sheet = sheet2d::render(&doc);
        DrawingState {
            doc,
            path,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            sheet,
            pan: DVec2::ZERO,
            zoom: 3.0,
            fit_pending: true,
            selection: DrwSel::None,
            tool: DrwTool::Select,
            hover: None,
            drag: None,
            cursor: None,
        }
    }

    fn rerender(&mut self) {
        self.sheet = sheet2d::render(&self.doc);
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.doc.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    /// Modify the document with undo, then redraw
    fn edit(&mut self, f: impl FnOnce(&mut DrawingDocument)) {
        self.checkpoint();
        f(&mut self.doc);
        self.rerender();
    }

    pub fn undo(&mut self) {
        if let Some(d) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, d));
            self.selection = DrwSel::None;
            self.rerender();
        }
    }

    pub fn redo(&mut self) {
        if let Some(d) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, d));
            self.selection = DrwSel::None;
            self.rerender();
        }
    }

    fn view_name(&self, id: Id) -> String {
        self.doc.view(id).map(|v| v.name.clone()).unwrap_or_default()
    }
}

impl NovarisApp {
    pub(super) fn drw(&self) -> Option<&DrawingState> {
        self.drawing.as_ref().filter(|_| self.active == Active::Drawing)
    }

    fn drw_mut(&mut self) -> Option<&mut DrawingState> {
        self.drawing.as_mut().filter(|_| self.active == Active::Drawing)
    }

    pub(super) fn drw_can_undo(&self) -> (bool, bool) {
        self.drw().map(|d| (!d.undo.is_empty(), !d.redo.is_empty())).unwrap_or((false, false))
    }

    pub(super) fn drw_undo(&mut self, redo: bool) {
        if let Some(d) = self.drw_mut() {
            if redo {
                d.redo();
            } else {
                d.undo();
            }
        }
    }

    /// File > Make Drawing from the open part or assembly
    pub(super) fn new_drawing_from_model(&mut self) {
        let (source, name) = match self.active {
            Active::Assembly => {
                let Some(a) = self.assembly.as_ref() else { return };
                if a.dirty || a.path.is_none() {
                    self.error("Enregistrez d'abord l'assemblage (Ctrl+S).");
                    return;
                }
                (a.path.clone().expect("checked"), a.doc.name.clone())
            }
            Active::Part => {
                if self.dirty || self.path.is_none() {
                    self.error("Enregistrez d'abord la pièce (Ctrl+S).");
                    return;
                }
                (self.path.clone().expect("checked"), self.doc.name.clone())
            }
            Active::Drawing => return,
        };
        let mut doc = DrawingDocument::new(name, &source);
        if doc.source_is_assembly() {
            doc.sheet.paper = Paper::A3;
            doc.show_bom = true;
        }
        doc.update();
        if let Some(e) = doc.source_error() {
            let e = e.to_string();
            self.error(format!("Mise en plan impossible : {e}"));
            return;
        }
        doc.standard_views(true);
        self.drawing = Some(DrawingState::new(doc, None));
        self.active = Active::Drawing;
        self.info("Mise en plan créée : 3 vues standard et une vue isométrique.");
    }

    pub(super) fn open_drawing(&mut self, path: &Path) -> Result<String, String> {
        let doc = DrawingDocument::open(path).map_err(|e| e.to_string())?;
        let warn = doc.source_error().map(|e| e.to_string());
        self.drawing = Some(DrawingState::new(doc, Some(path.to_path_buf())));
        self.active = Active::Drawing;
        match warn {
            None => Ok(format!("Ouvert : {}", path.display())),
            Some(e) => Err(format!("Modèle introuvable ou en erreur : {e}")),
        }
    }

    pub(super) fn drw_save_as(&mut self, path: &Path) -> Result<String, String> {
        let d = self.drawing.as_mut().ok_or("aucune mise en plan")?;
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            d.doc.name = name.to_string();
        }
        d.doc.save(path).map_err(|e| e.to_string())?;
        d.path = Some(path.to_path_buf());
        d.dirty = false;
        Ok(format!("Enregistré : {}", path.display()))
    }

    pub(super) fn drw_save(&mut self) {
        match self.drw().and_then(|d| d.path.clone()) {
            Some(p) => match self.drw_save_as(&p) {
                Ok(m) => self.info(m),
                Err(e) => self.error(format!("Échec de l'enregistrement : {e}")),
            },
            None => self.open_file_dialog(FileAction::SaveAs),
        }
    }

    pub(super) fn drw_export(&self, path: &Path, pdf: bool) -> Result<String, String> {
        let d = self.drw().ok_or("aucune mise en plan")?;
        let r = if pdf { d.doc.export_pdf(path) } else { d.doc.export_dxf(path) };
        r.map(|_| format!("Exporté : {}", path.display())).map_err(|e| e.to_string())
    }

    /// Open the drawing's model; the drawing stays open
    fn open_drawing_model(&mut self) {
        let Some(path) = self.drw().map(|d| d.doc.source_file()) else { return };
        self.active = Active::Part;
        self.run_file_action(FileAction::Open, path);
        self.mesh_dirty = true;
    }

    /// Back from the model: save it, then update the views
    pub(super) fn back_to_drawing(&mut self) {
        match self.active {
            Active::Part if self.dirty => {
                if self.path.is_none() {
                    self.error("Enregistrez d'abord la pièce.");
                    return;
                }
                self.save();
            }
            Active::Assembly if self.assembly.as_ref().is_some_and(|a| a.dirty) => self.asm_save(),
            _ => {}
        }
        self.active = Active::Drawing;
        if let Some(d) = self.drawing.as_mut() {
            d.doc.update();
            d.rerender();
        }
        let orphans = self.drw().map(|d| d.doc.dimensions.iter().filter(|x| d.doc.is_orphan(x.id)).count()).unwrap_or(0);
        if orphans > 0 {
            self.error(format!("{orphans} cote(s) ont perdu leur géométrie (en rouge)."));
        } else {
            self.info("Mise en plan mise à jour.");
        }
    }

    pub(super) fn drw_delete_selection(&mut self) {
        let Some(d) = self.drw_mut() else { return };
        match d.selection {
            DrwSel::View(id) => d.edit(|doc| doc.remove_view(id)),
            DrwSel::Dimension(id) => d.edit(|doc| doc.remove_dimension(id)),
            DrwSel::None => {}
        }
        d.selection = DrwSel::None;
    }

    pub(super) fn drw_escape(&mut self) {
        if let Some(d) = self.drw_mut() {
            if d.tool == DrwTool::Select {
                d.selection = DrwSel::None;
            }
            d.tool = DrwTool::Select;
        }
    }

    pub(super) fn drw_fit(&mut self) {
        if let Some(d) = self.drw_mut() {
            d.fit_pending = true;
        }
    }

    // ── Panels ───────────────────────────────────────────────────

    pub(super) fn drw_command_manager(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.drawing.as_mut() else { return };
        ui.horizontal(|ui| {
            let _ = ui.selectable_label(true, "Mise en plan");
        });
        ui.separator();
        let mut back = false;
        let mut msg: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            if big_button(ui, "Vue standard\n3 vues").clicked() {
                d.edit(|doc| doc.standard_views(true));
            }
            ui.menu_button("Vue du\nmodèle…", |ui| {
                for o in Orientation::ALL {
                    if ui.button(o.label()).clicked() {
                        d.tool = DrwTool::ModelView(o);
                        msg = Some(format!("Cliquez sur la feuille pour placer la {}.", o.label().to_lowercase()));
                        ui.close_menu();
                    }
                }
            });
            if big_button(ui, "Vue projetée").clicked() {
                let parent = match d.selection {
                    DrwSel::View(v) => Some(v),
                    _ => None,
                };
                d.tool = DrwTool::Projected(parent);
                msg = Some(if parent.is_some() {
                    "Déplacez la souris du côté voulu puis cliquez.".into()
                } else {
                    "Cliquez la vue à projeter, puis le côté voulu.".into()
                });
            }
            ui.separator();
            if big_button(ui, "Cotation\nintelligente").clicked() {
                d.tool = DrwTool::Dimension(DimStage::First);
                msg = Some("Cliquez deux sommets, une arête ou un cercle, puis placez la cote.".into());
            }
            if d.doc.source_is_assembly() && big_button(ui, "Nomenclature").clicked() {
                d.edit(|doc| doc.show_bom = !doc.show_bom);
            }
            ui.separator();
            if big_button(ui, "Mettre à jour").clicked() {
                d.doc.update();
                d.rerender();
                msg = Some("Vues mises à jour depuis le modèle.".into());
            }
            if big_button(ui, "Ouvrir le\nmodèle").clicked() {
                back = true;
            }
        });
        if let Some(m) = msg {
            self.info(m);
        }
        if back {
            self.open_drawing_model();
        }
    }

    pub(super) fn drw_feature_manager(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.drawing.as_mut() else { return };
        let paper = d.doc.sheet.paper;
        ui.label(egui::RichText::new(format!("📄 {}", d.doc.name)).strong());
        ui.separator();
        ui.label(format!(
            "Feuille1 ({}, {})",
            paper.label(),
            nova_drawing::format_scale(d.doc.sheet.scale)
        ));
        let mut select: Option<DrwSel> = None;
        let mut edit: Option<DocEdit> = None;
        for v in &d.doc.views {
            let id = v.id;
            let icon = match v.kind {
                ViewKind::Model(Orientation::Iso) => "◑",
                ViewKind::Model(_) => "⬛",
                ViewKind::Projected { .. } => "■",
            };
            let resp = ui.selectable_label(d.selection == DrwSel::View(id), format!("  {icon} {}", v.name));
            if resp.clicked() {
                select = Some(DrwSel::View(id));
            }
            let hidden = v.show_hidden;
            resp.context_menu(|ui| {
                if ui.button(if hidden { "Masquer les arêtes cachées" } else { "Afficher les arêtes cachées" }).clicked() {
                    edit = Some(Box::new(move |doc: &mut DrawingDocument| {
                        if let Some(v) = doc.view_mut(id) {
                            v.show_hidden = !hidden;
                        }
                        doc.refresh();
                    }));
                    ui.close_menu();
                }
                if ui.button("Effacer").clicked() {
                    edit = Some(Box::new(move |doc: &mut DrawingDocument| doc.remove_view(id)));
                    ui.close_menu();
                }
            });
        }
        if !d.doc.dimensions.is_empty() {
            egui::CollapsingHeader::new("Cotes").default_open(true).show(ui, |ui| {
                for dim in &d.doc.dimensions {
                    let id = dim.id;
                    let mut text = egui::RichText::new(format!("{} = {}", dim.name, d.doc.label(dim)));
                    if d.doc.is_orphan(id) {
                        text = text.color(CONFLICT);
                    }
                    let resp = ui.selectable_label(d.selection == DrwSel::Dimension(id), text);
                    if resp.clicked() {
                        select = Some(DrwSel::Dimension(id));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Effacer").clicked() {
                            edit = Some(Box::new(move |doc: &mut DrawingDocument| doc.remove_dimension(id)));
                            ui.close_menu();
                        }
                    });
                }
            });
        }
        if let Some(s) = select {
            d.selection = s;
        }
        if let Some(f) = edit {
            d.edit(f);
        }

        // PropertyManager of the selected view, or of the sheet
        ui.separator();
        if let DrwSel::View(id) = d.selection {
            let Some(v) = d.doc.view(id).cloned() else { return };
            ui.label(egui::RichText::new(&v.name).strong());
            let mut hidden = v.show_hidden;
            if ui.checkbox(&mut hidden, "Arêtes cachées visibles").changed() {
                d.edit(|doc| {
                    if let Some(v) = doc.view_mut(id) {
                        v.show_hidden = hidden;
                    }
                    doc.refresh();
                });
            }
            if let ViewKind::Model(_) = v.kind {
                let current = v.scale;
                let label = |s: Option<f64>| match s {
                    None => "Échelle de la feuille".to_string(),
                    Some(s) => nova_drawing::format_scale(s),
                };
                egui::ComboBox::from_id_source("view-scale").selected_text(label(current)).show_ui(ui, |ui| {
                    for s in std::iter::once(None).chain(SCALES.iter().map(|s| Some(*s))) {
                        if ui.selectable_label(current == s, label(s)).clicked() && current != s {
                            d.edit(|doc| {
                                if let Some(v) = doc.view_mut(id) {
                                    v.scale = s;
                                }
                            });
                        }
                    }
                });
            }
            if ui.button("Effacer la vue").clicked() {
                d.edit(|doc| doc.remove_view(id));
                d.selection = DrwSel::None;
            }
            return;
        }
        ui.label(egui::RichText::new("Propriétés de la feuille").strong());
        let mut paper_new = paper;
        egui::ComboBox::from_label("Format").selected_text(paper.label()).show_ui(ui, |ui| {
            for p in Paper::ALL {
                ui.selectable_value(&mut paper_new, p, p.label());
            }
        });
        if paper_new != paper {
            d.edit(|doc| doc.sheet.paper = paper_new);
            d.fit_pending = true;
        }
        let scale = d.doc.sheet.scale;
        let mut scale_new = scale;
        egui::ComboBox::from_label("Échelle").selected_text(nova_drawing::format_scale(scale)).show_ui(ui, |ui| {
            for s in SCALES {
                ui.selectable_value(&mut scale_new, s, nova_drawing::format_scale(s));
            }
        });
        if scale_new != scale {
            d.edit(|doc| doc.sheet.scale = scale_new);
        }
        let proj = d.doc.sheet.projection;
        let mut proj_new = proj;
        ui.radio_value(&mut proj_new, Projection::FirstAngle, Projection::FirstAngle.label());
        ui.radio_value(&mut proj_new, Projection::ThirdAngle, Projection::ThirdAngle.label());
        if proj_new != proj {
            d.edit(|doc| doc.set_projection(proj_new));
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Cartouche").strong());
        let mut tb = d.doc.sheet.title_block.clone();
        let mut changed = false;
        egui::Grid::new("title-block").num_columns(2).show(ui, |ui| {
            for (label, value) in [
                ("Titre", &mut tb.title),
                ("Dessiné par", &mut tb.drawn_by),
                ("Date", &mut tb.date),
                ("Matière", &mut tb.material),
            ] {
                ui.label(label);
                changed |= ui.add(egui::TextEdit::singleline(value).desired_width(140.0)).lost_focus();
                ui.end_row();
            }
        });
        if changed && tb != d.doc.sheet.title_block {
            d.edit(|doc| doc.sheet.title_block = tb);
        }
    }

    pub(super) fn drw_status(&self, ui: &mut egui::Ui) {
        let Some(d) = self.drw() else { return };
        if let Some(p) = d.cursor {
            ui.label(format!("{:.1} mm, {:.1} mm", p.x, p.y));
            ui.separator();
        }
        ui.label(format!(
            "Feuille {} — {} — {}",
            d.doc.sheet.paper.label(),
            nova_drawing::format_scale(d.doc.sheet.scale),
            if d.doc.sheet.projection == Projection::FirstAngle { "1er dièdre" } else { "3e dièdre" }
        ));
    }

    // ── Sheet canvas ─────────────────────────────────────────────

    pub(super) fn drw_canvas(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let mut message: Option<(String, bool)> = None;
        let Some(d) = self.drawing.as_mut() else { return };
        let size = d.doc.sheet.paper.size();
        if d.fit_pending {
            d.fit_pending = false;
            d.zoom = ((rect.width() as f64 - 40.0) / size.x).min((rect.height() as f64 - 40.0) / size.y).max(0.1);
            d.pan = size / 2.0;
        }
        let (zoom, pan) = (d.zoom, d.pan);
        let center = rect.center();
        let to_screen = move |p: DVec2| {
            Pos2::new(center.x + ((p.x - pan.x) * zoom) as f32, center.y - ((p.y - pan.y) * zoom) as f32)
        };
        let to_sheet = move |p: Pos2| {
            DVec2::new(pan.x + (p.x - center.x) as f64 / zoom, pan.y - (p.y - center.y) as f64 / zoom)
        };

        // Pan (middle / right drag) and zoom (wheel, about the cursor)
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if response.hovered() && scroll != 0.0 {
            if let Some(p) = response.hover_pos() {
                let before = to_sheet(p);
                d.zoom = (d.zoom * (scroll as f64 * 0.0015).exp()).clamp(0.2, 80.0);
                let after = DVec2::new(
                    d.pan.x + (p.x - center.x) as f64 / d.zoom,
                    d.pan.y - (p.y - center.y) as f64 / d.zoom,
                );
                d.pan += before - after;
            }
        }
        if response.dragged_by(egui::PointerButton::Middle) || response.dragged_by(egui::PointerButton::Secondary) {
            let delta = response.drag_delta();
            d.pan -= DVec2::new(delta.x as f64, -delta.y as f64) / d.zoom;
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(170, 176, 186));
        let sheet_rect = Rect::from_two_pos(to_screen(DVec2::ZERO), to_screen(size));
        painter.rect_filled(sheet_rect.translate(Vec2::new(3.0, 3.0)), 0.0, Color32::from_black_alpha(60));
        painter.rect_filled(sheet_rect, 0.0, Color32::WHITE);

        let cursor = response.hover_pos().map(to_sheet);
        d.cursor = cursor;
        let tol = 6.0 / d.zoom;
        d.hover = cursor.and_then(|c| d.doc.pick(c, tol));

        // Highlights: selection and what the active tool would pick
        let selected = match d.selection {
            DrwSel::View(id) | DrwSel::Dimension(id) => Some(id),
            DrwSel::None => None,
        };
        let ctx = ui.ctx().clone();
        for item in &d.sheet.items {
            let color = if item.owner.is_some() && item.owner == selected { Some(SELECTED) } else { None };
            paint_item(&painter, &ctx, item, &to_screen, zoom, color);
        }
        let highlight = |pick: &Pick, color: Color32| match *pick {
            Pick::Vertex { view, point } => {
                if let Some(p) = d.doc.frame(view).and_then(|f| d.doc.to_sheet(view, f.project(point))) {
                    painter.circle_filled(to_screen(p), 4.0, color);
                }
            }
            Pick::Circle { view, circle } => {
                if let Some(c) = d.doc.to_sheet(view, circle.center) {
                    let r = circle.radius * d.doc.view_scale(view) * zoom;
                    painter.circle_stroke(to_screen(c), r as f32, Stroke::new(2.5_f32, color));
                }
            }
            Pick::Edge { view, a, b } => {
                if let Some(f) = d.doc.frame(view) {
                    if let (Some(pa), Some(pb)) = (d.doc.to_sheet(view, f.project(a)), d.doc.to_sheet(view, f.project(b))) {
                        painter.line_segment([to_screen(pa), to_screen(pb)], Stroke::new(2.5_f32, color));
                    }
                }
            }
            Pick::View(view) => {
                if let Some((lo, hi)) = d.doc.view_rect(view) {
                    let r = Rect::from_two_pos(to_screen(lo), to_screen(hi)).expand(6.0);
                    painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, color));
                }
            }
            Pick::Dimension(_) => {}
        };
        if let Some(h) = &d.hover {
            let wanted = match (d.tool, h) {
                (DrwTool::Dimension(DimStage::Place { .. }), _) => false,
                (DrwTool::Dimension(_), Pick::View(_)) => false,
                (DrwTool::Select, Pick::Vertex { view, .. } | Pick::Circle { view, .. } | Pick::Edge { view, .. }) => {
                    highlight(&Pick::View(*view), PREVIEW);
                    false
                }
                _ => true,
            };
            if wanted {
                highlight(h, PREVIEW);
            }
        }
        if let DrwTool::Dimension(DimStage::Second { view, a }) = d.tool {
            highlight(&Pick::Vertex { view, point: a }, SELECTED);
        }

        // Preview of the dimension being placed
        if let (DrwTool::Dimension(DimStage::Place { view, kind }), Some(c)) = (d.tool, cursor) {
            if let Some(place) = d.doc.from_sheet(view, c) {
                let kind = with_inferred_dir(&d.doc, view, kind, place);
                let preview = Dimension { id: Id::new(), name: String::new(), view, kind, place };
                for item in sheet2d::dimension_items(&d.doc, &preview, place) {
                    paint_item(&painter, &ctx, &item, &to_screen, zoom, Some(PREVIEW));
                }
            }
        }
        // Projected view placement hint
        if let (DrwTool::Projected(Some(parent)), Some(c)) = (d.tool, cursor) {
            if let Some(pc) = d.doc.view_center(parent) {
                let side = side_toward(c - pc);
                painter.line_segment([to_screen(pc), to_screen(c)], Stroke::new(1.0_f32, PREVIEW));
                let arrow = match side {
                    Side::Left => "←",
                    Side::Right => "→",
                    Side::Up => "↑",
                    Side::Down => "↓",
                };
                painter.text(to_screen(c) + Vec2::new(12.0, -12.0), egui::Align2::LEFT_BOTTOM, arrow, egui::FontId::proportional(18.0), PREVIEW);
            }
        }

        // Dragging views and dimensions (Select tool)
        if response.drag_started_by(egui::PointerButton::Primary) && d.tool == DrwTool::Select {
            let start = response.interact_pointer_pos().map(|p| to_sheet(p - response.drag_delta()));
            let target = start.and_then(|p| d.doc.pick(p, tol)).map(|pick| match pick {
                Pick::Dimension(id) => DrwDrag::Dimension(id),
                Pick::Vertex { view, .. } | Pick::Circle { view, .. } | Pick::Edge { view, .. } | Pick::View(view) => {
                    DrwDrag::View(view)
                }
            });
            if let Some(t) = target {
                d.checkpoint();
                d.selection = match t {
                    DrwDrag::View(v) => DrwSel::View(v),
                    DrwDrag::Dimension(id) => DrwSel::Dimension(id),
                    DrwDrag::Pan => d.selection,
                };
            }
            d.drag = target.or(Some(DrwDrag::Pan));
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            let dm = DVec2::new(delta.x as f64, -delta.y as f64) / d.zoom;
            match d.drag {
                Some(DrwDrag::View(v)) => {
                    d.doc.move_view(v, dm);
                    d.rerender();
                }
                Some(DrwDrag::Dimension(id)) => {
                    let view = d.doc.dimensions.iter().find(|x| x.id == id).map(|x| x.view);
                    if let Some(view) = view {
                        let s = d.doc.view_scale(view);
                        if let Some(dim) = d.doc.dimensions.iter_mut().find(|x| x.id == id) {
                            dim.place += dm / s;
                        }
                        d.rerender();
                    }
                }
                Some(DrwDrag::Pan) => d.pan -= dm,
                None => {}
            }
        }
        if response.drag_stopped() {
            d.drag = None;
        }

        // Clicks
        if response.clicked_by(egui::PointerButton::Primary) {
            let Some(at) = response.interact_pointer_pos().map(to_sheet) else { return };
            let pick = d.doc.pick(at, tol);
            match d.tool {
                DrwTool::Select => {
                    d.selection = match pick {
                        Some(Pick::Dimension(id)) => DrwSel::Dimension(id),
                        Some(Pick::Vertex { view, .. } | Pick::Circle { view, .. } | Pick::Edge { view, .. } | Pick::View(view)) => {
                            DrwSel::View(view)
                        }
                        None => DrwSel::None,
                    };
                }
                DrwTool::ModelView(o) => {
                    d.edit(|doc| {
                        doc.add_model_view(o, at);
                    });
                    d.tool = DrwTool::Select;
                }
                DrwTool::Projected(None) => {
                    let parent = match pick {
                        Some(Pick::Vertex { view, .. } | Pick::Circle { view, .. } | Pick::Edge { view, .. } | Pick::View(view)) => {
                            Some(view)
                        }
                        _ => None,
                    };
                    if parent.is_some() {
                        d.tool = DrwTool::Projected(parent);
                        message = Some(("Déplacez la souris du côté voulu puis cliquez.".into(), false));
                    }
                }
                DrwTool::Projected(Some(parent)) => {
                    if let Some(pc) = d.doc.view_center(parent) {
                        let side = side_toward(at - pc);
                        let mut result = Ok(Id::new());
                        d.edit(|doc| {
                            result = doc.add_projected_view(parent, side);
                            if let Ok(id) = result {
                                let dist = (at - pc).dot(side.vector());
                                if let Some(v) = doc.view_mut(id) {
                                    v.distance = dist.max(5.0);
                                }
                            }
                        });
                        if let Ok(id) = result {
                            d.selection = DrwSel::View(id);
                            message = Some((format!("{} ajoutée", d.view_name(id)), false));
                        }
                    }
                    d.tool = DrwTool::Select;
                }
                DrwTool::Dimension(stage) => {
                    let next = match (stage, pick) {
                        (DimStage::First, Some(Pick::Vertex { view, point })) => DimStage::Second { view, a: point },
                        (DimStage::First, Some(Pick::Edge { view, a, b })) => {
                            DimStage::Place { view, kind: DimKind::Linear { a, b, dir: DimDir::Aligned } }
                        }
                        (DimStage::First | DimStage::Second { .. }, Some(Pick::Circle { view, circle })) => DimStage::Place {
                            view,
                            kind: if circle.full {
                                DimKind::Diameter { center: circle.center, radius: circle.radius }
                            } else {
                                DimKind::Radius { center: circle.center, radius: circle.radius }
                            },
                        },
                        (DimStage::Second { view, a }, Some(Pick::Vertex { view: v2, point })) if v2 == view && point != a => {
                            DimStage::Place { view, kind: DimKind::Linear { a, b: point, dir: DimDir::Aligned } }
                        }
                        (DimStage::Second { .. }, Some(Pick::Vertex { .. })) => {
                            message = Some(("Choisissez le second sommet dans la même vue.".into(), true));
                            stage
                        }
                        (DimStage::Place { view, kind }, _) => {
                            if let Some(place) = d.doc.from_sheet(view, at) {
                                let kind = with_inferred_dir(&d.doc, view, kind, place);
                                let mut created = None;
                                d.edit(|doc| created = Some(doc.add_dimension(view, kind, place)));
                                if let Some(id) = created {
                                    let label = d.doc.dimensions.iter().find(|x| x.id == id).map(|x| d.doc.label(x)).unwrap_or_default();
                                    message = Some((format!("Cote {label} ajoutée"), false));
                                }
                            }
                            DimStage::First
                        }
                        (s, _) => s,
                    };
                    d.tool = DrwTool::Dimension(next);
                }
            }
        }
        if let Some(m) = message {
            self.message = Some(m);
        }
    }
}

type DocEdit = Box<dyn FnOnce(&mut DrawingDocument)>;

/// Side of a view a point is on (the dominant direction)
fn side_toward(v: DVec2) -> Side {
    if v.x.abs() >= v.y.abs() {
        if v.x >= 0.0 {
            Side::Right
        } else {
            Side::Left
        }
    } else if v.y >= 0.0 {
        Side::Up
    } else {
        Side::Down
    }
}

fn with_inferred_dir(doc: &DrawingDocument, view: Id, kind: DimKind, place: DVec2) -> DimKind {
    match kind {
        DimKind::Linear { a, b, .. } => DimKind::Linear { a, b, dir: doc.infer_dir(view, a, b, place) },
        k => k,
    }
}

fn layer_color(l: Layer) -> Color32 {
    let [r, g, b] = l.color();
    Color32::from_rgb(r, g, b)
}

/// Draw one sheet primitive with the egui painter
fn paint_item(
    painter: &egui::Painter,
    ctx: &egui::Context,
    item: &Item,
    to_screen: &dyn Fn(DVec2) -> Pos2,
    zoom: f64,
    color: Option<Color32>,
) {
    let c = color.unwrap_or_else(|| layer_color(item.layer));
    let width = ((item.layer.width() * zoom) as f32).max(1.0);
    let stroke = Stroke::new(width, c);
    let dashes = item.layer.dashes();
    let polyline = |pts: Vec<Pos2>| {
        if dashes.is_empty() {
            painter.add(egui::Shape::line(pts, stroke));
        } else {
            let dash = (dashes[0] * zoom) as f32;
            let gap = (dashes[1] * zoom) as f32;
            painter.extend(egui::Shape::dashed_line(&pts, stroke, dash.max(2.0), gap.max(1.5)));
        }
    };
    let arc = |center: DVec2, r: f64, a0: f64, a1: f64| -> Vec<Pos2> {
        let mut a1 = a1;
        while a1 <= a0 {
            a1 += 360.0;
        }
        let n = (((a1 - a0) / 6.0).ceil() as usize).max(4);
        (0..=n)
            .map(|i| {
                let t = (a0 + (a1 - a0) * i as f64 / n as f64).to_radians();
                to_screen(center + DVec2::new(t.cos(), t.sin()) * r)
            })
            .collect()
    };
    match &item.prim {
        Prim::Line(a, b) => polyline(vec![to_screen(*a), to_screen(*b)]),
        Prim::Polyline(pts) => polyline(pts.iter().map(|p| to_screen(*p)).collect()),
        Prim::Circle { center, radius } => polyline(arc(*center, *radius, 0.0, 360.0)),
        Prim::Arc { center, radius, start, end } => polyline(arc(*center, *radius, *start, *end)),
        Prim::Arrow { tip, dir } => {
            let d = dir.normalize_or_zero();
            let base = *tip - d * 3.0;
            let side = d.perp() * 0.5;
            painter.add(egui::Shape::convex_polygon(
                vec![to_screen(*tip), to_screen(base + side), to_screen(base - side)],
                c,
                Stroke::NONE,
            ));
        }
        Prim::Text { at, height, text, align, angle } => {
            let px = (*height * FONT_EM * zoom) as f32;
            if px < 2.0 {
                return;
            }
            let galley = ctx.fonts(|f| f.layout_no_wrap(text.clone(), egui::FontId::proportional(px), c));
            let w = galley.size().x;
            let ascent = galley.size().y * 0.78;
            let shift = match align {
                Align::Left => 0.0,
                Align::Center => -w / 2.0,
                Align::Right => -w,
            };
            let a = angle.to_radians() as f32;
            // Screen axes of the text: x along (cos, -sin), y down along (sin, cos)
            let xdir = Vec2::new(a.cos(), -a.sin());
            let ydown = Vec2::new(a.sin(), a.cos());
            let pos = to_screen(*at) + xdir * shift - ydown * ascent;
            painter.add(egui::epaint::TextShape::new(pos, galley, c).with_angle(-a));
        }
    }
}
