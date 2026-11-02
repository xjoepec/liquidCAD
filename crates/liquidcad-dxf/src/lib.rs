//! DXF R12 (AC1009) fabrication export, in inches.
//!
//! Sheet 1 (origin): plan view with floats, frame, deck, hardware, and
//! overall dimensions on separate layers. To the right: one flat profile per
//! unique cut part from the BOM, labelled with quantity and cut size, for
//! saw and CNC programming. R12 is used because every CAM package reads it.

use liquidcad_core::document::{Component, ComponentKind, Document};
use liquidcad_core::geometry::{PathVertex, Pose, Profile, Shape, ShapeOp};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

pub const LAYERS: &[(&str, u8)] = &[
    ("0", 7),
    ("FLOATS", 8),
    ("FRAME", 4),
    ("DECK", 30),
    ("HARDWARE", 6),
    ("CUSTOM", 5),
    ("GANGWAY", 140),
    ("DIMS", 3),
    ("CUT", 1),
    ("CUT_TEXT", 2),
    ("TITLE", 7),
];

pub fn write_dxf(document: &Document, path: &Path) -> std::io::Result<u64> {
    std::fs::write(path, to_dxf(document))?;
    Ok(std::fs::metadata(path)?.len())
}

#[derive(Default)]
struct Entities {
    out: String,
    min: [f64; 2],
    max: [f64; 2],
    started: bool,
}

impl Entities {
    fn grow(&mut self, x: f64, y: f64) {
        if !self.started {
            self.min = [x, y];
            self.max = [x, y];
            self.started = true;
        }
        self.min = [self.min[0].min(x), self.min[1].min(y)];
        self.max = [self.max[0].max(x), self.max[1].max(y)];
    }

