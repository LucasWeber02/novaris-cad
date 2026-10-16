use super::*;
use crate::render::{render, Prim};
use nova_assembly::Placement;
use nova_features::{EndCondition, Extrude, ExtrudeOp};
use nova_sketch::Sketch;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nvdrw-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// 100 x 60 plate, `depth` thick along +Z, optional Ø20 through hole in the middle
fn write_plate(path: &Path, depth: f64, hole: bool) {
    let mut doc = PartDocument::new("Plaque");
    let mut s = Sketch::new("");
    s.add_rectangle(0.0, 0.0, 100.0, 60.0).unwrap();
    if hole {
        let c = s.add_point(50.0, 30.0);
        s.add_circle(c, 10.0).unwrap();
    }
    let sk = doc.tree.add_sketch(s);
    doc.tree
        .add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth }, reverse: false, op: ExtrudeOp::Boss })
        .unwrap();
    doc.save(path).unwrap();
}

fn drawing(dir: &Path, projection: Projection) -> DrawingDocument {
    let part = dir.join("plaque.nvcad");
    write_plate(&part, 20.0, true);
    let mut d = DrawingDocument::new("Plaque", &part);
    d.sheet.projection = projection;
    d.update();
    assert!(d.source_error().is_none(), "{:?}", d.source_error());
    d.standard_views(true);
    d
}

fn view_named(d: &DrawingDocument, name: &str) -> Id {
    d.views.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("no view {name}")).id
}

/// Pick the vertex at a model point in a view, through the sheet
fn vertex(d: &DrawingDocument, view: Id, p: DVec3) -> DVec3 {
    let at = d.to_sheet(view, d.frame(view).unwrap().project(p)).unwrap();
    match d.pick(at, 1.0) {
        Some(Pick::Vertex { point, .. }) => point,
        other => panic!("expected a vertex at {p:?}, got {other:?}"),
    }
}

#[test]
fn standard_views_follow_the_projection_convention() {
    let dir = TempDir::new("views");
    let d = drawing(&dir.0, Projection::FirstAngle);
    let names: Vec<&str> = d.views.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["Vue de face", "Vue de dessus", "Vue de gauche", "Vue isométrique"]);
    let c = |n: &str| d.view_center(view_named(&d, n)).unwrap();
    let (front, top, left) = (c("Vue de face"), c("Vue de dessus"), c("Vue de gauche"));
    assert!((front.x - top.x).abs() < 1e-9 && top.y < front.y, "first angle: top view below");
    assert!((front.y - left.y).abs() < 1e-9 && left.x > front.x, "first angle: left view on the right");
    assert_eq!(d.sheet.scale, 1.0);
    // Every view fits on the sheet, above the title block
    let paper = d.sheet.paper.size();
    for v in &d.views {
        let (lo, hi) = d.view_rect(v.id).unwrap();
        assert!(lo.x > MARGIN && hi.x < paper.x - MARGIN && lo.y > MARGIN && hi.y < paper.y - MARGIN, "{} {lo:?} {hi:?}", v.name);
    }

    let third = drawing(&dir.0, Projection::ThirdAngle);
    let c3 = |n: &str| third.view_center(view_named(&third, n)).unwrap();
    assert!(c3("Vue de dessus").y > c3("Vue de face").y, "third angle: top view above");
    assert!(c3("Vue de droite").x > c3("Vue de face").x);

    // Switching the convention keeps each view's content and flips its side
    let mut d = d;
    let mut flipped = d.clone();
    flipped.set_projection(Projection::ThirdAngle);
    let t = view_named(&flipped, "Vue de dessus");
    assert!(flipped.view_center(t).unwrap().y > flipped.view_center(view_named(&flipped, "Vue de face")).unwrap().y);
    assert_eq!(flipped.frame(t).unwrap(), Orientation::Top.frame());

    // Projected views keep their alignment when moved
    let top_id = view_named(&d, "Vue de dessus");
    d.move_view(top_id, DVec2::new(30.0, -5.0));
    assert!((d.view_center(top_id).unwrap().x - front.x).abs() < 1e-9);
    let f = view_named(&d, "Vue de face");
    d.move_view(f, DVec2::new(-3.0, 2.0));
    assert!((d.view_center(top_id).unwrap().x - (front.x - 3.0)).abs() < 1e-9, "children follow their parent");
}

