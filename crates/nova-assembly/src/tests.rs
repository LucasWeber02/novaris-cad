use super::*;
use nova_features::{EndCondition, Extrude, ExtrudeOp};
use nova_sketch::Sketch;
use std::f64::consts::PI;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nvasm-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Block w x h on the front plane, extruded `d` along +Z, with an optional
/// through hole of radius `hole` at its center
fn write_block(path: &Path, name: &str, w: f64, h: f64, d: f64, hole: Option<f64>) {
    let mut doc = PartDocument::new(name);
    let mut s = Sketch::new("");
    s.add_rectangle(0.0, 0.0, w, h).unwrap();
    if let Some(r) = hole {
        let c = s.add_point(w / 2.0, h / 2.0);
        s.add_circle(c, r).unwrap();
    }
    let sk = doc.tree.add_sketch(s);
    doc.tree
        .add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: d }, reverse: false, op: ExtrudeOp::Boss })
        .unwrap();
    doc.save(path).unwrap();
}

/// Cylinder of radius r around the Z axis, from z = 0 to z = length
fn write_pin(path: &Path, r: f64, length: f64) {
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

/// Face of a component's part matching a predicate
fn face(asm: &AssemblyDocument, c: Id, pred: impl Fn(&FaceInfo) -> bool) -> FaceRef {
    let i = asm.part(c).unwrap().faces.iter().position(pred).expect("face exists");
    asm.face_ref(c, i).unwrap()
}

fn plane(normal: DVec3, coord: f64) -> impl Fn(&FaceInfo) -> bool {
    move |f| match f.surface {
        Surface::Plane { normal: n, point } => n.dot(normal) > 0.999 && (point.dot(normal) - coord).abs() < 1e-6,
        _ => false,
    }
}

fn cylinder(f: &FaceInfo) -> bool {
    matches!(f.surface, Surface::Cylinder { .. })
}

fn world_bounds(asm: &AssemblyDocument, c: Id) -> (DVec3, DVec3) {
    asm.world_solid(c).unwrap().bounds().unwrap()
}

fn near(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-5
}

/// Base plate 100 x 60 x 20 with a Ø20 hole, fixed at the origin
fn base(dir: &Path) -> (AssemblyDocument, Id) {
    let plate = dir.join("plaque.nvcad");
    write_block(&plate, "Plaque", 100.0, 60.0, 20.0, Some(10.0));
    let mut asm = AssemblyDocument::new("Ensemble");
    let a = asm.insert_component(&plate, Placement::default()).unwrap();
    (asm, a)
}

#[test]
fn first_component_is_fixed_and_names_count_up() {
    let dir = TempDir::new("names");
    let (mut asm, a) = base(&dir.0);
    let b = asm.insert_component(&dir.0.join("plaque.nvcad"), Placement::default()).unwrap();
    assert_eq!(asm.component(a).unwrap().name, "Plaque<1>");
    assert_eq!(asm.component(b).unwrap().name, "Plaque<2>");
    assert_eq!(asm.component_state(a), ComponentState::Fixed);
    assert_eq!(asm.component_state(b), ComponentState::UnderDefined(6));
    assert!(asm.insert_component(&dir.0.join("absent.nvcad"), Placement::default()).is_err());
}

#[test]
fn coincident_and_distances_place_a_block_exactly() {
    let dir = TempDir::new("block");
    let (mut asm, a) = base(&dir.0);
    let cube = dir.0.join("cube.nvcad");
    write_block(&cube, "Cube", 40.0, 40.0, 10.0, None);
    // Start far away and tilted
    let start = Placement {
        rotation: DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 0.5).normalize(), 0.7),
        translation: DVec3::new(150.0, -40.0, 60.0),
    };
    let b = asm.insert_component(&cube, start).unwrap();

    // Cube bottom on plate top (face against face)
    let m1 = asm
        .add_mate(MateKind::Coincident, face(&asm, a, plane(DVec3::Z, 20.0)), face(&asm, b, plane(-DVec3::Z, 0.0)), false)
        .unwrap();
    assert_eq!(asm.component_state(b), ComponentState::UnderDefined(3));
    // Front faces flush
    asm.add_mate(MateKind::Coincident, face(&asm, a, plane(-DVec3::Y, 0.0)), face(&asm, b, plane(-DVec3::Y, 0.0)), true)
        .unwrap();
    // Left faces 30 mm apart, cube toward +X
    asm.add_mate(
        MateKind::Distance { value: -30.0 },
        face(&asm, a, plane(-DVec3::X, 0.0)),
        face(&asm, b, plane(-DVec3::X, 0.0)),
        true,
    )
    .unwrap();
    assert_eq!(asm.component_state(b), ComponentState::FullyDefined);
    let (lo, hi) = world_bounds(&asm, b);
    assert!(near(lo, DVec3::new(30.0, 0.0, 20.0)), "{lo:?}");
    assert!(near(hi, DVec3::new(70.0, 40.0, 30.0)), "{hi:?}");
    assert_eq!(asm.mate(m1).unwrap().name, "Coïncidente1");
    assert_eq!(asm.mates[2].name, "Distance1");
    assert_eq!(asm.summary().unsatisfied, 0);
    assert!(asm.interferences().is_empty(), "touching is not interfering");
}

