//! Feature generators. Each one reads parameters only through [`Inputs`],
//! which rejects anything the feature did not declare. That keeps the
//! declared inputs honest, and the incremental cache depends on them.

use crate::diagnostics::{Diagnostic, Severity};
use crate::document::world_bounds;
use crate::document::{
    Component, ComponentKind, Sketch, SketchConstraint, SketchDimension, SketchEntity, SolidBox,
    StockInfo,
};
use crate::feature::{
    ExtrudeDirection, ExtrudeSpec, Feature, FeatureKind, FeatureSpec, ModifySpec, SketchPlane,
};
use crate::geometry::{
    cross, dot, normalize, MassIntegrals, MassProps, PathVertex, Polyhedron, Pose, Profile, Shape,
    ShapeOp, ShapeOracle, Vec3,
};
use crate::ids::ComponentId;
use crate::parameters::fmt_num;
use crate::project::Project;

/// Rect tube wall thickness for frame members.
pub(crate) const TUBE_WALL_IN: f64 = 0.125;
/// Rotomolded float shell wall.
const FLOAT_WALL_IN: f64 = 0.25;
/// Closed-cell polystyrene fill inside the float shell.
const FLOAT_FOAM_LB_FT3: f64 = 1.5;
/// Gap between neighbouring float modules.
const FLOAT_GAP_IN: f64 = 0.5;
pub(crate) const BOARD_WIDTH_IN: f64 = 5.5;
pub(crate) const BOARD_GAP_IN: f64 = 0.25;
/// Narrowest rip the deck feature will emit; anything smaller is left open.
const MIN_RIP_IN: f64 = 1.5;
const CLEAT_SIZE: [f64; 3] = [2.5, 10.0, 2.0];
/// A cast horn cleat fills roughly a fifth of its bounding box.
const CLEAT_FILL: f64 = 0.2;
const CLEAT_EDGE_INSET_IN: f64 = 0.5;
const BUMPER_DEPTH_IN: f64 = 2.0;
/// Hollow D-profile bumper, fraction of bounding box that is material.
const BUMPER_FILL: f64 = 0.35;

#[derive(Clone, Debug, Default)]
pub(crate) struct FeatureOutput {
    pub components: Vec<Component>,
    /// Updated versions of parts made by earlier features (cuts, modifiers).
    pub replaced: Vec<Component>,
    pub sketch: Option<Sketch>,
    pub diagnostics: Vec<Diagnostic>,
}

const IN3_PER_FT3: f64 = 1728.0;

/// Unit-density integrals of a box from the origin to `size`.
fn cuboid(size: [f64; 3]) -> MassIntegrals {
    Polyhedron::cuboid(size).integrals()
}

/// Hollow rectangular tube whose world bounds are `origin` + `size`,
/// running along world `axis` (0 = x, 1 = y). The section is drawn in the
/// part's local XY plane (width across, height up) and extruded along
/// local Z, so the walls are real geometry: a hole drilled through it cuts
/// only the walls, and STEP carries the tube as made.
#[allow(clippy::too_many_arguments)]
fn tube_part(
    inputs: &Inputs,
    id: String,
    name: String,
    kind: ComponentKind,
    material: &str,
    origin: [f64; 3],
    size: [f64; 3],
    axis: usize,
    density_in3: f64,
) -> Component {
    let across = if axis == 0 { 1 } else { 0 };
    let (w, h, len) = (size[across], size[2], size[axis]);
    let t = TUBE_WALL_IN.min(w * 0.25).min(h * 0.25);
    let profile = Profile {
        outer: Profile::rectangle(w, h).outer,
        holes: vec![[[t, t], [t, h - t], [w - t, h - t], [w - t, t]]
            .iter()
            .map(|p| PathVertex::line(p[0], p[1]))
            .collect()],
    }
    .normalized()
    .expect("tube section is a valid profile");
    let pose = if axis == 0 {
        // Local x -> world y, local y -> world z, extrude along +x.
        Pose::from_axes(origin, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])
    } else {
        // Local x -> world x, local y -> world z, extrude along -y from the
        // far end so the tube spans origin.y .. origin.y + length.
        Pose::from_axes(
            [origin[0], origin[1] + len, origin[2]],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, -1.0, 0.0],
        )
    };
    let shape = Shape::Prism {
        profile,
        depth: len,
    };
    let integrals = shape.volume_integrals().expect("tube is polyhedral");
    let mut mass = MassProps::uniform(&integrals, density_in3, &pose);
    mass.volume_in3 = size[0] * size[1] * size[2];
    Component::new(
        ComponentId::new(id),
        name,
        kind,
        inputs.feature.id.clone(),
        material.to_string(),
        shape,
        pose,
        mass,
    )
}

