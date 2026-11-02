//! Assembly mates: rigid positioning of one feature's parts against another
//! part, by named edges.
//!
//! A mate references a straight edge on a parent part and one on a child
//! part. Solving moves every part of the child's feature (its "group") as
//! one rigid body so the child edge lies on the parent edge, midpoints
//! together, then applies the mate's value:
//!
//! - `fixed`: nothing more.
//! - `revolute`: turn the group by the value (degrees) about the parent
//!   edge, right-handed about its canonical direction.
//! - `prismatic`: slide the group by the value (inches) along that edge.
//!
//! Mates form a tree: a group may be the child of one mate. A cycle, or a
//! second mate moving the same group, is reported and skipped rather than
//! guessed at.

use crate::diagnostics::{Diagnostic, Severity};
use crate::document::{world_bounds, Component};
use crate::geometry::{add, cross, dot, norm, normalize, scale, sub, Pose, ShapeOracle, Vec3};
use crate::parameters::fmt_num;
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MateKind {
    Fixed,
    Revolute,
    Prismatic,
}

/// An edge of a part, by construction name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MateRef {
    pub component: String,
    pub edge: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mate {
    pub id: String,
    pub name: String,
    pub kind: MateKind,
    /// Stays put.
    pub parent: MateRef,
    /// Its whole feature group moves.
    pub child: MateRef,
    /// Parameter that drives the angle (deg) or offset (in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    /// Constant value when no parameter drives the mate.
    #[serde(default)]
    pub value: f64,
    /// Flip the sense of the value (e.g. so a positive angle raises the far
    /// end of a gangway).
    #[serde(default)]
    pub invert: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<[f64; 2]>,
}

/// Solved state of one mate, for the UI (axis glyph, value readout).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MateState {
    pub id: String,
    pub kind: MateKind,
    /// World axis: a point on it and its unit direction.
    pub origin: Vec3,
    pub direction: Vec3,
    /// Edge length on the parent, for drawing the axis.
    pub length: f64,
    pub value: f64,
    pub moved: Vec<String>,
}

/// Direction with its largest component positive, so an edge's sense does
/// not depend on which end the B-rep visited first.
fn canonical(d: Vec3) -> Vec3 {
    let d = normalize(d);
    let k = (0..3)
        .max_by(|a, b| d[*a].abs().total_cmp(&d[*b].abs()))
        .unwrap_or(0);
    if d[k] < 0.0 {
        scale(d, -1.0)
    } else {
        d
    }
}

