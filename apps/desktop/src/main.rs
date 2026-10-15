//! NOVARIS CAD desktop application
//!
//! Usage: `novaris [part.nvcad]`

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,wgpu_core=error,wgpu_hal=error".into()),
        )
        .init();
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    if let Err(e) = nova_ui::run(path) {
        eprintln!("NOVARIS CAD : {e}");
        std::process::exit(1);
    }
}