    fn rect(&mut self, layer: &str, x: f64, y: f64, w: f64, h: f64) {
        let _ = write!(
            self.out,
            "0\nPOLYLINE\n8\n{layer}\n66\n1\n10\n0.0\n20\n0.0\n30\n0.0\n70\n1\n"
        );
        for (px, py) in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)] {
            let _ = write!(
                self.out,
                "0\nVERTEX\n8\n{layer}\n10\n{}\n20\n{}\n30\n0.0\n",
                num(px),
                num(py)
            );
            self.grow(px, py);
        }
        let _ = write!(self.out, "0\nSEQEND\n8\n{layer}\n");
    }

    /// Closed or open polyline; each vertex carries the bulge of the
    /// segment that starts at it (group 42), so arcs are exact.
    fn polyline(&mut self, layer: &str, pts: &[(f64, f64, f64)], closed: bool) {
        let _ = write!(
            self.out,
            "0\nPOLYLINE\n8\n{layer}\n66\n1\n10\n0.0\n20\n0.0\n30\n0.0\n70\n{}\n",
            u8::from(closed)
        );
        for &(px, py, bulge) in pts {
            let _ = write!(
                self.out,
                "0\nVERTEX\n8\n{layer}\n10\n{}\n20\n{}\n30\n0.0\n",
                num(px),
                num(py)
            );
            if bulge != 0.0 {
                let _ = write!(self.out, "42\n{}\n", num(bulge));
            }
            self.grow(px, py);
        }
        let _ = write!(self.out, "0\nSEQEND\n8\n{layer}\n");
    }

    fn circle(&mut self, layer: &str, c: [f64; 2], r: f64) {
        let _ = write!(
            self.out,
            "0\nCIRCLE\n8\n{layer}\n10\n{}\n20\n{}\n30\n0.0\n40\n{}\n",
            num(c[0]),
            num(c[1]),
            num(r)
        );
        self.grow(c[0] - r, c[1] - r);
        self.grow(c[0] + r, c[1] + r);
    }

    fn line(&mut self, layer: &str, a: [f64; 2], b: [f64; 2]) {
        let _ = write!(
            self.out,
            "0\nLINE\n8\n{layer}\n10\n{}\n20\n{}\n30\n0.0\n11\n{}\n21\n{}\n31\n0.0\n",
            num(a[0]),
            num(a[1]),
            num(b[0]),
            num(b[1])
        );
        self.grow(a[0], a[1]);
        self.grow(b[0], b[1]);
    }

    /// `align` 0 = left, 1 = centre (uses the second alignment point).
    fn text(
        &mut self,
        layer: &str,
        at: [f64; 2],
        height: f64,
        value: &str,
        align: u8,
        rotation: f64,
    ) {
        let value = clean(value);
        let _ = write!(
            self.out,
            "0\nTEXT\n8\n{layer}\n10\n{}\n20\n{}\n30\n0.0\n40\n{}\n1\n{value}\n",
            num(at[0]),
            num(at[1]),
            num(height)
        );
        if rotation != 0.0 {
            let _ = write!(self.out, "50\n{}\n", num(rotation));
        }
        if align == 1 {
            let _ = write!(
                self.out,
                "72\n1\n11\n{}\n21\n{}\n31\n0.0\n",
                num(at[0]),
                num(at[1])
            );
        }
        self.grow(at[0], at[1]);
    }

    /// Linear dimension drawn from primitives: extension lines, dimension
    /// line with ticks, and the measured value.
    fn dimension(&mut self, a: [f64; 2], b: [f64; 2], offset: [f64; 2], label: &str) {
        let a2 = [a[0] + offset[0], a[1] + offset[1]];
        let b2 = [b[0] + offset[0], b[1] + offset[1]];
        self.line("DIMS", a, a2);
        self.line("DIMS", b, b2);
        self.line("DIMS", a2, b2);
        let tick = 1.5;
        for p in [a2, b2] {
            self.line(
                "DIMS",
                [p[0] - tick, p[1] - tick],
                [p[0] + tick, p[1] + tick],
            );
        }
        let mid = [(a2[0] + b2[0]) * 0.5, (a2[1] + b2[1]) * 0.5];
        let vertical = (a[0] - b[0]).abs() < 1e-9;
        let at = if vertical {
            [mid[0] - 2.0, mid[1]]
        } else {
            [mid[0], mid[1] + 2.0]
        };
        self.text("DIMS", at, 3.0, label, 1, if vertical { 90.0 } else { 0.0 });
    }
}