#[test]
fn pin_in_hole_keeps_its_rotation_free() {
    let dir = TempDir::new("pin");
    let (mut asm, a) = base(&dir.0);
    let pin = dir.0.join("axe.nvcad");
    write_pin(&pin, 10.0, 50.0);
    let b = asm
        .insert_component(&pin, Placement { rotation: DQuat::from_rotation_x(PI / 2.0), translation: DVec3::new(-80.0, 0.0, 0.0) })
        .unwrap();
    asm.add_mate(MateKind::Concentric, face(&asm, a, cylinder), face(&asm, b, cylinder), false).unwrap();
    assert_eq!(asm.component_state(b), ComponentState::UnderDefined(2), "slides and turns");
    // Pin end flush with the plate bottom
    asm.add_mate(MateKind::Coincident, face(&asm, a, plane(-DVec3::Z, 0.0)), face(&asm, b, plane(-DVec3::Z, 0.0)), true)
        .unwrap();
    assert_eq!(asm.component_state(b), ComponentState::UnderDefined(1), "still turns about its axis");
    let (lo, hi) = world_bounds(&asm, b);
    assert!(near(lo, DVec3::new(40.0, 20.0, 0.0)), "{lo:?}");
    assert!(near(hi, DVec3::new(60.0, 40.0, 50.0)), "{hi:?}");
    assert!(asm.interferences().is_empty(), "same diameter: no interference");
}

#[test]
fn contradictory_mate_is_refused_and_nothing_moves() {
    let dir = TempDir::new("conflict");
    let (mut asm, a) = base(&dir.0);
    let cube = dir.0.join("cube.nvcad");
    write_block(&cube, "Cube", 40.0, 40.0, 10.0, None);
    let b = asm.insert_component(&cube, Placement::from_translation(DVec3::new(0.0, 0.0, 50.0))).unwrap();
    asm.add_mate(MateKind::Coincident, face(&asm, a, plane(DVec3::Z, 20.0)), face(&asm, b, plane(-DVec3::Z, 0.0)), false)
        .unwrap();
    let placed = asm.component(b).unwrap().placement;
    let err = asm
        .add_mate(
            MateKind::Distance { value: 5.0 },
            face(&asm, a, plane(DVec3::Z, 20.0)),
            face(&asm, b, plane(-DVec3::Z, 0.0)),
            false,
        )
        .unwrap_err();
    assert!(err.to_string().contains("surcontraint"));
    assert_eq!(asm.mates.len(), 1);
    assert_eq!(asm.component(b).unwrap().placement, placed);
    // Same face twice on one component is refused too
    let f = face(&asm, b, plane(-DVec3::Z, 0.0));
    assert!(asm.add_mate(MateKind::Coincident, f, f, false).is_err());
    // Concentric needs cylinders
    assert!(asm
        .add_mate(MateKind::Concentric, face(&asm, a, cylinder), face(&asm, b, plane(DVec3::Z, 10.0)), false)
        .is_err());
}

#[test]
fn parallel_perpendicular_and_angle() {
    let dir = TempDir::new("angles");
    let (mut asm, a) = base(&dir.0);
    let cube = dir.0.join("cube.nvcad");
    write_block(&cube, "Cube", 40.0, 40.0, 10.0, None);
    let b = asm.insert_component(&cube, Placement::from_translation(DVec3::new(0.0, 0.0, 50.0))).unwrap();
    asm.add_mate(MateKind::Coincident, face(&asm, a, plane(DVec3::Z, 20.0)), face(&asm, b, plane(-DVec3::Z, 0.0)), false)
        .unwrap();
    let front_a = face(&asm, a, plane(-DVec3::Y, 0.0));
    let front_b = face(&asm, b, plane(-DVec3::Y, 0.0));
    let m = asm.add_mate(MateKind::Angle { degrees: 30.0 }, front_a, front_b, true).unwrap();
    let n = asm.component(b).unwrap().placement.vector(-DVec3::Y);
    assert!((n.dot(-DVec3::Y) - (30f64).to_radians().cos()).abs() < 1e-6);
    asm.edit_mate(m, MateKind::Perpendicular, true).unwrap();
    let n = asm.component(b).unwrap().placement.vector(-DVec3::Y);
    assert!(n.dot(DVec3::Y).abs() < 1e-6 && n.z.abs() < 1e-6);
    asm.edit_mate(m, MateKind::Parallel, true).unwrap();
    let n = asm.component(b).unwrap().placement.vector(-DVec3::Y);
    assert!(near(n, -DVec3::Y));
}