/// Box-shaped part at `origin` whose mass integrals (in its own frame) are
/// `local`.
#[allow(clippy::too_many_arguments)]
fn box_part(
    inputs: &Inputs,
    id: String,
    name: String,
    kind: ComponentKind,
    material: &str,
    origin: [f64; 3],
    size: [f64; 3],
    local: MassIntegrals,
) -> Component {
    let pose = Pose::translation(origin);
    let mut mass = MassProps::from_integrals(&local.transformed(&pose));
    mass.volume_in3 = size[0] * size[1] * size[2];
    Component::new(
        ComponentId::new(id),
        name,
        kind,
        inputs.feature.id.clone(),
        material.to_string(),
        Shape::cuboid(size),
        pose,
        mass,
    )
}

pub(crate) struct Inputs<'a> {
    project: &'a Project,
    feature: &'a Feature,
}

impl<'a> Inputs<'a> {
    pub fn new(project: &'a Project, feature: &'a Feature) -> Self {
        Self { project, feature }
    }

    pub fn get(&self, name: &str) -> crate::Result<f64> {
        if !self.feature.inputs.iter().any(|i| i == name) {
            return Err(crate::Error::Validation(format!(
                "feature {} read undeclared parameter {name}",
                self.feature.id
            )));
        }
        self.project.parameter_value(name)
    }

    fn material_key(&self) -> crate::Result<&'a str> {
        self.feature.material.as_deref().ok_or_else(|| {
            crate::Error::Validation(format!("feature {} has no material", self.feature.id))
        })
    }

    /// Density in lb/in³.
    fn density_in3(&self) -> crate::Result<f64> {
        self.density().map(|d| d / IN3_PER_FT3)
    }

    fn density(&self) -> crate::Result<f64> {
        let key = self.material_key()?;
        self.project
            .materials
            .get(key)
            .map(|m| m.density_lb_ft3)
            .ok_or_else(|| crate::Error::UnknownMaterial(key.to_string()))
    }

    fn stock_length(&self) -> Option<f64> {
        let key = self.feature.material.as_deref()?;
        self.project.materials.get(key)?.stock_length_in
    }
}

/// What a generator may look at besides parameters: parts made by earlier
/// features (for sketch-on-face and cuts) and the shape oracle.
pub(crate) struct Context<'a> {
    pub components: &'a [Component],
    pub oracle: &'a dyn ShapeOracle,
}

pub(crate) fn generate(
    project: &Project,
    feature: &Feature,
    ctx: &Context,
) -> crate::Result<FeatureOutput> {
    if feature.suppressed {
        return Ok(FeatureOutput::default());
    }
    let inputs = Inputs::new(project, feature);
    let output = match feature.kind {
        FeatureKind::Parameters | FeatureKind::Assembly | FeatureKind::Modify => {
            FeatureOutput::default()
        }
        FeatureKind::Gangway => gangway(&inputs)?,
        FeatureKind::Extrude => match &feature.spec {
            Some(FeatureSpec::Extrude(spec)) => extrude(&inputs, spec, ctx)?,
            _ => {
                return Err(crate::Error::Validation(format!(
                    "{} has no sketch",
                    feature.name
                )))
            }
        },
        FeatureKind::PlanSketch => plan_sketch(&inputs)?,
        FeatureKind::FloatPattern => floats(&inputs)?,
        FeatureKind::FrameBeams => frame(&inputs)?,
        FeatureKind::DeckBoards => deck(&inputs)?,
        FeatureKind::Cleats => cleats(&inputs, project)?,
        FeatureKind::Bumpers => bumpers(&inputs)?,
    };
    for component in &output.components {
        if !component.bounds.is_valid() || component.shape.validate().is_err() {
            return Err(crate::Error::Validation(format!(
                "{} produced a degenerate solid for {}",
                feature.name, component.id
            )));
        }
    }
    Ok(output)
}

