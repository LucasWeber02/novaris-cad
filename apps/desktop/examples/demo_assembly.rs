//! Writes a demo assembly: a base plate with two holes, two pins and a
//! cover plate, positioned with mates.
//!
//! cargo run -p novaris-desktop --example demo_assembly -- demo_assemblage
//! cargo run -p novaris-desktop -- demo_assemblage/ensemble.nvasm

use glam::{DQuat, DVec3};
use nova_assembly::{AssemblyDocument, FaceRef, MateKind, Placement};
use nova_core::Id;
use nova_features::{EndCondition, Extrude, ExtrudeOp};
use nova_io::PartDocument;
use nova_kernel::{FaceInfo, Surface};
use nova_sketch::Sketch;
use std::path::Path;

/// Plate w x h x thickness with two holes of radius r on its middle line
fn plate(path: &Path, name: &str, w: f64, h: f64, thickness: f64, r: f64) {
    let mut doc = PartDocument::new(name);
    let mut s = Sketch::new("");
    s.add_rectangle(0.0, 0.0, w, h).unwrap();
    for x in [w / 4.0, 3.0 * w / 4.0] {
        let c = s.add_point(x, h / 2.0);
        s.add_circle(c, r).unwrap();
    }
    let sk = doc.tree.add_sketch(s);
    doc.tree
        .add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: thickness }, reverse: false, op: ExtrudeOp::Boss })
        .unwrap();
    doc.save(path).unwrap();
}

fn pin(path: &Path, r: f64, length: f64) {
    let mut doc = PartDocument::new("Axe");
    let mut s = Sketch::new("");
    let c = s.add_point(0.0, 0.0);
    s.add_circle(c, r).unwrap();
    let sk = doc.tree.add_sketch(s);
    doc.tree
        .add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: length }, reverse: false, op: ExtrudeOp::Boss })
        .unwrap();
    doc.save(path).unwrap();
}

fn face(asm: &AssemblyDocument, c: Id, pred: impl Fn(&FaceInfo) -> bool) -> FaceRef {
    let i = asm.part(c).unwrap().faces.iter().position(pred).expect("face exists");
    asm.face_ref(c, i).unwrap()
}

fn plane(normal: DVec3, coord: f64) -> impl Fn(&FaceInfo) -> bool {
    move |f| matches!(f.surface, Surface::Plane { normal: n, point } if n.dot(normal) > 0.999 && (point.dot(normal) - coord).abs() < 1e-6)
}

/// Cylindrical face whose axis passes at (x, y)
fn hole(x: f64, y: f64) -> impl Fn(&FaceInfo) -> bool {
    move |f| matches!(f.surface, Surface::Cylinder { origin, .. } if (origin.x - x).abs() < 1e-6 && (origin.y - y).abs() < 1e-6)
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "demo_assemblage".into()));
    std::fs::create_dir_all(&dir).unwrap();
    plate(&dir.join("socle.nvcad"), "Socle", 120.0, 80.0, 15.0, 8.0);
    plate(&dir.join("couvercle.nvcad"), "Couvercle", 120.0, 80.0, 8.0, 8.0);
    pin(&dir.join("axe.nvcad"), 8.0, 45.0);

    let mut asm = AssemblyDocument::new("Ensemble");
    asm.set_base_dir(&dir);
    let base = asm.insert_component(&dir.join("socle.nvcad"), Placement::default()).unwrap();
    let tilted = |x: f64, z: f64| Placement {
        rotation: DQuat::from_axis_angle(DVec3::new(1.0, 0.3, 0.2).normalize(), 0.6),
        translation: DVec3::new(x, -60.0, z),
    };
    let pin1 = asm.insert_component(&dir.join("axe.nvcad"), tilted(160.0, 30.0)).unwrap();
    let pin2 = asm.insert_component(&dir.join("axe.nvcad"), tilted(200.0, 30.0)).unwrap();
    let cover = asm.insert_component(&dir.join("couvercle.nvcad"), tilted(0.0, 120.0)).unwrap();

    // Pin 1: in the first hole, flush with the bottom of the base
    asm.add_mate(MateKind::Concentric, face(&asm, base, hole(30.0, 40.0)), face(&asm, pin1, |f| matches!(f.surface, Surface::Cylinder { .. })), false)
        .unwrap();
    asm.add_mate(MateKind::Coincident, face(&asm, base, plane(-DVec3::Z, 0.0)), face(&asm, pin1, plane(-DVec3::Z, 0.0)), true)
        .unwrap();
    // Pin 2: only concentric, so it can still slide and turn: "(-)"
    asm.add_mate(MateKind::Concentric, face(&asm, base, hole(90.0, 40.0)), face(&asm, pin2, |f| matches!(f.surface, Surface::Cylinder { .. })), false)
        .unwrap();
    // Cover: 20 mm above the base, on pin 1, edges parallel to the base
    asm.add_mate(
        MateKind::Distance { value: 20.0 },
        face(&asm, base, plane(DVec3::Z, 15.0)),
        face(&asm, cover, plane(-DVec3::Z, 0.0)),
        false,
    )
    .unwrap();
    asm.add_mate(MateKind::Concentric, face(&asm, pin1, |f| matches!(f.surface, Surface::Cylinder { .. })), face(&asm, cover, hole(30.0, 40.0)), false)
        .unwrap();
    asm.add_mate(MateKind::Parallel, face(&asm, base, plane(-DVec3::Y, 0.0)), face(&asm, cover, plane(-DVec3::Y, 0.0)), true)
        .unwrap();

    for c in &asm.components {
        println!("{}{:<14} {:?}", asm.component_state(c.id).prefix(), c.name, c.placement.translation);
    }
    for m in &asm.mates {
        println!("  {:<16} {:?}", m.name, asm.mate_state(m.id));
    }
    for row in asm.bom() {
        println!("BOM {} {:<10} x{}  {:.1} mm3", row.item, row.name, row.quantity, row.volume);
    }
    println!("interferences: {}", asm.interferences().len());
    let path = dir.join("ensemble.nvasm");
    asm.save(&path).unwrap();
    println!("saved {}", path.display());
}
