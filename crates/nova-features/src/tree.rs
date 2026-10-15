//! Feature tree: manages features and their dependencies

use crate::{Feature, FeatureId};
use nova_core::{Id, Result, NovaError};
use nova_kernel::BRep;
use nova_sketch::Sketch;
use std::collections::HashMap;

/// Context for feature evaluation
pub struct FeatureContext {
    sketches: HashMap<Id, Sketch>,
    computed: HashMap<FeatureId, BRep>,
}

impl FeatureContext {
    pub fn new() -> Self {
        FeatureContext {
            sketches: HashMap::new(),
            computed: HashMap::new(),
        }
    }

    /// Add a sketch to the context
    pub fn add_sketch(&mut self, sketch: Sketch) {
        self.sketches.insert(sketch.id, sketch);
    }

    /// Get a sketch by ID
    pub fn get_sketch(&self, id: Id) -> Result<Sketch> {
        self.sketches
            .get(&id)
            .cloned()
            .ok_or_else(|| NovaError::DocumentError("Sketch not found".into()))
    }

    /// Cache computed feature result
    pub fn cache_result(&mut self, feature_id: FeatureId, brep: BRep) {
        self.computed.insert(feature_id, brep);
    }

    /// Get cached result
    pub fn get_cached(&self, feature_id: FeatureId) -> Option<BRep> {
        self.computed.get(&feature_id).cloned()
    }

    /// Clear all cached results (for recalculation)
    pub fn invalidate_cache(&mut self) {
        self.computed.clear();
    }
}

impl Default for FeatureContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Feature tree: ordered list of features with dependency tracking
pub struct FeatureTree {
    features: Vec<Feature>,
}

impl FeatureTree {
    pub fn new() -> Self {
        FeatureTree {
            features: Vec::new(),
        }
    }

    /// Add a feature to the tree
    pub fn add_feature(&mut self, feature: Feature) {
        self.features.push(feature);
    }

    /// Get feature by ID
    pub fn get_feature(&self, id: FeatureId) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }

    /// Get all features
    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    /// Get the parent feature (previous in tree)
    pub fn get_parent(&self, feature_id: FeatureId) -> Option<&Feature> {
        let idx = self.features.iter().position(|f| f.id == feature_id)?;
        if idx > 0 {
            Some(&self.features[idx - 1])
        } else {
            None
        }
    }

    /// Count total features
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Get index of feature
    pub fn index_of(&self, id: FeatureId) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }

    /// Evaluate all features in order
    pub fn evaluate(&self, context: &mut FeatureContext) -> Result<Vec<BRep>> {
        let mut results = Vec::new();

        for feature in &self.features {
            if feature.suppressed {
                continue;
            }

            // For now, just track that feature was processed
            // Real implementation would call feature.evaluate(context)
            // and cache results

            results.push(BRep::new());
        }

        Ok(results)
    }

    /// Recompute from a specific feature onwards
    pub fn recompute_from(&self, feature_id: FeatureId, context: &mut FeatureContext) -> Result<()> {
        let start_idx = self.index_of(feature_id)
            .ok_or_else(|| NovaError::OperationFailed("Feature not found".into()))?;

        // Invalidate cache from this point onwards
        context.invalidate_cache();

        // Re-evaluate all features from start_idx onwards
        for i in start_idx..self.features.len() {
            let _feature = &self.features[i];
            // Evaluate feature i using previous results from context
            // (Real implementation would do this)
        }

        Ok(())
    }
}

impl Default for FeatureTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Feature;

    #[test]
    fn test_feature_tree_creation() {
        let tree = FeatureTree::new();
        assert!(tree.is_empty());
    }

    #[test]
    fn test_add_feature() {
        let mut tree = FeatureTree::new();
        let sketch_id = Id::new();
        let feat = Feature::extrusion("Extrusion1", sketch_id, 20.0);

        tree.add_feature(feat.clone());
        assert_eq!(tree.len(), 1);

        let retrieved = tree.get_feature(feat.id);
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_feature_ordering() {
        let mut tree = FeatureTree::new();
        let sketch_id = Id::new();

        let feat1 = Feature::extrusion("Extrusion1", sketch_id, 20.0);
        let feat2 = Feature::pocket("Pocket1", sketch_id, 10.0);

        tree.add_feature(feat1.clone());
        tree.add_feature(feat2.clone());

        // Check ordering
        assert_eq!(tree.features[0].id, feat1.id);
        assert_eq!(tree.features[1].id, feat2.id);

        // Check parent relationship
        let parent = tree.get_parent(feat2.id);
        assert!(parent.is_some());
        assert_eq!(parent.unwrap().id, feat1.id);
    }

    #[test]
    fn test_recompute() {
        let mut tree = FeatureTree::new();
        let sketch_id = Id::new();

        let feat = Feature::extrusion("Extrusion1", sketch_id, 20.0);
        tree.add_feature(feat.clone());

        let mut context = FeatureContext::new();
        let result = tree.recompute_from(feat.id, &mut context);

        assert!(result.is_ok());
    }
}