struct Grid {
    width: f64,
    length: f64,
    module: f64,
    cols: u32,
    rows: u32,
}

fn grid(inputs: &Inputs) -> crate::Result<Grid> {
    let width = inputs.get("width_in")?;
    let length = inputs.get("length_in")?;
    let module = inputs.get("module_in")?;
    let cols = width / module;
    let rows = length / module;
    if (cols - cols.round()).abs() > 1e-9 || (rows - rows.round()).abs() > 1e-9 {
        return Err(crate::Error::Validation(format!(
            "width and length must be whole multiples of the {} in float module",
            fmt_num(module)
        )));
    }
    let (cols, rows) = (cols.round() as u32, rows.round() as u32);
    if cols < 2 || rows < 2 {
        return Err(crate::Error::Validation(format!(
            "dock needs at least 2 x 2 float modules; {} x {} in gives {cols} x {rows}",
            fmt_num(width),
            fmt_num(length)
        )));
    }
    Ok(Grid {
        width,
        length,
        module,
        cols,
        rows,
    })
}

fn plan_sketch(inputs: &Inputs) -> crate::Result<FeatureOutput> {
    let g = grid(inputs)?;
    let (w, l) = (g.width, g.length);
    let line = |id: &str, a: [f64; 2], b: [f64; 2], construction: bool| SketchEntity {
        id: id.into(),
        a,
        b,
        construction,
    };
    let mut entities = vec![
        line("plan.edge.stern", [0.0, 0.0], [w, 0.0], false),
        line("plan.edge.stbd", [w, 0.0], [w, l], false),
        line("plan.edge.bow", [w, l], [0.0, l], false),
        line("plan.edge.port", [0.0, l], [0.0, 0.0], false),
    ];
    for k in 1..g.cols {
        let x = f64::from(k) * g.module;
        entities.push(line(&format!("plan.grid.x{k}"), [x, 0.0], [x, l], true));
    }
    for r in 1..g.rows {
        let y = f64::from(r) * g.module;
        entities.push(line(&format!("plan.grid.y{r}"), [0.0, y], [w, y], true));
    }
    let dimensions = vec![
        SketchDimension {
            id: "dim.width".into(),
            parameter: "width_in".into(),
            a: [0.0, 0.0],
            b: [w, 0.0],
            value: w,
            offset: [0.0, -18.0],
        },
        SketchDimension {
            id: "dim.length".into(),
            parameter: "length_in".into(),
            a: [0.0, 0.0],
            b: [0.0, l],
            value: l,
            offset: [-18.0, 0.0],
        },
        SketchDimension {
            id: "dim.module".into(),
            parameter: "module_in".into(),
            a: [0.0, l],
            b: [g.module, l],
            value: g.module,
            offset: [0.0, 12.0],
        },
    ];
    let constraint = |kind: &str, entities: &[&str], note: &str| SketchConstraint {
        kind: kind.into(),
        entities: entities.iter().map(|s| s.to_string()).collect(),
        note: note.into(),
    };
    let constraints = vec![
        constraint(
            "fixed",
            &["plan.edge.port", "plan.edge.stern"],
            "origin at port-stern corner",
        ),
        constraint("horizontal", &["plan.edge.stern", "plan.edge.bow"], ""),
        constraint("vertical", &["plan.edge.port", "plan.edge.stbd"], ""),
        constraint(
            "on_grid",
            &["plan.edge.stbd", "plan.edge.bow"],
            "edges land on the float module grid",
        ),
        constraint(
            "equal",
            &["dim.module"],
            "all float cells share the module size",
        ),
    ];
    Ok(FeatureOutput {
        sketch: Some(Sketch {
            id: "sketch.plan".into(),
            feature_id: inputs.feature.id.clone(),
            plane: "XY".into(),
            entities,
            dimensions,
            constraints,
        }),
        ..FeatureOutput::default()
    })
}