/// Smallest rotation taking unit `from` onto unit `to`.
fn rotation_between(from: Vec3, to: Vec3) -> Pose {
    let c = dot(from, to).clamp(-1.0, 1.0);
    let axis = cross(from, to);
    if norm(axis) < 1e-12 {
        if c > 0.0 {
            return Pose::IDENTITY;
        }
        // Opposite: turn half way round any perpendicular axis.
        let helper = if from[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        return Pose::rotation(cross(from, helper), std::f64::consts::PI);
    }
    Pose::rotation(axis, c.acos())
}

fn world_edge(
    components: &[Component],
    oracle: &dyn ShapeOracle,
    r: &MateRef,
) -> Result<(Vec3, Vec3), String> {
    let c = components
        .iter()
        .find(|c| c.id.0 == r.component)
        .ok_or_else(|| format!("part {} does not exist", r.component))?;
    let (a, b) = oracle.edge(&c.shape, &r.edge)?;
    Ok((c.pose.apply(a), c.pose.apply(b)))
}

/// Positions mated groups. Returns per-mate state and any problems.
pub fn solve(
    project: &Project,
    components: &mut [Component],
    oracle: &dyn ShapeOracle,
) -> (Vec<MateState>, Vec<Diagnostic>) {
    let mut states = Vec::new();
    let mut out = Vec::new();
    let group_of: HashMap<String, String> = components
        .iter()
        .map(|c| (c.id.0.clone(), c.feature_id.0.clone()))
        .collect();
    // Order mates so a parent group settles before anything hanging off
    // it moves: a mate waits while another unsolved mate still moves the
    // group its parent part belongs to.
    let child_group = |m: &Mate| group_of.get(&m.child.component).cloned();
    let parent_group = |m: &Mate| group_of.get(&m.parent.component).cloned();
    let mut pending: Vec<&Mate> = project.mates.iter().collect();
    let mut moved_groups: HashSet<String> = HashSet::new();
    let mut ordered: Vec<&Mate> = Vec::new();
    while !pending.is_empty() {
        let ready: Vec<usize> = (0..pending.len())
            .filter(|&i| {
                let pg = parent_group(pending[i]);
                !pending
                    .iter()
                    .enumerate()
                    .any(|(j, other)| j != i && pg.is_some() && child_group(other) == pg)
            })
            .collect();
        if ready.is_empty() {
            for m in &pending {
                out.push(Diagnostic::new(
                    Severity::Error,
                    "assembly",
                    "MATE-01",
                    format!("{} is part of a loop of mates and was not solved", m.name),
                    vec![m.parent.component.clone(), m.child.component.clone()],
                ));
            }
            break;
        }
        let start = ordered.len();
        for &i in ready.iter().rev() {
            ordered.push(pending.remove(i));
        }
        ordered[start..].reverse();
    }
    for mate in ordered {
        let Some(child_group) = group_of.get(&mate.child.component).cloned() else {
            out.push(missing(mate, &mate.child.component));
            continue;
        };
        if group_of.get(&mate.parent.component) == Some(&child_group) {
            out.push(Diagnostic::new(
                Severity::Error,
                "assembly",
                "MATE-01",
                format!("{} joins a part to its own group", mate.name),
                vec![mate.child.component.clone()],
            ));
            continue;
        }
        if !moved_groups.insert(child_group.clone()) {
            out.push(Diagnostic::new(
                Severity::Error,
                "assembly",
                "MATE-01",
                format!(
                    "{} would move parts another mate already positions; one mate per moving group",
                    mate.name
                ),
                vec![mate.child.component.clone()],
            ));
            continue;
        }
        let (p0, p1) = match world_edge(components, oracle, &mate.parent) {
            Ok(e) => e,
            Err(e) => {
                out.push(broken(mate, &mate.parent, &e));
                continue;
            }
        };
        let (q0, q1) = match world_edge(components, oracle, &mate.child) {
            Ok(e) => e,
            Err(e) => {
                out.push(broken(mate, &mate.child, &e));
                continue;
            }
        };
        let (pd, qd) = (canonical(sub(p1, p0)), canonical(sub(q1, q0)));
        let pm = scale(add(p0, p1), 0.5);
        let qm = scale(add(q0, q1), 0.5);
        let mut value = match &mate.parameter {
            Some(name) => project.parameter_value(name).unwrap_or(mate.value),
            None => mate.value,
        };
        if let Some([lo, hi]) = mate.limits {
            if value < lo - 1e-9 || value > hi + 1e-9 {
                out.push(Diagnostic::new(
                    Severity::Warning,
                    "assembly",
                    "MATE-03",
                    format!(
                        "{} value {} is outside its limits {} to {}; held at the limit",
                        mate.name,
                        fmt_num(value),
                        fmt_num(lo),
                        fmt_num(hi)
                    ),
                    vec![mate.child.component.clone()],
                ));
                value = value.clamp(lo, hi);
            }
        }
        let signed = if mate.invert { -value } else { value };
        // Align: move the child edge's midpoint to the origin, turn its
        // direction onto the parent's, then move to the parent midpoint.
        let align = Pose::translation(pm)
            .then(&rotation_between(qd, pd))
            .then(&Pose::translation(scale(qm, -1.0)));
        let motion = match mate.kind {
            MateKind::Fixed => Pose::IDENTITY,
            MateKind::Revolute => Pose::rotation_about(pm, pd, signed.to_radians()),
            MateKind::Prismatic => Pose::translation(scale(pd, signed)),
        };
        let transform = motion.then(&align);
        let mut moved = Vec::new();
        for c in components.iter_mut() {
            if c.feature_id.0 == child_group {
                c.pose = transform.then(&c.pose);
                c.mass = c.mass.transformed(&transform);
                c.bounds = world_bounds(&c.shape, &c.pose);
                moved.push(c.id.0.clone());
            }
        }
        states.push(MateState {
            id: mate.id.clone(),
            kind: mate.kind,
            origin: pm,
            direction: pd,
            length: norm(sub(p1, p0)),
            value,
            moved,
        });
    }
    (states, out)
}

fn missing(m: &Mate, component: &str) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        "assembly",
        "MATE-02",
        format!(
            "{} references part {component}, which no longer exists",
            m.name
        ),
        vec![component.to_string()],
    )
}

fn broken(m: &Mate, r: &MateRef, why: &str) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        "assembly",
        "MATE-02",
        format!(
            "{} cannot use edge {} of {}: {why}",
            m.name, r.edge, r.component
        ),
        vec![r.component.clone()],
    )
}