#[test]
fn mates_follow_part_edits() {
    let dir = TempDir::new("edit");
    let (mut asm, a) = base(&dir.0);
    let cube = dir.0.join("cube.nvcad");
    write_block(&cube, "Cube", 40.0, 40.0, 10.0, None);
    let b = asm.insert_component(&cube, Placement::from_translation(DVec3::new(10.0, 10.0, 80.0))).unwrap();
    asm.add_mate(MateKind::Coincident, face(&asm, a, plane(DVec3::Z, 20.0)), face(&asm, b, plane(-DVec3::Z, 0.0)), false)
        .unwrap();
    assert!((world_bounds(&asm, b).0.z - 20.0).abs() < 1e-6);
    // The plate gets thicker: the cube must follow its top face
    write_block(&dir.0.join("plaque.nvcad"), "Plaque", 100.0, 60.0, 35.0, Some(10.0));
    let s = asm.reload_parts();
    assert_eq!(s.unsatisfied, 0);
    assert_eq!(asm.mate_state(asm.mates[0].id), Some(&MateState::Satisfied));
    assert!((world_bounds(&asm, b).0.z - 35.0).abs() < 1e-6);
}

#[test]
fn save_reopen_bom_interference_and_step() {
    let dir = TempDir::new("file");
    let (mut asm, a) = base(&dir.0);
    let pin = dir.0.join("axe.nvcad");
    write_pin(&pin, 12.0, 30.0);
    let b = asm.insert_component(&pin, Placement::from_translation(DVec3::new(50.0, 30.0, -5.0))).unwrap();
    let c = asm.insert_component(&pin, Placement::from_translation(DVec3::new(200.0, 0.0, 0.0))).unwrap();
    assert_eq!(asm.component(c).unwrap().name, "Axe<2>");

    // Ø24 pin in a Ø20 hole: a 20 mm long ring of interference
    let found = asm.interferences();
    assert_eq!(found.len(), 1);
    assert!((found[0].a == a && found[0].b == b) || (found[0].a == b && found[0].b == a));
    let ring = PI * (144.0 - 100.0) * 20.0;
    assert!((found[0].volume - ring).abs() < 1e-6, "{}", found[0].volume);

    let bom = asm.bom();
    assert_eq!(bom.len(), 2);
    assert_eq!((bom[0].name.as_str(), bom[0].quantity), ("Plaque", 1));
    assert_eq!((bom[1].name.as_str(), bom[1].quantity), ("Axe", 2));

    let sub = dir.0.join("assemblages");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("ensemble.nvasm");
    asm.save(&file).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"../axe.nvcad\""), "paths are relative to the assembly file");
    let back = AssemblyDocument::open(&file).unwrap();
    assert_eq!(back.components.len(), 3);
    assert_eq!(back.component(b).unwrap().placement, asm.component(b).unwrap().placement);
    assert!(back.part(b).unwrap().solid.is_some());

    let step = dir.0.join("ensemble.step");
    back.export_step(&step).unwrap();
    let text = std::fs::read_to_string(&step).unwrap();
    assert_eq!(text.matches("MANIFOLD_SOLID_BREP").count(), 3);

    assert!(AssemblyDocument::from_json("{}", &dir.0).is_err());
}

#[test]
fn free_component_without_mates_stays_put() {
    let dir = TempDir::new("still");
    let (mut asm, _) = base(&dir.0);
    let p = Placement { rotation: DQuat::from_rotation_z(0.3), translation: DVec3::new(5.0, 6.0, 7.0) };
    let b = asm.insert_component(&dir.0.join("plaque.nvcad"), p).unwrap();
    asm.solve();
    assert_eq!(asm.component(b).unwrap().placement, p);
    asm.move_component(b, &DMat4::from_translation(DVec3::X)).unwrap();
    assert!(near(asm.component(b).unwrap().placement.translation, DVec3::new(6.0, 6.0, 7.0)));
}

#[test]
fn relative_paths() {
    assert_eq!(relative_to(Path::new("/a/b/p.nvcad"), Path::new("/a/c")), PathBuf::from("../b/p.nvcad"));
    assert_eq!(relative_to(Path::new("/a/p.nvcad"), Path::new("/a")), PathBuf::from("p.nvcad"));
    assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
}
