// Mirrors of the Rust serde types in liquidcad-core, -mfg, -store, -protocol.

export type Unit = "inch" | "millimeter" | "psf" | "degree";
export type Vec3 = [number, number, number];
export type Mat3 = [Vec3, Vec3, Vec3];

export type Parameter = {
  name: string;
  label: string;
  value: number;
  unit: Unit;
  min: number;
  max: number;
  step: number;
  snap: boolean;
  group: string;
};

export type FeatureKindTag =
  | "parameters"
  | "plan_sketch"
  | "float_pattern"
  | "frame_beams"
  | "deck_boards"
  | "cleats"
  | "bumpers"
  | "gangway"
  | "extrude"
  | "modify"
  | "assembly";

export type PathVertex = { x: number; y: number; bulge?: number };
export type Profile = { outer: PathVertex[]; holes?: PathVertex[][] };
export type SketchPlane =
  | { on: "xy"; offset: number }
  | { on: "xz"; offset: number }
  | { on: "yz"; offset: number }
  | { on: "face"; component: string; face: string };
export type ShapeOp =
  | { op: "fillet"; edges: string[]; radius: number }
  | { op: "chamfer"; edges: string[]; distance: number }
  | { op: "hole"; face: string; at: Vec3; diameter: number; depth: number }
  | { op: "cut"; tool: Shape; pose: Pose };
export type Pose = { origin: Vec3; rotation: [number, number, number, number] };
export type Shape =
  | { type: "box"; size: Vec3 }
  | { type: "prism"; profile: Profile; depth: number }
  | { type: "revolve"; profile: Profile; angle_deg: number }
  | { type: "modified"; base: Shape; ops: ShapeOp[] };
export type ExtrudeSpec = {
  type: "extrude";
  plane: SketchPlane;
  profile: Profile;
  depth: number;
  depth_parameter?: string | null;
  direction: "forward" | "backward" | "symmetric";
  cut_into?: string | null;
};
export type ModifySpec = { type: "modify"; target: string; op: ShapeOp };
export type FeatureSpec = ExtrudeSpec | ModifySpec;

export type Feature = {
  id: string;
  name: string;
  parents: string[];
  kind: { type: FeatureKindTag };
  inputs: string[];
  material: string | null;
  suppressed: boolean;
  spec?: FeatureSpec;
};

export type MateKind = "fixed" | "revolute" | "prismatic";
export type Mate = {
  id: string;
  name: string;
  kind: MateKind;
  parent: { component: string; edge: string };
  child: { component: string; edge: string };
  parameter?: string | null;
  value: number;
  invert: boolean;
  limits?: [number, number] | null;
};
export type MateState = {
  id: string;
  kind: MateKind;
  origin: Vec3;
  direction: Vec3;
  length: number;
  value: number;
  moved: string[];
};

export type Material = {
  name: string;
  family: string;
  density_lb_ft3: number;
  color: [number, number, number];
  max_span_in: number | null;
  stock_length_in: number | null;
  applies_to: string[];
};

export type Project = {
  schema: number;
  id: string;
  name: string;
  units: Unit;
  parameters: Record<string, Parameter>;
  features: Feature[];
  materials: Record<string, Material>;
  placements: Record<string, { offset: [number, number, number] }>;
  mates: Mate[];
};

export type ComponentKind =
  | "float_module"
  | "stringer"
  | "joist"
  | "deck_board"
  | "cleat"
  | "bumper"
  | "custom"
  | "gangway_stringer"
  | "gangway_tread"
  | "gangway_hinge";

export type SolidBox = { origin: Vec3; size: Vec3 };

export type MassProps = {
  mass_lb: number;
  volume_in3: number;
  com: Vec3;
  inertia: Mat3;
  principal: Vec3;
  axes: Mat3;
};

export type Component = {
  id: string;
  name: string;
  kind: ComponentKind;
  feature_id: string;
  material: string;
  shape: Shape;
  pose: Pose;
  bounds: SolidBox;
  mass: MassProps;
  stock: { profile: string; cut_length_in: number; stock_length_in: number | null } | null;
  placement: Vec3;
};

export type SurfaceKind = "plane" | "cylinder" | "cone" | "sphere" | "torus" | "other";
export type CurveKind = "line" | "circle" | "arc" | "other";
export type FaceInfo = {
  id: string;
  kind: SurfaceKind;
  area_in2: number;
  centroid: Vec3;
  normal: Vec3;
  tri_start: number;
  tri_count: number;
};
export type EdgeInfo = { id: string; kind: CurveKind; length_in: number; polyline: number[] };

export type Mesh = {
  component_id: string;
  positions: number[];
  normals: number[];
  indices: number[];
  edges: number[];
  color: [number, number, number];
  faces: FaceInfo[];
  edge_info: EdgeInfo[];
};

export type BomLine = {
  item: string;
  kind: string;
  material: string;
  qty: number;
  size: string;
  cut_length_in: number | null;
  unit_weight_lb: number;
  total_weight_lb: number;
  notes: string;
  component_ids: string[];
};

export type Severity = "info" | "warning" | "error";

