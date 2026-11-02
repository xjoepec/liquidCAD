//! Stock optimisation and manufacturing validation.
//!
//! Cut parts are grouped by material and profile and packed into stock
//! lengths first-fit decreasing with a saw kerf. Parts longer than stock are
//! split into full bars plus a remainder and counted as splices.

use liquidcad_core::diagnostics::{Diagnostic, Severity};
use liquidcad_core::document::Document;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const KERF_IN: f64 = 0.125;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CutPlan {
    pub groups: Vec<StockGroup>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockGroup {
    pub material: String,
    pub profile: String,
    pub stock_length_in: Option<f64>,
    pub cut_count: usize,
    pub bars: Vec<Bar>,
    pub splices: usize,
    pub spliced_parts: Vec<String>,
    pub total_cut_in: f64,
    pub waste_ratio: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bar {
    pub cuts: Vec<f64>,
    pub remaining_in: f64,
}

/// (material, profile) -> (stock length, [(cut length, component id)]).
type CutGroups = BTreeMap<(String, String), (Option<f64>, Vec<(f64, String)>)>;

pub fn plan(document: &Document) -> CutPlan {
    let mut by_group: CutGroups = BTreeMap::new();
    for c in &document.components {
        let Some(stock) = &c.stock else { continue };
        let material = document
            .project
            .materials
            .get(&c.material)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| c.material.clone());
        by_group
            .entry((material, stock.profile.clone()))
            .or_insert((stock.stock_length_in, Vec::new()))
            .1
            .push((stock.cut_length_in, c.id.0.clone()));
    }
    let groups = by_group
        .into_iter()
        .map(|((material, profile), (stock_length, mut cuts))| {
            cuts.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let total_cut_in: f64 = cuts.iter().map(|c| c.0).sum();
            let mut bars: Vec<Bar> = Vec::new();
            let mut splices = 0;
            let mut spliced_parts = Vec::new();
            if let Some(stock) = stock_length {
                for (cut, id) in &cuts {
                    let mut remainder = *cut;
                    if remainder > stock + 1e-9 {
                        let full = (remainder / stock).floor() as usize;
                        let leftover = remainder - full as f64 * stock;
                        let pieces = full + usize::from(leftover > 1e-9);
                        splices += pieces - 1;
                        spliced_parts.push(id.clone());
                        for _ in 0..full {
                            bars.push(Bar {
                                cuts: vec![stock],
                                remaining_in: 0.0,
                            });
                        }
                        remainder = leftover;
                        if remainder <= 1e-9 {
                            continue;
                        }
                    }
                    let need = remainder + KERF_IN;
                    match bars.iter_mut().find(|b| b.remaining_in + 1e-9 >= need) {
                        Some(bar) => {
                            bar.cuts.push(remainder);
                            bar.remaining_in -= need;
                        }
                        None => bars.push(Bar {
                            cuts: vec![remainder],
                            remaining_in: (stock - need).max(0.0),
                        }),
                    }
                }
            }
            let bought: f64 = stock_length
                .map(|s| s * bars.len() as f64)
                .unwrap_or(total_cut_in);
            StockGroup {
                material,
                profile,
                stock_length_in: stock_length,
                cut_count: cuts.len(),
                bars,
                splices,
                spliced_parts,
                total_cut_in,
                waste_ratio: if bought > 0.0 {
                    1.0 - total_cut_in / bought
                } else {
                    0.0
                },
            }
        })
        .collect();
    CutPlan { groups }
}

pub fn validate(document: &Document, plan: &CutPlan) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for group in &plan.groups {
        if group.splices > 0 {
            out.push(Diagnostic::new(
                Severity::Warning,
                "manufacturing",
                "MFG-01",
                format!(
                    "{} {}: {} part(s) exceed {} in stock and need {} splice(s); land splices over a joist or stringer",
                    group.material,
                    group.profile,
                    group.spliced_parts.len(),
                    group.stock_length_in.unwrap_or(0.0),
                    group.splices
                ),
                group.spliced_parts.clone(),
            ));
        }
        if let Some(stock) = group.stock_length_in {
            out.push(Diagnostic::new(
                Severity::Info,
                "manufacturing",
                "MFG-00",
                format!(
                    "{} {}: {} cuts from {} x {} in stock, {:.0}% offcut",
                    group.material,
                    group.profile,
                    group.cut_count,
                    group.bars.len(),
                    stock,
                    group.waste_ratio * 100.0
                ),
                vec![],
            ));
        }
    }
    if document.components.is_empty() {
        out.push(Diagnostic::new(
            Severity::Warning,
            "manufacturing",
            "MFG-02",
            "nothing to fabricate: every solid-producing feature is suppressed",
            vec![],
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquidcad_core::{rebuild, templates::modular_dock};

    #[test]
    fn default_dock_frame_fits_stock_without_splices() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let plan = plan(&doc);
        let frame = plan
            .groups
            .iter()
            .find(|g| g.profile.contains("rect tube"))
            .unwrap();
        assert_eq!(frame.splices, 0);
        // 5 stringers @192 + 5 joists @96 = 1440 in of tube from 288 in bars.
        assert_eq!(frame.cut_count, 10);
        assert!((frame.total_cut_in - 1440.0).abs() < 1e-9);
        // 192 + 96 + 2 kerfs overruns a 288 in bar, so FFD lands on 5 bars of
        // one stringer each plus 3 bars of joists (2 + 2 + 1).
        assert_eq!(frame.bars.len(), 8);
        for bar in &frame.bars {
            let used: f64 = bar.cuts.iter().sum::<f64>() + KERF_IN * bar.cuts.len() as f64;
            assert!(used <= 288.0 + KERF_IN + 1e-9);
        }
    }

    #[test]
    fn long_docks_report_splices() {
        let mut project = modular_dock();
        project.set_parameter("length_in", 480.0).unwrap();
        let doc = rebuild(&project, 1).unwrap();
        let plan = plan(&doc);
        let frame = plan
            .groups
            .iter()
            .find(|g| g.profile.contains("rect tube"))
            .unwrap();
        assert_eq!(frame.splices, 5, "one splice per 480 in stringer");
        let diags = validate(&doc, &plan);
        assert!(diags.iter().any(|d| d.code == "MFG-01"));
    }
}
