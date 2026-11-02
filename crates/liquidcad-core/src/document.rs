use crate::diagnostics::Diagnostic;
use crate::geometry::{MassProps, Pose, Shape};
use crate::ids::{ComponentId, FeatureId};
use crate::project::Project;
use serde::{Deserialize, Serialize};

/// Result of evaluating a project: exact solids, sketches, engineering
/// results, and (after the kernel runs) display meshes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub project: Project,
    pub components: Vec<Component>,
    pub sketches: Vec<Sketch>,
    pub scene: Scene,
    pub bom: Vec<BomLine>,
    pub diagnostics: Vec<Diagnostic>,
    pub engineering: EngineeringSummary,
    /// Solved mates (axis, value, moved parts).
    #[serde(default)]
    pub mates: Vec<crate::mates::MateState>,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    FloatModule,
    Stringer,
    Joist,
    DeckBoard,
    Cleat,
    Bumper,
    /// Sketched and extruded by the user.
    Custom,
    GangwayStringer,
    GangwayTread,
    GangwayHinge,
}

impl ComponentKind {
    pub fn tag(self) -> &'static str {
        match self {
            Self::FloatModule => "float_module",
            Self::Stringer => "stringer",
            Self::Joist => "joist",
            Self::DeckBoard => "deck_board",
            Self::Cleat => "cleat",
            Self::Bumper => "bumper",
            Self::Custom => "custom",
            Self::GangwayStringer => "gangway_stringer",
            Self::GangwayTread => "gangway_tread",
            Self::GangwayHinge => "gangway_hinge",
        }
    }

    pub fn movable(self) -> bool {
        matches!(self, Self::Cleat)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub id: ComponentId,
    pub name: String,
    pub kind: ComponentKind,
    pub feature_id: FeatureId,
    pub material: String,
    /// Exact geometry in the part's own frame, inches.
    pub shape: Shape,
    /// Part frame to world. World axes: x across the dock (port to
    /// starboard), y along it (stern to bow), z up from the float bottoms.
    pub pose: Pose,
    /// World-axis bounding box of the posed shape. Derived; for layout,
    /// spans, and drawings that only need extents.
    pub bounds: SolidBox,
    pub mass: MassProps,
    /// Present for parts cut from stock (tube, boards, bumper).
    pub stock: Option<StockInfo>,
    /// User offset applied on top of the generated position.
    #[serde(default)]
    pub placement: [f64; 3],
}

impl Component {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ComponentId,
        name: String,
        kind: ComponentKind,
        feature_id: FeatureId,
        material: String,
        shape: Shape,
        pose: Pose,
        mass: MassProps,
    ) -> Self {
        let bounds = world_bounds(&shape, &pose);
        Self {
            id,
            name,
            kind,
            feature_id,
            material,
            shape,
            pose,
            bounds,
            mass,
            stock: None,
            placement: [0.0; 3],
        }
    }

    pub fn mass_lb(&self) -> f64 {
        self.mass.mass_lb
    }
}