#[test]
fn hidden_lines_and_circles() {
    let dir = TempDir::new("hidden");
    let d = drawing(&dir.0, Projection::FirstAngle);
    let front = d.geometry(view_named(&d, "Vue de face")).unwrap();
    assert!(front.circles.iter().any(|c| c.full && c.visible && (c.radius - 10.0).abs() < 1e-9));
    assert!((front.hi - front.lo - DVec2::new(100.0, 60.0)).length() < 1e-6);
    let top = d.geometry(view_named(&d, "Vue de dessus")).unwrap();
    assert!(top.edges.iter().filter(|e| !e.visible).count() >= 2, "the hole shows as hidden lines from the top");
    let iso = d.view(view_named(&d, "Vue isométrique")).unwrap();
    assert!(!iso.show_hidden);
    // Center marks and hidden lines reach the sheet
    let sheet = render(&d);
    assert!(sheet.items.iter().any(|i| i.layer == render::Layer::Hidden));
    assert!(sheet.items.iter().any(|i| i.layer == render::Layer::Center));
}

#[test]
fn dimensions_measure_and_follow_the_model() {
    let dir = TempDir::new("dims");
    let mut d = drawing(&dir.0, Projection::FirstAngle);
    let front = view_named(&d, "Vue de face");
    let top = view_named(&d, "Vue de dessus");

    let a = vertex(&d, front, DVec3::new(0.0, 0.0, 20.0));
    let b = vertex(&d, front, DVec3::new(100.0, 0.0, 20.0));
    assert!(a.z > 19.0, "the vertex nearest to the viewer is picked");
    let place = DVec2::new(50.0, -15.0);
    assert_eq!(d.infer_dir(front, a, b, place), DimDir::Horizontal);
    let width = d.add_dimension(front, DimKind::Linear { a, b, dir: DimDir::Horizontal }, place);

    // Thickness in the top view (its y axis is -Z)
    let t0 = vertex(&d, top, DVec3::new(100.0, 60.0, 0.0));
    let t1 = vertex(&d, top, DVec3::new(100.0, 60.0, 20.0));
    assert_eq!(d.infer_dir(top, t0, t1, DVec2::new(115.0, -10.0)), DimDir::Vertical);
    let thick = d.add_dimension(top, DimKind::Linear { a: t0, b: t1, dir: DimDir::Vertical }, DVec2::new(115.0, -10.0));

    let circle = match d.pick(d.to_sheet(front, DVec2::new(50.0, 40.0)).unwrap(), 1.0) {
        Some(Pick::Circle { circle, .. }) => circle,
        other => panic!("expected the hole, got {other:?}"),
    };
    let dia = d.add_dimension(front, DimKind::Diameter { center: circle.center, radius: circle.radius }, DVec2::new(80.0, 50.0));

    let label = |d: &DrawingDocument, id: Id| d.label(d.dimensions.iter().find(|x| x.id == id).unwrap());
    assert_eq!(label(&d, width), "100");
    assert_eq!(label(&d, thick), "20");
    assert_eq!(label(&d, dia), "Ø20");
    assert!(d.orphans.is_empty());
    // A dimension's text can be picked
    let t = render::dimension_text_pos(&d, &d.dimensions[0]).unwrap();
    assert_eq!(d.pick(t, 1.0), Some(Pick::Dimension(width)));

    // The plate gets thicker: the drawing follows
    write_plate(&dir.0.join("plaque.nvcad"), 30.0, true);
    d.update();
    assert_eq!(label(&d, width), "100");
    assert_eq!(label(&d, thick), "30");
    assert!(d.orphans.is_empty());

    // The hole disappears: its dimension is orphaned, the others hold
    write_plate(&dir.0.join("plaque.nvcad"), 30.0, false);
    d.update();
    assert!(d.is_orphan(dia));
    assert!(!d.is_orphan(width) && !d.is_orphan(thick));
    assert!(render(&d).items.iter().any(|i| i.layer == render::Layer::Error));
}

#[test]
fn save_and_reopen() {
    let dir = TempDir::new("file");
    let mut d = drawing(&dir.0, Projection::FirstAngle);
    let front = view_named(&d, "Vue de face");
    let a = vertex(&d, front, DVec3::new(0.0, 0.0, 20.0));
    let b = vertex(&d, front, DVec3::new(0.0, 60.0, 20.0));
    d.add_dimension(front, DimKind::Linear { a, b, dir: DimDir::Vertical }, DVec2::new(-15.0, 30.0));
    let sub = dir.0.join("plans");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("plaque.nvdrw");
    d.save(&file).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"../plaque.nvcad\""));
    let back = DrawingDocument::open(&file).unwrap();
    assert_eq!(back.views.len(), 4);
    assert_eq!(back.label(&back.dimensions[0]), "60");
    assert_eq!(back.view_center(front), d.view_center(front));
    assert!(DrawingDocument::from_json("{}", &dir.0).is_err());
}

