//! The sheet as 2D primitives in sheet millimetres (origin bottom left,
//! y up), shared by the screen and the PDF / DXF exports

use crate::{format_scale, DimDir, DimKind, Dimension, DrawingDocument, Projection, MARGIN, TITLE_H, TITLE_W};
use glam::DVec2;
use nova_core::Id;
use nova_kernel::{EdgeClass, ProjCurve, ProjectedEdge};

/// Line style group, as ISO 128 line types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    /// Visible edges, thick continuous
    Visible,
    /// Hidden edges, dashed
    Hidden,
    /// Tangent edges, thin continuous
    Tangent,
    /// Center lines, chain
    Center,
    /// Dimensions and annotations, thin
    Dimension,
    /// Dimensions that lost their geometry
    Error,
    /// Border and title block outline, thick
    Frame,
    /// Title block grid and texts, thin
    Thin,
}

impl Layer {
    pub const ALL: [Layer; 8] = [
        Layer::Visible,
        Layer::Hidden,
        Layer::Tangent,
        Layer::Center,
        Layer::Dimension,
        Layer::Error,
        Layer::Frame,
        Layer::Thin,
    ];

    /// Pen width in mm
    pub fn width(&self) -> f64 {
        match self {
            Layer::Visible | Layer::Frame => 0.5,
            Layer::Hidden => 0.35,
            _ => 0.25,
        }
    }

