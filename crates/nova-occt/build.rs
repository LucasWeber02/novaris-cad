//! Links the OCCT static libraries and compiles the C++ shim.
//!
//! OCCT comes from `NOVA_OCCT_DIR` (an existing install with `include/` and
//! `lib/`), or else from `<target>/OCCT`, built once by `occt-sys` (about
//! 35 minutes) and shared by every build profile.

use std::path::{Path, PathBuf};

/// Libraries used by the shim, in link order (dependents before dependencies)
const OCCT_LIBS: &[&str] = &[
    "TKDESTEP",
    "TKDE",
    "TKXSBase",
    "TKFillet",
    "TKOffset",
    "TKFeat",
    "TKBool",
    "TKBO",
    "TKMesh",
    "TKShHealing",
    "TKPrim",
    "TKTopAlgo",
    "TKGeomAlgo",
    "TKBRep",
    "TKGeomBase",
    "TKG3d",
    "TKG2d",
    "TKMath",
    "TKernel",
];

fn occt_dir() -> PathBuf {
    println!("cargo:rerun-if-env-changed=NOVA_OCCT_DIR");
    if let Some(dir) = std::env::var_os("NOVA_OCCT_DIR") {
        return PathBuf::from(dir);
    }
    // Same location occt-sys uses: <target>/OCCT
    let dir = occt_sys::occt_path();
    if !has_libs(&dir) {
        occt_sys::build_occt();
    }
    dir
}

fn has_libs(dir: &Path) -> bool {
    let lib = dir.join("lib");
    OCCT_LIBS.iter().all(|l| lib.join(format!("lib{l}.a")).exists() || lib.join(format!("{l}.lib")).exists())
        && dir.join("include").join("TopoDS_Shape.hxx").exists()
}

fn main() {
    let dir = occt_dir();
    let include = dir.join("include");
    let lib = dir.join("lib");
    assert!(has_libs(&dir), "OCCT not found in {} (set NOVA_OCCT_DIR)", dir.display());

    cxx_build::bridge("src/lib.rs")
        .file("src/shim.cpp")
        .include(&include)
        .include(".")
        .flag_if_supported("-std=c++17")
        .flag_if_supported("-w")
        .define("_USE_MATH_DEFINES", None)
        .compile("nova-occt-shim");

    println!("cargo:rustc-link-search=native={}", lib.display());
    for l in OCCT_LIBS {
        println!("cargo:rustc-link-lib=static={l}");
    }
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        println!("cargo:rustc-link-lib=dylib=user32");
    }
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-changed=include/shim.hxx");
}
