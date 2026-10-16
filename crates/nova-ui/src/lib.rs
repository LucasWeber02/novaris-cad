//! NOVARIS CAD — desktop user interface (egui + wgpu)

pub mod app;
pub mod camera;
pub mod render;
pub mod sketcher;

pub use app::NovarisApp;

use nova_io::PartDocument;
use std::path::PathBuf;

/// Open the main window; `path` is an optional .nvcad or .nvasm file to load
pub fn run(path: Option<PathBuf>) -> eframe::Result<()> {
    let assembly = path.clone().filter(|p| p.extension().is_some_and(|e| e == nova_assembly::EXTENSION));
    let path = if assembly.is_some() { None } else { path };
    let (doc, path) = match path {
        Some(p) => match PartDocument::load(&p) {
            Ok(doc) => (doc, Some(p)),
            Err(e) => {
                eprintln!("Impossible d'ouvrir {} : {e}", p.display());
                (PartDocument::new("Pièce1"), None)
            }
        },
        None => (PartDocument::new("Pièce1"), None),
    };
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("NOVARIS CAD")
            .with_inner_size([1400.0, 860.0]),
        ..Default::default()
    };
    eframe::run_native("NOVARIS CAD", options, Box::new(move |cc| {
            let mut app = NovarisApp::new(cc, doc, path);
            if let Some(a) = assembly {
                app.open(a);
            }
            Ok(Box::new(app))
        }),
    )
}
