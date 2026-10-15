use glam::DVec2;
use nova_kernel::*;
use std::time::Instant;

fn main() {
    let rect = |x: f64, y: f64, w: f64, h: f64| Loop::rectangle(DVec2::new(x, y), DVec2::new(x + w, y + h));
    let t = Instant::now();
    let base = Solid::prism(&Prism::new(Profile::simple(rect(-50.0, -30.0, 100.0, 60.0)), Plane::front(), 0.0, 12.0)).unwrap();
    println!("prism: {:?}", t.elapsed());
    let t = Instant::now();
    let _ = base.size();
    println!("size: {:?}", t.elapsed());
    let t = Instant::now();
    let _ = base.volume();
    println!("volume: {:?}", t.elapsed());
    let t = Instant::now();
    let tool = Prism::new(Profile::simple(rect(-35.0, -15.0, 30.0, 30.0)), Plane::front(), 12.0, 30.0);
    let u = base.boolean(BooleanOp::Union, &tool).unwrap();
    println!("union (coplanar): {:?}", t.elapsed());
    let t = Instant::now();
    let hole = Prism::new(Profile::simple(Loop::circle(DVec2::new(20.0, 0.0), 6.0)), Plane::front(), 0.0, 32.0);
    let c = u.boolean(BooleanOp::Subtract, &hole).unwrap();
    println!("cut hole: {:?}", t.elapsed());
    let t = Instant::now();
    let hole2 = Prism::new(Profile::simple(Loop::circle(DVec2::new(35.0, 0.0), 6.0)), Plane::front(), 0.0, 32.0);
    let _ = c.boolean(BooleanOp::Subtract, &hole2).unwrap();
    println!("cut hole 2: {:?}", t.elapsed());
}
