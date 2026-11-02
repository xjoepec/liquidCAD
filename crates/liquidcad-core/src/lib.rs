//! Authoritative parametric CAD model for liquidCAD.
//!
//! The project file holds parameters, materials, placements, and a feature
//! graph. [`Evaluator`] walks the graph in dependency order and regenerates
//! only features whose declared inputs (or parents) changed. Exact geometry is
//! described here as [`geometry::Shape`] plus a [`geometry::Pose`]; B-rep
//! construction, display tessellation, and STEP live in `liquidcad-occt`.

pub mod diagnostics;
pub mod document;
pub mod engineering;
pub mod evaluate;
pub mod feature;
mod generators;
pub mod geometry;
pub mod ids;
pub mod mates;
pub mod parameters;
pub mod project;
pub mod templates;

pub use diagnostics::{Diagnostic, Severity};
pub use document::{
    Component, ComponentKind, Document, EngineeringSummary, Sketch, SolidBox, StockInfo,
};
pub use evaluate::{rebuild, Evaluator, RebuildReport};
pub use feature::{Feature, FeatureKind};
pub use geometry::{MassProps, Pose, Shape};
pub use ids::{ComponentId, FeatureId};
pub use parameters::{Parameter, Unit};
pub use project::{Material, Placement, Project};
pub use templates::modular_dock;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Validation(String),
    #[error("unknown component {0}")]
    UnknownComponent(String),
    #[error("unknown parameter {0}")]
    UnknownParameter(String),
    #[error("unknown feature {0}")]
    UnknownFeature(String),
    #[error("unknown material {0}")]
    UnknownMaterial(String),
    #[error("cycle in feature graph")]
    FeatureCycle,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
