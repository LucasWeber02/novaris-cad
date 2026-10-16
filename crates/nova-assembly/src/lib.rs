//! NOVARIS CAD — Assemblies (.nvasm)
//!
//! An assembly places part files (`.nvcad`) with mates, like SolidWorks:
//! the first component is fixed, the others move until every mate holds.
//! The file stores component placements and mates; parts are referenced by a
//! path relative to the assembly file and rebuilt when the assembly opens.
//!
//! Mates point at faces by geometry (a point on the face and its normal or
//! axis, in part coordinates), so they survive edits of the parts as long as
//! the face still exists near the same place.

mod solver;

use glam::{DMat4, DQuat, DVec3};
use nova_core::{Id, NovaError, Result};
use nova_io::PartDocument;
use nova_kernel::{FaceInfo, Solid, Surface};
use serde::{Deserialize, Serialize};
use solver::{Body, Equation, Geom, Relation};
use std::collections::HashMap;
use std::path::{Component as PathPart, Path, PathBuf};

pub const FORMAT: &str = "nvcad-assembly";
pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION: &str = "nvasm";

/// Rigid placement of a component: world = rotation * part + translation
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub rotation: DQuat,
    pub translation: DVec3,
}

impl Default for Placement {
    fn default() -> Self {
        Placement { rotation: DQuat::IDENTITY, translation: DVec3::ZERO }
    }
}

impl Placement {
    pub fn from_translation(translation: DVec3) -> Self {
        Placement { rotation: DQuat::IDENTITY, translation }
    }

    pub fn matrix(&self) -> DMat4 {
        DMat4::from_rotation_translation(self.rotation, self.translation)
    }

    pub fn point(&self, p: DVec3) -> DVec3 {
        self.rotation * p + self.translation
    }

    pub fn vector(&self, v: DVec3) -> DVec3 {
        self.rotation * v
    }

    /// Placement after moving the component by `m` in world space
    pub fn moved(&self, m: &DMat4) -> Placement {
        let (_, r, t) = (*m * self.matrix()).to_scale_rotation_translation();
        Placement { rotation: r.normalize(), translation: t }
    }
}

/// An instance of a part in the assembly
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub id: Id,
    /// Instance name, e.g. "Plaque<1>"
    pub name: String,
    /// Part file, relative to the assembly file when possible
    pub part_path: PathBuf,
    pub placement: Placement,
    pub fixed: bool,
    #[serde(default)]
    pub suppressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaceKind {
    Plane,
    Cylinder,
}

/// A face of a component, named by its geometry in part coordinates:
/// a point inside the face and its normal (plane) or axis (cylinder)
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceRef {
    pub component: Id,
    pub kind: FaceKind,
    pub point: DVec3,
    pub dir: DVec3,
}

/// Mate types (SolidWorks standard mates)
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MateKind {
    Coincident,
    Parallel,
    Perpendicular,
    Distance { value: f64 },
    Angle { degrees: f64 },
    Concentric,
}

