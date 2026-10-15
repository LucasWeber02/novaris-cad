//! Times opening a part: cargo run -p novaris-desktop --example bench_open -- part.nvcad
fn main() {
    let path = std::env::args().nth(1).expect("path");
    let text = std::fs::read_to_string(&path).unwrap();
    let t = std::time::Instant::now();
    let doc = nova_io::PartDocument::from_json(&text).unwrap();
    println!("open+rebuild: {:?}", t.elapsed());
    let body = doc.tree.body().unwrap();
    let t = std::time::Instant::now();
    let m = body.tessellate((body.size() * 1e-3).max(0.01));
    println!("tessellate display: {:?} ({} tris)", t.elapsed(), m.indices.len() / 3);
    let t = std::time::Instant::now();
    let v = body.volume();
    println!("volume: {:?} = {v:.1}", t.elapsed());
}
