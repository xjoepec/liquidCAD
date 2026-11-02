use crate::geometry::{Profile, ShapeOp};
use crate::ids::FeatureId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    pub parents: Vec<FeatureId>,
    pub kind: FeatureKind,
    /// Parameters this feature is allowed to read. The evaluator rejects
    /// reads of anything else, so this list is the dependency declaration the
    /// incremental rebuild relies on.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Material override for every solid this feature produces.
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub suppressed: bool,
    /// Data for user features (extrude, modify).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<FeatureSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FeatureSpec {
    Extrude(ExtrudeSpec),
    Modify(ModifySpec),
}

/// Plane a sketch lies on. Principal planes pass through the origin offset
/// along their normal; a face plane follows a named face of a part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "camelCase")]
pub enum SketchPlane {
    /// Sketch x = world x, sketch y = world y; extrudes up (+z).
    Xy { offset: f64 },
    /// Sketch x = world x, sketch y = world z; extrudes toward -y.
    Xz { offset: f64 },
    /// Sketch x = world y, sketch y = world z; extrudes toward +x.
    Yz { offset: f64 },
    /// A planar face of a part, by construction name. Sketch y points as
    /// close to world up as the face allows; extrudes out of the face.
    Face { component: String, face: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtrudeDirection {
    #[default]
    Forward,
    Backward,
    Symmetric,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeSpec {
    pub plane: SketchPlane,
    pub profile: Profile,
    pub depth: f64,
    /// When set, the depth is this parameter's value (declared in inputs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth_parameter: Option<String>,
    #[serde(default)]
    pub direction: ExtrudeDirection,
    /// Subtract the extrusion from this part instead of making a new one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_into: Option<String>,
}

/// A modifier applied to one existing part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModifySpec {
    pub target: String,
    pub op: ShapeOp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeatureKind {
    Parameters,
    PlanSketch,
    FloatPattern,
    FrameBeams,
    DeckBoards,
    Cleats,
    Bumpers,
    /// Hinged access gangway, positioned by a mate.
    Gangway,
    /// User sketch extruded into a part.
    Extrude,
    /// Fillet, chamfer, hole, or cut on an existing part.
    Modify,
    Assembly,
}

impl FeatureKind {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Parameters => "parameters",
            Self::PlanSketch => "plan_sketch",
            Self::FloatPattern => "float_pattern",
            Self::FrameBeams => "frame_beams",
            Self::DeckBoards => "deck_boards",
            Self::Cleats => "cleats",
            Self::Bumpers => "bumpers",
            Self::Gangway => "gangway",
            Self::Extrude => "extrude",
            Self::Modify => "modify",
            Self::Assembly => "assembly",
        }
    }

    /// Features that emit solids need a material.
    pub fn produces_solids(self) -> bool {
        matches!(
            self,
            Self::FloatPattern
                | Self::FrameBeams
                | Self::DeckBoards
                | Self::Cleats
                | Self::Bumpers
                | Self::Gangway
                | Self::Extrude
        )
    }
}