fn floats(inputs: &Inputs) -> crate::Result<FeatureOutput> {
    let g = grid(inputs)?;
    let height = inputs.get("float_height_in")?;
    let shell_density = inputs.density_in3()?;
    let material = inputs.material_key()?.to_string();
    let size = [g.module - FLOAT_GAP_IN, g.module - FLOAT_GAP_IN, height];
    // Rotomolded shell (outer box minus cavity) filled with foam.
    let cavity = cuboid(size.map(|s| s - 2.0 * FLOAT_WALL_IN))
        .transformed(&Pose::translation([FLOAT_WALL_IN; 3]));
    let local = cuboid(size)
        .minus(&cavity)
        .scaled(shell_density)
        .plus(&cavity.scaled(FLOAT_FOAM_LB_FT3 / IN3_PER_FT3));
    let mut components = Vec::with_capacity((g.cols * g.rows) as usize);
    for row in 0..g.rows {
        for col in 0..g.cols {
            components.push(box_part(
                inputs,
                format!("float.r{row}.c{col}"),
                format!("Float R{}C{}", row + 1, col + 1),
                ComponentKind::FloatModule,
                &material,
                [
                    f64::from(col) * g.module + FLOAT_GAP_IN * 0.5,
                    f64::from(row) * g.module + FLOAT_GAP_IN * 0.5,
                    0.0,
                ],
                size,
                local,
            ));
        }
    }
    Ok(FeatureOutput {
        components,
        ..FeatureOutput::default()
    })
}

fn frame(inputs: &Inputs) -> crate::Result<FeatureOutput> {
    let g = grid(inputs)?;
    let float_h = inputs.get("float_height_in")?;
    let bw = inputs.get("beam_width_in")?;
    let bh = inputs.get("beam_height_in")?;
    let spacing = inputs.get("joist_spacing_in")?;
    let density = inputs.density_in3()?;
    let material = inputs.material_key()?.to_string();
    let stock_length = inputs.stock_length();
    let profile = format!(
        "{} x {} x {} rect tube",
        fmt_num(bw),
        fmt_num(bh),
        fmt_num(TUBE_WALL_IN)
    );
    let mut components = Vec::new();
    let mut push = |id: String, name: String, kind, solid: SolidBox, cut: f64| {
        let axis = if solid.size[0] >= solid.size[1] { 0 } else { 1 };
        let mut part = tube_part(
            inputs,
            id,
            name,
            kind,
            &material,
            solid.origin,
            solid.size,
            axis,
            density,
        );
        part.stock = Some(StockInfo {
            profile: profile.clone(),
            cut_length_in: cut,
            stock_length_in: stock_length,
        });
        components.push(part);
    };

    // Joists: lower layer, transverse, resting on the floats.
    let joist_z = float_h;
    let mut joist_ys = vec![("stern".to_string(), "Stern joist".to_string(), 0.0)];
    let mut j = 1;
    while f64::from(j) * spacing < g.length - bw - 1e-6 {
        let y = f64::from(j) * spacing - bw * 0.5;
        joist_ys.push((j.to_string(), format!("Joist {j}"), y));
        j += 1;
    }
    joist_ys.push(("bow".to_string(), "Bow joist".to_string(), g.length - bw));
    for (suffix, name, y) in joist_ys {
        push(
            format!("frame.joist.{suffix}"),
            name,
            ComponentKind::Joist,
            SolidBox {
                origin: [0.0, y, joist_z],
                size: [g.width, bw, bh],
            },
            g.width,
        );
    }

    // Stringers: upper layer, one on every module line so deck boards span
    // exactly one module between supports.
    let stringer_z = float_h + bh;
    for k in 0..=g.cols {
        let (suffix, name) = if k == 0 {
            ("port".to_string(), "Port stringer".to_string())
        } else if k == g.cols {
            ("stbd".to_string(), "Starboard stringer".to_string())
        } else {
            (k.to_string(), format!("Stringer {k}"))
        };
        let x = (f64::from(k) * g.module - bw * 0.5).clamp(0.0, g.width - bw);
        push(
            format!("frame.stringer.{suffix}"),
            name,
            ComponentKind::Stringer,
            SolidBox {
                origin: [x, 0.0, stringer_z],
                size: [bw, g.length, bh],
            },
            g.length,
        );
    }
    Ok(FeatureOutput {
        components,
        ..FeatureOutput::default()
    })
}