#[test]
fn pdf_is_well_formed() {
    let dir = TempDir::new("pdf");
    let mut d = drawing(&dir.0, Projection::FirstAngle);
    d.sheet.title_block.drawn_by = "Équipe méthodes".into();
    let front = view_named(&d, "Vue de face");
    let a = vertex(&d, front, DVec3::new(0.0, 0.0, 20.0));
    let b = vertex(&d, front, DVec3::new(100.0, 0.0, 20.0));
    d.add_dimension(front, DimKind::Linear { a, b, dir: DimDir::Horizontal }, DVec2::new(50.0, -15.0));
    let bytes = pdf::write(&render(&d));
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    let text = String::from_utf8_lossy(&bytes);
    // startxref points at the xref table, whose entries point at objects
    let start: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
    assert!(bytes[start..].starts_with(b"xref"));
    let table = String::from_utf8_lossy(&bytes[start..]).to_string();
    for (k, line) in table.lines().skip(3).take(6).enumerate() {
        let off: usize = line[..10].parse().unwrap();
        assert!(bytes[off..].starts_with(format!("{} 0 obj", k + 1).as_bytes()), "object {}", k + 1);
    }
    assert!(text.contains("(100) Tj"));
    assert!(text.contains("(Plaque) Tj"));
    // "É" is one WinAnsi byte
    assert!(bytes.windows(2).any(|w| w == [b'(', 0xC9]));
    let file = dir.0.join("plaque.pdf");
    d.export_pdf(&file).unwrap();
    assert!(std::fs::metadata(&file).unwrap().len() > 1000);
}

#[test]
fn dxf_has_layers_and_entities() {
    let dir = TempDir::new("dxf");
    let mut d = drawing(&dir.0, Projection::FirstAngle);
    let front = view_named(&d, "Vue de face");
    let g = d.geometry(front).unwrap();
    let c = g.circles[0];
    d.add_dimension(front, DimKind::Diameter { center: c.center, radius: c.radius }, DVec2::new(80.0, 50.0));
    let bytes = dxf::write(&render(&d));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("AC1009"));
    for l in render::Layer::ALL {
        assert!(text.contains(&format!("  2\n{}\n", l.name())), "layer {}", l.name());
    }
    assert!(text.matches("\n  0\nLINE\n").count() > 10);
    assert!(text.contains("\n  0\nCIRCLE\n"));
    assert!(text.contains("%%c20"));
    assert!(text.trim_end().ends_with("EOF"));
    // group codes and values alternate: an even number of lines
    assert_eq!(text.lines().count() % 2, 0);
}

#[test]
fn assembly_drawing_with_bill_of_materials() {
    let dir = TempDir::new("asm");
    let plate = dir.0.join("plaque.nvcad");
    write_plate(&plate, 20.0, true);
    let mut asm = AssemblyDocument::new("Ensemble");
    asm.set_base_dir(&dir.0);
    asm.insert_component(&plate, Placement::default()).unwrap();
    asm.insert_component(&plate, Placement::from_translation(DVec3::new(0.0, 0.0, 40.0))).unwrap();
    let file = dir.0.join("ensemble.nvasm");
    asm.save(&file).unwrap();

    let mut d = DrawingDocument::new("Ensemble", &file);
    d.sheet.paper = Paper::A3;
    d.show_bom = true;
    d.update();
    assert!(d.model().unwrap().is_assembly);
    d.standard_views(true);
    assert_eq!(d.model().unwrap().bom[0].quantity, 2);
    let top = d.geometry(view_named(&d, "Vue de dessus")).unwrap();
    // two plates 20 thick, 20 apart: 60 high in the top view
    assert!(((top.hi - top.lo).y - 60.0).abs() < 1e-6);
    let texts: Vec<String> = render(&d)
        .items
        .iter()
        .filter_map(|i| match &i.prim {
            Prim::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t == "Qté"));
    assert!(texts.iter().any(|t| t == "plaque.nvcad"));
}

#[test]
fn number_formats() {
    assert_eq!(format_mm(100.0), "100");
    assert_eq!(format_mm(12.5), "12,5");
    assert_eq!(format_mm(0.333), "0,33");
    assert_eq!(format_scale(0.5), "1:2");
    assert_eq!(format_scale(2.0), "2:1");
    assert_eq!(format_scale(1.0), "1:1");
}
