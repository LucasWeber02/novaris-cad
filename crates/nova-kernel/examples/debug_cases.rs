use glam::{DVec2, DVec3};
use nova_kernel::*;
use std::f64::consts::PI;
fn main() {
    let rect = |x: f64, y: f64, w: f64, h: f64| Loop::rectangle(DVec2::new(x, y), DVec2::new(x + w, y + h));
    for (ao, ad) in [(DVec2::ZERO, DVec2::Y), (DVec2::new(0.0, -10.0), DVec2::new(0.0, 60.0))] {
    let shaft = Solid::from_tool(&Revolve::new(Profile::simple(rect(0.0, 0.0, 10.0, 40.0)), Plane::front(), ao, ad, 2.0 * PI).into()).unwrap();
    println!("axis {:?} {:?}", ao, ad);
    for (name, plane, s, e) in [
        ("offset15 -26.48..0", Plane::front().offset(15.0), -26.48243836597223, 0.0),
    ] {
        let hole = Prism::new(Profile::simple(Loop::circle(DVec2::new(0.0, 20.0), 3.0)), plane, s, e);
        let t = std::time::Instant::now();
        println!("{name}: {:?} {:?}", shaft.boolean(BooleanOp::Subtract, hole).map(|c| c.volume()).map_err(|e| e.to_string()), t.elapsed());
    }
    }
    let _ = DVec3::ZERO;
}
