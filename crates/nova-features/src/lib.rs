//! NOVARIS CAD — Parametric feature tree
//!
//! A part is an ordered list of features (the FeatureManager tree in
//! SolidWorks). Rebuilding replays them in order: sketches are solved, then
//! extrusions, revolutions, holes and patterns add (boss) or remove (cut)
//! material from the single body.
//! A failing feature records its error and is skipped; the rest still
//! rebuild, as in SolidWorks.

use glam::{DVec2, DVec3};
use nova_core::{Id, NovaError, Result};
use nova_kernel::{mirror, rotation_about, translation, BooleanOp, Loop, Plane, Prism, Profile, Revolve, Solid, Tool};
use nova_sketch::{Sketch, SolveStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type FeatureId = Id;

/// How far an extrusion goes
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum EndCondition {
    /// Fixed depth from the sketch plane
    Blind { depth: f64 },
    /// Through every existing body
    ThroughAll,
    /// Fixed total depth, centred on the sketch plane
    MidPlane { depth: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtrudeOp {
    /// Add material
    Boss,
    /// Remove material
    Cut,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Extrude {
    /// Feature id of the sketch to extrude
    pub sketch: FeatureId,
    pub end: EndCondition,
    /// Extrude opposite to the sketch plane normal
    pub reverse: bool,
    pub op: ExtrudeOp,
}

/// Revolve a sketch around one of its centerlines
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevolveFeature {
    pub sketch: FeatureId,
    /// Centerline of the sketch used as axis (None = its first centerline)
    pub axis: Option<Id>,
    pub angle_deg: f64,
    /// Sweep symmetrically on both sides of the sketch plane
    pub mid_plane: bool,
    pub reverse: bool,
    pub op: ExtrudeOp,
}

/// A plane to build from
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlaneBase {
    Front,
    Top,
    Right,
    /// A fixed plane, e.g. taken from a face
    Fixed(Plane),
    /// Another reference plane feature
    Feature(FeatureId),
}

/// Reference plane offset from a base plane
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefPlane {
    pub base: PlaneBase,
    pub offset: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum HoleKind {
    Simple { diameter: f64 },
    Counterbore { diameter: f64, cb_diameter: f64, cb_depth: f64 },
}

/// Holes drilled at the free points of a sketch, into the material
/// (against the sketch normal unless reversed)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hole {
    pub sketch: FeatureId,
    pub kind: HoleKind,
    /// Blind or ThroughAll (MidPlane is treated as Blind)
    pub end: EndCondition,
    pub reverse: bool,
}

/// World axis through the origin
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn dir(self) -> DVec3 {
        match self {
            Axis::X => DVec3::X,
            Axis::Y => DVec3::Y,
            Axis::Z => DVec3::Z,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PatternKind {
    /// `count` instances (seed included) every `spacing` along an axis
    Linear { axis: Axis, spacing: f64, count: usize, reverse: bool },
    /// `count` instances (seed included) spread over `angle_deg` around an axis
    Circular { axis: Axis, angle_deg: f64, count: usize },
    /// Copy of the seeds mirrored across a plane
    Mirror { plane: PlaneBase },
}

/// Repeat earlier extrusions, revolutions or holes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pattern {
    pub seeds: Vec<FeatureId>,
    pub kind: PatternKind,
}

impl Pattern {
    /// Transforms of the copies (the seed itself is not included)
    pub fn transforms(&self, planes: &dyn Fn(PlaneBase) -> Result<Plane>) -> Result<Vec<glam::DMat4>> {
        match self.kind {
            PatternKind::Linear { axis, spacing, count, reverse } => {
                let d = axis.dir() * if reverse { -spacing } else { spacing };
                Ok((1..count).map(|i| translation(d * i as f64)).collect())
            }
            PatternKind::Circular { axis, angle_deg, count } => {
                // A full turn spreads instances evenly without repeating the seed
                let full = (angle_deg - 360.0).abs() < 1e-9;
                let step = angle_deg.to_radians() / if full { count as f64 } else { (count.max(2) - 1) as f64 };
                Ok((1..count).map(|i| rotation_about(DVec3::ZERO, axis.dir(), step * i as f64)).collect())
            }
            PatternKind::Mirror { plane } => Ok(vec![mirror(&planes(plane)?)]),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FeatureKind {
    Sketch(Sketch),
    Extrude(Extrude),
    Revolve(RevolveFeature),
    RefPlane(RefPlane),
    Hole(Hole),
    Pattern(Pattern),
}

impl FeatureKind {
    /// Features this one refers to
    pub fn references(&self) -> Vec<FeatureId> {
        let plane_ref = |b: &PlaneBase| match b {
            PlaneBase::Feature(id) => vec![*id],
            _ => vec![],
        };
        match self {
            FeatureKind::Sketch(s) => s.support.into_iter().collect(),
            FeatureKind::Extrude(e) => vec![e.sketch],
            FeatureKind::Revolve(r) => vec![r.sketch],
            FeatureKind::Hole(h) => vec![h.sketch],
            FeatureKind::RefPlane(p) => plane_ref(&p.base),
            FeatureKind::Pattern(p) => {
                let mut v = p.seeds.clone();
                if let PatternKind::Mirror { plane } = &p.kind {
                    v.extend(plane_ref(plane));
                }
                v
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    pub suppressed: bool,
    pub kind: FeatureKind,
}

/// Rebuild state of one feature
#[derive(Clone, Debug, PartialEq)]
pub enum FeatureState {
    Ok,
    /// Sketch solved but still has free degrees of freedom
    UnderDefined { dof: usize },
    Suppressed,
    /// After the rollback bar
    RolledBack,
    Error(String),
}

impl FeatureState {
    pub fn is_error(&self) -> bool {
        matches!(self, FeatureState::Error(_))
    }
}

/// The part's feature history and its last rebuild result
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FeatureTree {
    features: Vec<Feature>,
    /// Number of features before the rollback bar (None = all)
    rollback: Option<usize>,
    #[serde(skip)]
    body: Option<Solid>,
    #[serde(skip)]
    states: Vec<FeatureState>,
    /// Result after each feature, keyed by a hash of the history up to it
    #[serde(skip)]
    cache: Vec<CacheEntry>,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    key: u64,
    body: Option<Solid>,
    state: FeatureState,
    /// Material the feature applied (reused by patterns)
    tools: Option<(ExtrudeOp, Vec<Tool>)>,
    /// Plane produced by a reference plane feature
    plane: Option<Plane>,
}

/// Per-feature results gathered during a rebuild
#[derive(Default)]
struct RebuildContext {
    tools: HashMap<FeatureId, (ExtrudeOp, Vec<Tool>)>,
    planes: HashMap<FeatureId, Plane>,
}

impl RebuildContext {
    fn plane(&self, base: PlaneBase) -> Result<Plane> {
        Ok(match base {
            PlaneBase::Front => Plane::front(),
            PlaneBase::Top => Plane::top(),
            PlaneBase::Right => Plane::right(),
            PlaneBase::Fixed(p) => p,
            PlaneBase::Feature(id) => *self
                .planes
                .get(&id)
                .ok_or_else(|| NovaError::InvalidFeature("its reference plane is missing or has errors".into()))?,
        })
    }
}

/// Add or remove each tool in turn
fn apply_tools(body: Option<&Solid>, op: ExtrudeOp, tools: &[Tool]) -> Result<Solid> {
    let mut result = body.cloned();
    for tool in tools {
        result = Some(match (result, op) {
            (None, ExtrudeOp::Boss) => Solid::from_tool(tool)?,
            (Some(b), ExtrudeOp::Boss) => b.boolean(BooleanOp::Union, tool)?,
            (Some(b), ExtrudeOp::Cut) => b.boolean(BooleanOp::Subtract, tool)?,
            (None, ExtrudeOp::Cut) => return Err(NovaError::InvalidFeature("there is no material to cut".into())),
        });
    }
    result.ok_or_else(|| NovaError::InvalidFeature("nothing to build".into()))
}

fn hash_feature(previous: u64, feature: &Feature) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    previous.hash(&mut h);
    // Serialisation covers every parameter that affects the result
    serde_json::to_string(feature).unwrap_or_default().hash(&mut h);
    h.finish()
}

impl FeatureTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    pub fn len(&self) -> usize {
        self.features.len()
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    pub fn get(&self, id: FeatureId) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }

    pub fn get_mut(&mut self, id: FeatureId) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.id == id)
    }

    pub fn index_of(&self, id: FeatureId) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }

    fn next_name(&self, prefix: &str) -> String {
        let n = self.features.iter().filter(|f| f.name.starts_with(prefix)).count();
        format!("{}{}", prefix, n + 1)
    }

    /// Insert a feature at the rollback bar (or at the end)
    fn insert(&mut self, feature: Feature) -> FeatureId {
        let id = feature.id;
        match self.rollback {
            Some(at) => {
                self.features.insert(at, feature);
                self.rollback = Some(at + 1);
            }
            None => self.features.push(feature),
        }
        id
    }

    /// Add a sketch feature; returns its feature id
    pub fn add_sketch(&mut self, mut sketch: Sketch) -> FeatureId {
        let name = self.next_name("Esquisse");
        if sketch.name.is_empty() {
            sketch.name = name.clone();
        }
        self.insert(Feature {
            id: Id::new(),
            name,
            suppressed: false,
            kind: FeatureKind::Sketch(sketch),
        })
    }

    /// Add an extrusion of an earlier sketch feature
    pub fn add_extrude(&mut self, extrude: Extrude) -> Result<FeatureId> {
        let sketch_index = self
            .index_of(extrude.sketch)
            .filter(|&i| matches!(self.features[i].kind, FeatureKind::Sketch(_)))
            .ok_or_else(|| NovaError::InvalidFeature("extrusion must reference a sketch feature".into()))?;
        let insert_at = self.rollback.unwrap_or(self.features.len());
        if sketch_index >= insert_at {
            return Err(NovaError::InvalidFeature("the sketch must come before the extrusion".into()));
        }
        validate_end(&extrude.end)?;
        let name = self.next_name(match extrude.op {
            ExtrudeOp::Boss => "Bossage-Extrusion",
            ExtrudeOp::Cut => "Enlèvement-Extrusion",
        });
        Ok(self.insert(Feature {
            id: Id::new(),
            name,
            suppressed: false,
            kind: FeatureKind::Extrude(extrude),
        }))
    }

    /// Add any feature (at the rollback bar). References must point to
    /// earlier features of the right kind.
    pub fn add_feature(&mut self, kind: FeatureKind) -> Result<FeatureId> {
        let insert_at = self.rollback.unwrap_or(self.features.len());
        let earlier = |id: FeatureId| self.index_of(id).filter(|&i| i < insert_at).map(|i| &self.features[i].kind);
        let is_sketch = |id| matches!(earlier(id), Some(FeatureKind::Sketch(_)));
        let is_plane = |b: &PlaneBase| match b {
            PlaneBase::Feature(id) => matches!(earlier(*id), Some(FeatureKind::RefPlane(_))),
            _ => true,
        };
        let name = match &kind {
            FeatureKind::Sketch(_) => {
                let FeatureKind::Sketch(s) = kind else { unreachable!() };
                if let Some(id) = s.support {
                    if !matches!(earlier(id), Some(FeatureKind::RefPlane(_))) {
                        return Err(NovaError::InvalidFeature("a sketch can only lie on an earlier plane".into()));
                    }
                }
                return Ok(self.add_sketch(s));
            }
            FeatureKind::Extrude(e) => {
                let e = e.clone();
                return self.add_extrude(e);
            }
            FeatureKind::Revolve(r) => {
                if !is_sketch(r.sketch) {
                    return Err(NovaError::InvalidFeature("revolve must reference an earlier sketch".into()));
                }
                if !(r.angle_deg > 0.0 && r.angle_deg <= 360.0) {
                    return Err(NovaError::InvalidFeature("angle must be between 0 and 360°".into()));
                }
                match r.op {
                    ExtrudeOp::Boss => "Bossage-Révolution",
                    ExtrudeOp::Cut => "Enlèvement-Révolution",
                }
            }
            FeatureKind::RefPlane(p) => {
                if !is_plane(&p.base) || !p.offset.is_finite() {
                    return Err(NovaError::InvalidFeature("invalid reference plane".into()));
                }
                "Plan"
            }
            FeatureKind::Hole(h) => {
                if !is_sketch(h.sketch) {
                    return Err(NovaError::InvalidFeature("hole must reference an earlier sketch of points".into()));
                }
                validate_hole(h)?;
                "Perçage"
            }
            FeatureKind::Pattern(p) => {
                if p.seeds.is_empty() {
                    return Err(NovaError::InvalidFeature("select at least one feature to repeat".into()));
                }
                for s in &p.seeds {
                    if !matches!(earlier(*s), Some(FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Hole(_) | FeatureKind::Pattern(_))) {
                        return Err(NovaError::InvalidFeature("only earlier extrusions, revolutions, holes or patterns can be repeated".into()));
                    }
                }
                validate_pattern(&p.kind)?;
                if let PatternKind::Mirror { plane } = &p.kind {
                    if !is_plane(plane) {
                        return Err(NovaError::InvalidFeature("invalid mirror plane".into()));
                    }
                }
                match p.kind {
                    PatternKind::Linear { .. } => "Répétition linéaire",
                    PatternKind::Circular { .. } => "Répétition circulaire",
                    PatternKind::Mirror { .. } => "Symétrie",
                }
            }
        };
        let name = self.next_name(name);
        Ok(self.insert(Feature { id: Id::new(), name, suppressed: false, kind }))
    }

    /// Features that reference `id`
    pub fn dependents(&self, id: FeatureId) -> Vec<FeatureId> {
        self.features
            .iter()
            .filter(|f| f.kind.references().contains(&id))
            .map(|f| f.id)
            .collect()
    }

    /// Delete a feature. Refuses while other features still depend on it.
    pub fn remove(&mut self, id: FeatureId) -> Result<Feature> {
        let deps = self.dependents(id);
        if !deps.is_empty() {
            return Err(NovaError::InvalidFeature(format!("{} feature(s) depend on this one", deps.len())));
        }
        let i = self.index_of(id).ok_or_else(|| NovaError::InvalidFeature("unknown feature".into()))?;
        if let Some(r) = self.rollback.as_mut() {
            if i < *r {
                *r -= 1;
            }
        }
        Ok(self.features.remove(i))
    }

    pub fn rename(&mut self, id: FeatureId, name: impl Into<String>) -> Result<()> {
        self.get_mut(id).ok_or_else(|| NovaError::InvalidFeature("unknown feature".into()))?.name = name.into();
        Ok(())
    }

    pub fn set_suppressed(&mut self, id: FeatureId, suppressed: bool) -> Result<()> {
        self.get_mut(id).ok_or_else(|| NovaError::InvalidFeature("unknown feature".into()))?.suppressed = suppressed;
        Ok(())
    }

    /// Place the rollback bar after `count` features (None = after all)
    pub fn set_rollback(&mut self, count: Option<usize>) {
        self.rollback = count.map(|c| c.min(self.features.len())).filter(|&c| c < self.features.len());
    }

    pub fn rollback(&self) -> Option<usize> {
        self.rollback
    }

    pub fn sketch_mut(&mut self, id: FeatureId) -> Option<&mut Sketch> {
        match &mut self.get_mut(id)?.kind {
            FeatureKind::Sketch(s) => Some(s),
            _ => None,
        }
    }

    pub fn extrude_mut(&mut self, id: FeatureId) -> Option<&mut Extrude> {
        match &mut self.get_mut(id)?.kind {
            FeatureKind::Extrude(e) => Some(e),
            _ => None,
        }
    }

    /// Change an extrusion's depth (blind or mid-plane)
    pub fn set_extrude_depth(&mut self, id: FeatureId, depth: f64) -> Result<()> {
        let e = self.extrude_mut(id).ok_or_else(|| NovaError::InvalidFeature("not an extrusion".into()))?;
        let end = match e.end {
            EndCondition::Blind { .. } => EndCondition::Blind { depth },
            EndCondition::MidPlane { .. } => EndCondition::MidPlane { depth },
            EndCondition::ThroughAll => return Err(NovaError::InvalidFeature("through-all has no depth".into())),
        };
        validate_end(&end)?;
        e.end = end;
        Ok(())
    }

    /// Result of the last rebuild
    pub fn body(&self) -> Option<&Solid> {
        self.body.as_ref()
    }

    /// State of each feature after the last rebuild (same order as `features`)
    pub fn states(&self) -> &[FeatureState] {
        &self.states
    }

    pub fn state(&self, id: FeatureId) -> Option<&FeatureState> {
        self.states.get(self.index_of(id)?)
    }

    /// Plane computed for a reference plane feature at the last rebuild
    pub fn feature_plane(&self, id: FeatureId) -> Option<Plane> {
        self.cache.get(self.index_of(id)?)?.plane
    }

    /// Replay every feature up to the rollback bar. Features whose history
    /// is unchanged since the last rebuild reuse their cached result.
    pub fn rebuild(&mut self) -> Option<&Solid> {
        let mut body: Option<Solid> = None;
        let mut states = Vec::with_capacity(self.features.len());
        let active = self.rollback.unwrap_or(self.features.len());
        let mut key = 0u64;
        let mut cache = Vec::with_capacity(self.features.len());
        let mut ctx = RebuildContext::default();

        for i in 0..self.features.len() {
            if i >= active {
                states.push(FeatureState::RolledBack);
                continue;
            }
            if self.features[i].suppressed {
                states.push(FeatureState::Suppressed);
                key = hash_feature(key, &self.features[i]);
                cache.push(CacheEntry { key, body: body.clone(), state: FeatureState::Suppressed, tools: None, plane: None });
                continue;
            }
            // Sketches on reference planes follow them; then they are solved,
            // so their key reflects the solved geometry
            let sketch_state = if let FeatureKind::Sketch(sketch) = &mut self.features[i].kind {
                let support = sketch.support.map(|id| ctx.planes.get(&id).copied());
                if let Some(Some(p)) = support {
                    sketch.plane = p;
                }
                Some(match sketch.solve() {
                    _ if matches!(support, Some(None)) => FeatureState::Error("its plane is missing or has errors".into()),
                    Err(e) => FeatureState::Error(e.to_string()),
                    Ok(r) => match r.status {
                        SolveStatus::Inconsistent => FeatureState::Error("sketch relations conflict".into()),
                        SolveStatus::UnderConstrained { dof } => FeatureState::UnderDefined { dof },
                        _ => FeatureState::Ok,
                    },
                })
            } else {
                None
            };
            key = hash_feature(key, &self.features[i]);
            let id = self.features[i].id;
            if let Some(hit) = self.cache.get(i).filter(|c| c.key == key) {
                body = hit.body.clone();
                states.push(hit.state.clone());
                if let Some(t) = &hit.tools {
                    ctx.tools.insert(id, t.clone());
                }
                if let Some(p) = hit.plane {
                    ctx.planes.insert(id, p);
                }
                cache.push(hit.clone());
                continue;
            }
            let mut entry_tools = None;
            let mut entry_plane = None;
            let kind = self.features[i].kind.clone();
            let state = match &kind {
                FeatureKind::Sketch(_) => sketch_state.expect("solved above"),
                FeatureKind::RefPlane(rp) => match ctx.plane(rp.base) {
                    Ok(p) => {
                        let p = p.offset(rp.offset);
                        ctx.planes.insert(id, p);
                        entry_plane = Some(p);
                        FeatureState::Ok
                    }
                    Err(e) => FeatureState::Error(e.to_string()),
                },
                _ => match self.material(&kind, &states, body.as_ref(), &ctx).and_then(|(op, tools)| {
                    let solid = apply_tools(body.as_ref(), op, &tools)?;
                    Ok((solid, op, tools))
                }) {
                    Ok((solid, op, tools)) => {
                        body = Some(solid);
                        ctx.tools.insert(id, (op, tools.clone()));
                        entry_tools = Some((op, tools));
                        FeatureState::Ok
                    }
                    Err(e) => FeatureState::Error(e.to_string()),
                },
            };
            cache.push(CacheEntry { key, body: body.clone(), state: state.clone(), tools: entry_tools, plane: entry_plane });
            states.push(state);
        }

        self.body = body;
        self.states = states;
        self.cache = cache;
        self.body.as_ref()
    }

    /// An earlier sketch that rebuilt without errors
    fn usable_sketch<'a>(&'a self, id: FeatureId, states: &[FeatureState]) -> Result<&'a Sketch> {
        let si = self.index_of(id).ok_or_else(|| NovaError::InvalidFeature("its sketch was deleted".into()))?;
        match (&self.features[si].kind, states.get(si)) {
            (FeatureKind::Sketch(s), Some(FeatureState::Ok | FeatureState::UnderDefined { .. })) => Ok(s),
            (FeatureKind::Sketch(_), Some(FeatureState::Suppressed)) => Err(NovaError::InvalidFeature("its sketch is suppressed".into())),
            (FeatureKind::Sketch(_), Some(_)) => Err(NovaError::InvalidFeature("its sketch has errors".into())),
            _ => Err(NovaError::InvalidFeature("its sketch must come before it".into())),
        }
    }

    /// The material a feature adds or removes
    fn material(
        &self,
        kind: &FeatureKind,
        states: &[FeatureState],
        body: Option<&Solid>,
        ctx: &RebuildContext,
    ) -> Result<(ExtrudeOp, Vec<Tool>)> {
        match kind {
            FeatureKind::Extrude(e) => {
                let sketch = self.usable_sketch(e.sketch, states)?;
                let (start, end) = self.extent(e.end, e.reverse, &sketch.plane, body)?;
                let tools = sketch.profiles()?.into_iter().map(|p| Prism::new(p, sketch.plane, start, end).into()).collect();
                Ok((e.op, tools))
            }
            FeatureKind::Revolve(r) => {
                let sketch = self.usable_sketch(r.sketch, states)?;
                let axis = match r.axis {
                    Some(id) => sketch.centerlines().into_iter().find(|l| l.id == id),
                    None => sketch.centerlines().into_iter().next(),
                }
                .ok_or_else(|| NovaError::InvalidFeature("the sketch needs a centerline as revolve axis".into()))?;
                let (a, b) = (sketch.get_point(axis.p1), sketch.get_point(axis.p2));
                let (Some(a), Some(b)) = (a, b) else {
                    return Err(NovaError::InvalidFeature("revolve axis is broken".into()));
                };
                let angle = r.angle_deg.to_radians();
                let start = if r.mid_plane {
                    -angle / 2.0
                } else if r.reverse {
                    -angle
                } else {
                    0.0
                };
                let tools = sketch
                    .profiles()?
                    .into_iter()
                    .map(|p| {
                        let mut rev = Revolve::new(p, sketch.plane, DVec2::new(a.x, a.y), DVec2::new(b.x - a.x, b.y - a.y), angle);
                        rev.start_angle = start;
                        rev.into()
                    })
                    .collect();
                Ok((r.op, tools))
            }
            FeatureKind::Hole(h) => {
                let sketch = self.usable_sketch(h.sketch, states)?;
                let points = sketch.free_points();
                if points.is_empty() {
                    return Err(NovaError::InvalidFeature("place points in the sketch to position the holes".into()));
                }
                // Into the material = against the plane normal
                let into = !h.reverse;
                let (start, end) = match h.end {
                    EndCondition::Blind { depth } | EndCondition::MidPlane { depth } => {
                        if into { (-depth, 0.0) } else { (0.0, depth) }
                    }
                    EndCondition::ThroughAll => self.extent(EndCondition::ThroughAll, into, &sketch.plane, body)?,
                };
                let mut tools = Vec::new();
                for p in points {
                    let c = DVec2::new(p.x, p.y);
                    let (d, cb) = match h.kind {
                        HoleKind::Simple { diameter } => (diameter, None),
                        HoleKind::Counterbore { diameter, cb_diameter, cb_depth } => (diameter, Some((cb_diameter, cb_depth))),
                    };
                    let circle = |dia: f64| Profile::simple(Loop::circle(c, dia / 2.0));
                    // Counterbore first, then the hole through its bottom
                    if let Some((cbd, cbh)) = cb {
                        let (s0, s1) = if into { (-cbh, 0.0) } else { (0.0, cbh) };
                        tools.push(Prism::new(circle(cbd), sketch.plane, s0, s1).into());
                    }
                    tools.push(Prism::new(circle(d), sketch.plane, start, end).into());
                }
                Ok((ExtrudeOp::Cut, tools))
            }
            FeatureKind::Pattern(p) => {
                let transforms = p.transforms(&|b| ctx.plane(b))?;
                let mut op = None;
                let mut tools = Vec::new();
                for seed in &p.seeds {
                    let (seed_op, seed_tools) = ctx
                        .tools
                        .get(seed)
                        .ok_or_else(|| NovaError::InvalidFeature("a repeated feature is missing or has errors".into()))?;
                    if op.is_some_and(|o| o != *seed_op) {
                        return Err(NovaError::InvalidFeature("repeat either bosses or cuts, not both at once".into()));
                    }
                    op = Some(*seed_op);
                    for m in &transforms {
                        tools.extend(seed_tools.iter().map(|t| t.transformed(*m)));
                    }
                }
                Ok((op.unwrap_or(ExtrudeOp::Boss), tools))
            }
            FeatureKind::Sketch(_) | FeatureKind::RefPlane(_) => Ok((ExtrudeOp::Boss, Vec::new())),
        }
    }

    /// Start/end distances along the sketch normal
    fn extent(&self, end: EndCondition, reverse: bool, plane: &Plane, body: Option<&Solid>) -> Result<(f64, f64)> {
        let sign = if reverse { -1.0 } else { 1.0 };
        Ok(match end {
            EndCondition::Blind { depth } => {
                if sign > 0.0 {
                    (0.0, depth)
                } else {
                    (-depth, 0.0)
                }
            }
            EndCondition::MidPlane { depth } => (-depth / 2.0, depth / 2.0),
            EndCondition::ThroughAll => {
                let (lo, hi) = body
                    .and_then(|b| b.bounds())
                    .ok_or_else(|| NovaError::InvalidFeature("through all needs an existing body".into()))?;
                // Project the 8 box corners on the normal
                let n = plane.normal();
                let proj: Vec<f64> = (0..8)
                    .map(|k| {
                        let c = DVec3::new(
                            if k & 1 == 0 { lo.x } else { hi.x },
                            if k & 2 == 0 { lo.y } else { hi.y },
                            if k & 4 == 0 { lo.z } else { hi.z },
                        );
                        (c - plane.origin).dot(n)
                    })
                    .collect();
                let margin = 1.0 + (hi - lo).length() * 0.01;
                let max = proj.iter().cloned().fold(f64::MIN, f64::max) + margin;
                let min = proj.iter().cloned().fold(f64::MAX, f64::min) - margin;
                if sign > 0.0 {
                    if max <= 0.0 {
                        return Err(NovaError::InvalidFeature("no material in the extrusion direction".into()));
                    }
                    (0.0, max)
                } else {
                    if min >= 0.0 {
                        return Err(NovaError::InvalidFeature("no material in the extrusion direction".into()));
                    }
                    (min, 0.0)
                }
            }
        })
    }
}

fn validate_hole(h: &Hole) -> Result<()> {
    let ok = |v: f64| v > 0.0 && v.is_finite();
    let valid = match h.kind {
        HoleKind::Simple { diameter } => ok(diameter),
        HoleKind::Counterbore { diameter, cb_diameter, cb_depth } => ok(diameter) && ok(cb_depth) && cb_diameter > diameter,
    };
    if !valid {
        return Err(NovaError::InvalidFeature("hole sizes must be positive (counterbore wider than the hole)".into()));
    }
    validate_end(&h.end)
}

fn validate_pattern(k: &PatternKind) -> Result<()> {
    match *k {
        PatternKind::Linear { spacing, count, .. } if !(spacing > 0.0 && spacing.is_finite()) || count < 2 => {
            Err(NovaError::InvalidFeature("a linear pattern needs a positive spacing and at least 2 instances".into()))
        }
        PatternKind::Circular { angle_deg, count, .. } if !(angle_deg > 0.0 && angle_deg <= 360.0) || count < 2 => {
            Err(NovaError::InvalidFeature("a circular pattern needs an angle in (0, 360]° and at least 2 instances".into()))
        }
        _ => Ok(()),
    }
}

fn validate_end(end: &EndCondition) -> Result<()> {
    match *end {
        EndCondition::Blind { depth } | EndCondition::MidPlane { depth } if !(depth > 0.0 && depth.is_finite()) => {
            Err(NovaError::InvalidFeature("depth must be a positive number".into()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_sketch::{Constraint, Plane};
    use std::f64::consts::PI;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1.0)
    }

    fn plate_tree() -> (FeatureTree, FeatureId, FeatureId) {
        let mut tree = FeatureTree::new();
        let mut s = Sketch::new("");
        s.add_rectangle(0.0, 0.0, 100.0, 60.0).unwrap();
        let sk = tree.add_sketch(s);
        let ex = tree
            .add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: 20.0 }, reverse: false, op: ExtrudeOp::Boss })
            .unwrap();
        (tree, sk, ex)
    }

    fn volume(tree: &FeatureTree) -> f64 {
        tree.body().unwrap().volume()
    }

    #[test]
    fn names_follow_solidworks() {
        let (tree, sk, ex) = plate_tree();
        assert_eq!(tree.get(sk).unwrap().name, "Esquisse1");
        assert_eq!(tree.get(ex).unwrap().name, "Bossage-Extrusion1");
    }

    #[test]
    fn boss_then_through_all_hole() {
        let (mut tree, _, _) = plate_tree();
        let mut hole = Sketch::new("");
        let c = hole.add_point(50.0, 30.0);
        hole.add_circle(c, 5.0).unwrap();
        let hs = tree.add_sketch(hole);
        // Sketched on the bottom face plane: cut upward through all
        tree.add_extrude(Extrude { sketch: hs, end: EndCondition::ThroughAll, reverse: false, op: ExtrudeOp::Cut })
            .unwrap();
        tree.rebuild();
        assert!(tree.states().iter().all(|s| !s.is_error()), "{:?}", tree.states());
        assert!(close(volume(&tree), 120_000.0 - PI * 25.0 * 20.0, 1e-4), "{}", volume(&tree));
    }

    #[test]
    fn editing_depth_changes_volume() {
        let (mut tree, _, ex) = plate_tree();
        tree.rebuild();
        assert!(close(volume(&tree), 120_000.0, 1e-6));
        tree.set_extrude_depth(ex, 35.0).unwrap();
        tree.rebuild();
        assert!(close(volume(&tree), 210_000.0, 1e-6));
        assert!(tree.set_extrude_depth(ex, -1.0).is_err());
    }

    #[test]
    fn editing_a_sketch_dimension_rebuilds_the_part() {
        let mut tree = FeatureTree::new();
        let mut s = Sketch::new("");
        let pts = s.add_rectangle(0.0, 0.0, 90.0, 50.0).unwrap();
        s.add_constraint(Constraint::distance(pts[0], pts[1], 100.0));
        let sk = tree.add_sketch(s);
        tree.add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: 10.0 }, reverse: false, op: ExtrudeOp::Boss })
            .unwrap();
        tree.rebuild();
        let v1 = volume(&tree);

        // "Double-click the dimension" and change it
        let sketch = tree.sketch_mut(sk).unwrap();
        if let nova_sketch::ConstraintType::Distance { distance, .. } = &mut sketch.constraints[0].constraint_type {
            *distance = 120.0;
        }
        tree.rebuild();
        assert!(volume(&tree) > v1);
        assert!(matches!(tree.state(sk), Some(FeatureState::UnderDefined { .. })));
    }

    #[test]
    fn mid_plane_and_reverse() {
        let (mut tree, _, ex) = plate_tree();
        tree.extrude_mut(ex).unwrap().end = EndCondition::MidPlane { depth: 20.0 };
        tree.rebuild();
        let (lo, hi) = tree.body().unwrap().bounds().unwrap();
        assert!((lo.z + 10.0).abs() < 1e-9 && (hi.z - 10.0).abs() < 1e-9);

        let e = tree.extrude_mut(ex).unwrap();
        e.end = EndCondition::Blind { depth: 20.0 };
        e.reverse = true;
        tree.rebuild();
        let (lo, hi) = tree.body().unwrap().bounds().unwrap();
        assert!((lo.z + 20.0).abs() < 1e-9 && hi.z.abs() < 1e-9);
    }

    #[test]
    fn boss_on_top_plane() {
        let (mut tree, _, _) = plate_tree();
        let mut s = Sketch::on_plane("", Plane::front().offset(20.0));
        s.add_rectangle(10.0, 10.0, 20.0, 20.0).unwrap();
        let sk = tree.add_sketch(s);
        tree.add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: 15.0 }, reverse: false, op: ExtrudeOp::Boss })
            .unwrap();
        tree.rebuild();
        assert!(close(volume(&tree), 126_000.0, 1e-4), "{}", volume(&tree));
    }

    #[test]
    fn suppress_rollback_and_errors() {
        let (mut tree, _, _) = plate_tree();
        let mut s = Sketch::new("");
        s.add_rectangle(-10.0, -10.0, 300.0, 300.0).unwrap();
        let big = tree.add_sketch(s);
        let cut = tree
            .add_extrude(Extrude { sketch: big, end: EndCondition::ThroughAll, reverse: false, op: ExtrudeOp::Cut })
            .unwrap();
        tree.rebuild();
        // Removing everything is an error on that feature only
        assert!(tree.state(cut).unwrap().is_error());
        assert!(close(volume(&tree), 120_000.0, 1e-6));

        tree.set_suppressed(cut, true).unwrap();
        tree.rebuild();
        assert_eq!(tree.state(cut), Some(&FeatureState::Suppressed));

        tree.set_rollback(Some(1));
        tree.rebuild();
        assert!(tree.body().is_none());
        assert_eq!(tree.states()[1], FeatureState::RolledBack);
    }

    #[test]
    fn cannot_delete_a_used_sketch_and_open_sketch_errors() {
        let (mut tree, sk, ex) = plate_tree();
        assert!(tree.remove(sk).is_err());
        tree.remove(ex).unwrap();
        tree.remove(sk).unwrap();
        assert!(tree.is_empty());

        let mut s = Sketch::new("");
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(1.0, 0.0);
        s.add_line(a, b).unwrap();
        let sk = tree.add_sketch(s);
        let ex = tree
            .add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: 1.0 }, reverse: false, op: ExtrudeOp::Boss })
            .unwrap();
        tree.rebuild();
        assert!(matches!(tree.state(ex), Some(FeatureState::Error(m)) if m.contains("open contour")));
    }

    #[test]
    fn incremental_rebuild_reuses_unchanged_features() {
        let (mut tree, _, ex) = plate_tree();
        let mut hole = Sketch::new("");
        let c = hole.add_point(50.0, 30.0);
        hole.add_circle(c, 5.0).unwrap();
        let hs = tree.add_sketch(hole);
        let cut = tree
            .add_extrude(Extrude { sketch: hs, end: EndCondition::ThroughAll, reverse: false, op: ExtrudeOp::Cut })
            .unwrap();
        tree.rebuild();
        let full = volume(&tree);

        // Nothing changed: the second rebuild is served from the cache
        let t = std::time::Instant::now();
        tree.rebuild();
        assert!(t.elapsed().as_millis() < 50, "{:?}", t.elapsed());
        assert!((volume(&tree) - full).abs() < 1e-9);

        // Changing the first extrusion invalidates everything after it
        tree.set_extrude_depth(ex, 30.0).unwrap();
        tree.rebuild();
        assert!(close(volume(&tree), 180_000.0 - PI * 25.0 * 30.0, 1e-4), "{}", volume(&tree));
        assert_eq!(tree.state(cut), Some(&FeatureState::Ok));
    }

    fn holes_sketch(tree: &mut FeatureTree, pts: &[(f64, f64)], plane: Plane) -> FeatureId {
        let mut s = Sketch::on_plane("", plane);
        for &(x, y) in pts {
            s.add_point(x, y);
        }
        tree.add_sketch(s)
    }

    fn ok_states(tree: &FeatureTree) {
        assert!(tree.states().iter().all(|s| !s.is_error()), "{:?}", tree.states());
    }

    #[test]
    fn revolved_shaft_with_cross_hole() {
        let mut tree = FeatureTree::new();
        let mut s = Sketch::new("");
        s.add_rectangle(0.0, 0.0, 10.0, 40.0).unwrap();
        let a = s.add_point(0.0, -10.0);
        let b = s.add_point(0.0, 50.0);
        s.add_centerline(a, b).unwrap();
        let sk = tree.add_sketch(s);
        let rev = tree
            .add_feature(FeatureKind::Revolve(RevolveFeature { sketch: sk, axis: None, angle_deg: 360.0, mid_plane: false, reverse: false, op: ExtrudeOp::Boss }))
            .unwrap();
        assert_eq!(tree.get(rev).unwrap().name, "Bossage-Révolution1");
        tree.rebuild();
        ok_states(&tree);
        assert!(close(volume(&tree), PI * 100.0 * 40.0, 2e-3), "{}", volume(&tree));

        // Cross hole drilled from a plane offset in front of the shaft
        let plane = tree.add_feature(FeatureKind::RefPlane(RefPlane { base: PlaneBase::Front, offset: 15.0 })).unwrap();
        let mut hs = Sketch::new("");
        hs.support = Some(plane);
        hs.add_point(0.0, 20.0);
        let hs = tree.add_sketch(hs);
        tree.add_feature(FeatureKind::Hole(Hole { sketch: hs, kind: HoleKind::Simple { diameter: 6.0 }, end: EndCondition::ThroughAll, reverse: false }))
            .unwrap();
        tree.rebuild();
        ok_states(&tree);
        let v = volume(&tree);
        assert!(v < PI * 100.0 * 40.0 - 500.0 && v > PI * 100.0 * 40.0 - 700.0, "{}", v);
    }

    #[test]
    fn half_revolution_mid_plane() {
        let mut tree = FeatureTree::new();
        let mut s = Sketch::new("");
        s.add_rectangle(5.0, 0.0, 5.0, 10.0).unwrap();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(0.0, 10.0);
        s.add_centerline(a, b).unwrap();
        let sk = tree.add_sketch(s);
        tree.add_feature(FeatureKind::Revolve(RevolveFeature { sketch: sk, axis: None, angle_deg: 180.0, mid_plane: true, reverse: false, op: ExtrudeOp::Boss }))
            .unwrap();
        tree.rebuild();
        ok_states(&tree);
        assert!(close(volume(&tree), PI * 75.0 * 10.0 / 2.0, 3e-3), "{}", volume(&tree));
        // symmetric about the sketch plane
        let (lo, hi) = tree.body().unwrap().bounds().unwrap();
        assert!((lo.z + hi.z).abs() < 1e-6, "{:?} {:?}", lo, hi);
    }

    #[test]
    fn revolve_without_centerline_errors() {
        let (mut tree, sk, _) = plate_tree();
        let r = tree
            .add_feature(FeatureKind::Revolve(RevolveFeature { sketch: sk, axis: None, angle_deg: 90.0, mid_plane: false, reverse: false, op: ExtrudeOp::Boss }))
            .unwrap();
        tree.rebuild();
        assert!(matches!(tree.state(r), Some(FeatureState::Error(m)) if m.contains("centerline")));
    }

    #[test]
    fn sketch_follows_its_reference_plane() {
        let (mut tree, _, _) = plate_tree();
        let plane = tree.add_feature(FeatureKind::RefPlane(RefPlane { base: PlaneBase::Front, offset: 20.0 })).unwrap();
        assert_eq!(tree.get(plane).unwrap().name, "Plan1");
        let mut s = Sketch::new("");
        s.support = Some(plane);
        s.add_rectangle(10.0, 10.0, 20.0, 20.0).unwrap();
        let sk = tree.add_sketch(s);
        tree.add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: 10.0 }, reverse: false, op: ExtrudeOp::Boss }).unwrap();
        tree.rebuild();
        ok_states(&tree);
        assert!(close(volume(&tree), 124_000.0, 1e-4), "{}", volume(&tree));
        let hi = tree.body().unwrap().bounds().unwrap().1;
        assert!((hi.z - 30.0).abs() < 1e-6);

        // Move the plane: the boss now floats as a second lump at z 40..50
        if let FeatureKind::RefPlane(rp) = &mut tree.get_mut(plane).unwrap().kind {
            rp.offset = 40.0;
        }
        tree.rebuild();
        ok_states(&tree);
        assert!((tree.body().unwrap().bounds().unwrap().1.z - 50.0).abs() < 1e-6);
        assert_eq!(tree.body().unwrap().lump_count(), 2);
        // The plane cannot be deleted while the sketch uses it
        assert!(tree.remove(plane).is_err());
    }

    #[test]
    fn simple_and_counterbore_holes() {
        let (mut tree, _, _) = plate_tree();
        // Points sketched on the top face (z = 20): holes go down into the plate
        let top = Plane::front().offset(20.0);
        let hs = holes_sketch(&mut tree, &[(20.0, 20.0), (80.0, 40.0)], top);
        let h = tree
            .add_feature(FeatureKind::Hole(Hole { sketch: hs, kind: HoleKind::Simple { diameter: 8.0 }, end: EndCondition::ThroughAll, reverse: false }))
            .unwrap();
        assert_eq!(tree.get(h).unwrap().name, "Perçage1");
        tree.rebuild();
        ok_states(&tree);
        let v1 = 120_000.0 - 2.0 * PI * 16.0 * 20.0;
        assert!(close(volume(&tree), v1, 2e-4), "{}", volume(&tree));

        let cs = holes_sketch(&mut tree, &[(50.0, 30.0)], top);
        tree.add_feature(FeatureKind::Hole(Hole {
            sketch: cs,
            kind: HoleKind::Counterbore { diameter: 6.0, cb_diameter: 12.0, cb_depth: 5.0 },
            end: EndCondition::Blind { depth: 15.0 },
            reverse: false,
        }))
        .unwrap();
        tree.rebuild();
        ok_states(&tree);
        let v2 = v1 - PI * 9.0 * 15.0 - PI * (36.0 - 9.0) * 5.0;
        assert!(close(volume(&tree), v2, 3e-4), "{} vs {}", volume(&tree), v2);
    }

    #[test]
    fn linear_and_circular_patterns_of_holes() {
        let (mut tree, _, _) = plate_tree();
        let top = Plane::front().offset(20.0);
        let hs = holes_sketch(&mut tree, &[(15.0, 15.0)], top);
        let h = tree
            .add_feature(FeatureKind::Hole(Hole { sketch: hs, kind: HoleKind::Simple { diameter: 6.0 }, end: EndCondition::ThroughAll, reverse: false }))
            .unwrap();
        let lp = tree
            .add_feature(FeatureKind::Pattern(Pattern { seeds: vec![h], kind: PatternKind::Linear { axis: Axis::X, spacing: 20.0, count: 4, reverse: false } }))
            .unwrap();
        assert_eq!(tree.get(lp).unwrap().name, "Répétition linéaire1");
        tree.rebuild();
        ok_states(&tree);
        let hole = PI * 9.0 * 20.0;
        assert!(close(volume(&tree), 120_000.0 - 4.0 * hole, 3e-4), "{}", volume(&tree));

        // Pattern of the pattern along Y
        tree.add_feature(FeatureKind::Pattern(Pattern { seeds: vec![h, lp], kind: PatternKind::Linear { axis: Axis::Y, spacing: 30.0, count: 2, reverse: false } }))
            .unwrap();
        tree.rebuild();
        ok_states(&tree);
        assert!(close(volume(&tree), 120_000.0 - 8.0 * hole, 5e-4), "{}", volume(&tree));
    }

    #[test]
    fn circular_pattern_on_a_disc() {
        let mut tree = FeatureTree::new();
        let mut s = Sketch::new("");
        let c = s.add_point(0.0, 0.0);
        s.add_circle(c, 50.0).unwrap();
        let sk = tree.add_sketch(s);
        tree.add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: 10.0 }, reverse: false, op: ExtrudeOp::Boss }).unwrap();
        let hs = holes_sketch(&mut tree, &[(35.0, 0.0)], Plane::front().offset(10.0));
        let h = tree
            .add_feature(FeatureKind::Hole(Hole { sketch: hs, kind: HoleKind::Simple { diameter: 8.0 }, end: EndCondition::ThroughAll, reverse: false }))
            .unwrap();
        tree.add_feature(FeatureKind::Pattern(Pattern { seeds: vec![h], kind: PatternKind::Circular { axis: Axis::Z, angle_deg: 360.0, count: 6 } }))
            .unwrap();
        tree.rebuild();
        ok_states(&tree);
        let expected = PI * 2500.0 * 10.0 - 6.0 * PI * 16.0 * 10.0;
        assert!(close(volume(&tree), expected, 2e-3), "{} vs {}", volume(&tree), expected);
    }

    #[test]
    fn mirror_boss_and_pattern_follow_seed_edits() {
        let (mut tree, _, _) = plate_tree();
        let mut s = Sketch::on_plane("", Plane::front().offset(20.0));
        s.add_rectangle(10.0, 20.0, 20.0, 20.0).unwrap();
        let sk = tree.add_sketch(s);
        let boss = tree.add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: 10.0 }, reverse: false, op: ExtrudeOp::Boss }).unwrap();
        // Right plane is x = 0; mirror across a plane at x = 50 parallel to it
        let mid = tree.add_feature(FeatureKind::RefPlane(RefPlane { base: PlaneBase::Right, offset: 50.0 })).unwrap();
        let m = tree.add_feature(FeatureKind::Pattern(Pattern { seeds: vec![boss], kind: PatternKind::Mirror { plane: PlaneBase::Feature(mid) } })).unwrap();
        assert_eq!(tree.get(m).unwrap().name, "Symétrie1");
        tree.rebuild();
        ok_states(&tree);
        assert!(close(volume(&tree), 128_000.0, 1e-4), "{}", volume(&tree));

        // Editing the seed updates the mirrored copy too
        tree.set_extrude_depth(boss, 20.0).unwrap();
        tree.rebuild();
        ok_states(&tree);
        assert!(close(volume(&tree), 136_000.0, 1e-4), "{}", volume(&tree));
        assert!(tree.remove(boss).is_err());
    }

    #[test]
    fn invalid_new_features_are_rejected() {
        let (mut tree, sk, ex) = plate_tree();
        let bad = |k| FeatureKind::Pattern(Pattern { seeds: vec![ex], kind: k });
        assert!(tree.add_feature(bad(PatternKind::Linear { axis: Axis::X, spacing: 0.0, count: 3, reverse: false })).is_err());
        assert!(tree.add_feature(bad(PatternKind::Circular { axis: Axis::Z, angle_deg: 400.0, count: 3 })).is_err());
        assert!(tree.add_feature(FeatureKind::Pattern(Pattern { seeds: vec![sk], kind: PatternKind::Linear { axis: Axis::X, spacing: 5.0, count: 2, reverse: false } })).is_err());
        assert!(tree
            .add_feature(FeatureKind::Hole(Hole { sketch: sk, kind: HoleKind::Counterbore { diameter: 10.0, cb_diameter: 8.0, cb_depth: 2.0 }, end: EndCondition::ThroughAll, reverse: false }))
            .is_err());
    }

    #[test]
    fn serde_keeps_history_not_geometry() {
        let (mut tree, _, _) = plate_tree();
        tree.rebuild();
        let json = serde_json::to_string(&tree).unwrap();
        let mut back: FeatureTree = serde_json::from_str(&json).unwrap();
        assert!(back.body().is_none());
        back.rebuild();
        assert!(close(volume(&back), 120_000.0, 1e-6));
    }
}
