//! NOVARIS CAD Desktop Application — Entry point

use nova_core::Document;
use tracing_subscriber;

fn main() -> nova_core::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("╔═══════════════════════════════════════╗");
    println!("║      NOVARIS CAD v{:<20}║", nova_core::VERSION);
    println!("║      Rust-based Professional CAO     ║");
    println!("╚═══════════════════════════════════════╝");
    println!();

    // Create a demo document
    let doc = Document::new("Demo Part");
    println!("✓ Created new document: '{}'", doc.name);
    println!("✓ Document ID: {}", doc.id);
    println!("✓ Version: {}", doc.version);

    println!();
    println!("Currently in: Phase 0 (Initialization)");
    println!("Status: ✓ Basic workspace structure in place");
    println!();

    #[cfg(not(target_os = "windows"))]
    {
        println!("Platform: Linux");
        println!("Rust version: {}", std::env::var("RUSTC_VERSION").unwrap_or("1.97+".into()));
    }

    Ok(())
}
