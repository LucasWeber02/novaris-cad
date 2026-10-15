# NOVARIS CAD — Feature Matrix

This document tracks the implementation status of each feature.

**Statuses**:
- 🟡 **Prévue** (Planned) — In roadmap, not started
- 🔵 **En développement** (In Development) — Active work
- 🟢 **Implémentée** (Implemented) — Code written
- 🟦 **Testée** (Tested) — Validated with tests
- 🔶 **Expérimentale** (Experimental) — Works but may be unstable
- 🔴 **Bloquée** (Blocked) — Waiting on dependency or decision

---

## Phase 0: Core Infrastructure ✅

| Feature | Status | Notes |
|---------|--------|-------|
| Rust workspace structure | 🟢 | Cargo monorepo with 9 crates |
| Core types (Vector3, Point3, Matrix4) | 🟢 | Using `glam` for SIMD math |
| Document model (Sketch, Feature, Document) | 🟢 | Serializable with `serde` |
| ID system (UUID-based stable IDs) | 🟢 | Unique, persistent identifiers |
| Error handling (`NovaError`) | 🟢 | Thiserror-based |
| Command pattern skeleton | 🟢 | Undo/redo infrastructure |
| Mesh representation | 🟢 | Basic triangle mesh |
| Basic cube mesh generation | 🟢 | Test utility |
| Project documentation | 🟢 | README, ARCHITECTURE, ROADMAP |

---

## Phase 1: First Working CAD 🔵

| Feature | Status | Notes |
|---------|--------|-------|
| Geometry kernel abstraction | 🟡 | Decision: Truck vs OCCT |
| Sketch plane selection (XY, XZ, YZ) | 🟡 | Phase 1a |
| Point geometry element | 🟡 | Phase 1b |
| Line geometry element | 🟡 | Phase 1b |
| Rectangle tool | 🟡 | Phase 1b |
| Circle tool | 🟡 | Phase 1b |
| Horizontal constraint | 🟡 | Phase 1b |
| Vertical constraint | 🟡 | Phase 1b |
| Distance constraint | 🟡 | Phase 1b |
| Sketch solver (constraint satisfaction) | 🟡 | Phase 1b |
| 2D sketch viewport | 🟡 | Phase 1b |
| Extrusion feature | 🟡 | Phase 1c |
| Parameter modification | 🟡 | Phase 1c |
| Feature tree visualization | 🟡 | Phase 1c |
| 3D viewport (orbit/pan/zoom) | 🟡 | Phase 1d |
| Face selection in 3D | 🟡 | Phase 1d |
| Shaded rendering | 🟡 | Phase 1d |
| Save document (.nvcad format) | 🟡 | Phase 1e |
| Load document (persistence) | 🟡 | Phase 1e |
| STEP format export | 🟡 | Phase 1f |
| STL format export | 🟡 | Phase 1f |
| Undo/redo functionality | 🟡 | Core done, UI integration pending |

---

## Phase 2: Advanced CAD ⏳

| Feature | Status | Notes |
|---------|--------|-------|
| Constraint solver (over/under/fully constrained detection) | 🟡 | Requires robust solver |
| Boolean operations (union, difference, intersection) | 🟡 | Requires geometry kernel |
| Revolution feature | 🟡 | Requires extrusion base |
| Sweep feature | 🟡 | Complex; Phase 2 |
| Loft feature | 🟡 | Complex; Phase 2 |
| Fillet (3D) | 🟡 | Requires geometry kernel |
| Chamfer (3D) | 🟡 | Requires geometry kernel |
| Pocket (negative extrusion) | 🟡 | Requires boolean ops |
| Hole tool (simplified drilling) | 🟡 | Special case of pocket |
| Linear pattern | 🟡 | Copy + translate |
| Circular pattern | 🟡 | Copy + rotate array |
| Advanced sketch tools (ellipse, spline, arc) | 🟡 | Sketch enhancement |
| Sketch mirror | 🟡 | Sketch tool |
| Coincident constraint | 🟡 | Sketch constraint |
| Concentric constraint | 🟡 | Sketch constraint |
| Parallel constraint | 🟡 | Sketch constraint |
| Perpendicular constraint | 🟡 | Sketch constraint |
| Tangent constraint | 🟡 | Sketch constraint |
| Equal constraint | 🟡 | Sketch constraint |

---

## Phase 3: Assemblies 📦

| Feature | Status | Notes |
|---------|--------|-------|
| Multi-part document management | 🟡 | Requires file references |
| Part insertion | 🟡 | Load external .nvcad |
| Part positioning (manual) | 🟡 | Transform matrix |
| Sub-assemblies | 🟡 | Hierarchical structure |
| Coincidence constraint (assembly) | 🟡 | Face-to-face matching |
| Concentric constraint (assembly) | 🟡 | Axis-to-axis |
| Distance constraint (assembly) | 🟡 | Gap definition |
| Angle constraint (assembly) | 🟡 | Rotation constraint |
| Interference detection | 🟡 | Mesh collision |
| Exploded view | 🟡 | Manual disassembly |
| Bill of Materials (BOM) | 🟡 | Auto-generated |
| Part list | 🟡 | Reference table |

