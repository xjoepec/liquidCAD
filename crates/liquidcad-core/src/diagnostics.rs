use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    /// Component or feature ids the UI highlights for this diagnostic.
    pub related_ids: Vec<String>,
    /// Which module produced it: `engineering`, `manufacturing`, `placement`.
    #[serde(default)]
    pub source: String,
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        source: &str,
        code: &str,
        message: impl Into<String>,
        related_ids: Vec<String>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            related_ids,
            source: source.into(),
        }
    }
}