fn deck(inputs: &Inputs) -> crate::Result<FeatureOutput> {
    let width = inputs.get("width_in")?;
    let length = inputs.get("length_in")?;
    let float_h = inputs.get("float_height_in")?;
    let bh = inputs.get("beam_height_in")?;
    let thickness = inputs.get("deck_thickness_in")?;
    let density = inputs.density_in3()?;
    let material = inputs.material_key()?.to_string();
    let stock_length = inputs.stock_length();
    let z = float_h + 2.0 * bh;
    let pitch = BOARD_WIDTH_IN + BOARD_GAP_IN;
    let mut components = Vec::new();
    let mut y = 0.0;
    let mut index = 0u32;
    while y < length - 1e-9 {
        let remaining = length - y;
        let board_w = BOARD_WIDTH_IN.min(remaining);
        if board_w < MIN_RIP_IN {
            break;
        }
        let ripped = board_w < BOARD_WIDTH_IN - 1e-9;
        let size = [width, board_w, thickness];
        let mut board = box_part(
            inputs,
            format!("deck.board.{index}"),
            if ripped {
                format!("Deck board {} (ripped)", index + 1)
            } else {
                format!("Deck board {}", index + 1)
            },
            ComponentKind::DeckBoard,
            &material,
            [0.0, y, z],
            size,
            cuboid(size).scaled(density),
        );
        board.stock = Some(StockInfo {
            profile: format!(
                "{} x {} board",
                fmt_num(thickness),
                fmt_num((board_w * 1000.0).round() / 1000.0)
            ),
            cut_length_in: width,
            stock_length_in: stock_length,
        });
        components.push(board);
        index += 1;
        y += pitch;
    }
    Ok(FeatureOutput {
        components,
        ..FeatureOutput::default()
    })
}

fn cleats(inputs: &Inputs, project: &Project) -> crate::Result<FeatureOutput> {
    let width = inputs.get("width_in")?;
    let length = inputs.get("length_in")?;
    let float_h = inputs.get("float_height_in")?;
    let bh = inputs.get("beam_height_in")?;
    // Declared so the cleat feature regenerates when the stringer it bolts
    // through changes width.
    let _bw = inputs.get("beam_width_in")?;
    let thickness = inputs.get("deck_thickness_in")?;
    let density = inputs.density_in3()?;
    let material = inputs.material_key()?.to_string();
    let z = float_h + 2.0 * bh + thickness;
    let mut stations = vec![("stern", 0.15), ("bow", 0.85)];
    if length >= 192.0 {
        stations.insert(1, ("mid", 0.5));
    }
    let mut components = Vec::new();
    let mut diagnostics = Vec::new();
    for (side, x) in [
        ("port", CLEAT_EDGE_INSET_IN),
        ("stbd", width - CLEAT_EDGE_INSET_IN - CLEAT_SIZE[0]),
    ] {
        for (station, frac) in &stations {
            let id = format!("cleat.{side}.{station}");
            let nominal = SolidBox {
                origin: [x, frac * length - CLEAT_SIZE[1] * 0.5, z],
                size: CLEAT_SIZE,
            };
            let requested = project
                .placements
                .get(&id)
                .map(|p| p.offset)
                .unwrap_or([0.0; 3]);
            let (offset, clamped) = clamp_to_deck(nominal, requested, width, length);
            if clamped {
                diagnostics.push(Diagnostic::new(
                    Severity::Warning,
                    "placement",
                    "PLACE-01",
                    format!(
                        "{id} offset ({}, {}) left the deck after a resize; held at the deck edge",
                        fmt_num(requested[0]),
                        fmt_num(requested[1])
                    ),
                    vec![id.clone()],
                ));
            }
            let solid = nominal.translated(offset);
            // Modelled as its bounding box with the cast cleat's fill
            // fraction spread uniformly through it.
            let mut cleat = box_part(
                inputs,
                id,
                format!(
                    "{} cleat, {}",
                    if side == "port" { "Port" } else { "Starboard" },
                    station
                ),
                ComponentKind::Cleat,
                &material,
                solid.origin,
                solid.size,
                cuboid(solid.size).scaled(density * CLEAT_FILL),
            );
            cleat.placement = offset;
            components.push(cleat);
        }
    }
    Ok(FeatureOutput {
        components,
        diagnostics,
        ..FeatureOutput::default()
    })
}

