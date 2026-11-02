use crate::feature::{Feature, FeatureKind};
use crate::ids::FeatureId;
use crate::mates::{Mate, MateKind, MateRef};
use crate::parameters::{Parameter, Unit};
use crate::project::{Material, Project, SCHEMA_VERSION};
use indexmap::IndexMap;
use std::collections::BTreeMap;

/// The included sample: an 8 ft x 16 ft modular floating dock on 24 in
/// rotomolded floats, aluminum tube frame, ipe deck, and stainless cleats.
pub fn modular_dock() -> Project {
    let mut parameters = IndexMap::new();
    let mut param = |name: &str,
                     label: &str,
                     group: &str,
                     value: f64,
                     unit: Unit,
                     (min, max, step): (f64, f64, f64),
                     snap: bool| {
        parameters.insert(
            name.to_string(),
            Parameter {
                name: name.into(),
                label: label.into(),
                value,
                unit,
                min,
                max,
                step,
                snap,
                group: group.into(),
            },
        );
    };
    param(
        "width_in",
        "Dock width",
        "plan",
        96.0,
        Unit::Inch,
        (48.0, 240.0, 24.0),
        true,
    );
    param(
        "length_in",
        "Dock length",
        "plan",
        192.0,
        Unit::Inch,
        (96.0, 480.0, 24.0),
        true,
    );
    param(
        "module_in",
        "Float module",
        "plan",
        24.0,
        Unit::Inch,
        (24.0, 48.0, 24.0),
        true,
    );
    param(
        "float_height_in",
        "Float height",
        "floats",
        16.0,
        Unit::Inch,
        (12.0, 24.0, 1.0),
        false,
    );
    param(
        "beam_width_in",
        "Frame tube width",
        "frame",
        2.0,
        Unit::Inch,
        (1.5, 4.0, 0.25),
        false,
    );
    param(
        "beam_height_in",
        "Frame tube depth",
        "frame",
        4.0,
        Unit::Inch,
        (2.0, 8.0, 0.5),
        false,
    );
    param(
        "joist_spacing_in",
        "Joist spacing",
        "frame",
        48.0,
        Unit::Inch,
        (24.0, 96.0, 24.0),
        true,
    );
    param(
        "deck_thickness_in",
        "Deck thickness",
        "deck",
        1.0,
        Unit::Inch,
        (0.75, 2.0, 0.25),
        false,
    );
    param(
        "live_load_psf",
        "Design live load",
        "loads",
        30.0,
        Unit::Psf,
        (0.0, 100.0, 5.0),
        false,
    );
    param(
        "gangway_length_in",
        "Gangway length",
        "gangway",
        144.0,
        Unit::Inch,
        (72.0, 360.0, 12.0),
        true,
    );
    param(
        "gangway_width_in",
        "Gangway width",
        "gangway",
        36.0,
        Unit::Inch,
        (30.0, 60.0, 6.0),
        true,
    );
    param(
        "gangway_angle_deg",
        "Gangway slope (shore end up)",
        "gangway",
        4.0,
        Unit::Degree,
        (-20.0, 30.0, 0.5),
        false,
    );

    let mut materials = IndexMap::new();
    let mut material = |key: &str,
                        name: &str,
                        family: &str,
                        density_lb_ft3: f64,
                        color: [f32; 3],
                        max_span_in: Option<f64>,
                        stock_length_in: Option<f64>,
                        applies_to: &[&str]| {
        materials.insert(
            key.to_string(),
            Material {
                name: name.into(),
                family: family.into(),
                density_lb_ft3,
                color,
                max_span_in,
                stock_length_in,
                applies_to: applies_to.iter().map(|s| s.to_string()).collect(),
            },
        );
    };
    material(
        "hdpe_float",
        "Rotomolded HDPE float, foam filled",
        "polymer",
        59.0,
        [0.10, 0.23, 0.30],
        None,
        None,
        &["float_pattern"],
    );
    material(
        "alum_6061",
        "6061-T6 aluminum rectangular tube",
        "aluminum",
        169.0,
        [0.70, 0.74, 0.78],
        None,
        Some(288.0),
        &["frame_beams", "gangway"],
    );
    material(
        "galv_steel",
        "Hot-dip galvanized steel tube",
        "steel",
        490.0,
        [0.52, 0.55, 0.57],
        None,
        Some(240.0),
        &["frame_beams"],
    );
    material(
        "ipe",
        "Ipe hardwood decking",
        "hardwood",
        69.0,
        [0.45, 0.28, 0.17],
        Some(24.0),
        Some(240.0),
        &["deck_boards"],
    );
    material(
        "composite_deck",
        "Capped composite decking",
        "composite",
        62.0,
        [0.36, 0.33, 0.30],
        Some(16.0),
        Some(240.0),
        &["deck_boards"],
    );
    material(
        "pt_pine",
        "Pressure-treated southern pine",
        "softwood",
        36.0,
        [0.62, 0.50, 0.33],
        Some(24.0),
        Some(192.0),
        &["deck_boards"],
    );
    material(
        "ss316",
        "316 stainless cast cleat",
        "stainless",
        499.0,
        [0.84, 0.86, 0.88],
        None,
        None,
        &["cleats"],
    );
    material(
        "pvc_bumper",
        "Marine PVC D-profile bumper",
        "polymer",
        87.0,
        [0.92, 0.92, 0.88],
        None,
        Some(192.0),
        &["bumpers"],
    );

    const W: &str = "width_in";
    const L: &str = "length_in";
    const M: &str = "module_in";
    const FH: &str = "float_height_in";
    const BW: &str = "beam_width_in";
    const BH: &str = "beam_height_in";
    const JS: &str = "joist_spacing_in";
    const DT: &str = "deck_thickness_in";
    const LL: &str = "live_load_psf";
    const GL: &str = "gangway_length_in";
    const GW: &str = "gangway_width_in";
    const GA: &str = "gangway_angle_deg";

    Project {
        schema: SCHEMA_VERSION,
        id: "proj.modular_dock".into(),
        name: "Modular floating dock".into(),
        units: Unit::Inch,
        parameters,
        features: vec![
            feature(
                "feat.parameters",
                "Parameters",
                &[],
                FeatureKind::Parameters,
                &[],
                None,
            ),
            feature(
                "feat.plan",
                "Plan sketch",
                &["feat.parameters"],
                FeatureKind::PlanSketch,
                &[W, L, M],
                None,
            ),
            feature(
                "feat.floats",
                "Float pattern",
                &["feat.plan"],
                FeatureKind::FloatPattern,
                &[W, L, M, FH],
                Some("hdpe_float"),
            ),
            feature(
                "feat.frame",
                "Frame beams",
                &["feat.plan"],
                FeatureKind::FrameBeams,
                &[W, L, M, FH, BW, BH, JS],
                Some("alum_6061"),
            ),
            feature(
                "feat.deck",
                "Deck boards",
                &["feat.frame"],
                FeatureKind::DeckBoards,
                &[W, L, FH, BH, DT],
                Some("ipe"),
            ),
            feature(
                "feat.cleats",
                "Mooring cleats",
                &["feat.deck"],
                FeatureKind::Cleats,
                &[W, L, FH, BW, BH, DT],
                Some("ss316"),
            ),
            feature(
                "feat.bumpers",
                "End bumpers",
                &["feat.frame"],
                FeatureKind::Bumpers,
                &[W, L, FH, BH],
                Some("pvc_bumper"),
            ),
            feature(
                "feat.gangway",
                "Access gangway",
                &["feat.deck"],
                FeatureKind::Gangway,
                &[W, FH, BH, DT, GL, GW],
                Some("alum_6061"),
            ),
            feature(
                "feat.assembly",
                "Dock assembly",
                &[
                    "feat.floats",
                    "feat.frame",
                    "feat.deck",
                    "feat.cleats",
                    "feat.bumpers",
                    "feat.gangway",
                ],
                FeatureKind::Assembly,
                &[LL, GA],
                None,
            ),
        ],
        materials,
        placements: BTreeMap::new(),
        mates: vec![Mate {
            id: "mate.gangway_hinge".into(),
            name: "Gangway hinge".into(),
            kind: MateKind::Revolute,
            // The hinge leaf's front lower edge sits on the stern deck edge.
            parent: MateRef {
                component: "deck.board.0".into(),
                edge: "y-|z+".into(),
            },
            child: MateRef {
                component: "gangway.hinge".into(),
                edge: "y-|z-".into(),
            },
            parameter: Some(GA.into()),
            value: 0.0,
            // Positive slope raises the shore end, which lies toward -y.
            invert: true,
            limits: Some([-20.0, 30.0]),
        }],
    }
}

fn feature(
    id: &str,
    name: &str,
    parents: &[&str],
    kind: FeatureKind,
    inputs: &[&str],
    material: Option<&str>,
) -> Feature {
    Feature {
        id: FeatureId::new(id),
        name: name.into(),
        parents: parents.iter().map(|p| FeatureId::new(*p)).collect(),
        kind,
        inputs: inputs.iter().map(|s| s.to_string()).collect(),
        material: material.map(str::to_string),
        suppressed: false,
        spec: None,
    }
}