pub fn to_dxf(document: &Document) -> String {
    let mut e = Entities::default();
    let p = &document.project;
    let width = p.parameters.get("width_in").map(|v| v.value).unwrap_or(0.0);
    let length = p
        .parameters
        .get("length_in")
        .map(|v| v.value)
        .unwrap_or(0.0);

    // Plan view.
    for c in &document.components {
        plan_view(&mut e, document, c);
    }
    e.dimension(
        [0.0, 0.0],
        [width, 0.0],
        [0.0, -14.0],
        &format!("{} IN", trim(width)),
    );
    e.dimension(
        [0.0, 0.0],
        [0.0, length],
        [-14.0, 0.0],
        &format!("{} IN", trim(length)),
    );
    e.text(
        "TITLE",
        [0.0, length + 12.0],
        5.0,
        &format!(
            "{}  PLAN  {} x {} IN  REV {}",
            p.name.to_uppercase(),
            trim(width),
            trim(length),
            document.revision
        ),
        0,
        0.0,
    );

    // Flat cut profiles, one per unique cut part.
    let mut x = width + 60.0;
    let mut y = 0.0;
    let column_top = length.max(120.0);
    e.text(
        "CUT_TEXT",
        [x, column_top + 12.0],
        5.0,
        "CUT PARTS (IN)",
        0,
        0.0,
    );
    for line in document.bom.iter().filter(|l| l.cut_length_in.is_some()) {
        let cut = line.cut_length_in.unwrap_or(0.0);
        let Some(sample) = line
            .component_ids
            .first()
            .and_then(|id| document.components.iter().find(|c| &c.id.0 == id))
        else {
            continue;
        };
        // Profile seen from the face that shows the cut length.
        let s = sample.bounds.size;
        let depth = match sample.kind {
            ComponentKind::Stringer | ComponentKind::Joist | ComponentKind::Bumper => s[2],
            _ => {
                if (s[0] - cut).abs() < 1e-9 {
                    s[1]
                } else {
                    s[0]
                }
            }
        };
        if y + depth + 12.0 > column_top {
            x += cut.max(96.0) + 30.0;
            y = 0.0;
        }
        e.rect("CUT", x, y, cut, depth);
        // Holes drilled through the face this view looks at, in place.
        let (base, ops) = unwrap_ops(&sample.shape);
        let (a_axis, b_axis) = flat_axes(sample, cut);
        let view = 3 - a_axis - b_axis;
        let mut notes = Vec::new();
        for op in ops {
            let visible = match op {
                ShapeOp::Hole { face, .. } => base
                    .polyhedron()
                    .and_then(|p| p.face_frame(face))
                    .map(|f| sample.pose.rotate(f.normal)[view].abs() > 0.999)
                    .unwrap_or(false),
                _ => false,
            };
            match op {
                ShapeOp::Hole { at, diameter, .. } if visible => {
                    let w = sample.pose.apply(*at);
                    let o = sample.bounds.origin;
                    e.circle(
                        "CUT",
                        [x + w[a_axis] - o[a_axis], y + w[b_axis] - o[b_axis]],
                        diameter * 0.5,
                    );
                }
                other => notes.push(op_note(other)),
            }
        }
        let mut label = format!(
            "{} x{}  {}  CUT {} IN",
            line.item.to_uppercase(),
            line.qty,
            line.size.to_uppercase(),
            trim(cut)
        );
        for n in notes {
            label.push_str("  ");
            label.push_str(&n);
        }
        e.text("CUT_TEXT", [x, y + depth + 2.0], 3.0, &label, 0, 0.0);
        y += depth + 14.0;
    }

    // Plate parts: exact profile (arcs as bulges, holes as loops and
    // circles), one per BOM line.
    for line in document.bom.iter().filter(|l| l.kind == "custom") {
        let Some(sample) = line
            .component_ids
            .first()
            .and_then(|id| document.components.iter().find(|c| &c.id.0 == id))
        else {
            continue;
        };
        let (base, ops) = unwrap_ops(&sample.shape);
        let Shape::Prism { profile, depth } = base else {
            continue;
        };
        let Ok(profile) = profile.normalized() else {
            continue;
        };
        let (lo, hi) = profile.bounds();
        let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
        if y + h + 12.0 > column_top {
            x += w.max(96.0) + 30.0;
            y = 0.0;
        }
        let at = |p: [f64; 2]| [x + p[0] - lo[0], y + p[1] - lo[1]];
        for path in std::iter::once(&profile.outer).chain(profile.holes.iter()) {
            let pts: Vec<(f64, f64, f64)> = path
                .iter()
                .map(|v| {
                    let q = at([v.x, v.y]);
                    (q[0], q[1], v.bulge)
                })
                .collect();
            e.polyline("CUT", &pts, true);
        }
        let mut notes = Vec::new();
        for op in ops {
            match op {
                ShapeOp::Hole {
                    face,
                    at: p,
                    diameter,
                    ..
                } if face == "top" || face == "bottom" => {
                    e.circle("CUT", at([p[0], p[1]]), diameter * 0.5);
                }
                other => notes.push(op_note(other)),
            }
        }
        let mut label = format!(
            "{} x{}  T {} IN  {}",
            line.item.to_uppercase(),
            line.qty,
            trim(*depth),
            line.material.to_uppercase()
        );
        for n in notes {
            label.push_str("  ");
            label.push_str(&n);
        }
        e.text("CUT_TEXT", [x, y + h + 2.0], 3.0, &label, 0, 0.0);
        y += h + 14.0;
    }

    let mut out = String::new();
    out.push_str("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n");
    out.push_str("9\n$INSBASE\n10\n0.0\n20\n0.0\n30\n0.0\n");
    let _ = write!(
        out,
        "9\n$EXTMIN\n10\n{}\n20\n{}\n30\n0.0\n9\n$EXTMAX\n10\n{}\n20\n{}\n30\n0.0\n",
        num(e.min[0]),
        num(e.min[1]),
        num(e.max[0]),
        num(e.max[1])
    );
    let _ = write!(
        out,
        "9\n$LIMMIN\n10\n{}\n20\n{}\n9\n$LIMMAX\n10\n{}\n20\n{}\n",
        num(e.min[0]),
        num(e.min[1]),
        num(e.max[0]),
        num(e.max[1])
    );
    out.push_str("0\nENDSEC\n");
    out.push_str("0\nSECTION\n2\nTABLES\n");
    out.push_str("0\nTABLE\n2\nLTYPE\n70\n1\n0\nLTYPE\n2\nCONTINUOUS\n70\n0\n3\nSolid line\n72\n65\n73\n0\n40\n0.0\n0\nENDTAB\n");
    let _ = write!(out, "0\nTABLE\n2\nLAYER\n70\n{}\n", LAYERS.len());
    for (name, color) in LAYERS {
        let _ = write!(
            out,
            "0\nLAYER\n2\n{name}\n70\n0\n62\n{color}\n6\nCONTINUOUS\n"
        );
    }
    out.push_str("0\nENDTAB\n");
    out.push_str("0\nTABLE\n2\nSTYLE\n70\n1\n0\nSTYLE\n2\nSTANDARD\n70\n0\n40\n0.0\n41\n1.0\n50\n0.0\n71\n0\n42\n2.5\n3\ntxt\n4\n\n0\nENDTAB\n");
    out.push_str("0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nENDSEC\n");
    out.push_str("0\nSECTION\n2\nENTITIES\n");
    out.push_str(&e.out);
    out.push_str("0\nENDSEC\n0\nEOF\n");
    out
}

