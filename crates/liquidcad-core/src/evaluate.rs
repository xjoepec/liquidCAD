//! Dependency-aware evaluation of the feature graph.
//!
//! Every feature gets a cache key from its kind, material, suppression,
//! the values of its declared inputs, and the keys of its parents. A feature
//! regenerates only when that key changes; otherwise its previous output is
//! reused. Because parent keys fold into child keys, a change propagates to
//! exactly the downstream features that can observe it.

use crate::document::{Document, Scene};
use crate::engineering;
use crate::feature::{Feature, FeatureKind, FeatureSpec};
use crate::generators::{apply_modify, generate, Context, FeatureOutput};
use crate::geometry::{PolyhedralOracle, ShapeOracle};
use crate::ids::FeatureId;
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Instant;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RebuildReport {
    pub regenerated: Vec<FeatureId>,
    pub reused: Vec<FeatureId>,
    pub elapsed_ms: f64,
}

#[derive(Clone)]
struct CacheEntry {
    key: u64,
    output: FeatureOutput,
}

pub struct Evaluator {
    cache: HashMap<FeatureId, CacheEntry>,
    oracle: Box<dyn ShapeOracle>,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl Evaluator {
    /// Polyhedral shapes only; features that need OpenCascade report OCCT-01.
    pub fn new() -> Self {
        Self::with_oracle(Box::new(PolyhedralOracle))
    }

    pub fn with_oracle(oracle: Box<dyn ShapeOracle>) -> Self {
        Self {
            cache: HashMap::new(),
            oracle,
        }
    }

    pub fn oracle(&self) -> &dyn ShapeOracle {
        self.oracle.as_ref()
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Evaluates the project. On error the cache is left untouched, so a
    /// rejected edit does not poison the next rebuild.
    pub fn rebuild(
        &mut self,
        project: &Project,
        revision: u64,
    ) -> crate::Result<(Document, RebuildReport)> {
        let started = Instant::now();
        project.validate()?;
        let ordered = project.ordered_features()?;
        let mut keys: HashMap<&FeatureId, u64> = HashMap::new();
        let mut next_cache = HashMap::with_capacity(ordered.len());
        let mut report = RebuildReport::default();
        let mut components = Vec::new();
        let mut sketches = Vec::new();
        let mut diagnostics = Vec::new();

        for feature in ordered {
            let key = feature_key(project, feature, &keys);
            keys.insert(&feature.id, key);
            let fresh = !matches!(self.cache.get(&feature.id), Some(entry) if entry.key == key);
            if fresh {
                report.regenerated.push(feature.id.clone());
            } else {
                report.reused.push(feature.id.clone());
            }
            if feature.kind == FeatureKind::Modify {
                // Modifiers rewrite a part made earlier; the oracle caches
                // the expensive kernel work, so they simply rerun.
                if let (false, Some(FeatureSpec::Modify(spec))) =
                    (feature.suppressed, &feature.spec)
                {
                    if let Err(d) = apply_modify(
                        &mut components,
                        project,
                        feature,
                        spec,
                        self.oracle.as_ref(),
                        false,
                    ) {
                        diagnostics.push(d);
                    }
                }
                next_cache.insert(
                    feature.id.clone(),
                    CacheEntry {
                        key,
                        output: FeatureOutput::default(),
                    },
                );
                continue;
            }
            let output = if fresh {
                let ctx = Context {
                    components: &components,
                    oracle: self.oracle.as_ref(),
                };
                generate(project, feature, &ctx)?
            } else {
                self.cache[&feature.id].output.clone()
            };
            for part in &output.replaced {
                if let Some(slot) = components.iter_mut().find(|c| c.id == part.id) {
                    *slot = part.clone();
                }
            }
            components.extend(output.components.iter().cloned());
            sketches.extend(output.sketch.iter().cloned());
            diagnostics.extend(output.diagnostics.iter().cloned());
            next_cache.insert(feature.id.clone(), CacheEntry { key, output });
        }

        let (mates, mate_checks) =
            crate::mates::solve(project, &mut components, self.oracle.as_ref());
        diagnostics.extend(mate_checks);
        let (engineering, checks) = engineering::analyze(project, &components);
        diagnostics.extend(checks);
        let bom = engineering::bill_of_materials(project, &components);
        let mut document = Document {
            project: project.clone(),
            components,
            sketches,
            scene: Scene::default(),
            bom,
            diagnostics,
            engineering,
            mates,
            revision,
        };
        let (min, max) = document.bounds();
        document.scene.bounds_min = min;
        document.scene.bounds_max = max;
        self.cache = next_cache;
        report.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok((document, report))
    }
}

/// One-shot rebuild without a cache.
pub fn rebuild(project: &Project, revision: u64) -> crate::Result<Document> {
    Evaluator::new()
        .rebuild(project, revision)
        .map(|(doc, _)| doc)
}

fn feature_key(project: &Project, feature: &Feature, parents: &HashMap<&FeatureId, u64>) -> u64 {
    let mut h = DefaultHasher::new();
    feature.kind.tag().hash(&mut h);
    feature.suppressed.hash(&mut h);
    for input in &feature.inputs {
        input.hash(&mut h);
        project
            .parameters
            .get(input)
            .map(|p| p.value.to_bits())
            .hash(&mut h);
    }
    if let Some(key) = &feature.material {
        key.hash(&mut h);
        if let Some(material) = project.materials.get(key) {
            // Hash the material definition so edits to the library propagate.
            serde_json::to_string(material)
                .unwrap_or_default()
                .hash(&mut h);
        }
    }
    if let Some(spec) = &feature.spec {
        serde_json::to_string(spec).unwrap_or_default().hash(&mut h);
    }
    if feature.kind == FeatureKind::Cleats {
        for (id, placement) in &project.placements {
            id.hash(&mut h);
            for v in placement.offset {
                v.to_bits().hash(&mut h);
            }
        }
    }
    for parent in &feature.parents {
        parents.get(parent).hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Severity;
    use crate::document::ComponentKind;
    use crate::project::Placement;
    use crate::templates::modular_dock;

    fn ids(list: &[FeatureId]) -> Vec<&str> {
        list.iter().map(|f| f.as_str()).collect()
    }

    fn count(doc: &Document, kind: ComponentKind) -> usize {
        doc.components.iter().filter(|c| c.kind == kind).count()
    }

    #[test]
    fn default_dock_has_expected_parts() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        // 96 x 192 in on a 24 in module: 4 x 8 floats.
        assert_eq!(count(&doc, ComponentKind::FloatModule), 32);
        // A stringer on every module line: 5 across.
        assert_eq!(count(&doc, ComponentKind::Stringer), 5);
        // Joists at stern, bow, and every 48 in between: 0, 48, 96, 144, end.
        assert_eq!(count(&doc, ComponentKind::Joist), 5);
        assert_eq!(count(&doc, ComponentKind::Cleat), 6);
        assert_eq!(count(&doc, ComponentKind::Bumper), 2);
        assert!(count(&doc, ComponentKind::DeckBoard) >= 33);
        assert_eq!(doc.sketches.len(), 1);
        assert!(doc
            .components
            .iter()
            .all(|c| c.bounds.is_valid() && c.mass_lb() > 0.0));
    }

    #[test]
    fn deck_boards_cover_the_length_without_overlap() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let mut boards: Vec<_> = doc
            .components
            .iter()
            .filter(|c| c.kind == ComponentKind::DeckBoard)
            .map(|c| (c.bounds.origin[1], c.bounds.max()[1]))
            .collect();
        boards.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for pair in boards.windows(2) {
            assert!(pair[1].0 >= pair[0].1 - 1e-9, "boards overlap: {pair:?}");
        }
        let last = boards.last().unwrap().1;
        assert!(last <= 192.0 + 1e-9 && 192.0 - last < 1.5 + 0.25 + 1e-9);
    }

    #[test]
    fn widening_regenerates_geometry_and_keeps_ids() {
        let mut project = modular_dock();
        let mut eval = Evaluator::new();
        let (before, _) = eval.rebuild(&project, 1).unwrap();
        project.set_parameter("width_in", 144.0).unwrap();
        let (after, report) = eval.rebuild(&project, 2).unwrap();
        assert_eq!(count(&after, ComponentKind::FloatModule), 48);
        assert_eq!(count(&after, ComponentKind::Stringer), 7);
        let joist = |d: &Document| {
            d.component(&crate::ComponentId::new("frame.joist.stern"))
                .unwrap()
                .bounds
                .size[0]
        };
        assert_eq!(joist(&after) - joist(&before), 48.0);
        let stbd = |d: &Document| {
            d.component(&crate::ComponentId::new("frame.stringer.stbd"))
                .unwrap()
                .bounds
                .origin[0]
        };
        assert_eq!(stbd(&after) - stbd(&before), 48.0);
        let floats = after.bom.iter().find(|l| l.kind == "float_module").unwrap();
        assert_eq!(floats.qty, 48);
        assert!(ids(&report.regenerated).contains(&"feat.floats"));
        assert!(after.engineering.buoyancy_capacity_lb > before.engineering.buoyancy_capacity_lb);
    }

    #[test]
    fn unrelated_features_are_reused() {
        let mut project = modular_dock();
        let mut eval = Evaluator::new();
        let (_, first) = eval.rebuild(&project, 1).unwrap();
        assert_eq!(first.reused.len(), 0);

        project.set_parameter("deck_thickness_in", 1.5).unwrap();
        let (doc, report) = eval.rebuild(&project, 2).unwrap();
        assert_eq!(
            ids(&report.regenerated),
            vec!["feat.deck", "feat.cleats", "feat.gangway", "feat.assembly"]
        );
        assert!(ids(&report.reused).contains(&"feat.floats"));
        assert!(ids(&report.reused).contains(&"feat.frame"));
        let board = doc
            .components
            .iter()
            .find(|c| c.kind == ComponentKind::DeckBoard)
            .unwrap();
        assert_eq!(board.bounds.size[2], 1.5);

        project.set_parameter("live_load_psf", 60.0).unwrap();
        let (_, report) = eval.rebuild(&project, 3).unwrap();
        assert_eq!(ids(&report.regenerated), vec!["feat.assembly"]);

        let (_, report) = eval.rebuild(&project, 4).unwrap();
        assert!(report.regenerated.is_empty());
    }

    #[test]
    fn material_change_regenerates_only_that_branch() {
        let mut project = modular_dock();
        let mut eval = Evaluator::new();
        let (before, _) = eval.rebuild(&project, 1).unwrap();
        project
            .set_feature_material("feat.deck", "composite_deck")
            .unwrap();
        let (after, report) = eval.rebuild(&project, 2).unwrap();
        assert!(ids(&report.regenerated).contains(&"feat.deck"));
        assert!(!ids(&report.regenerated).contains(&"feat.frame"));
        assert!(after.diagnostics.iter().any(|d| d.code == "SPAN-01"));
        assert!(!before.diagnostics.iter().any(|d| d.code == "SPAN-01"));
    }

    #[test]
    fn invalid_edit_leaves_cache_usable() {
        let mut project = modular_dock();
        let mut eval = Evaluator::new();
        eval.rebuild(&project, 1).unwrap();
        project.parameters["module_in"].value = 48.0;
        project.parameters["width_in"].value = 72.0;
        let err = eval.rebuild(&project, 2).unwrap_err().to_string();
        assert!(err.contains("whole multiples"), "{err}");
        let (_, report) = eval.rebuild(&modular_dock(), 3).unwrap();
        assert!(report.regenerated.is_empty(), "{:?}", report.regenerated);
    }

    #[test]
    fn placements_move_cleats_and_clamp_on_resize() {
        let mut project = modular_dock();
        project.placements.insert(
            "cleat.stbd.mid".into(),
            Placement {
                offset: [0.0, 30.0, 0.0],
            },
        );
        let doc = rebuild(&project, 1).unwrap();
        let cleat = doc
            .component(&crate::ComponentId::new("cleat.stbd.mid"))
            .unwrap();
        assert_eq!(cleat.bounds.origin[1], 96.0 - 5.0 + 30.0);
        assert!(!doc.diagnostics.iter().any(|d| d.code == "PLACE-01"));

        project.placements.insert(
            "cleat.stbd.mid".into(),
            Placement {
                offset: [0.0, 500.0, 0.0],
            },
        );
        let doc = rebuild(&project, 2).unwrap();
        let cleat = doc
            .component(&crate::ComponentId::new("cleat.stbd.mid"))
            .unwrap();
        assert!((cleat.bounds.max()[1] - 192.0).abs() < 1e-9);
        assert!(doc.diagnostics.iter().any(|d| d.code == "PLACE-01"));
    }

    #[test]
    fn hydrostatics_are_consistent() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let e = doc.engineering;
        // 32 floats of 23.5 x 23.5 x 16 in.
        let expected = 32.0 * 23.5 * 23.5 * 16.0 / 1728.0 * 62.4;
        assert!((e.buoyancy_capacity_lb - expected).abs() < 1e-6);
        assert!(
            e.dead_load_lb > 500.0 && e.dead_load_lb < 2500.0,
            "{}",
            e.dead_load_lb
        );
        assert!((e.live_load_lb - 30.0 * 128.0).abs() < 1e-9);
        assert!(e.reserve_buoyancy_ratio > 0.0);
        assert!(e.draft_loaded_in > e.draft_dead_in);
        assert!(
            (e.center_of_gravity[0] - 48.0).abs() < 0.5,
            "{:?}",
            e.center_of_gravity
        );
        assert!(doc.diagnostics.iter().any(|d| d.code == "BUOY-00"));
        assert!(!doc.diagnostics.iter().any(|d| d.code == "SLEND-01"));
    }

