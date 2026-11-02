use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

/// Stable identifier of a feature in the parametric graph.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureId(pub String);

/// Semantic component identity. Ids encode the role of a part
/// (`frame.stringer.port`, `float.r3.c1`) rather than an index into a list, so
/// they survive regeneration and selection, placements, and history can refer
/// to them across rebuilds.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComponentId(pub String);

impl Display for FeatureId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Display for ComponentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FeatureId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ComponentId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
