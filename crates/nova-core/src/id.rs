//! Stable identifiers for CAO entities

use serde::{Deserialize, Serialize};
use std::fmt;

/// A unique identifier using UUID v4
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id(uuid::Uuid);

impl Id {
    /// Generate a new random ID
    pub fn new() -> Self {
        Id(uuid::Uuid::new_v4())
    }

    /// Create from UUID
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Id(uuid)
    }

    /// Get the underlying UUID
    pub fn uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifier for an entity in the CAO model
pub type EntityId = Id;