export type Diagnostic = {
  severity: Severity;
  code: string;
  message: string;
  related_ids: string[];
  source: string;
};

export type SketchEntity = { id: string; a: [number, number]; b: [number, number]; construction: boolean };
export type SketchDimension = {
  id: string;
  parameter: string;
  a: [number, number];
  b: [number, number];
  value: number;
  offset: [number, number];
};
export type Sketch = {
  id: string;
  feature_id: string;
  plane: string;
  entities: SketchEntity[];
  dimensions: SketchDimension[];
  constraints: { kind: string; entities: string[]; note: string }[];
};

export type EngineeringSummary = {
  mass: MassProps;
  floating: MassProps;
  deck_top_in: number;
  center_of_buoyancy: Vec3;
  gm_transverse_in: number;
  gm_longitudinal_in: number;
  kb_in: number;
  kg_in: number;
  heel_static_deg: number;
  trim_static_deg: number;
  heel_crowd_deg: number;
  edge_freeboard_crowd_in: number;
  float_count: number;
  deck_area_ft2: number;
  dead_load_lb: number;
  live_load_lb: number;
  buoyancy_capacity_lb: number;
  draft_dead_in: number;
  draft_loaded_in: number;
  freeboard_dead_in: number;
  freeboard_loaded_in: number;
  reserve_buoyancy_ratio: number;
  center_of_gravity: [number, number, number];
  deck_span_in: number;
  stringer_span_in: number;
};

export type Document = {
  project: Project;
  components: Component[];
  sketches: Sketch[];
  scene: { meshes: Mesh[]; bounds_min: [number, number, number]; bounds_max: [number, number, number] };
  bom: BomLine[];
  diagnostics: Diagnostic[];
  engineering: EngineeringSummary;
  mates: MateState[];
  revision: number;
};

export type RebuildReport = { regenerated: string[]; reused: string[]; elapsed_ms: number };

export type SessionState = {
  document: Document;
  selected: string | null;
  kernel: string;
  project_path: string | null;
  dirty: boolean;
  can_undo: boolean;
  can_redo: boolean;
  undo_label: string | null;
  redo_label: string | null;
  rebuild: RebuildReport;
  meshes_built: number;
  meshes_reused: number;
};

export type SelectionInfo = {
  component: Component;
  feature_name: string;
  material: Material;
  volume_in3: number;
  depends_on: string[];
  driven_by: string[];
  movable: boolean;
};

export type RevisionSummary = {
  id: number;
  created_at: string;
  note: string;
  component_count: number;
  mass_lb: number;
};

export type Anchor = "start" | "middle" | "end";
export type SheetPrim =
  | { type: "line"; a: [number, number]; b: [number, number]; weight: number; color: [number, number, number]; dash: boolean }
  | {
      type: "rect";
      x: number;
      y: number;
      w: number;
      h: number;
      weight: number;
      stroke: [number, number, number] | null;
      fill: [number, number, number] | null;
    }
  | {
      type: "text";
      at: [number, number];
      size: number;
      text: string;
      color: [number, number, number];
      anchor: Anchor;
      bold: boolean;
      rotate: number;
    };
export type Sheet = { width: number; height: number; title: string; scale_label: string; items: SheetPrim[] };

export type CutPlan = {
  groups: {
    material: string;
    profile: string;
    stock_length_in: number | null;
    cut_count: number;
    bars: { cuts: number[]; remaining_in: number }[];
    splices: number;
    spliced_parts: string[];
    total_cut_in: number;
    waste_ratio: number;
  }[];
};

export type PreviewResult = { meshes: Mesh[]; diagnostics: Diagnostic[]; mass: MassProps | null };

export type SimFrame = {
  t: number;
  heave_in: number;
  roll_deg: number;
  pitch_deg: number;
  min_freeboard_in: number;
  walker?: [number, number];
};
export type SimResult = {
  params: import("../shared/ipc").SimParams | null;
  fps: number;
  pivot: Vec3;
  water_level_in: number;
  frames: SimFrame[];
  summary: {
    still_draft_in: number;
    static_heel_deg: number;
    static_trim_deg: number;
    heave_period_s: number;
    max_heave_in: number;
    max_roll_deg: number;
    max_pitch_deg: number;
    min_freeboard_in: number;
    max_float_emergence_in: number;
    max_deck_accel_g: number;
    wave_length_in: number;
  };
  diagnostics: Diagnostic[];
  coefficients: { added_mass_heave: number; added_inertia: number; damping_ratio: number } | null;
};

export type EngineValue =
  | { type: "pong"; data: { kernel: string; native_occt: boolean; protocol: number; version: string } }
  | { type: "session"; data: SessionState }
  | { type: "selection"; data: SelectionInfo | null }
  | { type: "history"; data: RevisionSummary[] }
  | { type: "drawing"; data: Sheet }
  | { type: "cutPlan"; data: CutPlan }
  | { type: "exported"; data: { format: string; path: string; bytes: number } }
  | { type: "saved"; data: { path: string } }
  | { type: "preview"; data: PreviewResult }
  | { type: "simulation"; data: SimResult }
  | { type: "superseded" };
