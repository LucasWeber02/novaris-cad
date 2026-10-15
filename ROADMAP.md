# NOVARIS CAD — Development Roadmap

## Phase 0: Initialization ✅ COMPLETE

**Timeline**: 2026-10-14 (1 day)

**Deliverables**:
- [x] Rust workspace structure (`cargo workspace`)
- [x] Core types and abstractions
- [x] ID system (stable UUIDs)
- [x] Error handling framework
- [x] Document model (Sketch, Feature, Document)
- [x] Command pattern skeleton
- [x] Basic math types (Vector3, Point3, Transform)
- [x] Mesh representation
- [x] Project documentation

**Blockers**: None

**What's NOT done yet**:
- No GUI (not needed yet)
- No geometry kernel (on purpose; will evaluate in Phase 1)
- No file I/O
- Project doesn't compile in cloud (DNS issue) — **will compile on CachyOS**

---

## Phase 1: First Working CAD ⏳ IN PROGRESS

**Timeline**: 2026-10-14 to 2026-10-31 (3-4 weeks target)

**Scenario**: User can create a 100×60mm rectangle, extrude it 20mm, see a 3D solid, modify dimensions, and export to STEP.

### Step 1a: Geometry Kernel Decision & Setup (This week)
- [ ] Evaluate Truck vs OpenCASCADE
- [ ] Set up FFI bindings (if OCCT) or direct integration (if Truck)
- [ ] Create `nova-kernel` bridge layer
- [ ] Validate extrusion: create a simple box, verify volume = 120,000 mm³
- [ ] Test STEP export

**Decision**: Use **Truck** (pure Rust) for MVP. OpenCASCADE planned for Phase 2.

### Step 1b: Sketch Editor (Next 1-2 weeks)
- [ ] Implement sketch plane selection (XY, XZ, YZ)
- [ ] 2D geometry tools: point, line, rectangle, circle
- [ ] Basic constraint system: horizontal, vertical, distance
- [ ] Sketch solver (Gauss-Newton or similar)
- [ ] Render sketch in 2D viewport

### Step 1c: Feature Operations (1-2 weeks)
- [ ] Implement Extrusion feature
- [ ] Parameter modification & recalculation
- [ ] Build feature tree visualization

### Step 1d: 3D Viewport & Rendering (1 week)
- [ ] OpenGL/wgpu viewport
- [ ] Orbit rotation, pan, zoom
- [ ] Face/edge selection
- [ ] Shaded display

### Step 1e: Persistence (3-5 days)
- [ ] Implement `.nvcad` format (JSON)
- [ ] Save/load documents
- [ ] Validate round-trip

### Step 1f: Export (3-5 days)
- [ ] STEP format export (via Truck)
- [ ] STL format export

**Definition of Done** (Phase 1):
- All 16 user actions work end-to-end (see prompt section 23)
- No synthetic/fake features
- Document saves & reloads correctly
- Tests pass with geometric validation (volume, bounds)

---

## Phase 2: Advanced CAD Features ⏱️ PLANNED

**Timeline**: Q4 2026 (4-6 weeks)

- [ ] Solveur de contraintes avancé (surcontrainte, déf. liberté)
- [ ] Opérations booléennes (union, soustraction, intersection)
- [ ] Révolution, balayage
- [ ] Congés & chanfreins 3D
- [ ] Perçages & taraudages
- [ ] Répétitions linéaires & circulaires
- [ ] Édition d'esquisse avancée

**Geometry Kernel**: Evaluate OpenCASCADE bindings for more robust operations.

---

## Phase 3: Assemblies 📦 PLANNED

**Timeline**: Q1 2027 (4 weeks)

- [ ] Multi-part projects
- [ ] Part insertion & positioning
- [ ] Mechanical constraints (coïncidence, concentricité, distance)
- [ ] Détection des interférences
- [ ] Éclatés
- [ ] Nomenclature automatique

---

## Phase 4: 2D Drawings 📋 PLANNED

**Timeline**: Q2 2027 (4 weeks)

- [ ] Drawing creation from 3D models
- [ ] Vues orthographiques & isométriques
- [ ] Cotation associative
- [ ] Coupes & demi-coupes
- [ ] Tolérances (ISO 286, ASME)
- [ ] Cartouches & formats (A0-A4)
- [ ] Export PDF & DXF

---

## Phase 5: Specialized Modules 🔧 PLANNED

**Timeline**: Q3-Q4 2027 (ongoing)

### 5A: Sheet Metal (Tôlerie)
- Folding, unfolding, K-factors
- Bend allowance calculations

### 5B: Surfacing (Surfaces complexes)
- NURBS surfaces
- Surface continuity analysis

### 5C: Weldments (Structures soudées)
- Beam libraries
- Weld management

### 5D: Routing (Tuyauteries)
- Pipe routing
- Fittings library

### 5E: Mold Design (Moules)
- Draft analysis
- Parting surface generation

---

## Phase 6: Simulation 🧮 PLANNED

**Timeline**: Q1 2028 (6-8 weeks)

- [ ] Material database
- [ ] Mesh generation & quality checking
- [ ] FEM solver integration (FreeFEM or Code_Aster)
- [ ] Static analysis (stress, strain)
- [ ] Modal analysis (frequencies)
- [ ] Thermal analysis
- [ ] Results visualization & reporting

---

## Phase 7: Ecosystem & Polish 🌟 PLANNED

**Timeline**: Q2-Q3 2028 (ongoing)

- [ ] Plugin SDK & examples
- [ ] Python/Lua scripting
- [ ] REST API for external tools
- [ ] Cloud sync (optional)
- [ ] Collaboration features
- [ ] Performance optimization
- [ ] UI refinement & localization

---

## Known Blockers & Risks

| Item | Severity | Status | Mitigation |
|------|----------|--------|-----------|
| Geometry kernel (OCCT vs Truck) | HIGH | Phase 1 | Evaluate immediately; Truck for MVP |
| Constraint solver robustness | HIGH | Phase 2 | Use proven algorithm (Gauss-Newton) |
| Topological references after modification | MEDIUM | Phase 2 | Implement edge tracking system |
| FEM solver integration | MEDIUM | Phase 6 | Plan FFI early; test with FreeFEM |
| UI responsiveness with large assemblies | MEDIUM | Phase 3 | Profile early; implement LOD |

---

## Success Criteria

By end of each phase:

| Phase | Must-Haves | Can-Haves |
|-------|-----------|-----------|
| 0 | Compilable, documented, core types | — |
| 1 | Sketch → Extrusion → STEP, save/load | Undo/redo |
| 2 | Advanced features, boolean ops, complex sketches | Surfacing start |
| 3 | Multi-part projects, constraints | Éclatés |
| 4 | Associative 2D drawings with dimensions | Tolerances |
| 5+ | Domain-specific modules, simulation | Cloud features |

---

## Performance Targets

| Operation | Target | Priority |
|-----------|--------|----------|
| Create 100×60×20 box | < 200ms | HIGH |
| Boolean operation | < 500ms | HIGH |
| Rotate viewport (10k tris) | 60 FPS | HIGH |
| Save document | < 100ms | MEDIUM |
| Import STEP (1MB file) | < 2s | MEDIUM |
| Assemble 100 parts | < 5s | MEDIUM |

---

## Notes

- **Modularity first**: Each phase adds complete modules, not half-baked features
- **No TODOs**: If code is committed, it works
- **Testing throughout**: Every feature gets unit + integration tests
- **Documentation**: Architecture, decisions, and limitations documented at each phase

---

**Last Updated**: 2026-10-14
**Next Review**: 2026-10-21
