//! NOVARIS CAD — Core Types and Traits
//!
//! This crate defines the fundamental types, identifiers, and traits
//! that all other modules depend on.

pub mod errors;
pub mod id;
pub mod types;
pub mod document;
pub mod command;

pub use errors::{NovaError, Result};
pub use id::{Id, EntityId};
pub use types::{Vector3, Point3, Matrix4, Transform};
pub use document::Document;
pub use command::{Command, CommandExecutor};

/// Semantic version of the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