    /// Dash pattern in mm (empty = continuous)
    pub fn dashes(&self) -> &'static [f64] {
        match self {
            Layer::Hidden => &[3.0, 1.5],
            Layer::Center => &[8.0, 1.5, 1.0, 1.5],
            _ => &[],
        }
    }

    /// DXF layer name
    pub fn name(&self) -> &'static str {
        match self {
            Layer::Visible => "VISIBLE",
            Layer::Hidden => "CACHE",
            Layer::Tangent => "TANGENTE",
            Layer::Center => "AXES",
            Layer::Dimension => "COTATION",
            Layer::Error => "ERREUR",
            Layer::Frame => "CADRE",
            Layer::Thin => "CARTOUCHE",
        }
    }

    /// RGB colour on paper
    pub fn color(&self) -> [u8; 3] {
        match self {
            Layer::Error => [220, 20, 20],
            Layer::Dimension => [20, 20, 20],
            Layer::Center | Layer::Tangent => [60, 60, 60],
            _ => [0, 0, 0],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Line(DVec2, DVec2),
    Polyline(Vec<DVec2>),
    Circle { center: DVec2, radius: f64 },
    /// Counter-clockwise from `start` to `end` (degrees)
    Arc { center: DVec2, radius: f64, start: f64, end: f64 },
    /// `at` is the baseline anchor; `angle` in degrees
    Text { at: DVec2, height: f64, text: String, align: Align, angle: f64 },
    /// Filled arrow head with its tip at `tip`, pointing along `dir`
    Arrow { tip: DVec2, dir: DVec2 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub prim: Prim,
    pub layer: Layer,
    /// View or dimension this belongs to, for selection on screen
    pub owner: Option<Id>,
}

#[derive(Clone, Debug, Default)]
pub struct Sheet2D {
    pub size: DVec2,
    pub items: Vec<Item>,
}

/// Font size per mm of text height (ISO text height is the capital height)
pub const FONT_EM: f64 = 1.4;

/// Approximate width of a Helvetica text of capital height `height` (mm)
pub fn text_width(text: &str, height: f64) -> f64 {
    text.chars()
        .map(|c| match c {
            ' ' | '.' | ',' | ':' | ';' | 'i' | 'l' | 'j' | 'I' | '\'' | '|' => 0.28,
            'f' | 't' | 'r' | '(' | ')' | '-' | '/' => 0.36,
            'm' | 'w' => 0.85,
            '0'..='9' | 'a'..='z' => 0.556,
            'M' | 'W' => 0.85,
            'A'..='Z' | 'Ø' => 0.68,
            _ => 0.6,
        })
        .sum::<f64>()
        * height
        * FONT_EM
}

struct Out {
    items: Vec<Item>,
    owner: Option<Id>,
}

impl Out {
    fn add(&mut self, prim: Prim, layer: Layer) {
        self.items.push(Item { prim, layer, owner: self.owner });
    }

    fn text(&mut self, at: DVec2, height: f64, text: impl Into<String>, align: Align, angle: f64, layer: Layer) {
        self.add(Prim::Text { at, height, text: text.into(), align, angle }, layer);
    }
}

pub fn render(doc: &DrawingDocument) -> Sheet2D {
    let size = doc.sheet.paper.size();
    let mut out = Out { items: Vec::new(), owner: None };
    frame_and_title(doc, &mut out);
    if doc.show_bom {
        bom_table(doc, &mut out);
    }
    for v in &doc.views {
        out.owner = Some(v.id);
        view_items(doc, v.id, &mut out);
    }
    for d in &doc.dimensions {
        out.owner = Some(d.id);
        let layer = if doc.is_orphan(d.id) { Layer::Error } else { Layer::Dimension };
        dimension(doc, d, d.place, layer, &mut out);
    }
    Sheet2D { size, items: out.items }
}

/// Items of one dimension drawn at `place` (for previews while placing)
pub fn dimension_items(doc: &DrawingDocument, d: &Dimension, place: DVec2) -> Vec<Item> {
    let mut out = Out { items: Vec::new(), owner: Some(d.id) };
    dimension(doc, d, place, Layer::Dimension, &mut out);
    out.items
}

fn view_items(doc: &DrawingDocument, view: Id, out: &mut Out) {
    let Some(g) = doc.geometry(view) else { return };
    let s = |p: DVec2| doc.to_sheet(view, p).unwrap_or(p);
    let scale = doc.view_scale(view);
    for e in &g.edges {
        let layer = match (e.visible, e.class) {
            (false, _) => Layer::Hidden,
            (true, EdgeClass::Smooth) => Layer::Tangent,
            (true, _) => Layer::Visible,
        };
        edge_prim(e, &s, scale, layer, out);
    }
    // Center marks on whole visible circles
    for c in g.circles.iter().filter(|c| c.full && c.visible) {
        let r = c.radius * scale;
        let ext = r + 2.0;
        let cs = s(c.center);
        out.add(Prim::Line(cs - DVec2::new(ext, 0.0), cs + DVec2::new(ext, 0.0)), Layer::Center);
        out.add(Prim::Line(cs - DVec2::new(0.0, ext), cs + DVec2::new(0.0, ext)), Layer::Center);
    }
}

fn edge_prim(e: &ProjectedEdge, s: &dyn Fn(DVec2) -> DVec2, scale: f64, layer: Layer, out: &mut Out) {
    match e.curve {
        ProjCurve::Line => out.add(Prim::Line(s(e.a), s(e.b)), layer),
        ProjCurve::Arc { center, radius } => {
            let span = crate::arc_span(e, center);
            let c = s(center);
            let r = radius * scale;
            if span >= std::f64::consts::TAU - 1e-3 {
                out.add(Prim::Circle { center: c, radius: r }, layer);
                return;
            }
            let ang = |p: DVec2| (p - center).y.atan2((p - center).x).to_degrees();
            let (a0, a1) = (ang(e.a), ang(e.b));
            // Counter-clockwise when the samples turn left
            let mid = e.points.get(e.points.len() / 2).copied().unwrap_or(e.a);
            let ccw = (e.a - center).perp_dot(mid - center) >= 0.0;
            let (start, end) = if ccw { (a0, a1) } else { (a1, a0) };
            out.add(Prim::Arc { center: c, radius: r, start, end }, layer);
        }
        ProjCurve::Other => out.add(Prim::Polyline(e.points.iter().map(|p| s(*p)).collect()), layer),
    }
}

const TEXT_H: f64 = 3.5;
const GAP: f64 = 1.0;
const OVERSHOOT: f64 = 2.0;

/// Sheet position of a dimension's text
pub fn dimension_text_pos(doc: &DrawingDocument, d: &Dimension) -> Option<DVec2> {
    let items = dimension_items(doc, d, d.place);
    items.iter().find_map(|i| match &i.prim {
        Prim::Text { at, .. } => Some(*at),
        _ => None,
    })
}

fn dimension(doc: &DrawingDocument, d: &Dimension, place: DVec2, layer: Layer, out: &mut Out) {
    let Some(frame) = doc.frame(d.view) else { return };
    let s = |p: DVec2| doc.to_sheet(d.view, p);
    let Some(pp) = s(place) else { return };
    let label = doc.label(d);
    match d.kind {
        DimKind::Linear { a, b, dir } => {
            let (Some(pa), Some(pb)) = (s(frame.project(a)), s(frame.project(b))) else { return };
            let (u, n) = match dir {
                DimDir::Horizontal => (DVec2::X, DVec2::Y),
                DimDir::Vertical => (DVec2::Y, DVec2::NEG_X),
                DimDir::Aligned => {
                    let u = (pb - pa).normalize_or_zero();
                    if u == DVec2::ZERO {
                        return;
                    }
                    (u, u.perp())
                }
            };
            // Dimension line through the drop point, parallel to u
            let off = (pp - pa).dot(n);
            let da = pa + n * off;
            let db = pb + n * (off - (pb - pa).dot(n));
            for (p, q) in [(pa, da), (pb, db)] {
                let dirn = (q - p).normalize_or_zero();
                if (q - p).length() > GAP {
                    out.add(Prim::Line(p + dirn * GAP, q + dirn * OVERSHOOT), layer);
                }
            }
            out.add(Prim::Line(da, db), layer);
            let along = (db - da).normalize_or_zero();
            if along != DVec2::ZERO {
                out.add(Prim::Arrow { tip: da, dir: -along }, layer);
                out.add(Prim::Arrow { tip: db, dir: along }, layer);
            }
            // Text above the line, readable from the bottom or the right
            let mut angle = u.y.atan2(u.x).to_degrees();
            if angle > 90.0 + 1e-6 {
                angle -= 180.0;
            } else if angle <= -90.0 + 1e-6 {
                angle += 180.0;
            }
            let up = DVec2::new(-angle.to_radians().sin(), angle.to_radians().cos());
            let mid = (da + db) / 2.0 + up * GAP;
            out.text(mid, TEXT_H, label, Align::Center, angle, layer);
        }
        DimKind::Diameter { center, radius } | DimKind::Radius { center, radius } => {
            let (Some(c), r) = (s(center), radius * doc.view_scale(d.view)) else { return };
            let u = (pp - c).normalize_or(DVec2::new(1.0, 1.0).normalize());
            let on = c + u * r;
            let is_dia = matches!(d.kind, DimKind::Diameter { .. });
            if is_dia {
                // Across the circle, out to the text
                out.add(Prim::Line(c - u * r, pp), layer);
                out.add(Prim::Arrow { tip: on, dir: u }, layer);
                out.add(Prim::Arrow { tip: c - u * r, dir: -u }, layer);
            } else {
                out.add(Prim::Line(c, pp), layer);
                out.add(Prim::Arrow { tip: on, dir: u }, layer);
            }
            // Horizontal shoulder and text
            let w = text_width(&label, TEXT_H) + 2.0;
            let right = u.x >= 0.0;
            let end = pp + DVec2::new(if right { w } else { -w }, 0.0);
            out.add(Prim::Line(pp, end), layer);
            let at = DVec2::new((pp.x + end.x) / 2.0, pp.y + GAP);
            out.text(at, TEXT_H, label, Align::Center, 0.0, layer);
        }
    }
}

fn frame_and_title(doc: &DrawingDocument, out: &mut Out) {
    let size = doc.sheet.paper.size();
    let (x0, y0, x1, y1) = (MARGIN, MARGIN, size.x - MARGIN, size.y - MARGIN);
    let rect = |out: &mut Out, a: DVec2, b: DVec2, layer: Layer| {
        out.add(Prim::Polyline(vec![a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y), a]), layer);
    };
    rect(out, DVec2::new(x0, y0), DVec2::new(x1, y1), Layer::Frame);

    // Title block (ISO 7200 inspired), bottom right inside the frame
    let tx = x1 - TITLE_W;
    let ty = y0;
    rect(out, DVec2::new(tx, ty), DVec2::new(x1, ty + TITLE_H), Layer::Frame);
    let rows = [ty + 14.0, ty + 28.0];
    for y in rows {
        out.add(Prim::Line(DVec2::new(tx, y), DVec2::new(x1, y)), Layer::Thin);
    }
    let tb = &doc.sheet.title_block;
    let file = doc.source_file().file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    // (x, width, row index 0 = bottom, label, value, big)
    let cells: [(f64, f64, usize, &str, String, bool); 9] = [
        (0.0, 120.0, 2, "Titre", tb.title.clone(), true),
        (120.0, 60.0, 2, "Projection", String::new(), false),
        (0.0, 80.0, 1, "Fichier", file, false),
        (80.0, 40.0, 1, "Échelle", format_scale(doc.sheet.scale), false),
        (120.0, 30.0, 1, "Format", doc.sheet.paper.label().to_string(), false),
        (150.0, 30.0, 1, "Feuille", "1 / 1".to_string(), false),
        (0.0, 60.0, 0, "Dessiné par", tb.drawn_by.clone(), false),
        (60.0, 40.0, 0, "Date", tb.date.clone(), false),
        (100.0, 80.0, 0, "Matière", tb.material.clone(), false),
    ];
    for (x, w, row, label, value, big) in cells {
        let cy = ty + 14.0 * row as f64;
        let h = 14.0;
        if x > 0.0 {
            out.add(Prim::Line(DVec2::new(tx + x, cy), DVec2::new(tx + x, cy + h)), Layer::Thin);
        }
        out.text(DVec2::new(tx + x + 1.5, cy + h - 3.5), 2.0, label, Align::Left, 0.0, Layer::Thin);
        let vh = if big { 5.0 } else { 3.5 };
        let max = (w - 3.0) / text_width(&value, vh).max(1e-6);
        let vh = if max < 1.0 { vh * max } else { vh };
        out.text(DVec2::new(tx + x + 1.5, cy + 3.0), vh, value, Align::Left, 0.0, Layer::Thin);
    }
    out.text(DVec2::new(x1 - 2.0, ty + 3.0), 2.5, "NOVARIS CAD", Align::Right, 0.0, Layer::Thin);
    projection_symbol(doc.sheet.projection, DVec2::new(tx + 150.0, ty + 34.0), out);
}

/// ISO 128 projection symbol: a truncated cone seen from the side and end
fn projection_symbol(p: Projection, center: DVec2, out: &mut Out) {
    let (cone_x, circles_x) = match p {
        Projection::FirstAngle => (center.x - 8.0, center.x + 8.0),
        Projection::ThirdAngle => (center.x + 8.0, center.x - 8.0),
    };
    let (big, small, len) = (3.5, 2.0, 9.0);
    // The small end of the cone faces the circles in third angle
    let toward = (circles_x - cone_x).signum();
    let (sx, bx) = match p {
        Projection::ThirdAngle => (cone_x + toward * len / 2.0, cone_x - toward * len / 2.0),
        Projection::FirstAngle => (cone_x - toward * len / 2.0, cone_x + toward * len / 2.0),
    };
    let pts = vec![
        DVec2::new(bx, center.y - big),
        DVec2::new(sx, center.y - small),
        DVec2::new(sx, center.y + small),
        DVec2::new(bx, center.y + big),
        DVec2::new(bx, center.y - big),
    ];
    out.add(Prim::Polyline(pts), Layer::Thin);
    let c = DVec2::new(circles_x, center.y);
    out.add(Prim::Circle { center: c, radius: big }, Layer::Thin);
    out.add(Prim::Circle { center: c, radius: small }, Layer::Thin);
}

fn bom_table(doc: &DrawingDocument, out: &mut Out) {
    let Some(model) = doc.model() else { return };
    if model.bom.is_empty() {
        return;
    }
    let size = doc.sheet.paper.size();
    let x1 = size.x - MARGIN;
    let x0 = x1 - TITLE_W;
    let row_h = 7.0;
    let mut y = MARGIN + TITLE_H;
    let cols = [(0.0, "Rep."), (15.0, "Désignation"), (95.0, "Fichier"), (160.0, "Qté")];
    let row = |out: &mut Out, y: f64, values: [String; 4], header: bool| {
        out.add(
            Prim::Polyline(vec![
                DVec2::new(x0, y),
                DVec2::new(x1, y),
                DVec2::new(x1, y + row_h),
                DVec2::new(x0, y + row_h),
                DVec2::new(x0, y),
            ]),
            if header { Layer::Frame } else { Layer::Thin },
        );
        for (i, (x, _)) in cols.iter().enumerate() {
            if *x > 0.0 {
                out.add(Prim::Line(DVec2::new(x0 + x, y), DVec2::new(x0 + x, y + row_h)), Layer::Thin);
            }
            out.text(DVec2::new(x0 + x + 1.5, y + 2.0), 3.0, values[i].clone(), Align::Left, 0.0, Layer::Thin);
        }
    };
    row(out, y, cols.map(|(_, h)| h.to_string()), true);
    for r in &model.bom {
        y += row_h;
        let file = r.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        row(out, y, [r.item.to_string(), r.name.clone(), file, r.quantity.to_string()], false);
    }
}