---

## Phase 4: 2D Drawings 📋

| Feature | Status | Notes |
|---------|--------|-------|
| Drawing creation from 3D | 🟡 | View projection |
| Orthographic views (front/back/top/bottom/left/right) | 🟡 | Standard views |
| Isometric view | 🟡 | 3D projection to 2D |
| Projected views | 🟡 | Linked to 3D model |
| Section view | 🟡 | Cut plane projection |
| Half section | 🟡 | Partial cut |
| Detail view | 🟡 | Zoomed inset |
| Exploded view (2D) | 🟡 | From assembly |
| Hidden line removal | 🟡 | Back-face culling |
| Dimension annotation | 🟡 | Associative dimensions |
| Manual dimensioning | 🟡 | User-placed notes |
| Geometric tolerance (GD&T) | 🟡 | ISO 1101 symbols |
| Surface finish symbols | 🟡 | ISO 1302 |
| Title block / Cartouche | 🟡 | Template-based |
| Sheet formats (A0 to A4, custom) | 🟡 | Parametric sizes |
| PDF export | 🟡 | Output format |
| DXF export | 🟡 | CAD interchange |
| Associative update (3D changes → 2D) | 🟡 | Link management |

---

## Phase 5: Specialized Modules 🔧

| Feature | Status | Notes |
|---------|--------|-------|
| **Sheet Metal** |  |  |
| Fold definition | 🟡 | Bend parameters |
| Unfold (developable surface) | 🟡 | Flat pattern |
| K-factor calculation | 🟡 | Material-dependent |
| **Surfacing** |  |  |
| NURBS surface creation | 🟡 | Complex geometry |
| Surface continuity analysis | 🟡 | G0, G1, G2 |
| **Weldments** |  |  |
| Structural beam library | 🟡 | Standard profiles |
| Weld management | 🟡 | Seam tracking |
| Cut lists | 🟡 | Material planning |
| **Routing** |  |  |
| Pipe routing tools | 🟡 | 3D paths |
| Fitting library | 🟡 | Component database |
| **Mold Design** |  |  |
| Draft analysis | 🟡 | Undercut detection |
| Parting surface generation | 🟡 | Mold splitting |

---

## Phase 6: Simulation 🧮

| Feature | Status | Notes |
|---------|--------|-------|
| Material database | 🟡 | Steel, aluminum, plastic, etc. |
| Mesh generation | 🟡 | Tetrahedral, quality checking |
| **Static Analysis** |  |  |
| Stress calculation (Von Mises) | 🟡 | FEM solver |
| Strain visualization | 🟡 | Result mapping |
| Deformation preview | 🟡 | Scaled display |
| **Modal Analysis** |  |  |
| Frequency calculation | 🟡 | Eigenvalue solver |
| Mode shapes | 🟡 | Vibration patterns |
| **Thermal Analysis** |  |  |
| Heat conduction | 🟡 | FEM equation |
| Temperature field | 🟡 | Result visualization |
| **Advanced** |  |  |
| Fatigue analysis | 🟡 | Life prediction |
| Buckling analysis | 🟡 | Instability |
| Nonlinear analysis | 🟡 | Large deformation |
| Optimization | 🟡 | Topology optimization |

---

## Phase 7: Ecosystem & Polish 🌟

| Feature | Status | Notes |
|---------|--------|-------|
| Plugin SDK | 🟡 | Rust API for extensions |
| Python scripting | 🟡 | Embedded interpreter |
| Macro recording | 🟡 | User automation |
| REST API | 🟡 | External tool integration |
| Cloud synchronization | 🟡 | Optional; low priority |
| Collaborative editing | 🟡 | Real-time multi-user |
| Performance profiling | 🟡 | Benchmarking tools |
| Dark/Light theme | 🟡 | UI themes |
| Localization (i18n) | 🟡 | Multi-language (FR, EN first) |
| Keyboard shortcuts customization | 🟡 | User preferences |

---

## Not Planned / Out of Scope

- SolidWorks file format (`.sldprt`, `.sldasm`) import/export
  - Reason: Complex proprietary format; STEP is standard alternative
- Proprietary binary format optimization
  - Reason: JSON `.nvcad` is sufficient; can add binary later
- Real-time collaboration (Figma-style)
  - Reason: Complex infrastructure; Phase 7 if needed
- Cloud-based only (always offline-first)
  - Reason: Professional users need local control

---

**Total Features Tracked**: 120+

**Phase 0 Completion**: 100% ✅

**Phase 1 Estimated Completion**: 2026-10-31

---

**Last Updated**: 2026-10-14
