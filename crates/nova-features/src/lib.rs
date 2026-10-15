//! NOVARIS CAD — Parametric feature tree
//!
//! A part is an ordered list of features (the FeatureManager tree in
//! SolidWorks). Rebuilding replays them in order: sketches are solved, then
//! extrusions add (boss) or remove (cut) material from the single body.
//! A failing feature records its error and is skipped; the rest still
//! rebuild, as in SolidWorks.

use glam::DVec3;
use nova_core::{Id, NovaError, Result};
use nova_kernel::{BooleanOp, Prism, Solid};
use nova_sketch::{Sketch, SolveStatus};
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FeatureKind {
    Sketch(Sketch),
    Extrude(Extrude),
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

    /// Features that reference `id`
    pub fn dependents(&self, id: FeatureId) -> Vec<FeatureId> {
        self.features
            .iter()
            .filter(|f| matches!(&f.kind, FeatureKind::Extrude(e) if e.sketch == id))
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

    /// Replay every feature up to the rollback bar. Features whose history
    /// is unchanged since the last rebuild reuse their cached result.
    pub fn rebuild(&mut self) -> Option<&Solid> {
        let mut body: Option<Solid> = None;
        let mut states = Vec::with_capacity(self.features.len());
        let active = self.rollback.unwrap_or(self.features.len());
        let mut key = 0u64;
        let mut cache = Vec::with_capacity(self.features.len());

        for i in 0..self.features.len() {
            if i >= active {
                states.push(FeatureState::RolledBack);
                continue;
            }
            if self.features[i].suppressed {
                states.push(FeatureState::Suppressed);
                key = hash_feature(key, &self.features[i]);
                cache.push(CacheEntry { key, body: body.clone(), state: FeatureState::Suppressed });
                continue;
            }
            // Sketches are solved first so their key reflects solved geometry
            let sketch_state = if let FeatureKind::Sketch(sketch) = &mut self.features[i].kind {
                Some(match sketch.solve() {
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
            if let Some(hit) = self.cache.get(i).filter(|c| c.key == key) {
                body = hit.body.clone();
                states.push(hit.state.clone());
                cache.push(hit.clone());
                continue;
            }
            let state = match &mut self.features[i].kind {
                FeatureKind::Sketch(_) => sketch_state.expect("solved above"),
                FeatureKind::Extrude(extrude) => {
                    let extrude = extrude.clone();
                    match self.evaluate_extrude(&extrude, &states, body.as_ref()) {
                        Ok(solid) => {
                            body = Some(solid);
                            FeatureState::Ok
                        }
                        Err(e) => FeatureState::Error(e.to_string()),
                    }
                }
            };
            cache.push(CacheEntry { key, body: body.clone(), state: state.clone() });
            states.push(state);
        }

        self.body = body;
        self.states = states;
        self.cache = cache;
        self.body.as_ref()
    }

    fn evaluate_extrude(&self, extrude: &Extrude, states: &[FeatureState], body: Option<&Solid>) -> Result<Solid> {
        let si = self
            .index_of(extrude.sketch)
            .ok_or_else(|| NovaError::InvalidFeature("its sketch was deleted".into()))?;
        let sketch = match (&self.features[si].kind, states.get(si)) {
            (FeatureKind::Sketch(s), Some(FeatureState::Ok | FeatureState::UnderDefined { .. })) => s,
            (FeatureKind::Sketch(_), Some(FeatureState::Suppressed)) => {
                return Err(NovaError::InvalidFeature("its sketch is suppressed".into()))
            }
            (FeatureKind::Sketch(_), Some(_)) => return Err(NovaError::InvalidFeature("its sketch has errors".into())),
            _ => return Err(NovaError::InvalidFeature("its sketch must come before it".into())),
        };
        let profiles = sketch.profiles()?;
        let (start, end) = self.extent(extrude, &sketch.plane, body)?;

        let mut result = body.cloned();
        for profile in profiles {
            let prism = Prism::new(profile, sketch.plane, start, end);
            result = Some(match (result, extrude.op) {
                (None, ExtrudeOp::Boss) => Solid::prism(&prism)?,
                (Some(b), ExtrudeOp::Boss) => b.boolean(BooleanOp::Union, &prism)?,
                (Some(b), ExtrudeOp::Cut) => b.boolean(BooleanOp::Subtract, &prism)?,
                (None, ExtrudeOp::Cut) => return Err(NovaError::InvalidFeature("there is no material to cut".into())),
            });
        }
        result.ok_or_else(|| NovaError::InvalidFeature("nothing to extrude".into()))
    }

    /// Start/end distances along the sketch normal
    fn extent(&self, extrude: &Extrude, plane: &nova_kernel::Plane, body: Option<&Solid>) -> Result<(f64, f64)> {
        let sign = if extrude.reverse { -1.0 } else { 1.0 };
        Ok(match extrude.end {
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