impl MateKind {
    /// Base of the feature name, as in the French SolidWorks
    pub fn label(&self) -> &'static str {
        match self {
            MateKind::Coincident => "Coïncidente",
            MateKind::Parallel => "Parallèle",
            MateKind::Perpendicular => "Perpendiculaire",
            MateKind::Distance { .. } => "Distance",
            MateKind::Angle { .. } => "Angle",
            MateKind::Concentric => "Coaxiale",
        }
    }

    fn relation(&self) -> Relation {
        match *self {
            MateKind::Coincident => Relation::Coincident,
            MateKind::Parallel => Relation::Parallel,
            MateKind::Perpendicular => Relation::Perpendicular,
            MateKind::Distance { value } => Relation::Distance(value),
            MateKind::Angle { degrees } => Relation::Angle(degrees.to_radians()),
            MateKind::Concentric => Relation::Concentric,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mate {
    pub id: Id,
    pub name: String,
    pub kind: MateKind,
    pub a: FaceRef,
    pub b: FaceRef,
    /// Normals point the same way (otherwise opposite, face against face)
    #[serde(default)]
    pub aligned: bool,
    #[serde(default)]
    pub suppressed: bool,
}

/// A part file loaded and rebuilt
#[derive(Clone, Debug)]
pub struct LoadedPart {
    pub name: String,
    pub path: PathBuf,
    pub solid: Option<Solid>,
    pub faces: Vec<FaceInfo>,
    pub volume: f64,
    /// Bounding box center (part coordinates)
    pub center: DVec3,
    pub error: Option<String>,
}

/// How well a component is positioned (SolidWorks tree prefixes)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentState {
    /// "(f)"
    Fixed,
    /// "(-)": can still move along `n` independent directions
    UnderDefined(usize),
    FullyDefined,
}

impl ComponentState {
    pub fn prefix(&self) -> &'static str {
        match self {
            ComponentState::Fixed => "(f) ",
            ComponentState::UnderDefined(_) => "(-) ",
            ComponentState::FullyDefined => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MateState {
    Satisfied,
    /// Contradicts other mates
    Unsatisfied,
    /// One of the faces no longer exists
    MissingFace,
    Suppressed,
}

/// Result of a solve
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SolveSummary {
    pub iterations: usize,
    pub unsatisfied: usize,
    /// Mate equations that add no information
    pub redundant: usize,
}

/// Two components sharing material
#[derive(Clone, Debug)]
pub struct Interference {
    pub a: Id,
    pub b: Id,
    pub volume: f64,
    /// Shared material, in world coordinates
    pub solid: Option<Solid>,
}

/// One line of the bill of materials
#[derive(Clone, Debug, PartialEq)]
pub struct BomRow {
    pub item: usize,
    pub name: String,
    pub path: PathBuf,
    pub quantity: usize,
    pub volume: f64,
}

/// An assembly: components, mates and the parts they use
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AssemblyDocument {
    pub name: String,
    pub components: Vec<Component>,
    pub mates: Vec<Mate>,
    #[serde(skip)]
    base_dir: PathBuf,
    #[serde(skip)]
    parts: HashMap<PathBuf, LoadedPart>,
    #[serde(skip)]
    component_states: HashMap<Id, ComponentState>,
    #[serde(skip)]
    mate_states: HashMap<Id, MateState>,
    #[serde(skip)]
    summary: SolveSummary,
}

#[derive(Serialize, Deserialize)]
struct AssemblyFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    assembly: AssemblyDocument,
}

impl AssemblyDocument {
    pub fn new(name: impl Into<String>) -> Self {
        AssemblyDocument { name: name.into(), ..Default::default() }
    }

    /// Folder the relative part paths start from
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn set_base_dir(&mut self, dir: impl Into<PathBuf>) {
        self.base_dir = dir.into();
    }

    // ---------------------------------------------------------------- parts

    /// Absolute path of a component's part file
    pub fn part_file(&self, c: &Component) -> PathBuf {
        if c.part_path.is_absolute() {
            normalize(&c.part_path)
        } else {
            normalize(&absolute(&self.base_dir.join(&c.part_path)))
        }
    }

    fn load_part(path: &Path) -> LoadedPart {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut part = LoadedPart {
            name: stem,
            path: path.to_path_buf(),
            solid: None,
            faces: Vec::new(),
            volume: 0.0,
            center: DVec3::ZERO,
            error: None,
        };
        match PartDocument::open(path) {
            Ok(doc) => {
                if !doc.name.is_empty() {
                    part.name = doc.name.clone();
                }
                match doc.tree.body() {
                    Some(solid) => {
                        part.faces = solid.faces();
                        part.volume = solid.volume();
                        part.center = solid.bounds().map(|(lo, hi)| (lo + hi) / 2.0).unwrap_or_default();
                        part.solid = Some(solid.clone());
                    }
                    None => part.error = Some("la pièce ne contient aucun corps".into()),
                }
            }
            Err(e) => part.error = Some(e.to_string()),
        }
        part
    }

    /// Load every part file not loaded yet
    pub fn load_parts(&mut self) {
        let paths: Vec<PathBuf> = self.components.iter().map(|c| self.part_file(c)).collect();
        for path in paths {
            if let std::collections::hash_map::Entry::Vacant(slot) = self.parts.entry(path) {
                let part = Self::load_part(slot.key());
                slot.insert(part);
            }
        }
    }

    /// Reload all parts from disk (after they were edited) and re-solve
    pub fn reload_parts(&mut self) -> SolveSummary {
        self.parts.clear();
        self.load_parts();
        self.solve()
    }

    pub fn part(&self, component: Id) -> Option<&LoadedPart> {
        let c = self.component(component)?;
        self.parts.get(&self.part_file(c))
    }

    pub fn component(&self, id: Id) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    pub fn component_mut(&mut self, id: Id) -> Option<&mut Component> {
        self.components.iter_mut().find(|c| c.id == id)
    }

    pub fn mate(&self, id: Id) -> Option<&Mate> {
        self.mates.iter().find(|m| m.id == id)
    }

    /// Insert a part file. The first component is fixed.
    pub fn insert_component(&mut self, path: &Path, placement: Placement) -> Result<Id> {
        let abs = normalize(&absolute(path));
        if !self.parts.contains_key(&abs) {
            let part = Self::load_part(&abs);
            if let Some(e) = &part.error {
                return Err(NovaError::DocumentError(format!("{}: {e}", abs.display())));
            }
            self.parts.insert(abs.clone(), part);
        }
        let part_name = self.parts[&abs].name.clone();
        let count = self.components.iter().filter(|c| self.part_file(c) == abs).count();
        let instance = (1..)
            .map(|n| format!("{part_name}<{}>", count + n))
            .find(|n| self.components.iter().all(|c| &c.name != n))
            .expect("unbounded");
        let part_path = if self.base_dir.as_os_str().is_empty() {
            abs.clone()
        } else {
            relative_to(&abs, &normalize(&absolute(&self.base_dir)))
        };
        let id = Id::new();
        let fixed = self.components.is_empty();
        self.components.push(Component { id, name: instance, part_path, placement, fixed, suppressed: false });
        self.component_states.insert(id, if fixed { ComponentState::Fixed } else { ComponentState::UnderDefined(6) });
        Ok(id)
    }

    /// Delete a component and the mates that use it
    pub fn remove_component(&mut self, id: Id) {
        self.components.retain(|c| c.id != id);
        self.mates.retain(|m| m.a.component != id && m.b.component != id);
        self.solve();
    }

    pub fn set_fixed(&mut self, id: Id, fixed: bool) {
        if let Some(c) = self.component_mut(id) {
            c.fixed = fixed;
        }
        self.solve();
    }

    pub fn set_component_suppressed(&mut self, id: Id, suppressed: bool) {
        if let Some(c) = self.component_mut(id) {
            c.suppressed = suppressed;
        }
        self.solve();
    }

    /// Move a free component (by a world-space transform), then re-apply the mates
    pub fn move_component(&mut self, id: Id, m: &DMat4) -> Result<SolveSummary> {
        let c = self.component_mut(id).ok_or_else(|| NovaError::DocumentError("unknown component".into()))?;
        if c.fixed {
            return Err(NovaError::InvalidFeature(format!("{} est fixe", c.name)));
        }
        c.placement = c.placement.moved(m);
        Ok(self.solve())
    }

    // ---------------------------------------------------------------- faces

    /// Name a face (index from the part's mesh) for use in a mate
    pub fn face_ref(&self, component: Id, face: usize) -> Option<FaceRef> {
        let info = self.part(component)?.faces.get(face)?;
        match info.surface {
            Surface::Plane { normal, .. } => {
                Some(FaceRef { component, kind: FaceKind::Plane, point: info.center, dir: normal })
            }
            Surface::Cylinder { axis, .. } => {
                Some(FaceRef { component, kind: FaceKind::Cylinder, point: info.center, dir: axis })
            }
            Surface::Other => None,
        }
    }

    /// Index of the face a reference points to, in the current part
    pub fn resolve(&self, r: &FaceRef) -> Option<usize> {
        let part = self.part(r.component)?;
        let size = part
            .solid
            .as_ref()
            .and_then(|s| s.bounds())
            .map(|(lo, hi)| (hi - lo).length())
            .unwrap_or(1.0);
        part.faces
            .iter()
            .enumerate()
            .filter(|(_, f)| match (r.kind, f.surface) {
                (FaceKind::Plane, Surface::Plane { normal, .. }) => normal.dot(r.dir) > 0.999,
                (FaceKind::Cylinder, Surface::Cylinder { axis, .. }) => axis.dot(r.dir).abs() > 0.999,
                _ => false,
            })
            .map(|(i, f)| (i, (f.center - r.point).length()))
            .filter(|(_, d)| *d <= size.max(1.0) * 0.5)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    fn geom(&self, r: &FaceRef) -> Option<Geom> {
        let face = self.part(r.component)?.faces.get(self.resolve(r)?)?;
        match face.surface {
            Surface::Plane { normal, .. } => Some(Geom::Plane { point: face.center, normal }),
            Surface::Cylinder { origin, axis, .. } => Some(Geom::Axis { point: origin, dir: axis }),
            Surface::Other => None,
        }
    }

    /// Whether the two faces' normals currently point the same way
    pub fn currently_aligned(&self, a: &FaceRef, b: &FaceRef) -> bool {
        let dir = |r: &FaceRef| self.component(r.component).map(|c| c.placement.vector(r.dir)).unwrap_or(r.dir);
        dir(a).dot(dir(b)) > 0.0
    }

    // ---------------------------------------------------------------- mates

    fn next_mate_name(&self, kind: &MateKind) -> String {
        let label = kind.label();
        (1..)
            .map(|n| format!("{label}{n}"))
            .find(|n| self.mates.iter().all(|m| &m.name != n))
            .expect("unbounded")
    }

    fn check_mate(&self, kind: &MateKind, a: &FaceRef, b: &FaceRef) -> Result<()> {
        if a.component == b.component {
            return Err(NovaError::InvalidFeature("choisissez des faces de deux composants différents".into()));
        }
        let (Some(ga), Some(gb)) = (self.geom(a), self.geom(b)) else {
            return Err(NovaError::InvalidFeature("face introuvable".into()));
        };
        if !solver::supported(kind.relation(), &ga, &gb) {
            return Err(NovaError::InvalidFeature(format!(
                "une contrainte {} demande deux faces cylindriques",
                kind.label().to_lowercase()
            )));
        }
        Ok(())
    }

    /// Add a mate and solve. A mate that cannot be satisfied is refused and
    /// the assembly is left as it was.
    pub fn add_mate(&mut self, kind: MateKind, a: FaceRef, b: FaceRef, aligned: bool) -> Result<Id> {
        self.check_mate(&kind, &a, &b)?;
        let id = Id::new();
        let name = self.next_mate_name(&kind);
        let before = self.clone();
        self.mates.push(Mate { id, name, kind, a, b, aligned, suppressed: false });
        self.solve();
        if self.mate_state(id) != Some(&MateState::Satisfied) {
            *self = before;
            return Err(NovaError::ConstraintError(
                "cette contrainte surcontraint l'assemblage (elle contredit les contraintes existantes)".into(),
            ));
        }
        Ok(id)
    }

    /// Change a mate's type, value or alignment; refused if it cannot hold
    pub fn edit_mate(&mut self, id: Id, kind: MateKind, aligned: bool) -> Result<()> {
        let m = self.mate(id).ok_or_else(|| NovaError::DocumentError("unknown mate".into()))?.clone();
        self.check_mate(&kind, &m.a, &m.b)?;
        let before = self.clone();
        if let Some(m) = self.mates.iter_mut().find(|m| m.id == id) {
            m.kind = kind;
            m.aligned = aligned;
        }
        self.solve();
        if self.mate_state(id) != Some(&MateState::Satisfied) {
            *self = before;
            return Err(NovaError::ConstraintError("cette valeur ne peut pas être satisfaite".into()));
        }
        Ok(())
    }

    pub fn remove_mate(&mut self, id: Id) {
        self.mates.retain(|m| m.id != id);
        self.solve();
    }

    pub fn set_mate_suppressed(&mut self, id: Id, suppressed: bool) {
        if let Some(m) = self.mates.iter_mut().find(|m| m.id == id) {
            m.suppressed = suppressed;
        }
        self.solve();
    }

    pub fn rename(&mut self, id: Id, name: String) {
        if let Some(c) = self.component_mut(id) {
            c.name = name;
        } else if let Some(m) = self.mates.iter_mut().find(|m| m.id == id) {
            m.name = name;
        }
    }

    // ---------------------------------------------------------------- solve

    /// Move the free components so the mates hold
    pub fn solve(&mut self) -> SolveSummary {
        self.load_parts();
        let index: HashMap<Id, usize> = self.components.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        let bodies: Vec<Body> = self
            .components
            .iter()
            .map(|c| Body {
                placement: c.placement,
                free: !c.fixed && !c.suppressed,
                pivot: self.parts.get(&self.part_file(c)).map(|p| p.center).unwrap_or_default(),
            })
            .collect();
        self.mate_states.clear();
        let mut eqs = Vec::new();
        let mut eq_mates = Vec::new();
        for m in &self.mates {
            let off = |id: Id| index.get(&id).map(|&k| self.components[k].suppressed).unwrap_or(true);
            if m.suppressed || off(m.a.component) || off(m.b.component) {
                self.mate_states.insert(m.id, MateState::Suppressed);
                continue;
            }
            let (Some(ga), Some(gb)) = (self.geom(&m.a), self.geom(&m.b)) else {
                self.mate_states.insert(m.id, MateState::MissingFace);
                continue;
            };
            eqs.push(Equation {
                a: (index[&m.a.component], ga),
                b: (index[&m.b.component], gb),
                relation: m.kind.relation(),
                aligned: m.aligned,
            });
            eq_mates.push(m.id);
        }
        let out = solver::solve(&bodies, &eqs);
        let mut unsatisfied = 0;
        for (id, r) in eq_mates.iter().zip(&out.residuals) {
            let ok = *r < solver::TOLERANCE;
            unsatisfied += usize::from(!ok);
            self.mate_states.insert(*id, if ok { MateState::Satisfied } else { MateState::Unsatisfied });
        }
        // Keep the old positions when the mates cannot all hold, like SolidWorks
        if unsatisfied == 0 {
            for (c, p) in self.components.iter_mut().zip(&out.placements) {
                c.placement = *p;
            }
        }
        self.component_states = self
            .components
            .iter()
            .zip(&out.dof)
            .map(|(c, &dof)| {
                let state = if c.fixed {
                    ComponentState::Fixed
                } else if dof > 0 {
                    ComponentState::UnderDefined(dof)
                } else {
                    ComponentState::FullyDefined
                };
                (c.id, state)
            })
            .collect();
        self.summary = SolveSummary { iterations: out.iterations, unsatisfied, redundant: out.redundant };
        self.summary
    }

    pub fn component_state(&self, id: Id) -> ComponentState {
        self.component_states.get(&id).copied().unwrap_or_else(|| {
            if self.component(id).map(|c| c.fixed).unwrap_or(false) {
                ComponentState::Fixed
            } else {
                ComponentState::UnderDefined(6)
            }
        })
    }

    pub fn mate_state(&self, id: Id) -> Option<&MateState> {
        self.mate_states.get(&id)
    }

    pub fn summary(&self) -> SolveSummary {
        self.summary
    }

    // ---------------------------------------------------------------- output

    /// A component's body in world coordinates
    pub fn world_solid(&self, id: Id) -> Option<Solid> {
        let c = self.component(id)?;
        self.part(id)?.solid.as_ref()?.transformed(&c.placement.matrix()).ok()
    }

    /// World bounding box of the visible components
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut out: Option<(DVec3, DVec3)> = None;
        for c in self.components.iter().filter(|c| !c.suppressed) {
            let Some((lo, hi)) = self.part(c.id).and_then(|p| p.solid.as_ref()).and_then(|s| s.bounds()) else {
                continue;
            };
            for i in 0..8 {
                let corner = DVec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                );
                let p = c.placement.point(corner);
                out = Some(match out {
                    None => (p, p),
                    Some((a, b)) => (a.min(p), b.max(p)),
                });
            }
        }
        out
    }

    /// Pairs of components that share material (touching is fine)
    pub fn interferences(&self) -> Vec<Interference> {
        let visible: Vec<(Id, Solid, (DVec3, DVec3))> = self
            .components
            .iter()
            .filter(|c| !c.suppressed)
            .filter_map(|c| {
                let s = self.world_solid(c.id)?;
                let b = s.bounds()?;
                Some((c.id, s, b))
            })
            .collect();
        let mut out = Vec::new();
        for i in 0..visible.len() {
            for j in i + 1..visible.len() {
                let (a, sa, ba) = &visible[i];
                let (b, sb, bb) = &visible[j];
                let eps = 1e-6;
                let overlap = (0..3).all(|k| ba.0[k] < bb.1[k] - eps && bb.0[k] < ba.1[k] - eps);
                if !overlap {
                    continue;
                }
                let Ok(Some(common)) = sa.intersection(sb) else { continue };
                let volume = common.volume();
                if volume > 1e-6 {
                    out.push(Interference { a: *a, b: *b, volume, solid: Some(common) });
                }
            }
        }
        out
    }

    /// Bill of materials: one row per part file, in insertion order
    pub fn bom(&self) -> Vec<BomRow> {
        let mut rows: Vec<BomRow> = Vec::new();
        for c in self.components.iter().filter(|c| !c.suppressed) {
            let path = self.part_file(c);
            if let Some(row) = rows.iter_mut().find(|r| r.path == path) {
                row.quantity += 1;
                continue;
            }
            let part = self.parts.get(&path);
            rows.push(BomRow {
                item: rows.len() + 1,
                name: part.map(|p| p.name.clone()).unwrap_or_default(),
                path,
                quantity: 1,
                volume: part.map(|p| p.volume).unwrap_or(0.0),
            });
        }
        rows
    }

    /// Write all visible components, placed, into one STEP file
    pub fn export_step(&self, path: &Path) -> Result<()> {
        let solids: Vec<Solid> =
            self.components.iter().filter(|c| !c.suppressed).filter_map(|c| self.world_solid(c.id)).collect();
        let refs: Vec<&Solid> = solids.iter().collect();
        Solid::compound(&refs)?.write_step(path)
    }

    // ---------------------------------------------------------------- files

    pub fn to_json(&self) -> Result<String> {
        let file = AssemblyFile { format: FORMAT.into(), version: FORMAT_VERSION, assembly: self.clone() };
        serde_json::to_string_pretty(&file).map_err(|e| NovaError::DocumentError(e.to_string()))
    }

    /// Parse an assembly; relative part paths start from `base_dir`.
    /// Parts are loaded and the mates solved.
    pub fn from_json(text: &str, base_dir: &Path) -> Result<Self> {
        let header: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| NovaError::DocumentError(format!("not a valid assembly file: {e}")))?;
        if header.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
            return Err(NovaError::DocumentError("not a NOVARIS assembly file".into()));
        }
        let version = header.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
        if version > FORMAT_VERSION as u64 {
            return Err(NovaError::DocumentError(format!(
                "assembly file version {version} is newer than this program supports ({FORMAT_VERSION})"
            )));
        }
        let file: AssemblyFile = serde_json::from_value(header)
            .map_err(|e| NovaError::DocumentError(format!("corrupt assembly file: {e}")))?;
        let mut asm = file.assembly;
        asm.base_dir = base_dir.to_path_buf();
        asm.solve();
        Ok(asm)
    }

    /// Save, storing part paths relative to the new file
    pub fn save(&mut self, path: &Path) -> Result<()> {
        let new_base = absolute(path).parent().map(normalize).unwrap_or_default();
        for i in 0..self.components.len() {
            let abs = self.part_file(&self.components[i]);
            self.components[i].part_path = relative_to(&abs, &new_base);
        }
        self.base_dir = new_base;
        std::fs::write(path, self.to_json()?)?;
        Ok(())
    }

    pub fn open(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let base = absolute(path).parent().map(normalize).unwrap_or_default();
        Self::from_json(&text, &base)
    }
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    }
}

/// Remove `.` and `..` without touching the file system
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in p.components() {
        match part {
            PathPart::CurDir => {}
            PathPart::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` relative to `base` (both absolute); absolute if they share no root
fn relative_to(path: &Path, base: &Path) -> PathBuf {
    let p: Vec<_> = path.components().collect();
    let b: Vec<_> = base.components().collect();
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return path.to_path_buf();
    }
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in &p[common..] {
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests;