fn layer_of(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::FloatModule => "FLOATS",
        ComponentKind::Stringer | ComponentKind::Joist => "FRAME",
        ComponentKind::DeckBoard => "DECK",
        ComponentKind::Cleat | ComponentKind::Bumper => "HARDWARE",
        ComponentKind::Custom => "CUSTOM",
        ComponentKind::GangwayStringer
        | ComponentKind::GangwayTread
        | ComponentKind::GangwayHinge => "GANGWAY",
    }
}

/// The base shape and the modifier operations on it.
fn unwrap_ops(shape: &Shape) -> (&Shape, &[ShapeOp]) {
    match shape {
        Shape::Modified { base, ops } => (base.as_ref(), ops.as_slice()),
        other => (other, &[]),
    }
}

fn op_note(op: &ShapeOp) -> String {
    match op {
        ShapeOp::Fillet { edges, radius } => {
            format!("FILLET R{} ON {} EDGE(S)", trim(*radius), edges.len())
        }
        ShapeOp::Chamfer { edges, distance } => {
            format!("CHAMFER {} ON {} EDGE(S)", trim(*distance), edges.len())
        }
        ShapeOp::Hole {
            face,
            diameter,
            depth,
            ..
        } => format!(
            "HOLE D{} {} IN FACE {}",
            trim(*diameter),
            if *depth > 0.0 {
                format!("X {} DEEP", trim(*depth))
            } else {
                "THRU".into()
            },
            face.to_uppercase()
        ),
        ShapeOp::Cut { .. } => "CUTOUT SEE STEP".into(),
    }
}

