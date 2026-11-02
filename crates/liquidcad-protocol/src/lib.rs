//! JSON-lines protocol between the Electron main process and `liquidcad serve`.
//!
//! Requests: `{"id":7,"payload":{"method":"setParameter","params":{...}}}`.
//! Responses: `{"id":7,"result":{"type":"session","data":{...}}}` or
//! `{"id":7,"error":{"code":"validation","message":"..."}}`.
//! Events (no id): `{"event":"ready","kernel":"..."}`.

use liquidcad_core::diagnostics::Diagnostic;
use liquidcad_core::document::{Component, Document, Mesh};
use liquidcad_core::evaluate::RebuildReport;
use liquidcad_core::feature::FeatureSpec;
use liquidcad_core::geometry::MassProps;
use liquidcad_core::ids::{ComponentId, FeatureId};
use liquidcad_core::mates::Mate;
use liquidcad_core::project::Material;
use liquidcad_mfg::{CutPlan, Sheet};
use liquidcad_store::RevisionSummary;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub id: u64,
    pub payload: Request,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "params", rename_all = "camelCase")]
pub enum Request {
    Ping,
    Open {
        path: String,
    },
    Document,
    SetParameter {
        name: String,
        value: f64,
    },
    SetMaterial {
        feature: String,
        material: String,
    },
    SetSuppressed {
        feature: String,
        suppressed: bool,
    },
    MovePart {
        id: String,
        offset: [f64; 3],
    },
    Select {
        id: Option<String>,
    },
    Undo,
    Redo,
    History {
        limit: Option<u32>,
    },
    Restore {
        revision: i64,
    },
    Drawing,
    CutPlan,
    Export {
        format: ExportFormat,
        path: String,
    },
    Save {
        path: Option<String>,
    },
    /// New sketched part or modifier. The engine assigns the id, parents,
    /// and inputs.
    AddFeature {
        draft: FeatureDraft,
    },
    EditFeature {
        id: String,
        draft: FeatureDraft,
    },
    DeleteFeature {
        id: String,
    },
    /// Builds the draft without committing it.
    PreviewFeature {
        draft: FeatureDraft,
    },
    AddMate {
        mate: Mate,
    },
    DeleteMate {
        id: String,
    },
    Simulate {
        params: liquidcad_sim::SimParams,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FeatureDraft {
    pub name: String,
    /// Material key; sketched parts default to aluminium.
    #[serde(default)]
    pub material: Option<String>,
    pub spec: FeatureSpec,
}

impl Request {
    /// Requests that change the model; the server coalesces back-to-back
    /// duplicates of these.
    pub fn coalesce_key(&self) -> Option<String> {
        match self {
            Self::SetParameter { name, .. } => Some(format!("param:{name}")),
            Self::MovePart { id, .. } => Some(format!("move:{id}")),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Dxf,
    Step,
    Pdf,
    Csv,
}

impl ExportFormat {
    pub const ALL: [Self; 4] = [Self::Dxf, Self::Step, Self::Pdf, Self::Csv];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Dxf => "dxf",
            Self::Step => "step",
            Self::Pdf => "pdf",
            Self::Csv => "csv",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum Value {
    Pong(KernelStatus),
    Session(Box<SessionState>),
    Selection(Option<Box<SelectionInfo>>),
    History(Vec<RevisionSummary>),
    Drawing(Box<Sheet>),
    CutPlan(CutPlan),
    Exported {
        format: ExportFormat,
        path: String,
        bytes: u64,
    },
    Saved {
        path: String,
    },
    Preview(Box<PreviewResult>),
    Simulation(Box<liquidcad_sim::SimResult>),
    /// A later queued request of the same kind replaced this one.
    Superseded,
}

/// Meshes and checks for a draft feature, not committed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewResult {
    /// Parts the draft adds or changes.
    pub meshes: Vec<Mesh>,
    pub diagnostics: Vec<Diagnostic>,
    pub mass: Option<MassProps>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KernelStatus {
    pub kernel: String,
    pub native_occt: bool,
    pub protocol: u32,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionState {
    pub document: Document,
    pub selected: Option<ComponentId>,
    pub kernel: String,
    pub project_path: Option<String>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
    pub rebuild: RebuildReport,
    pub meshes_built: usize,
    pub meshes_reused: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionInfo {
    pub component: Component,
    pub feature_name: String,
    pub material: Material,
    pub volume_in3: f64,
    /// Upstream features this component depends on, nearest first.
    pub depends_on: Vec<FeatureId>,
    /// Parameters whose change regenerates this component.
    pub driven_by: Vec<String>,
    pub movable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum Event {
    Ready(KernelStatus),
}

impl Response {
    pub fn ok(id: u64, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: u64, code: &str, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(ProtocolError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format_matches_the_typescript_side() {
        let env: Envelope = serde_json::from_str(
            r#"{"id":3,"payload":{"method":"setParameter","params":{"name":"width_in","value":144}}}"#,
        )
        .unwrap();
        assert_eq!(
            env.payload,
            Request::SetParameter {
                name: "width_in".into(),
                value: 144.0
            }
        );
        let undo: Envelope =
            serde_json::from_str(r#"{"id":4,"payload":{"method":"undo"}}"#).unwrap();
        assert_eq!(undo.payload, Request::Undo);
        let export: Envelope = serde_json::from_str(
            r#"{"id":5,"payload":{"method":"export","params":{"format":"step","path":"/tmp/a.step"}}}"#,
        )
        .unwrap();
        assert!(matches!(
            export.payload,
            Request::Export {
                format: ExportFormat::Step,
                ..
            }
        ));
        let ready = serde_json::to_string(&Event::Ready(KernelStatus {
            kernel: "k".into(),
            native_occt: false,
            protocol: PROTOCOL_VERSION,
            version: "0".into(),
        }))
        .unwrap();
        assert!(ready.starts_with(r#"{"event":"ready""#));
        let superseded = serde_json::to_string(&Response::ok(1, Value::Superseded)).unwrap();
        assert_eq!(superseded, r#"{"id":1,"result":{"type":"superseded"}}"#);
    }

    #[test]
    fn rejects_unknown_methods_and_bad_params() {
        assert!(serde_json::from_str::<Envelope>(
            r#"{"id":1,"payload":{"method":"rm","params":{}}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<Envelope>(
            r#"{"id":1,"payload":{"method":"setParameter","params":{"name":"w"}}}"#
        )
        .is_err());
    }
}
