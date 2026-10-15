//! Writes a demo part: a plate with a through hole and a boss on top.
//!
//! cargo run -p novaris-desktop --example demo_part -- plaque.nvcad

use nova_features::{EndCondition, Extrude, ExtrudeOp};
use nova_io::PartDocument;
use nova_sketch::{Constraint, Plane, Sketch};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "plaque.nvcad".into());
    let mut doc = PartDocument::new("Plaque");

    let mut base = Sketch::new("");
    let p = base.add_rectangle(-50.0, -30.0, 100.0, 60.0).unwrap();
    base.add_constraint(Constraint::fixed(p[0], -50.0, -30.0));
    base.add_constraint(Constraint::distance(p[0], p[1], 100.0));
    base.add_constraint(Constraint::distance(p[1], p[2], 60.0));
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

    doc.tree.rebuild();
    for (f, s) in doc.tree.features().iter().zip(doc.tree.states()) {
        println!("{:<24} {:?}", f.name, s);
    }
    println!("volume: {:.1} mm3", doc.tree.body().unwrap().volume());
    doc.save(&path).unwrap();
    println!("saved {path}");
}