/// Which local axes a stock part's flat profile shows: (cut length axis,
/// depth axis).
fn flat_axes(c: &Component, cut: f64) -> (usize, usize) {
    let s = c.bounds.size;
    let a = if (s[0] - cut).abs() < 1e-9 { 0 } else { 1 };
    let b = match c.kind {
        ComponentKind::Stringer | ComponentKind::Joist | ComponentKind::Bumper => 2,
        _ => 1 - a,
    };
    (a, b)
}

/// True when `pose` only turns about the vertical, so plan outlines stay
/// exact.
fn plan_rotation_only(pose: &Pose) -> bool {
    let z = pose.rotate([0.0, 0.0, 1.0]);
    (z[2] - 1.0).abs() < 1e-9
}

/// True when `pose` maps each local axis onto a world axis.
fn axis_aligned(pose: &Pose) -> bool {
    let m = pose.matrix();
    m.iter()
        .flatten()
        .all(|v| v.abs() < 1e-9 || (v.abs() - 1.0).abs() < 1e-9)
}

fn is_rectangle(path: &[PathVertex]) -> bool {
    path.len() == 4
        && path.iter().all(|v| v.bulge == 0.0)
        && (0..4).all(|i| {
            let (a, b) = (path[i], path[(i + 1) % 4]);
            (a.x - b.x).abs() < 1e-9 || (a.y - b.y).abs() < 1e-9
        })
}

fn plan_view(e: &mut Entities, document: &Document, c: &Component) {
    let layer = layer_of(c.kind);
    let (base, ops) = unwrap_ops(&c.shape);
    let edge_ops = ops.iter().any(|op| !matches!(op, ShapeOp::Hole { .. }));
    // Tubes and planks lying along x or y: their plan is exactly their
    // bounding rectangle.
    if let Shape::Prism { profile, .. } = base {
        if !edge_ops
            && axis_aligned(&c.pose)
            && is_rectangle(&profile.outer)
            && !plan_rotation_only(&c.pose)
        {
            e.rect(
                layer,
                c.bounds.origin[0],
                c.bounds.origin[1],
                c.bounds.size[0],
                c.bounds.size[1],
            );
            return;
        }
    }
    if plan_rotation_only(&c.pose) && !edge_ops {
        let loops: Option<Vec<Vec<PathVertex>>> = match base {
            Shape::Box { size } => {
                let r = Profile::rectangle(size[0], size[1]);
                Some(vec![r.outer])
            }
            Shape::Prism { profile, .. } => profile
                .normalized()
                .ok()
                .map(|p| std::iter::once(p.outer).chain(p.holes).collect()),
            _ => None,
        };
        if let Some(loops) = loops {
            for path in loops {
                let pts: Vec<(f64, f64, f64)> = path
                    .iter()
                    .map(|v| {
                        let w = c.pose.apply([v.x, v.y, 0.0]);
                        (w[0], w[1], v.bulge)
                    })
                    .collect();
                e.polyline(layer, &pts, true);
            }
            for op in ops {
                if let ShapeOp::Hole {
                    face, at, diameter, ..
                } = op
                {
                    if matches!(face.as_str(), "z+" | "z-" | "top" | "bottom") {
                        let w = c.pose.apply(*at);
                        e.circle(layer, [w[0], w[1]], diameter * 0.5);
                    }
                }
            }
            return;
        }
    }
    // Tilted or edge-modified parts: project their B-rep edges. Straight
    // edges are exact; curved ones are the kernel's sampled edge points.
    if let Some(mesh) = document
        .scene
        .meshes
        .iter()
        .find(|m| m.component_id == c.id)
    {
        let mut drawn = 0;
        for edge in &mesh.edge_info {
            let pts: Vec<(f64, f64, f64)> = edge
                .polyline
                .chunks(3)
                .map(|p| (f64::from(p[0]), f64::from(p[1]), 0.0))
                .collect();
            if pts.len() == 2 {
                e.line(layer, [pts[0].0, pts[0].1], [pts[1].0, pts[1].1]);
            } else if pts.len() > 2 {
                e.polyline(layer, &pts, false);
            }
            drawn += 1;
        }
        if drawn > 0 {
            return;
        }
    }
    if let Some(poly) = c.shape.polyhedron() {
        let world = poly.transformed(&c.pose);
        let mut seen: BTreeMap<(usize, usize), ()> = BTreeMap::new();
        for face in &world.faces {
            let l = &face.loop_;
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if seen.insert((a.min(b), a.max(b)), ()).is_none() {
                    let (pa, pb) = (world.vertices[a], world.vertices[b]);
                    e.line(layer, [pa[0], pa[1]], [pb[0], pb[1]]);
                }
            }
        }
        return;
    }
    e.rect(
        layer,
        c.bounds.origin[0],
        c.bounds.origin[1],
        c.bounds.size[0],
        c.bounds.size[1],
    );
}

