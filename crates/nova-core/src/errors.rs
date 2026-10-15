//! Error types for NOVARIS CAD

use thiserror::Error;

#[derive(Error, Debug)]
pub enum NovaError {
    #[error("Geometry error: {0}")]
    GeometryError(String),

    #[error("Invalid sketch: {0}")]
    InvalidSketch(String),

    #[error("Invalid feature: {0}")]
    InvalidFeature(String),

    #[error("Document error: {0}")]
    DocumentError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Constraint solver error: {0}")]
    ConstraintError(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Operation failed: {0}")]
    OperationFailed(String),
}

pub type Result<T> = std::result::Result<T, NovaError>;
