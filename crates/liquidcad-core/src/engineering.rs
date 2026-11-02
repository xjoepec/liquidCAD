//! Hydrostatics, structural rules of thumb, mass properties, and the BOM.

use crate::diagnostics::{Diagnostic, Severity};
use crate::document::{BomLine, Component, ComponentKind, EngineeringSummary};
use crate::feature::FeatureKind;
use crate::geometry::MassProps;
use crate::parameters::fmt_num;
use crate::project::Project;
use std::collections::BTreeMap;

/// Fresh water.
pub const WATER_LB_FT3: f64 = 62.4;
/// Stringer span to depth ratio above which deflection gets noticeable
/// underfoot for aluminum tube at dock live loads.
const MAX_SPAN_TO_DEPTH: f64 = 20.0;
/// Below this the deck is awash under dead load in a light chop.
const MIN_FREEBOARD_IN: f64 = 10.0;
/// Keep at least this fraction of float height above water at full load.
const MIN_RESERVE_HEIGHT: f64 = 0.2;

/// Heel above this under a half-deck live load is uncomfortable underfoot.
const MAX_ECCENTRIC_HEEL_DEG: f64 = 5.0;
/// Static trim or heel from an off-centre dead load.
const MAX_STATIC_ANGLE_DEG: f64 = 2.0;

/// Waterplane of the float array: area, centroid, and second moments about
/// axes through the centroid (in², in, in⁴).
struct Waterplane {
    area: f64,
    centroid: [f64; 2],
    /// About the longitudinal axis (governs roll / heel).
    i_t: f64,
    /// About the transverse axis (governs pitch / trim).
    i_l: f64,
}

fn waterplane(floats: &[&Component]) -> Waterplane {
    let area: f64 = floats
        .iter()
        .map(|c| c.bounds.size[0] * c.bounds.size[1])
        .sum();
    let mut centroid = [0.0; 2];
    if area > 0.0 {
        for c in floats {
            let a = c.bounds.size[0] * c.bounds.size[1];
            let m = c.bounds.center();
            centroid[0] += a * m[0] / area;
            centroid[1] += a * m[1] / area;
        }
    }
    let (mut i_t, mut i_l) = (0.0, 0.0);
    for c in floats {
        let [w, l, _] = c.bounds.size;
        let a = w * l;
        let m = c.bounds.center();
        i_t += l * w.powi(3) / 12.0 + a * (m[0] - centroid[0]).powi(2);
        i_l += w * l.powi(3) / 12.0 + a * (m[1] - centroid[1]).powi(2);
    }
    Waterplane {
        area,
        centroid,
        i_t,
        i_l,
    }
}

/// Small-angle stability of wall-sided floats at the draft that supports
/// `load_lb` with centre of gravity `cg`.
struct Equilibrium {
    draft: f64,
    kb: f64,
    gm_t: f64,
    gm_l: f64,
    heel_deg: f64,
    trim_deg: f64,
}

fn equilibrium(wp: &Waterplane, load_lb: f64, cg: [f64; 3]) -> Equilibrium {
    let rho = WATER_LB_FT3 / 1728.0;
    let volume = load_lb / rho;
    let draft = if wp.area > 0.0 {
        volume / wp.area
    } else {
        f64::INFINITY
    };
    let kb = draft * 0.5;
    let (bm_t, bm_l) = if volume > 0.0 {
        (wp.i_t / volume, wp.i_l / volume)
    } else {
        (0.0, 0.0)
    };
    let gm_t = kb + bm_t - cg[2];
    let gm_l = kb + bm_l - cg[2];
    // The centre of buoyancy sits over the waterplane centroid when level;
    // an offset centre of gravity rolls the dock until they line up.
    let heel_deg = ((cg[0] - wp.centroid[0]) / gm_t).atan().to_degrees();
    let trim_deg = ((cg[1] - wp.centroid[1]) / gm_l).atan().to_degrees();
    Equilibrium {
        draft,
        kb,
        gm_t,
        gm_l,
        heel_deg,
        trim_deg,
    }
}

