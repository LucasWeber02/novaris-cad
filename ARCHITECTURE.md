# NOVARIS CAD — Architecture Document

## Overview

NOVARIS CAD is a professional parametric CAD system built entirely in Rust. The architecture is modular, with clear separation of concerns and minimal coupling.

## Design Principles

1. **Geometry ≠ Rendering** — Exact B-Rep geometry is kept separate from tessellated meshes for display
2. **Parametric-First** — All operations are parametric and can be modified retroactively
3. **Command Pattern** — All mutations use Commands for undo/redo
4. **Type Safety** — Heavy use of Rust's type system to prevent invalid states
5. **FFI Boundary** — If external geometry kernels are used, they're behind a clean abstraction layer

## Crate Structure

### Core Layer
- **nova-core**: Types, IDs, errors, documents, commands
- **nova-math**: Vector math, linear algebra (using glam + nalgebra)

### Geometry Layer
- **nova-geometry**: Mesh representation, B-Rep abstractions, tessellation
- **nova-kernel**: Pluggable geometry engine (currently: architecture only)
  - Plans to support OpenCASCADE via FFI or pure Rust (Truck) later

### Modeling Layer
- **nova-sketch**: 2D sketch editor with constraint solver
- **nova-parametric**: Parametric feature graph evaluation
- **nova-features**: 3D CAD operations (Extrusion, Revolution, etc.)

### Application Layer
- **nova-ui**: GUI framework (egui + wgpu)
- **nova-io**: File I/O, format readers/writers (STEP, STL, DXF, etc.)
- **novaris-desktop**: Main application entry point

## Data Flow

```
User Input
    ↓
[UI Layer]
    ↓
[Command System] → Undo/Redo
    ↓
[Parametric Engine]
    ↓
[Geometry Kernel] → B-Rep
    ↓
[Renderer] → Tessellation → Display
```

## Key Abstractions

### Document
The root container for all model data:
```rust
pub struct Document {
    pub id: Id,
    pub name: String,
    pub sketches: HashMap<Id, Sketch>,
    pub features: Vec<Feature>,
    pub history: History,
}
```

### Feature Tree
Features are evaluated sequentially:
```
Document
  ├── Sketch001
  ├── Extrusion001 (from Sketch001)
  ├── Sketch002
  ├── Pocket001 (from Sketch002, subtracts from Extrusion001)
  └── Fillet001 (modifies faces of Pocket001)
```

### Geometry Kernel Interface
```rust
pub trait GeometryKernel {
    fn extrude_profile(sketch: &Sketch, depth: f64) -> Result<BRep>;
    fn boolean_operation(a: &BRep, b: &BRep, op: BoolOp) -> Result<BRep>;
    fn create_fillet(brep: &BRep, edges: &[EdgeId], radius: f64) -> Result<BRep>;
    fn tessellate(&self, tolerance: f64) -> Mesh;
}
```

## Geometry Kernel Decision

**Status: To Be Determined**

### Option A: OpenCASCADE (OCCT)
- ✓ Industrial-grade, production-ready
- ✓ Mature B-Rep engine with 30+ years of validation
- ✓ Handles complex operations (boolean, fillets, lofts)
- ✗ C++ dependency, requires FFI bindings
- ✗ Not pure Rust

**If chosen**: Use `opencascade-sys` (Rust FFI bindings) and wrap behind `nova-kernel::OcctEngine`

### Option B: Truck (Rust)
- ✓ Pure Rust, no external dependencies
- ✓ Modern design, educational value
- ✗ Still under development
- ✗ Missing some operations (advanced fillets, complex booleans)

**If chosen**: Use Truck directly in `nova-kernel` with capability gaps documented

### Decision Criteria (Phase 1)
1. Can we create a simple extrusion? ✓ Both can
2. Can we export to STEP? ✓ Both can (via different paths)
3. Which is faster to integrate? **→ Truck** (no FFI complexity)
4. Which is more accurate? **→ OCCT** (proven in industry)

**Current plan for Phase 1**: Use Truck as MVP, plan OpenCASCADE integration for Phase 2

## Thread Safety

- Document is `Sync` and `Send` (immutable reads via snapshot)
- Feature evaluation uses `rayon` for parallel computation
- UI thread is single-threaded (egui requirement)
- Long operations use `tokio` async for responsiveness

## Error Handling

All fallible operations return `nova_core::Result<T>`:
```rust
pub enum NovaError {
    GeometryError(String),
    InvalidSketch(String),
    ConstraintError(String),
    // ...
}
```

Errors are propagated upward; UI displays them to the user without crashing.

## Persistence

Documents are serialized using `serde`:
- Native format: `.nvcad` (versioned JSON)
- Supports round-trip: save → load → exact same state
- Schema versioning for future upgrades

## Testing Strategy

- **Unit tests** in each crate for math/geometry functions
- **Integration tests** for feature evaluation chains
- **Geometry validation** tests (volume, mass, bounds checks)
- **Regression tests** for complex operations
- **Fuzzing** for file parsers

## Performance Targets (Phase 1)

- Open empty document: < 100ms
- Create sketch: < 50ms
- Extrude 100×60 rect by 20mm: < 200ms
- Rotate viewport: 60 FPS (with 10k triangles)
- Save document: < 100ms

## Future Extensibility

### Plugin System
Plugins will be loaded from `.so` / `.dll` / `.dylib` files:
```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, doc: &mut Document) -> Result<()>;
}
```

### Custom Features
Users will be able to write Python or Rust scripts to generate features parametrically.

### Simulation Integration
FEM solver integration via external processes (FreeFEM, Code_Aster) with mesh export.

## Design Decisions Log

| Decision | Rationale |
|----------|-----------|
| Use `glam` for math | Fast, SIMD-enabled, actively maintained |
| Use `egui` for UI | Immediate-mode, rapid iteration, no Electron bloat |
| Use `wgpu` for graphics | Cross-platform, Rust-first, no OpenGL headaches |
| Command pattern for mutations | Clean undo/redo, testable state changes |
| Workspace with multiple crates | Enforces modularity, enables parallel builds |

---

**Last Updated**: 2026-10-14
**Status**: Phase 0 Complete, Phase 1 In Progress
