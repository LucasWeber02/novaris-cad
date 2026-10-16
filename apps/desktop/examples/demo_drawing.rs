//! Writes a demo drawing of the demo plate: three views, an isometric view
//! and a few dimensions, plus its PDF and DXF exports. With a second
//! argument (an .nvasm file), also draws that assembly with its BOM.
//!
//! cargo run -p novaris-desktop --example demo_drawing -- demo_plan
//! cargo run -p novaris-desktop --example demo_drawing -- demo_plan demo_assemblage/ensemble.nvasm
//! cargo run -p novaris-desktop -- demo_plan/plaque.nvdrw

use glam::{DVec2, DVec3};
use nova_core::Id;
use nova_drawing::{DimDir, DimKind, DrawingDocument, Paper, Pick};
use nova_features::{EndCondition, Extrude, ExtrudeOp};
use nova_io::PartDocument;
use nova_sketch::{Plane, Sketch};
use std::path::{Path, PathBuf};

fn plate(path: &Path) {
    let mut doc = PartDocument::new("Plaque");
    let mut base = Sketch::new("");
    base.add_rectangle(-50.0, -30.0, 100.0, 60.0).unwrap();
    let base = doc.tree.add_sketch(base);
    doc.tree
        .add_extrude(Extrude { sketch: base, end: EndCondition::Blind { depth: 12.0 }, reverse: false, op: ExtrudeOp::Boss })
        .unwrap();
    let mut boss = Sketch::on_plane("", Plane::front().offset(12.0));
    boss.add_rectangle(-35.0, -15.0, 30.0, 30.0).unwrap();
    let boss = doc.tree.add_sketch(boss);
    doc.tree
        .add_extrude(Extrude { sketch: boss, end: EndCondition::Blind { depth: 18.0 }, reverse: false, op: ExtrudeOp::Boss })
        .unwrap();
    let mut holes = Sketch::new("");
    for x in [20.0, 35.0] {
        let c = holes.add_point(x, 0.0);
        holes.add_circle(c, 6.0).unwrap();
    }
    let holes = doc.tree.add_sketch(holes);
    doc.tree
        .add_extrude(Extrude { sketch: holes, end: EndCondition::ThroughAll, reverse: false, op: ExtrudeOp::Cut })
        .unwrap();
    doc.save(path).unwrap();
}

fn view(d: &DrawingDocument, name: &str) -> Id {
    d.views.iter().find(|v| v.name == name).expect("view exists").id
}

fn vertex(d: &DrawingDocument, v: Id, p: DVec3) -> DVec3 {
    let at = d.to_sheet(v, d.frame(v).unwrap().project(p)).unwrap();
    match d.pick(at, 1.0) {
        Some(Pick::Vertex { point, .. }) => point,
        other => panic!("no vertex at {p:?}: {other:?}"),
    }
}

fn linear(d: &mut DrawingDocument, v: Id, a: DVec3, b: DVec3, dir: DimDir, place: DVec2) {
    let (a, b) = (vertex(d, v, a), vertex(d, v, b));
    d.add_dimension(v, DimKind::Linear { a, b, dir }, place);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "demo_plan".into()));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("plaque.nvcad");
    plate(&part);

    let mut d = DrawingDocument::new("Plaque", &part);
    d.sheet.title_block.material = "S235".into();
    d.sheet.title_block.date = "16/10/2026".into();
    d.update();
    d.standard_views(true);
    let front = view(&d, "Vue de face");
    let top = view(&d, "Vue de dessus");
    linear(&mut d, front, DVec3::new(-50.0, -30.0, 12.0), DVec3::new(50.0, -30.0, 12.0), DimDir::Horizontal, DVec2::new(0.0, -42.0));
    linear(&mut d, front, DVec3::new(-50.0, -30.0, 12.0), DVec3::new(-50.0, 30.0, 12.0), DimDir::Vertical, DVec2::new(-58.0, 0.0));
    linear(&mut d, top, DVec3::new(50.0, 30.0, 0.0), DVec3::new(50.0, 30.0, 12.0), DimDir::Vertical, DVec2::new(60.0, 0.0));
    linear(&mut d, top, DVec3::new(-35.0, 15.0, 12.0), DVec3::new(-35.0, 15.0, 30.0), DimDir::Vertical, DVec2::new(-58.0, 0.0));
    let hole = d.geometry(front).unwrap().circles.iter().find(|c| c.full && c.center.x > 30.0).copied().unwrap();
    d.add_dimension(front, DimKind::Diameter { center: hole.center, radius: hole.radius }, DVec2::new(55.0, 25.0));

    let file = dir.join("plaque.nvdrw");
    d.save(&file).unwrap();
    d.export_pdf(&dir.join("plaque.pdf")).unwrap();
    d.export_dxf(&dir.join("plaque.dxf")).unwrap();
    for dim in &d.dimensions {
        println!("{:<4} {}", dim.name, d.label(dim));
    }
    println!("saved {} (+ .pdf, .dxf), scale {}", file.display(), nova_drawing::format_scale(d.sheet.scale));

    if let Some(asm) = args.next() {
        let mut a = DrawingDocument::new("Ensemble", Path::new(&asm));
        a.sheet.paper = Paper::A3;
        a.show_bom = true;
        a.update();
        if let Some(e) = a.source_error() {
            eprintln!("{asm}: {e}");
            return;
        }
        a.standard_views(true);
        let file = dir.join("ensemble.nvdrw");
        a.save(&file).unwrap();
        a.export_pdf(&dir.join("ensemble.pdf")).unwrap();
        a.export_dxf(&dir.join("ensemble.dxf")).unwrap();
        println!("saved {} (+ .pdf, .dxf)", file.display());
    }
}