pub fn analyze(
    project: &Project,
    components: &[Component],
) -> (EngineeringSummary, Vec<Diagnostic>) {
    let mut out = Vec::new();
    let value = |name: &str| project.parameter_value(name).unwrap_or(0.0);
    let width = value("width_in");
    let length = value("length_in");
    let module = value("module_in");
    let float_h = value("float_height_in");
    let beam_h = value("beam_height_in");
    let deck_t = value("deck_thickness_in");
    let joist_spacing = value("joist_spacing_in");
    let live_psf = value("live_load_psf");

    let floats: Vec<&Component> = components
        .iter()
        .filter(|c| c.kind == ComponentKind::FloatModule)
        .collect();
    let float_count = floats.len() as u32;
    let float_volume_ft3: f64 = floats.iter().map(|c| c.bounds.volume_in3() / 1728.0).sum();
    let wp = waterplane(&floats);
    let float_plan_ft2 = wp.area / 144.0;
    let capacity = float_volume_ft3 * WATER_LB_FT3;
    // Everything the floats carry: the dock itself plus half the gangway,
    // which bears on the hinge (the shore end carries the other half).
    let is_gangway = |c: &Component| {
        matches!(
            c.kind,
            ComponentKind::GangwayStringer
                | ComponentKind::GangwayTread
                | ComponentKind::GangwayHinge
        )
    };
    let mass = MassProps::combine(components.iter().map(|c| &c.mass));
    let gangway_lb: f64 = components
        .iter()
        .filter(|c| is_gangway(c))
        .map(|c| c.mass_lb())
        .sum();
    let hinge = components
        .iter()
        .find(|c| c.kind == ComponentKind::GangwayHinge)
        .map(|c| {
            let (_, hi) = c.shape.local_bounds();
            c.pose.apply([hi[0] * 0.5, 0.0, 0.0])
        });
    let mut supported = MassProps::combine(
        components
            .iter()
            .filter(|c| !is_gangway(c))
            .map(|c| &c.mass),
    )
    .integrals;
    if let (Some(p), true) = (hinge, gangway_lb > 0.0) {
        let m = gangway_lb * 0.5;
        supported = supported.plus(&crate::geometry::MassIntegrals {
            mass: m,
            first: [m * p[0], m * p[1], m * p[2]],
            second: [
                [m * p[0] * p[0], m * p[0] * p[1], m * p[0] * p[2]],
                [m * p[1] * p[0], m * p[1] * p[1], m * p[1] * p[2]],
                [m * p[2] * p[0], m * p[2] * p[1], m * p[2] * p[2]],
            ],
        });
    }
    let floating = supported.props();
    let dead = floating.mass_lb;
    let deck_area_ft2 = width * length / 144.0;
    let live = live_psf * deck_area_ft2;
    let draft = |load: f64| {
        if float_plan_ft2 > 0.0 {
            load / (WATER_LB_FT3 * float_plan_ft2) * 12.0
        } else {
            f64::INFINITY
        }
    };
    let deck_top = components
        .iter()
        .filter(|c| c.kind == ComponentKind::DeckBoard)
        .map(|c| c.bounds.max()[2])
        .fold(float_h + 2.0 * beam_h + deck_t, f64::max);
    let draft_dead = draft(dead);
    let draft_loaded = draft(dead + live);
    let reserve = if capacity > 0.0 {
        1.0 - (dead + live) / capacity
    } else {
        0.0
    };
    let cg = floating.com;

    let rest = equilibrium(&wp, dead, cg);
    // Crowd on one half of the deck: the port half carries half the live
    // load at its centroid, a quarter width in from the port edge.
    let crowd = live * 0.5;
    let crowd_at = [width * 0.25, length * 0.5, deck_top];
    let total = dead + crowd;
    let crowded_cg = if total > 0.0 {
        [
            (dead * cg[0] + crowd * crowd_at[0]) / total,
            (dead * cg[1] + crowd * crowd_at[1]) / total,
            (dead * cg[2] + crowd * crowd_at[2]) / total,
        ]
    } else {
        cg
    };
    let crowded = equilibrium(&wp, total, crowded_cg);
    let edge_freeboard =
        deck_top - crowded.draft - (width * 0.5) * crowded.heel_deg.abs().to_radians().tan();
    let center_of_buoyancy = [wp.centroid[0], wp.centroid[1], rest.kb];

    if float_count > 0 && dead > 0.0 {
        if rest.gm_t <= 0.0 || rest.gm_l <= 0.0 {
            out.push(Diagnostic::new(
                Severity::Error,
                "engineering",
                "STAB-01",
                format!(
                    "unstable: metacentric height GM is {} in across and {} in along; it must be positive",
                    fmt_num((rest.gm_t * 10.0).round() / 10.0),
                    fmt_num((rest.gm_l * 10.0).round() / 10.0)
                ),
                vec!["feat.floats".into()],
            ));
        }
        if edge_freeboard < 0.0 {
            out.push(Diagnostic::new(
                Severity::Error,
                "engineering",
                "STAB-02",
                format!(
                    "with {} lb of crowd on the port half the dock heels {} deg and the port deck edge goes under",
                    fmt_num(crowd.round()),
                    fmt_num((crowded.heel_deg.abs() * 10.0).round() / 10.0)
                ),
                vec!["feat.floats".into()],
            ));
        } else if crowded.heel_deg.abs() > MAX_ECCENTRIC_HEEL_DEG {
            out.push(Diagnostic::new(
                Severity::Warning,
                "engineering",
                "STAB-02",
                format!(
                    "with {} lb of crowd on the port half the dock heels {} deg (over {} deg); port edge freeboard {} in",
                    fmt_num(crowd.round()),
                    fmt_num((crowded.heel_deg.abs() * 10.0).round() / 10.0),
                    fmt_num(MAX_ECCENTRIC_HEEL_DEG),
                    fmt_num((edge_freeboard * 10.0).round() / 10.0)
                ),
                vec!["feat.floats".into()],
            ));
        }
        if rest.heel_deg.abs() > MAX_STATIC_ANGLE_DEG || rest.trim_deg.abs() > MAX_STATIC_ANGLE_DEG
        {
            out.push(Diagnostic::new(
                Severity::Warning,
                "engineering",
                "STAB-03",
                format!(
                    "off-centre dead load: dock floats at {} deg heel and {} deg trim (over {} deg)",
                    fmt_num((rest.heel_deg * 10.0).round() / 10.0),
                    fmt_num((rest.trim_deg * 10.0).round() / 10.0),
                    fmt_num(MAX_STATIC_ANGLE_DEG)
                ),
                vec![],
            ));
        }
    }

    if gangway_lb > 0.0 {
        let angle = value("gangway_angle_deg");
        let rise = angle.to_radians().tan().abs();
        if rise > 1.0 / 12.0 + 1e-9 {
            out.push(Diagnostic::new(
                Severity::Warning,
                "engineering",
                "GANG-01",
                format!(
                    "gangway slope 1:{} ({} deg) is steeper than 1:12, the accessible ramp limit",
                    fmt_num((1.0 / rise * 10.0).round() / 10.0),
                    fmt_num(angle.abs())
                ),
                vec!["feat.gangway".into()],
            ));
        }
        out.push(Diagnostic::new(
            Severity::Info,
            "engineering",
            "GANG-00",
            format!(
                "gangway weighs {} lb; {} lb bears on the stern hinge",
                fmt_num(gangway_lb.round()),
                fmt_num((gangway_lb * 0.5).round())
            ),
            vec!["feat.gangway".into()],
        ));
    }

    let float_ids = || floats.iter().map(|c| c.id.0.clone()).collect::<Vec<_>>();
    if float_count == 0 {
        out.push(Diagnostic::new(
            Severity::Error,
            "engineering",
            "BUOY-02",
            "no floats: the float pattern feature is suppressed or empty",
            vec!["feat.floats".into()],
        ));
    } else if dead + live > capacity {
        out.push(Diagnostic::new(
            Severity::Error,
            "engineering",
            "BUOY-01",
            format!(
                "floats submerge: dead {} lb + live {} lb exceeds {} lb displacement",
                fmt_num(dead.round()),
                fmt_num(live.round()),
                fmt_num(capacity.round())
            ),
            float_ids(),
        ));
    } else if draft_loaded > float_h * (1.0 - MIN_RESERVE_HEIGHT) {
        out.push(Diagnostic::new(
            Severity::Warning,
            "engineering",
            "BUOY-03",
            format!(
                "loaded draft {} in leaves under {}% of the {} in float above water",
                fmt_num((draft_loaded * 10.0).round() / 10.0),
                fmt_num(MIN_RESERVE_HEIGHT * 100.0),
                fmt_num(float_h)
            ),
            float_ids(),
        ));
    }
    if float_count > 0 {
        out.push(Diagnostic::new(
            Severity::Info,
            "engineering",
            "BUOY-00",
            format!(
                "{float_count} floats displace {} lb; dead {} lb + live {} lb ({} psf) leaves {}% reserve, draft {} in loaded",
                fmt_num(capacity.round()),
                fmt_num(dead.round()),
                fmt_num(live.round()),
                fmt_num(live_psf),
                fmt_num((reserve * 100.0).round()),
                fmt_num((draft_loaded * 10.0).round() / 10.0)
            ),
            vec![],
        ));
    }
    let freeboard_dead = deck_top - draft_dead;
    if float_count > 0 && freeboard_dead < MIN_FREEBOARD_IN {
        out.push(Diagnostic::new(
            Severity::Warning,
            "engineering",
            "FREE-01",
            format!(
                "deck sits {} in above water at rest; aim for at least {} in",
                fmt_num((freeboard_dead * 10.0).round() / 10.0),
                fmt_num(MIN_FREEBOARD_IN)
            ),
            vec!["feat.floats".into(), "feat.frame".into()],
        ));
    }

    let deck_feature = project
        .features
        .iter()
        .find(|f| f.kind == FeatureKind::DeckBoards && !f.suppressed);
    if let Some(deck) = deck_feature {
        let material = deck
            .material
            .as_deref()
            .and_then(|key| project.materials.get(key));
        if let Some(max_span) = material.and_then(|m| m.max_span_in) {
            if module > max_span + 1e-9 {
                out.push(Diagnostic::new(
                    Severity::Warning,
                    "engineering",
                    "SPAN-01",
                    format!(
                        "deck spans {} in between stringers; {} is rated for {} in",
                        fmt_num(module),
                        material.map(|m| m.name.as_str()).unwrap_or("deck"),
                        fmt_num(max_span)
                    ),
                    vec![deck.id.0.clone(), "feat.frame".into()],
                ));
            }
        }
    }

    let stringer_span = joist_spacing.min(length);
    let ratio = if beam_h > 0.0 {
        stringer_span / beam_h
    } else {
        f64::INFINITY
    };
    if components.iter().any(|c| c.kind == ComponentKind::Stringer) && ratio > MAX_SPAN_TO_DEPTH {
        out.push(Diagnostic::new(
            Severity::Warning,
            "engineering",
            "SLEND-01",
            format!(
                "stringers span {} in between joists on a {} in deep tube (L/d {}); keep L/d at or under {}",
                fmt_num(stringer_span),
                fmt_num(beam_h),
                fmt_num(ratio.round()),
                fmt_num(MAX_SPAN_TO_DEPTH)
            ),
            vec!["feat.frame".into()],
        ));
    }

    let summary = EngineeringSummary {
        float_count,
        deck_area_ft2,
        dead_load_lb: dead,
        live_load_lb: live,
        buoyancy_capacity_lb: capacity,
        draft_dead_in: draft_dead,
        draft_loaded_in: draft_loaded,
        freeboard_dead_in: freeboard_dead,
        freeboard_loaded_in: deck_top - draft_loaded,
        reserve_buoyancy_ratio: reserve,
        center_of_gravity: cg,
        center_of_buoyancy,
        gm_transverse_in: rest.gm_t,
        gm_longitudinal_in: rest.gm_l,
        kb_in: rest.kb,
        kg_in: cg[2],
        heel_static_deg: rest.heel_deg,
        trim_static_deg: rest.trim_deg,
        heel_crowd_deg: crowded.heel_deg,
        edge_freeboard_crowd_in: edge_freeboard,
        mass,
        floating,
        deck_top_in: deck_top,
        deck_span_in: module,
        stringer_span_in: stringer_span,
    };
    (summary, out)
}