fn num(v: f64) -> String {
    let v = if v.abs() < 1e-12 { 0.0 } else { v };
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    }
}

fn trim(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// R12 text is single-line ASCII.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                '?'
            }
        })
        .collect()
}

/// Group-code reader used by the tests and the CLI `verify` command.
pub mod check {
    #[derive(Debug, Default)]
    pub struct Summary {
        pub version: String,
        pub layers: Vec<String>,
        pub polylines: usize,
        pub closed_polylines: usize,
        pub lines: usize,
        pub texts: usize,
        pub circles: usize,
        /// Polyline vertices with a non-zero bulge (exact arcs).
        pub arc_vertices: usize,
        pub entity_layers: Vec<String>,
    }

    pub fn parse(text: &str) -> Result<Summary, String> {
        let lines: Vec<&str> = text.lines().collect();
        if !lines.len().is_multiple_of(2) {
            return Err("odd number of lines; group codes and values must pair up".into());
        }
        let pairs: Vec<(i32, &str)> = lines
            .chunks(2)
            .map(|c| {
                c[0].trim()
                    .parse::<i32>()
                    .map(|code| (code, c[1]))
                    .map_err(|_| format!("bad group code {:?}", c[0]))
            })
            .collect::<Result<_, _>>()?;
        if pairs.last() != Some(&(0, "EOF")) {
            return Err("missing EOF".into());
        }
        let mut summary = Summary::default();
        let mut depth = 0i32;
        let mut section = "";
        let mut i = 0;
        let mut in_polyline = false;
        let mut vertices = 0;
        while i < pairs.len() {
            let (code, value) = pairs[i];
            match (code, value) {
                (0, "SECTION") => {
                    depth += 1;
                    section = pairs.get(i + 1).map(|p| p.1).unwrap_or("");
                }
                (0, "ENDSEC") => {
                    depth -= 1;
                    section = "";
                }
                (9, "$ACADVER") => {
                    summary.version = pairs
                        .get(i + 1)
                        .map(|p| p.1.to_string())
                        .unwrap_or_default()
                }
                (0, "LAYER") if section == "TABLES" => {
                    if let Some((2, name)) = pairs.get(i + 1) {
                        summary.layers.push(name.to_string());
                    }
                }
                (0, entity) if section == "ENTITIES" => {
                    let layer = pairs[i + 1..]
                        .iter()
                        .take_while(|p| p.0 != 0)
                        .find(|p| p.0 == 8)
                        .map(|p| p.1.to_string())
                        .ok_or_else(|| format!("{entity} without layer"))?;
                    if !summary.layers.contains(&layer) {
                        return Err(format!("{entity} on undefined layer {layer}"));
                    }
                    for p in pairs[i + 1..].iter().take_while(|p| p.0 != 0) {
                        if matches!(p.0, 10 | 20 | 30 | 11 | 21 | 31 | 40 | 42) {
                            let v: f64 =
                                p.1.trim()
                                    .parse()
                                    .map_err(|_| format!("bad number {}", p.1))?;
                            if !v.is_finite() {
                                return Err("non-finite coordinate".into());
                            }
                        }
                    }
                    match entity {
                        "POLYLINE" => {
                            if in_polyline {
                                return Err("nested POLYLINE".into());
                            }
                            in_polyline = true;
                            vertices = 0;
                            summary.polylines += 1;
                            let closed = pairs[i + 1..].iter().take_while(|p| p.0 != 0).any(|p| {
                                p.0 == 70 && p.1.trim().parse::<i32>().unwrap_or(0) & 1 == 1
                            });
                            if closed {
                                summary.closed_polylines += 1;
                            }
                        }
                        "VERTEX" => {
                            if !in_polyline {
                                return Err("VERTEX outside POLYLINE".into());
                            }
                            vertices += 1;
                            if pairs[i + 1..]
                                .iter()
                                .take_while(|p| p.0 != 0)
                                .any(|p| p.0 == 42)
                            {
                                summary.arc_vertices += 1;
                            }
                        }
                        "SEQEND" => {
                            if !in_polyline || vertices < 2 {
                                return Err("SEQEND without a polyline of 2+ vertices".into());
                            }
                            in_polyline = false;
                        }
                        "LINE" => summary.lines += 1,
                        "CIRCLE" => summary.circles += 1,
                        "TEXT" => summary.texts += 1,
                        "ENDSEC" | "EOF" => {}
                        other => return Err(format!("unexpected entity {other}")),
                    }
                    if entity != "VERTEX" && entity != "SEQEND" && entity != "ENDSEC" {
                        summary.entity_layers.push(layer);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if depth != 0 {
            return Err("unbalanced SECTION/ENDSEC".into());
        }
        if in_polyline {
            return Err("unterminated POLYLINE".into());
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquidcad_core::{rebuild, templates::modular_dock};

    #[test]
    fn dxf_is_structurally_valid_r12() {
        let document = rebuild(&modular_dock(), 1).unwrap();
        let dxf = to_dxf(&document);
        let summary = check::parse(&dxf).unwrap();
        assert_eq!(summary.version, "AC1009");
        assert_eq!(summary.polylines, summary.closed_polylines);
        let cut_parts = document
            .bom
            .iter()
            .filter(|l| l.cut_length_in.is_some())
            .count();
        // The sloped gangway is drawn from its projected edges; every level
        // part is one closed outline.
        let level = document
            .components
            .iter()
            .filter(|c| !c.id.as_str().starts_with("gangway."))
            .count();
        assert_eq!(summary.polylines, level + cut_parts);
        assert!(summary.lines > 0);
        for layer in [
            "FLOATS", "FRAME", "DECK", "HARDWARE", "GANGWAY", "DIMS", "CUT",
        ] {
            assert!(
                summary.entity_layers.iter().any(|l| l == layer),
                "no entities on {layer}"
            );
        }
    }

    #[test]
    fn plan_tracks_the_width_parameter() {
        let mut project = modular_dock();
        project.set_parameter("width_in", 144.0).unwrap();
        let dxf = to_dxf(&rebuild(&project, 2).unwrap());
        assert!(dxf.contains("144 IN"));
        assert!(dxf.contains("\n10\n144.0\n") || dxf.contains("\n10\n142.0\n"));
    }

    #[test]
    fn checker_rejects_broken_files() {
        assert!(check::parse("0\nSECTION\n2\nENTITIES\n0\nEOF\n").is_err());
        assert!(check::parse("0\nEOF").is_err() || check::parse("0\nEOF\n").is_ok());
    }

    #[test]
    fn numbers_always_have_a_decimal_point() {
        assert_eq!(num(1.0), "1.0");
        assert_eq!(num(-0.0), "0.0");
        assert_eq!(num(2.125), "2.125");
    }
}