    #[test]
    fn tube_and_float_masses_are_exact() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let stringer = doc
            .component(&crate::ComponentId::new("frame.stringer.port"))
            .unwrap();
        // 2 x 4 x 1/8 tube, 192 in.
        let density = doc.project.materials["alum_6061"].density_lb_ft3;
        let area = 2.0 * 4.0 - 1.75 * 3.75;
        let expected = area * 192.0 / 1728.0 * density;
        assert!((stringer.mass_lb() - expected).abs() < 1e-9);
        // A hollow tube's inertia about its length axis beats a solid bar
        // of equal mass spread evenly.
        let solid_like = stringer.mass_lb() * (2.0f64.powi(2) + 4.0f64.powi(2)) / 12.0;
        assert!(stringer.mass.inertia[1][1] > solid_like);
        let float = doc
            .component(&crate::ComponentId::new("float.r0.c0"))
            .unwrap();
        // Shell plus foam sits low: centre of mass at mid-height by symmetry.
        assert!((float.mass.com[2] - 8.0).abs() < 1e-9);
    }

    #[test]
    fn stability_matches_the_barge_formula() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let e = doc.engineering;
        let rho = 62.4 / 1728.0;
        let volume = e.dead_load_lb / rho;
        // 4 x 8 floats of 23.5 in on a 24 in grid.
        let mut i_t = 0.0;
        for col in 0..4 {
            let x = f64::from(col) * 24.0 + 12.0 - 48.0;
            i_t += 8.0 * (23.5 * 23.5f64.powi(3) / 12.0 + 23.5 * 23.5 * x * x);
        }
        let kb = volume / (32.0 * 23.5 * 23.5) / 2.0;
        let gm = kb + i_t / volume - e.center_of_gravity[2];
        assert!(
            (e.gm_transverse_in - gm).abs() < 1e-6,
            "{} vs {gm}",
            e.gm_transverse_in
        );
        assert!(e.gm_transverse_in > 0.0 && e.gm_longitudinal_in > e.gm_transverse_in);
        // Symmetric layout: no static heel.
        assert!(e.heel_static_deg.abs() < 0.1, "{}", e.heel_static_deg);
        // A half-deck crowd heels toward port.
        assert!(e.heel_crowd_deg < 0.0);
        assert!(!doc.diagnostics.iter().any(|d| d.code == "STAB-01"));
    }

    #[test]
    fn narrow_tall_dock_fails_stability() {
        let mut project = modular_dock();
        project.set_parameter("width_in", 48.0).unwrap();
        project.set_parameter("live_load_psf", 60.0).unwrap();
        let doc = rebuild(&project, 1).unwrap();
        assert!(
            doc.diagnostics.iter().any(|d| d.code == "STAB-02"),
            "{:?}",
            doc.engineering
        );
    }

    fn part<'a>(doc: &'a Document, id: &str) -> &'a crate::Component {
        doc.component(&crate::ComponentId::new(id)).unwrap()
    }

    #[test]
    fn gangway_hinges_on_the_stern_deck_edge() {
        let mut project = modular_dock();
        project.set_parameter("gangway_angle_deg", 0.0).unwrap();
        let level = rebuild(&project, 1).unwrap();
        let deck_top = part(&level, "deck.board.0").bounds.max()[2];
        let s = part(&level, "gangway.stringer.port").bounds;
        assert!((s.max()[2] - deck_top).abs() < 1e-9, "{s:?}");
        assert!((s.origin[1] + 144.0).abs() < 1e-9);

        project.set_parameter("gangway_angle_deg", 10.0).unwrap();
        let mut eval = Evaluator::new();
        let (raised, _) = eval.rebuild(&project, 2).unwrap();
        let s = part(&raised, "gangway.stringer.port");
        // The shore end (y = -144 when level) rises by L sin(10°) about the
        // stern deck edge.
        // Tube section local (x, y) = (across, up); local z runs from the
        // dock end (z = 0) out to the shore end (z = length).
        let far_top = s.pose.apply([0.0, 6.0, 144.0]);
        let expected = deck_top + 144.0 * 10f64.to_radians().sin();
        assert!(
            (far_top[2] - expected).abs() < 1e-6,
            "{far_top:?} vs {expected}"
        );
        assert!((far_top[1] + 144.0 * 10f64.to_radians().cos()).abs() < 1e-6);
        let hinge = &raised.mates[0];
        assert_eq!(hinge.value, 10.0);
        assert!((hinge.origin[1]).abs() < 1e-9 && (hinge.origin[2] - deck_top).abs() < 1e-9);
        assert!(raised.diagnostics.iter().any(|d| d.code == "GANG-01"));

        // Slope only moves the gangway: nothing regenerates but the checks.
        project.set_parameter("gangway_angle_deg", 4.0).unwrap();
        let (_, report) = eval.rebuild(&project, 3).unwrap();
        assert_eq!(ids(&report.regenerated), vec!["feat.assembly"]);
        // Modifiers count as reused when nothing they read changed.
        project
            .add_feature(crate::Feature {
                id: crate::FeatureId::new("feat.modify.1"),
                name: "Hole".into(),
                parents: vec![crate::FeatureId::new("feat.deck")],
                kind: FeatureKind::Modify,
                inputs: vec![],
                material: None,
                suppressed: false,
                spec: Some(crate::feature::FeatureSpec::Modify(
                    crate::feature::ModifySpec {
                        target: "deck.board.3".into(),
                        op: crate::geometry::ShapeOp::Hole {
                            face: "z+".into(),
                            at: [10.0, 2.0, 1.0],
                            diameter: 0.5,
                            depth: 0.0,
                        },
                    },
                )),
            })
            .unwrap();
        eval.rebuild(&project, 4).unwrap();
        project.set_parameter("gangway_angle_deg", 6.0).unwrap();
        let (_, report) = eval.rebuild(&project, 5).unwrap();
        assert_eq!(ids(&report.regenerated), vec!["feat.assembly"]);
    }

    #[test]
    fn gangway_loads_the_stern_through_the_hinge() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let gangway: f64 = doc
            .components
            .iter()
            .filter(|c| c.id.as_str().starts_with("gangway."))
            .map(|c| c.mass_lb())
            .sum();
        let dock: f64 = doc
            .components
            .iter()
            .filter(|c| !c.id.as_str().starts_with("gangway."))
            .map(|c| c.mass_lb())
            .sum();
        assert!(gangway > 100.0 && gangway < 500.0, "{gangway}");
        assert!((doc.engineering.dead_load_lb - (dock + gangway * 0.5)).abs() < 1e-6);
        // The hinge load pulls the centre of gravity toward the stern.
        assert!(doc.engineering.center_of_gravity[1] < 96.0);
        assert!(doc.engineering.trim_static_deg < 0.0);
    }

    fn bracket(plane: crate::feature::SketchPlane) -> crate::Feature {
        use crate::feature::{ExtrudeDirection, ExtrudeSpec, FeatureSpec};
        use crate::geometry::{PathVertex, Profile};
        crate::Feature {
            id: crate::FeatureId::new("feat.extrude.1"),
            name: "Ladder bracket".into(),
            parents: vec![crate::FeatureId::new("feat.deck")],
            kind: FeatureKind::Extrude,
            inputs: vec![],
            material: Some("alum_6061".into()),
            suppressed: false,
            spec: Some(FeatureSpec::Extrude(ExtrudeSpec {
                plane,
                profile: Profile {
                    outer: [[0., 0.], [6., 0.], [6., 2.], [2., 2.], [2., 6.], [0., 6.]]
                        .iter()
                        .map(|p| PathVertex::line(p[0], p[1]))
                        .collect(),
                    holes: vec![],
                },
                depth: 0.5,
                depth_parameter: None,
                direction: ExtrudeDirection::Forward,
                cut_into: None,
            })),
        }
    }

    #[test]
    fn sketched_part_on_a_face_has_exact_mass() {
        let mut project = modular_dock();
        project
            .add_feature(bracket(crate::feature::SketchPlane::Face {
                component: "deck.board.5".into(),
                face: "z+".into(),
            }))
            .unwrap();
        let doc = rebuild(&project, 1).unwrap();
        let c = part(&doc, "custom.extrude.1");
        let density = doc.project.materials["alum_6061"].density_lb_ft3 / 1728.0;
        assert!((c.mass_lb() - 20.0 * 0.5 * density).abs() < 1e-9);
        // Sits on top of the board it was sketched on.
        let board = part(&doc, "deck.board.5");
        assert!((c.bounds.origin[2] - board.bounds.max()[2]).abs() < 1e-9);
        assert!(doc
            .bom
            .iter()
            .any(|l| l.item == "Ladder bracket" && l.size.starts_with("0.5 in plate")));
        assert!(
            doc.engineering.dead_load_lb
                > rebuild(&modular_dock(), 1)
                    .unwrap()
                    .engineering
                    .dead_load_lb
        );
        // The same sketch on the front plane stands upright.
        let mut project = modular_dock();
        project
            .add_feature(bracket(crate::feature::SketchPlane::Xz { offset: -10.0 }))
            .unwrap();
        let doc = rebuild(&project, 1).unwrap();
        let c = part(&doc, "custom.extrude.1");
        assert!((c.bounds.size[2] - 6.0).abs() < 1e-9 && (c.bounds.size[1] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn bad_sketches_are_rejected_with_a_reason() {
        let mut project = modular_dock();
        let mut f = bracket(crate::feature::SketchPlane::Xy { offset: 0.0 });
        if let Some(crate::feature::FeatureSpec::Extrude(spec)) = &mut f.spec {
            spec.profile.outer.swap(1, 2);
        }
        let err = project.add_feature(f).unwrap_err().to_string();
        assert!(err.contains("crosses itself"), "{err}");
    }

    #[test]
    fn modifiers_without_opencascade_fail_visibly() {
        use crate::feature::{FeatureSpec, ModifySpec};
        use crate::geometry::ShapeOp;
        let mut project = modular_dock();
        project
            .add_feature(crate::Feature {
                id: crate::FeatureId::new("feat.modify.1"),
                name: "Ease deck edge".into(),
                parents: vec![crate::FeatureId::new("feat.deck")],
                kind: FeatureKind::Modify,
                inputs: vec![],
                material: None,
                suppressed: false,
                spec: Some(FeatureSpec::Modify(ModifySpec {
                    target: "deck.board.0".into(),
                    op: ShapeOp::Fillet {
                        edges: vec!["y-|z+".into()],
                        radius: 0.25,
                    },
                })),
            })
            .unwrap();
        let doc = rebuild(&project, 1).unwrap();
        let d = doc
            .diagnostics
            .iter()
            .find(|d| d.code == "OCCT-01")
            .unwrap();
        assert_eq!(d.severity, Severity::Error);
        assert!(
            d.message.contains("fillet needs OpenCascade"),
            "{}",
            d.message
        );
        assert!(matches!(
            part(&doc, "deck.board.0").shape,
            crate::Shape::Box { .. }
        ));
    }

    #[test]
    fn schema_one_files_migrate() {
        let mut json: serde_json::Value =
            serde_json::from_str(&modular_dock().to_json().unwrap()).unwrap();
        json["schema"] = 1.into();
        json.as_object_mut().unwrap().remove("mates");
        let project = crate::Project::from_json(&json.to_string()).unwrap();
        assert_eq!(project.schema, crate::project::SCHEMA_VERSION);
        assert!(project.mates.is_empty());
    }

    #[test]
    fn overload_is_an_error() {
        let mut project = modular_dock();
        project.set_parameter("float_height_in", 12.0).unwrap();
        project.set_parameter("live_load_psf", 100.0).unwrap();
        let doc = rebuild(&project, 1).unwrap();
        assert!(doc
            .diagnostics
            .iter()
            .any(|d| d.code == "BUOY-01" || d.code == "BUOY-03"));
    }

    #[test]
    fn feature_graph_is_dependency_ordered() {
        let project = modular_dock();
        let order: Vec<_> = project
            .ordered_features()
            .unwrap()
            .into_iter()
            .map(|f| f.id.0.as_str())
            .collect();
        let pos = |id| order.iter().position(|x| *x == id).unwrap();
        assert!(pos("feat.plan") < pos("feat.floats"));
        assert!(pos("feat.frame") < pos("feat.deck"));
        assert!(pos("feat.deck") < pos("feat.cleats"));
        assert_eq!(*order.last().unwrap(), "feat.assembly");
    }

    #[test]
    fn generators_cannot_read_undeclared_parameters() {
        let mut project = modular_dock();
        let frame = project
            .features
            .iter_mut()
            .find(|f| f.id.0 == "feat.frame")
            .unwrap();
        frame.inputs.retain(|i| i != "beam_height_in");
        let err = rebuild(&project, 1).unwrap_err().to_string();
        assert!(err.contains("undeclared parameter beam_height_in"), "{err}");
    }
}
