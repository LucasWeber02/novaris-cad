//! NOVARIS CAD — Parametric Features
//!
//! Features are parametric operations that transform geometry.
//! Example: Extrusion takes a sketch and creates a 3D solid.

pub mod feature;
pub mod extrusion;
pub mod tree;

pub use feature::{Feature, FeatureKind, FeatureId};
pub use extrusion::ExtrusionFeature;
pub use tree::{FeatureTree, FeatureContext};

use nova_core::Result;
use nova_kernel::BRep;

/// A feature operation that produces geometry
pub trait FeatureOp {
    /// Execute the feature and produce output geometry
    fn evaluate(&self, context: &FeatureContext) -> Result<BRep>;

    /// Get the name of this feature type
    fn feature_type(&self) -> &str;
}