/// Keeps a deck-mounted part inside the deck footprint. Returns the offset
/// actually applied and whether it had to be reduced.
pub(crate) fn clamp_to_deck(
    nominal: SolidBox,
    requested: [f64; 3],
    width: f64,
    length: f64,
) -> ([f64; 3], bool) {
    let min_dx = -nominal.origin[0];
    let max_dx = width - nominal.origin[0] - nominal.size[0];
    let min_dy = -nominal.origin[1];
    let max_dy = length - nominal.origin[1] - nominal.size[1];
    let dx = requested[0].clamp(min_dx, max_dx);
    let dy = requested[1].clamp(min_dy, max_dy);
    let clamped = (dx - requested[0]).abs() > 1e-9
        || (dy - requested[1]).abs() > 1e-9
        || requested[2].abs() > 1e-9;
    ([dx, dy, 0.0], clamped)
}

fn bumpers(inputs: &Inputs) -> crate::Result<FeatureOutput> {
    let width = inputs.get("width_in")?;
    let length = inputs.get("length_in")?;
    let float_h = inputs.get("float_height_in")?;
    let bh = inputs.get("beam_height_in")?;
    let density = inputs.density_in3()?;
    let material = inputs.material_key()?.to_string();
    let stock_length = inputs.stock_length();
    let height = (2.0 * bh).min(4.0);
    let z = float_h + (2.0 * bh - height) * 0.5;
    let mut components = Vec::new();
    for (id, name, y) in [
        ("bumper.stern", "Stern bumper", -BUMPER_DEPTH_IN),
        ("bumper.bow", "Bow bumper", length),
    ] {
        let size = [width, BUMPER_DEPTH_IN, height];
        let mut bumper = box_part(
            inputs,
            id.into(),
            name.into(),
            ComponentKind::Bumper,
            &material,
            [0.0, y, z],
            size,
            cuboid(size).scaled(density * BUMPER_FILL),
        );
        bumper.stock = Some(StockInfo {
            profile: format!(
                "{} x {} D-profile",
                fmt_num(BUMPER_DEPTH_IN),
                fmt_num(height)
            ),
            cut_length_in: width,
            stock_length_in: stock_length,
        });
        components.push(bumper);
    }
    Ok(FeatureOutput {
        components,
        ..FeatureOutput::default()
    })
}

/// Rect tube profile wall for the gangway stringers and tread planks.
const GANGWAY_STRINGER: [f64; 2] = [2.0, 6.0];
const GANGWAY_TREAD: [f64; 2] = [5.5, 1.0];
const GANGWAY_TREAD_PITCH: f64 = 6.0;
const HINGE_LEAF: [f64; 2] = [3.0, 0.25];

/// Aluminium gangway laid level off the stern, centred on the dock. Its
/// mate swings it about the stern deck edge to the set slope.
fn gangway(inputs: &Inputs) -> crate::Result<FeatureOutput> {
    let width = inputs.get("width_in")?;
    let float_h = inputs.get("float_height_in")?;
    let bh = inputs.get("beam_height_in")?;
    let deck_t = inputs.get("deck_thickness_in")?;
    let length = inputs.get("gangway_length_in")?;
    let gw = inputs.get("gangway_width_in")?;
    let density = inputs.density_in3()?;
    let material = inputs.material_key()?.to_string();
    let stock_length = inputs.stock_length();
    let deck_top = float_h + 2.0 * bh + deck_t;
    let x0 = (width - gw) * 0.5;
    let [sw, sh] = GANGWAY_STRINGER;
    let mut components = Vec::new();

    // Hinge leaf: plate whose lower front edge (y-|z-) rides on the stern
    // deck edge; the mate solves against that edge.
    let leaf = [gw, HINGE_LEAF[0], HINGE_LEAF[1]];
    components.push(box_part(
        inputs,
        "gangway.hinge".into(),
        "Gangway hinge leaf".into(),
        ComponentKind::GangwayHinge,
        &material,
        [x0, 0.0, deck_top],
        leaf,
        cuboid(leaf).scaled(density),
    ));
    let stringer_profile = format!(
        "{} x {} x {} rect tube",
        fmt_num(sw),
        fmt_num(sh),
        fmt_num(TUBE_WALL_IN)
    );
    for (side, x) in [("port", x0), ("stbd", x0 + gw - sw)] {
        let size = [sw, length, sh];
        let mut part = tube_part(
            inputs,
            format!("gangway.stringer.{side}"),
            format!(
                "Gangway stringer, {}",
                if side == "port" { "port" } else { "starboard" }
            ),
            ComponentKind::GangwayStringer,
            &material,
            [x, -length, deck_top - sh],
            size,
            1,
            density,
        );
        part.stock = Some(StockInfo {
            profile: stringer_profile.clone(),
            cut_length_in: length,
            stock_length_in: stock_length,
        });
        components.push(part);
    }
    let [tw, tt] = GANGWAY_TREAD;
    let tread_len = gw - 2.0 * sw;
    let count = (length / GANGWAY_TREAD_PITCH).floor() as u32;
    let gap = (length - f64::from(count) * tw) / f64::from(count.max(1));
    for k in 0..count {
        let size = [tread_len, tw, tt];
        let y = -length + gap * 0.5 + f64::from(k) * (tw + gap);
        let mut part = tube_part(
            inputs,
            format!("gangway.tread.{k}"),
            format!("Gangway tread {}", k + 1),
            ComponentKind::GangwayTread,
            &material,
            [x0 + sw, y, deck_top - tt],
            size,
            0,
            density,
        );
        part.stock = Some(StockInfo {
            profile: format!(
                "{} x {} x {} plank",
                fmt_num(tw),
                fmt_num(tt),
                fmt_num(TUBE_WALL_IN)
            ),
            cut_length_in: tread_len,
            stock_length_in: stock_length,
        });
        components.push(part);
    }
    Ok(FeatureOutput {
        components,
        ..FeatureOutput::default()
    })
}

