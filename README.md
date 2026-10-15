# NOVARIS CAD

**Professional 3D parametric CAD software — built entirely in Rust**

A modern alternative to SolidWorks, designed for precision, stability, and performance.

## Current Status: Phase 0 — Initialization ✓

- [x] Rust workspace structure
- [x] Core types and abstractions
- [x] Document model  
- [x] ID and error systems
- [ ] Geometry kernel integration
- [ ] First compilation

## Building

### Requirements
- Rust 1.97+ (stable)
- Linux (CachyOS, Arch), Windows 11, macOS
- 8GB+ RAM recommended

### Compile

```bash
cd novaris-cad
cargo build --release
```

### Run demo

```bash
cargo run --bin novaris
```

## Architecture

```
novaris-cad/
├── crates/
│   ├── nova-core/         # Types, errors, IDs
│   ├── nova-math/         # Linear algebra, geometry
│   ├── nova-geometry/     # Mesh, B-Rep abstractions
│   ├── nova-kernel/       # Geometry kernel (OpenCASCADE bridge)
│   ├── nova-sketch/       # 2D sketch editor
│   ├── nova-parametric/   # Parametric engine
│   ├── nova-features/     # CAD features (extrusion, etc.)
│   ├── nova-ui/           # UI framework
│   └── nova-io/           # File I/O, STEP, STL, etc.
└── apps/
    └── desktop/           # Main application
```

## Phases

| Phase | Status | Deliverable |
|-------|--------|-------------|
| 0 | ✓ Done | Workspace + core types |
| 1 | → In progress | Sketch → Extrusion → STEP |
| 2 | Planned | Advanced CAD features |
| 3 | Planned | Assemblies |
| 4 | Planned | 2D drawings |
| 5+ | Planned | Simulations, sheet metal, etc. |

## License

AGPL-3.0-or-later — See LICENSE file

## Development Notes

- All features are **real**, not simulated in the UI
- Geometry engine: Evaluating OpenCASCADE vs Truck
- No empty TODOs; only implemented features are marked complete