/// World-axis box around a posed shape: exact for polyhedra, the posed
/// local box otherwise.
pub fn world_bounds(shape: &Shape, pose: &Pose) -> SolidBox {
    let points: Vec<[f64; 3]> = match shape.polyhedron() {
        Some(poly) => poly.vertices.iter().map(|v| pose.apply(*v)).collect(),
        None => {
            let (lo, hi) = shape.local_bounds();
            (0..8)
                .map(|k| {
                    pose.apply([
                        if k & 1 == 0 { lo[0] } else { hi[0] },
                        if k & 2 == 0 { lo[1] } else { hi[1] },
                        if k & 4 == 0 { lo[2] } else { hi[2] },
                    ])
                })
                .collect()
        }
    };
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in points {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    SolidBox {
        origin: lo,
        size: [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]],
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StockInfo {
    pub profile: String,
    pub cut_length_in: f64,
    pub stock_length_in: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolidBox {
    pub origin: [f64; 3],
    pub size: [f64; 3],
}

impl SolidBox {
    pub fn max(self) -> [f64; 3] {
        [
            self.origin[0] + self.size[0],
            self.origin[1] + self.size[1],
            self.origin[2] + self.size[2],
        ]
    }

    pub fn center(self) -> [f64; 3] {
        [
            self.origin[0] + self.size[0] * 0.5,
            self.origin[1] + self.size[1] * 0.5,
            self.origin[2] + self.size[2] * 0.5,
        ]
    }

    pub fn volume_in3(self) -> f64 {
        self.size[0] * self.size[1] * self.size[2]
    }

    pub fn surface_in2(self) -> f64 {
        let [a, b, c] = self.size;
        2.0 * (a * b + b * c + a * c)
    }

    pub fn translated(self, offset: [f64; 3]) -> Self {
        Self {
            origin: [
                self.origin[0] + offset[0],
                self.origin[1] + offset[1],
                self.origin[2] + offset[2],
            ],
            size: self.size,
        }
    }

    pub fn is_valid(self) -> bool {
        self.size.iter().all(|s| s.is_finite() && *s > 0.0)
            && self.origin.iter().all(|o| o.is_finite())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scene {
    pub meshes: Vec<Mesh>,
    pub bounds_min: [f64; 3],
    pub bounds_max: [f64; 3],
}

/// Display tessellation. Never used for engineering or export.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mesh {
    pub component_id: ComponentId,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    /// Feature edges (pairs of vertex positions) for the wire overlay.
    pub edges: Vec<f32>,
    pub color: [f32; 3],
    /// B-rep faces, in the order their triangles appear in `indices`.
    #[serde(default)]
    pub faces: Vec<FaceInfo>,
    /// B-rep edges with world-space polylines.
    #[serde(default)]
    pub edge_info: Vec<EdgeInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SurfaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CurveKind {
    Line,
    Circle,
    Arc,
    Other,
}

/// One B-rep face: construction name (stable across rebuilds), world
/// geometry, and its triangles in the mesh (`tri_start`, `tri_count` count
/// triangles, not indices).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceInfo {
    pub id: String,
    pub kind: SurfaceKind,
    pub area_in2: f64,
    pub centroid: [f64; 3],
    /// Outward normal at the centroid.
    pub normal: [f64; 3],
    pub tri_start: u32,
    pub tri_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeInfo {
    pub id: String,
    pub kind: CurveKind,
    pub length_in: f64,
    /// World-space points along the edge, xyz triplets.
    pub polyline: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BomLine {
    pub item: String,
    pub kind: String,
    pub material: String,
    pub qty: u32,
    pub size: String,
    pub cut_length_in: Option<f64>,
    pub unit_weight_lb: f64,
    pub total_weight_lb: f64,
    pub notes: String,
    pub component_ids: Vec<String>,
}

/// Plan sketch produced by the plan feature. Geometry is fully determined by
/// driving dimensions bound to parameters; the UI edits those parameters.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sketch {
    pub id: String,
    pub feature_id: FeatureId,
    pub plane: String,
    pub entities: Vec<SketchEntity>,
    pub dimensions: Vec<SketchDimension>,
    pub constraints: Vec<SketchConstraint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SketchEntity {
    pub id: String,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub construction: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SketchDimension {
    pub id: String,
    pub parameter: String,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub value: f64,
    /// Offset of the dimension line from the measured edge, sketch units.
    pub offset: [f64; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SketchConstraint {
    pub kind: String,
    pub entities: Vec<String>,
    pub note: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct EngineeringSummary {
    /// Mass properties of the whole assembly.
    pub mass: MassProps,
    /// What the floats carry: the dock plus the gangway's hinge reaction.
    pub floating: MassProps,
    /// Deck top above the float bottoms, inches.
    pub deck_top_in: f64,
    pub center_of_buoyancy: [f64; 3],
    /// Metacentric heights at rest (in). Positive is stable.
    pub gm_transverse_in: f64,
    pub gm_longitudinal_in: f64,
    pub kb_in: f64,
    pub kg_in: f64,
    /// Equilibrium heel and trim from the dead load's centre of gravity.
    pub heel_static_deg: f64,
    pub trim_static_deg: f64,
    /// Heel with half the live load on the port half of the deck.
    pub heel_crowd_deg: f64,
    pub edge_freeboard_crowd_in: f64,
    pub float_count: u32,
    pub deck_area_ft2: f64,
    pub dead_load_lb: f64,
    pub live_load_lb: f64,
    pub buoyancy_capacity_lb: f64,
    pub draft_dead_in: f64,
    pub draft_loaded_in: f64,
    pub freeboard_dead_in: f64,
    pub freeboard_loaded_in: f64,
    pub reserve_buoyancy_ratio: f64,
    pub center_of_gravity: [f64; 3],
    pub deck_span_in: f64,
    pub stringer_span_in: f64,
}

impl Document {
    pub fn component(&self, id: &ComponentId) -> crate::Result<&Component> {
        self.components
            .iter()
            .find(|component| component.id == *id)
            .ok_or_else(|| crate::Error::UnknownComponent(id.0.clone()))
    }

    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let mut min = [f64::MAX; 3];
        let mut max = [f64::MIN; 3];
        for c in &self.components {
            let hi = c.bounds.max();
            for i in 0..3 {
                min[i] = min[i].min(c.bounds.origin[i]);
                max[i] = max[i].max(hi[i]);
            }
        }
        if self.components.is_empty() {
            ([0.0; 3], [0.0; 3])
        } else {
            (min, max)
        }
    }
}