/// Sketch frame (sketch x, y, and extrude normal) in world coordinates.
pub(crate) fn plane_frame(
    plane: &SketchPlane,
    components: &[Component],
    oracle: &dyn ShapeOracle,
) -> Result<Pose, String> {
    let (origin, x, y): (Vec3, Vec3, Vec3) = match plane {
        SketchPlane::Xy { offset } => ([0.0, 0.0, *offset], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        SketchPlane::Xz { offset } => ([0.0, *offset, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        SketchPlane::Yz { offset } => ([*offset, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        SketchPlane::Face { component, face } => {
            let c = components
                .iter()
                .find(|c| c.id.0 == *component)
                .ok_or_else(|| format!("part {component} no longer exists"))?;
            let f = oracle.face(&c.shape, face)?;
            let n = normalize(c.pose.rotate(f.normal));
            let origin = c.pose.apply(f.centroid);
            // Sketch "up" follows world up when the face allows it.
            let up = if n[2].abs() < 0.9 {
                [0.0, 0.0, 1.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let y = normalize(crate::geometry::sub(
                up,
                crate::geometry::scale(n, dot(up, n)),
            ));
            let x = cross(y, n);
            (origin, x, y)
        }
    };
    if !origin.iter().all(|v| v.is_finite()) {
        return Err("sketch plane offset is not finite".into());
    }
    Ok(Pose::from_axes(origin, x, y, cross(x, y)))
}

/// Mass integrals for a shape, or a diagnostic saying why they could not
/// be computed (OCCT-01 when OpenCascade is required but absent).
fn shape_integrals(
    oracle: &dyn ShapeOracle,
    shape: &Shape,
    feature: &Feature,
    related: &str,
) -> Result<MassIntegrals, Diagnostic> {
    oracle.integrals(shape).map_err(|why| {
        let needs = shape.needs_occt().filter(|_| !oracle.native());
        Diagnostic::new(
            Severity::Error,
            "feature",
            if needs.is_some() {
                "OCCT-01"
            } else {
                "FEAT-01"
            },
            format!("{} failed: {why}", feature.name),
            vec![related.to_string(), feature.id.0.clone()],
        )
    })
}

fn extrude(inputs: &Inputs, spec: &ExtrudeSpec, ctx: &Context) -> crate::Result<FeatureOutput> {
    let feature = inputs.feature;
    let depth = match &spec.depth_parameter {
        Some(name) => inputs.get(name)?,
        None => spec.depth,
    };
    if !(depth.is_finite() && depth > 0.0) {
        return Err(crate::Error::Validation(format!(
            "{}: depth must be positive",
            feature.name
        )));
    }
    let profile = spec
        .profile
        .normalized()
        .map_err(|e| crate::Error::Validation(format!("{}: {e}", feature.name)))?;
    let failed = |d: Diagnostic| FeatureOutput {
        diagnostics: vec![d],
        ..FeatureOutput::default()
    };
    let frame = match plane_frame(&spec.plane, ctx.components, ctx.oracle) {
        Ok(f) => f,
        Err(why) => {
            return Ok(failed(Diagnostic::new(
                Severity::Error,
                "feature",
                "FEAT-01",
                format!("{} lost its sketch plane: {why}", feature.name),
                vec![feature.id.0.clone()],
            )))
        }
    };
    let shift = match spec.direction {
        ExtrudeDirection::Forward => 0.0,
        ExtrudeDirection::Backward => -depth,
        ExtrudeDirection::Symmetric => -depth * 0.5,
    };
    let pose = frame.then(&Pose::translation([0.0, 0.0, shift]));
    let shape = Shape::Prism { profile, depth };
    let density = inputs.density_in3()?;
    let material = inputs.material_key()?.to_string();

    if let Some(target) = &spec.cut_into {
        // A cut is a modifier on the target part with this prism as tool.
        let modify = ModifySpec {
            target: target.clone(),
            op: ShapeOp::Cut {
                tool: Box::new(shape),
                pose,
            },
        };
        let mut parts = ctx.components.to_vec();
        return Ok(
            match apply_modify(
                &mut parts,
                inputs.project,
                feature,
                &modify,
                ctx.oracle,
                true,
            ) {
                Ok(()) => FeatureOutput {
                    replaced: parts.into_iter().filter(|c| c.id.0 == *target).collect(),
                    ..FeatureOutput::default()
                },
                Err(d) => failed(d),
            },
        );
    }

    let id = format!(
        "custom.{}",
        feature.id.0.strip_prefix("feat.").unwrap_or(&feature.id.0)
    );
    let integrals = match shape_integrals(ctx.oracle, &shape, feature, &id) {
        Ok(i) => i,
        Err(d) => return Ok(failed(d)),
    };
    let mut mass = MassProps::uniform(&integrals, density, &pose);
    mass.volume_in3 = integrals.mass;
    let part = Component::new(
        ComponentId::new(id),
        feature.name.clone(),
        ComponentKind::Custom,
        feature.id.clone(),
        material,
        shape,
        pose,
        mass,
    );
    Ok(FeatureOutput {
        components: vec![part],
        ..FeatureOutput::default()
    })
}

/// Applies a modifier to its target in `components`. Material removed by
/// the modifier comes off the part's mass at its material's density.
pub(crate) fn apply_modify(
    components: &mut [Component],
    project: &Project,
    feature: &Feature,
    spec: &ModifySpec,
    oracle: &dyn ShapeOracle,
    as_cut: bool,
) -> Result<(), Diagnostic> {
    let fail = |code: &str, why: String| {
        Diagnostic::new(
            Severity::Error,
            "feature",
            code,
            format!("{} failed: {why}", feature.name),
            vec![spec.target.clone(), feature.id.0.clone()],
        )
    };
    let target = components
        .iter_mut()
        .find(|c| c.id.0 == spec.target)
        .ok_or_else(|| fail("FEAT-01", format!("part {} no longer exists", spec.target)))?;
    let next = match &target.shape {
        Shape::Modified { base, ops } => {
            let mut ops = ops.clone();
            ops.push(spec.op.clone());
            Shape::Modified {
                base: base.clone(),
                ops,
            }
        }
        other => Shape::Modified {
            base: Box::new(other.clone()),
            ops: vec![spec.op.clone()],
        },
    };
    next.validate().map_err(|why| fail("FEAT-01", why))?;
    let before = shape_integrals(oracle, &target.shape, feature, &spec.target)?;
    let after = shape_integrals(oracle, &next, feature, &spec.target)?;
    if after.mass <= 1e-9 {
        return Err(fail("FEAT-01", "nothing of the part would remain".into()));
    }
    if as_cut && (before.mass - after.mass) < 1e-9 {
        return Err(fail(
            "FEAT-01",
            format!("the sketch does not overlap {}", target.name),
        ));
    }
    let density = project
        .materials
        .get(&target.material)
        .map(|m| m.density_lb_ft3 / IN3_PER_FT3)
        .unwrap_or(0.0);
    let removed = before
        .minus(&after)
        .scaled(density)
        .transformed(&target.pose);
    let mut mass = MassProps::from_integrals(&target.mass.integrals.minus(&removed));
    mass.volume_in3 = after.mass;
    target.mass = mass;
    target.shape = next;
    target.bounds = world_bounds(&target.shape, &target.pose);
    Ok(())
}