/// Groups identical parts. Cut parts group by profile and cut length so the
/// BOM doubles as a cut list.
pub fn bill_of_materials(project: &Project, components: &[Component]) -> Vec<BomLine> {
    let mut groups: BTreeMap<(u8, String), BomLine> = BTreeMap::new();
    for c in components {
        let material = project
            .materials
            .get(&c.material)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| c.material.clone());
        let [sx, sy, sz] = c.bounds.size;
        let (order, item, size, cut) = match (&c.kind, &c.stock) {
            (ComponentKind::FloatModule, _) => (
                0,
                "Float module".to_string(),
                format!("{} x {} x {} in", fmt_num(sx), fmt_num(sy), fmt_num(sz)),
                None,
            ),
            (ComponentKind::Stringer, Some(stock)) => (
                1,
                "Stringer".into(),
                stock.profile.clone(),
                Some(stock.cut_length_in),
            ),
            (ComponentKind::Joist, Some(stock)) => (
                2,
                "Joist".into(),
                stock.profile.clone(),
                Some(stock.cut_length_in),
            ),
            (ComponentKind::DeckBoard, Some(stock)) => (
                3,
                "Deck board".into(),
                stock.profile.clone(),
                Some(stock.cut_length_in),
            ),
            (ComponentKind::Bumper, Some(stock)) => (
                5,
                "Bumper".into(),
                stock.profile.clone(),
                Some(stock.cut_length_in),
            ),
            (ComponentKind::GangwayStringer, Some(stock)) => (
                6,
                "Gangway stringer".into(),
                stock.profile.clone(),
                Some(stock.cut_length_in),
            ),
            (ComponentKind::GangwayTread, Some(stock)) => (
                7,
                "Gangway tread".into(),
                stock.profile.clone(),
                Some(stock.cut_length_in),
            ),
            (ComponentKind::GangwayHinge, _) => (
                8,
                "Gangway hinge leaf".into(),
                format!(
                    "{} x {} x {} in plate",
                    fmt_num(sz),
                    fmt_num(sy),
                    fmt_num(sx)
                ),
                None,
            ),
            (ComponentKind::Custom, _) => (10, c.name.clone(), custom_size(c), None),
            (ComponentKind::Cleat, _) => (
                4,
                "Horn cleat".into(),
                format!("{} in", fmt_num(c.bounds.size[1])),
                None,
            ),
            (kind, _) => (
                9,
                kind.tag().into(),
                format!("{} x {} x {} in", fmt_num(sx), fmt_num(sy), fmt_num(sz)),
                None,
            ),
        };
        let key = (
            order,
            format!(
                "{item}|{material}|{size}|{}",
                cut.map(|v| format!("{v:.3}")).unwrap_or_default()
            ),
        );
        let line = groups.entry(key).or_insert_with(|| BomLine {
            item: item.clone(),
            kind: c.kind.tag().into(),
            material: material.clone(),
            qty: 0,
            size: size.clone(),
            cut_length_in: cut,
            unit_weight_lb: c.mass_lb(),
            total_weight_lb: 0.0,
            notes: String::new(),
            component_ids: Vec::new(),
        });
        line.qty += 1;
        line.total_weight_lb += c.mass_lb();
        line.component_ids.push(c.id.0.clone());
    }
    groups
        .into_values()
        .map(|mut line| {
            line.notes = match (line.kind.as_str(), line.cut_length_in) {
                ("float_module", _) => "purchased, bolt through frame".into(),
                ("cleat", _) => "through-bolt to stringer with backing plate".into(),
                ("gangway_hinge", _) => "pin to stern joist, hinge on the deck edge".into(),
                ("custom", _) => "cut from plate (see DXF flat profile)".into(),
                (_, Some(cut)) => format!("cut to {} in", fmt_num(cut)),
                _ => String::new(),
            };
            line
        })
        .collect()
}

/// Plate size of a sketched part: thickness x profile extents.
fn custom_size(c: &Component) -> String {
    let base = match &c.shape {
        crate::geometry::Shape::Modified { base, .. } => base.as_ref(),
        other => other,
    };
    match base {
        crate::geometry::Shape::Prism { profile, depth } => {
            let (lo, hi) = profile.bounds();
            format!(
                "{} in plate, {} x {} in",
                fmt_num((depth * 1000.0).round() / 1000.0),
                fmt_num(((hi[0] - lo[0]) * 1000.0).round() / 1000.0),
                fmt_num(((hi[1] - lo[1]) * 1000.0).round() / 1000.0)
            )
        }
        _ => {
            let [x, y, z] = c.bounds.size;
            format!("{} x {} x {} in", fmt_num(x), fmt_num(y), fmt_num(z))
        }
    }
}
