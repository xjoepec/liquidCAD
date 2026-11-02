//! Drawing sheet model: ANSI B landscape, PDF points, origin bottom-left.
//!
//! The same [`Sheet`] renders to PDF here and to SVG in the desktop drawing
//! workspace, so what you see on screen is what gets exported.

use liquidcad_core::diagnostics::Severity;
use liquidcad_core::document::{ComponentKind, Document};
use serde::{Deserialize, Serialize};

pub const SHEET_W: f64 = 1224.0;
pub const SHEET_H: f64 = 792.0;
const INK: [f32; 3] = [0.08, 0.10, 0.12];
const FAINT: [f32; 3] = [0.55, 0.58, 0.62];
const ACCENT: [f32; 3] = [0.75, 0.42, 0.10];
const WATER: [f32; 3] = [0.15, 0.45, 0.75];
const FLOAT_FILL: [f32; 3] = [0.88, 0.91, 0.94];
const DECK_FILL: [f32; 3] = [0.93, 0.89, 0.83];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Prim {
    Line {
        a: [f64; 2],
        b: [f64; 2],
        weight: f64,
        color: [f32; 3],
        dash: bool,
    },
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        weight: f64,
        stroke: Option<[f32; 3]>,
        fill: Option<[f32; 3]>,
    },
    Text {
        at: [f64; 2],
        size: f64,
        text: String,
        color: [f32; 3],
        anchor: Anchor,
        bold: bool,
        rotate: f64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sheet {
    pub width: f64,
    pub height: f64,
    pub title: String,
    pub scale_label: String,
    pub items: Vec<Prim>,
}

impl Sheet {
    fn line(&mut self, a: [f64; 2], b: [f64; 2], weight: f64, color: [f32; 3], dash: bool) {
        self.items.push(Prim::Line {
            a,
            b,
            weight,
            color,
            dash,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn rect(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        weight: f64,
        stroke: Option<[f32; 3]>,
        fill: Option<[f32; 3]>,
    ) {
        self.items.push(Prim::Rect {
            x,
            y,
            w,
            h,
            weight,
            stroke,
            fill,
        });
    }

    fn text(
        &mut self,
        at: [f64; 2],
        size: f64,
        text: impl Into<String>,
        anchor: Anchor,
        bold: bool,
    ) {
        self.items.push(Prim::Text {
            at,
            size,
            text: text.into(),
            color: INK,
            anchor,
            bold,
            rotate: 0.0,
        });
    }

    fn text_rot(&mut self, at: [f64; 2], size: f64, text: impl Into<String>, rotate: f64) {
        self.items.push(Prim::Text {
            at,
            size,
            text: text.into(),
            color: INK,
            anchor: Anchor::Middle,
            bold: false,
            rotate,
        });
    }

    /// Horizontal or vertical dimension between two sheet points.
    fn dim(&mut self, a: [f64; 2], b: [f64; 2], offset: f64, label: String) {
        let vertical = (a[0] - b[0]).abs() < 1e-6;
        let (a2, b2) = if vertical {
            ([a[0] + offset, a[1]], [b[0] + offset, b[1]])
        } else {
            ([a[0], a[1] + offset], [b[0], b[1] + offset])
        };
        let ext = if offset >= 0.0 { 3.0 } else { -3.0 };
        let (ea, eb) = if vertical {
            ([a2[0] + ext, a2[1]], [b2[0] + ext, b2[1]])
        } else {
            ([a2[0], a2[1] + ext], [b2[0], b2[1] + ext])
        };
        self.line(a, ea, 0.35, FAINT, false);
        self.line(b, eb, 0.35, FAINT, false);
        self.line(a2, b2, 0.5, INK, false);
        for p in [a2, b2] {
            self.line(
                [p[0] - 2.5, p[1] - 2.5],
                [p[0] + 2.5, p[1] + 2.5],
                0.7,
                INK,
                false,
            );
        }
        let mid = [(a2[0] + b2[0]) * 0.5, (a2[1] + b2[1]) * 0.5];
        if vertical {
            let side = if offset >= 0.0 { 9.0 } else { -4.0 };
            self.text_rot([mid[0] + side, mid[1]], 7.5, label, 90.0);
        } else {
            let side = if offset >= 0.0 { 3.0 } else { -9.0 };
            self.text([mid[0], mid[1] + side], 7.5, label, Anchor::Middle, false);
        }
    }
}

/// Standard architectural/engineering ratios, model inches per sheet inch.
const SCALES: [f64; 10] = [4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0];

fn pick_scale(fits: impl Fn(f64) -> bool) -> f64 {
    SCALES
        .iter()
        .copied()
        .find(|s| fits(72.0 / s))
        .unwrap_or(192.0)
}

pub fn fmt_in(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} in")
}

fn ft_in(v: f64) -> String {
    let feet = (v / 12.0).floor();
    let inches = v - feet * 12.0;
    if inches.abs() < 1e-6 {
        format!("{feet:.0}'-0\"")
    } else {
        let s = format!("{inches:.2}");
        format!(
            "{feet:.0}'-{}\"",
            s.trim_end_matches('0').trim_end_matches('.')
        )
    }
}

pub fn build(document: &Document, date: &str) -> Sheet {
    let p = &document.project;
    let value = |name: &str| p.parameters.get(name).map(|v| v.value).unwrap_or(0.0);
    let (width, length) = (value("width_in"), value("length_in"));
    let (bmin, bmax) = document.bounds();
    let height = (bmax[2] - bmin[2]).max(1.0);
    let span_len = (bmax[1] - bmin[1]).max(length);

    let mut s = Sheet {
        width: SHEET_W,
        height: SHEET_H,
        title: format!("{} - fabrication drawing", p.name),
        scale_label: String::new(),
        items: Vec::new(),
    };

    // Frame and zones.
    s.rect(
        18.0,
        18.0,
        SHEET_W - 36.0,
        SHEET_H - 36.0,
        1.4,
        Some(INK),
        None,
    );
    s.rect(
        24.0,
        24.0,
        SHEET_W - 48.0,
        SHEET_H - 48.0,
        0.5,
        Some(INK),
        None,
    );

    // Views share one scale; elevation sits under the plan.
    let view_x = 70.0;
    let view_w = 700.0;
    let plan_top = SHEET_H - 56.0;
    let plan_h_avail = 360.0;
    let elev_h_avail = 150.0;
    let scale = pick_scale(|k| {
        span_len * k <= view_w && width * k <= plan_h_avail && height * k <= elev_h_avail
    });
    let k = 72.0 / scale;
    s.scale_label = format!("1:{scale:.0}");

    // Plan view: model y -> sheet x, model x -> sheet y (bow to the right).
    let plan_w = length * k;
    let plan_h = width * k;
    let px0 = view_x + (view_w - plan_w) * 0.5;
    let py0 = plan_top - plan_h - 48.0;
    let to_plan = |x: f64, y: f64| [px0 + y * k, py0 + x * k];
    s.text(
        [view_x, plan_top],
        10.0,
        "FRAMING PLAN - DECK REMOVED",
        Anchor::Start,
        true,
    );
    s.text(
        [view_x, plan_top - 12.0],
        7.5,
        format!("SCALE {}   BOW TO THE RIGHT", s.scale_label),
        Anchor::Start,
        false,
    );

    let mut layered: Vec<_> = document
        .components
        .iter()
        .filter(|c| c.kind != ComponentKind::DeckBoard)
        .collect();
    layered.sort_by_key(|c| match c.kind {
        ComponentKind::FloatModule => 0,
        ComponentKind::Joist => 1,
        ComponentKind::Stringer => 2,
        ComponentKind::Bumper => 3,
        _ => 4,
    });
    for c in layered {
        let o = to_plan(c.bounds.origin[0], c.bounds.origin[1]);
        let (w, h) = (c.bounds.size[1] * k, c.bounds.size[0] * k);
        match c.kind {
            ComponentKind::FloatModule => {
                s.rect(o[0], o[1], w, h, 0.3, Some(FAINT), Some(FLOAT_FILL))
            }
            ComponentKind::Joist | ComponentKind::Stringer => {
                s.rect(o[0], o[1], w, h, 0.5, Some(INK), Some([1.0, 1.0, 1.0]))
            }
            ComponentKind::Cleat => s.rect(o[0], o[1], w, h, 0.6, Some(ACCENT), Some(ACCENT)),
            _ => s.rect(o[0], o[1], w, h, 0.4, Some(INK), None),
        }
    }
    let a = to_plan(0.0, 0.0);
    let b = to_plan(0.0, length);
    s.dim(
        a,
        b,
        -16.0,
        format!("{}  ({})", fmt_in(length), ft_in(length)),
    );
    let c = to_plan(width, length);
    s.dim(b, c, 16.0, format!("{}  ({})", fmt_in(width), ft_in(width)));
    let module = value("module_in");
    if module > 0.0 {
        let m0 = to_plan(width, 0.0);
        let m1 = to_plan(width, module);
        s.dim(m0, m1, 14.0, fmt_in(module));
    }

    // Port elevation: model y -> sheet x, z -> sheet y.
    let ex0 = px0;
    let ey0 = 96.0;
    let to_elev = |y: f64, z: f64| [ex0 + y * k, ey0 + z * k];
    s.text(
        [view_x, ey0 + height * k + 24.0],
        10.0,
        "PORT ELEVATION",
        Anchor::Start,
        true,
    );
    let mut side: Vec<_> = document
        .components
        .iter()
        .filter(|c| {
            c.bounds.origin[0] < module.max(1.0)
                || c.kind == ComponentKind::Joist
                || c.kind == ComponentKind::DeckBoard
                || c.kind == ComponentKind::Bumper
        })
        .collect();
    side.sort_by_key(|c| match c.kind {
        ComponentKind::DeckBoard => 0,
        ComponentKind::Joist => 1,
        _ => 2,
    });
    for c in side {
        let o = to_elev(c.bounds.origin[1], c.bounds.origin[2]);
        let (w, h) = (c.bounds.size[1] * k, c.bounds.size[2] * k);
        match c.kind {
            ComponentKind::FloatModule => {
                s.rect(o[0], o[1], w, h, 0.4, Some(INK), Some(FLOAT_FILL))
            }
            ComponentKind::DeckBoard => {
                s.rect(o[0], o[1], w, h, 0.25, Some(FAINT), Some(DECK_FILL))
            }
            ComponentKind::Cleat => s.rect(o[0], o[1], w, h, 0.6, Some(ACCENT), Some(ACCENT)),
            _ => s.rect(o[0], o[1], w, h, 0.5, Some(INK), Some([1.0, 1.0, 1.0])),
        }
    }
    // Sloped gangway: the exact side silhouette of its port stringer and
    // hinge leaf (both convex, so the hull of the posed vertices is exact).
    let port_x = document
        .components
        .iter()
        .filter(|c| c.kind == ComponentKind::GangwayStringer)
        .map(|c| c.bounds.origin[0])
        .fold(f64::INFINITY, f64::min);
    for c in document.components.iter().filter(|c| {
        c.kind == ComponentKind::GangwayHinge
            || (c.kind == ComponentKind::GangwayStringer && c.bounds.origin[0] <= port_x + 1e-9)
    }) {
        let Some(poly) = c.shape.polyhedron() else {
            continue;
        };
        let pts: Vec<[f64; 2]> = poly
            .vertices
            .iter()
            .map(|v| {
                let w = c.pose.apply(*v);
                to_elev(w[1], w[2])
            })
            .collect();
        let hull = convex_hull(&pts);
        for i in 0..hull.len() {
            s.line(hull[i], hull[(i + 1) % hull.len()], 0.5, INK, false);
        }
    }
    let e = &document.engineering;
    if e.draft_dead_in.is_finite() && e.float_count > 0 {
        let wl_a = to_elev(bmin[1] - 6.0, e.draft_dead_in);
        let wl_b = to_elev(bmax[1] + 6.0, e.draft_dead_in);
        s.line(wl_a, wl_b, 0.6, WATER, true);
        s.items.push(Prim::Text {
            at: [wl_b[0] + 4.0, wl_b[1] - 2.5],
            size: 7.0,
            text: format!("DWL {}", fmt_in((e.draft_dead_in * 10.0).round() / 10.0)),
            color: WATER,
            anchor: Anchor::Start,
            bold: false,
            rotate: 0.0,
        });
    }
    let top = bmax[2];
    s.dim(
        to_elev(0.0, 0.0),
        to_elev(0.0, top),
        -16.0,
        fmt_in((top * 100.0).round() / 100.0),
    );
    let fh = value("float_height_in");
    s.dim(to_elev(length, 0.0), to_elev(length, fh), 22.0, fmt_in(fh));

    // Right column: BOM table.
    let tx = 800.0;
    let tw = SHEET_W - 24.0 - tx - 14.0;
    let mut ty = SHEET_H - 56.0;
    s.text([tx, ty], 10.0, "BILL OF MATERIALS", Anchor::Start, true);
    ty -= 8.0;
    let cols = [tx, tx + 22.0, tx + 140.0, tx + 268.0, tx + 312.0];
    let header = |s: &mut Sheet, y: f64| {
        s.rect(tx, y - 4.0, tw, 13.0, 0.0, None, Some([0.90, 0.91, 0.92]));
        for (x, label) in cols.iter().zip(["#", "ITEM", "SIZE", "QTY", "WT LB"]) {
            s.text([x + 2.0, y], 7.0, label, Anchor::Start, true);
        }
    };
    ty -= 12.0;
    header(&mut s, ty);
    for (i, line) in document.bom.iter().enumerate() {
        ty -= 12.0;
        if ty < 330.0 {
            s.text(
                [tx + 2.0, ty],
                7.0,
                format!("... {} more lines in CSV", document.bom.len() - i),
                Anchor::Start,
                false,
            );
            break;
        }
        let item = match line.cut_length_in {
            Some(cut) => format!("{} @ {}", line.item, fmt_in(cut)),
            None => line.item.clone(),
        };
        s.text(
            [cols[0] + 2.0, ty],
            7.0,
            format!("{}", i + 1),
            Anchor::Start,
            false,
        );
        s.text(
            [cols[1] + 2.0, ty],
            7.0,
            truncate(&item, 26),
            Anchor::Start,
            false,
        );
        s.text(
            [cols[2] + 2.0, ty],
            7.0,
            truncate(&line.size, 26),
            Anchor::Start,
            false,
        );
        s.text(
            [cols[3] + 2.0, ty],
            7.0,
            line.qty.to_string(),
            Anchor::Start,
            false,
        );
        s.text(
            [cols[4] + 2.0, ty],
            7.0,
            format!("{:.1}", line.total_weight_lb),
            Anchor::Start,
            false,
        );
        s.line([tx, ty - 4.0], [tx + tw, ty - 4.0], 0.25, FAINT, false);
    }

    // Engineering summary and notes.
    let mut ny = 318.0;
    s.text([tx, ny], 10.0, "HYDROSTATICS", Anchor::Start, true);
    let rows = [
        format!(
            "Floats: {}   displacement {:.0} lb",
            e.float_count, e.buoyancy_capacity_lb
        ),
        format!(
            "Dead load {:.0} lb   live {:.0} lb ({:.0} psf)",
            e.dead_load_lb,
            e.live_load_lb,
            value("live_load_psf")
        ),
        format!(
            "Draft {:.1} in at rest, {:.1} in loaded",
            e.draft_dead_in, e.draft_loaded_in
        ),
        format!(
            "Freeboard {:.1} in at rest, {:.1} in loaded",
            e.freeboard_dead_in, e.freeboard_loaded_in
        ),
        format!("Reserve buoyancy {:.0}%", e.reserve_buoyancy_ratio * 100.0),
    ];
    for row in rows {
        ny -= 11.0;
        s.text([tx, ny], 7.5, row, Anchor::Start, false);
    }
    ny -= 18.0;
    s.text([tx, ny], 10.0, "NOTES", Anchor::Start, true);
    let mut notes = vec![
        "1. Dimensions in inches. Do not scale drawing.".to_string(),
        "2. Frame: weld or through-bolt stringers to joists at every crossing.".into(),
        "3. Fasten deck to every stringer; 1/4 in board gap.".into(),
    ];
    for d in document
        .diagnostics
        .iter()
        .filter(|d| d.severity >= Severity::Warning)
        .take(4)
    {
        notes.push(format!("{}. {} {}", notes.len() + 1, d.code, d.message));
    }
    for note in notes {
        for chunk in wrap(&note, 78) {
            ny -= 10.0;
            if ny < 150.0 {
                break;
            }
            s.text([tx, ny], 7.0, chunk, Anchor::Start, false);
        }
    }

    // Title block.
    let bx = tx;
    let by = 30.0;
    let bh = 110.0;
    s.rect(bx, by, tw, bh, 0.8, Some(INK), None);
    s.line([bx, by + 70.0], [bx + tw, by + 70.0], 0.5, INK, false);
    s.line([bx, by + 36.0], [bx + tw, by + 36.0], 0.5, INK, false);
    s.line(
        [bx + tw * 0.5, by],
        [bx + tw * 0.5, by + 70.0],
        0.5,
        INK,
        false,
    );
    s.text(
        [bx + 6.0, by + 94.0],
        12.0,
        truncate(&p.name.to_uppercase(), 40),
        Anchor::Start,
        true,
    );
    s.text(
        [bx + 6.0, by + 78.0],
        7.5,
        format!("{} x {}   {}", ft_in(width), ft_in(length), p.id),
        Anchor::Start,
        false,
    );
    let cell = |s: &mut Sheet, x: f64, y: f64, label: &str, value: String| {
        s.text([x + 6.0, y + 24.0], 6.0, label, Anchor::Start, false);
        s.text([x + 6.0, y + 9.0], 9.0, value, Anchor::Start, true);
    };
    cell(&mut s, bx, by + 36.0, "DRAWING", "LC-DOCK-001".into());
    cell(
        &mut s,
        bx + tw * 0.5,
        by + 36.0,
        "REVISION",
        format!("{}", document.revision),
    );
    let scale_cell = format!("{}  IN", s.scale_label);
    cell(&mut s, bx, by, "SCALE / UNITS", scale_cell);
    cell(
        &mut s,
        bx + tw * 0.5,
        by,
        "DATE / SHEET",
        format!("{date}  1 OF 1"),
    );
    s.text([24.0 + 6.0, 28.0], 6.0, "liquidCAD", Anchor::Start, false);
    s
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n - 3).collect::<String>())
    }
}

fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in s.split_whitespace() {
        if !current.is_empty() && current.len() + word.len() + 1 > width {
            lines.push(std::mem::take(&mut current));
            current.push_str("   ");
        }
        if !current.is_empty() && !current.ends_with(' ') {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.trim().is_empty() {
        lines.push(current);
    }
    lines
}

/// Convex hull (monotone chain), counter-clockwise.
fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p: Vec<[f64; 2]> = points.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    if p.len() < 3 {
        return p;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<[f64; 2]> = Vec::with_capacity(p.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 {
            Box::new(p.iter())
        } else {
            Box::new(p.iter().rev())
        };
        for &q in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0.0
            {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    hull
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquidcad_core::{rebuild, templates::modular_dock};

    fn inside(s: &Sheet) -> bool {
        s.items.iter().all(|p| match p {
            Prim::Line { a, b, .. } => [a, b]
                .iter()
                .all(|p| p[0] >= 0.0 && p[0] <= SHEET_W && p[1] >= 0.0 && p[1] <= SHEET_H),
            Prim::Rect { x, y, w, h, .. } => {
                *x >= 0.0 && *y >= 0.0 && x + w <= SHEET_W && y + h <= SHEET_H
            }
            Prim::Text { at, .. } => {
                at[0] >= 0.0 && at[0] <= SHEET_W && at[1] >= 0.0 && at[1] <= SHEET_H
            }
        })
    }

    #[test]
    fn default_sheet_fits_and_picks_a_standard_scale() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let sheet = build(&doc, "2026-11-01");
        assert!(inside(&sheet));
        assert!(sheet.scale_label.starts_with("1:"));
        assert!(sheet.items.len() > 100);
    }

    #[test]
    fn largest_dock_still_fits() {
        let mut project = modular_dock();
        project.set_parameter("width_in", 240.0).unwrap();
        project.set_parameter("length_in", 480.0).unwrap();
        let sheet = build(&rebuild(&project, 1).unwrap(), "2026-11-01");
        assert!(inside(&sheet));
    }

    #[test]
    fn feet_inches_formatting() {
        assert_eq!(ft_in(96.0), "8'-0\"");
        assert_eq!(ft_in(100.5), "8'-4.5\"");
    }
}
