//! ISO 10303-21 writer for AP214 (automotive_design) advanced B-rep solids.
//!
//! Output units are millimetres. Each solid is a MANIFOLD_SOLID_BREP named by
//! its stable component id and carries a surface colour through a
//! MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION, which is how
//! OpenCascade, FreeCAD, and most commercial readers pick up colour.

use crate::brep::BrepSolid;
use std::collections::HashMap;
use std::fmt::Write as _;

pub struct StepSolid<'a> {
    pub name: &'a str,
    pub brep: &'a BrepSolid,
    pub color: [f32; 3],
}

pub struct StepMeta<'a> {
    pub file_name: &'a str,
    pub timestamp: &'a str,
    pub product_id: &'a str,
    pub product_name: &'a str,
}

pub fn write_ap214(solids: &[StepSolid], meta: &StepMeta) -> String {
    let mut w = Writer::default();
    let app =
        w.add("APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into());
    w.add(format!(
        "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"
    ));
    let product_ctx = w.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let product = w.add(format!(
        "PRODUCT('{}','{}','',(#{product_ctx}))",
        esc(meta.product_id),
        esc(meta.product_name)
    ));
    w.add(format!(
        "PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{product}))"
    ));
    let formation = w.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
    let def_ctx = w.add(format!(
        "PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"
    ));
    let definition = w.add(format!(
        "PRODUCT_DEFINITION('design','',#{formation},#{def_ctx})"
    ));
    let def_shape = w.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
    let mm = w.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let rad = w.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let sr = w.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let uncertainty = w.add(format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{mm},'distance_accuracy_value','confusion accuracy')"
    ));
    let ctx = w.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{rad},#{sr}))REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))"
    ));
    let origin = w.point([0.0; 3]);
    let z = w.direction([0.0, 0.0, 1.0]);
    let x = w.direction([1.0, 0.0, 0.0]);
    let world = w.add(format!("AXIS2_PLACEMENT_3D('',#{origin},#{z},#{x})"));

    let mut items = vec![world];
    let mut styled = Vec::new();
    let mut styles: HashMap<[u32; 3], usize> = HashMap::new();
    for solid in solids {
        let id = w.solid(solid.name, solid.brep);
        items.push(id);
        let key = solid
            .color
            .map(|c| (c.clamp(0.0, 1.0) * 1000.0).round() as u32);
        let style = match styles.get(&key) {
            Some(style) => *style,
            None => {
                let [r, g, b] = solid.color.map(|c| real(f64::from(c.clamp(0.0, 1.0))));
                let colour = w.add(format!("COLOUR_RGB('',{r},{g},{b})"));
                let fill_colour = w.add(format!("FILL_AREA_STYLE_COLOUR('',#{colour})"));
                let fill = w.add(format!("FILL_AREA_STYLE('',(#{fill_colour}))"));
                let area = w.add(format!("SURFACE_STYLE_FILL_AREA(#{fill})"));
                let side = w.add(format!("SURFACE_SIDE_STYLE('',(#{area}))"));
                let usage = w.add(format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
                let assignment = w.add(format!("PRESENTATION_STYLE_ASSIGNMENT((#{usage}))"));
                styles.insert(key, assignment);
                assignment
            }
        };
        styled.push(w.add(format!("STYLED_ITEM('color',(#{style}),#{id})")));
    }
    let shape = w.add(format!(
        "ADVANCED_BREP_SHAPE_REPRESENTATION('{}',({}),#{ctx})",
        esc(meta.product_name),
        refs(&items)
    ));
    w.add(format!(
        "SHAPE_DEFINITION_REPRESENTATION(#{def_shape},#{shape})"
    ));
    w.add(format!(
        "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',({}),#{ctx})",
        refs(&styled)
    ));

    let mut out = String::new();
    out.push_str("ISO-10303-21;\nHEADER;\n");
    out.push_str("FILE_DESCRIPTION(('liquidCAD exact B-rep export'),'2;1');\n");
    let _ = writeln!(
        out,
        "FILE_NAME('{}','{}',('liquidCAD'),('liquidCAD'),'liquidCAD {}','liquidCAD','');",
        esc(meta.file_name),
        esc(meta.timestamp),
        env!("CARGO_PKG_VERSION")
    );
    out.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n");
    for (i, line) in w.lines.iter().enumerate() {
        let _ = writeln!(out, "#{}={line};", i + 1);
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}

#[derive(Default)]
struct Writer {
    lines: Vec<String>,
    directions: HashMap<[i64; 3], usize>,
}

impl Writer {
    fn add(&mut self, entity: String) -> usize {
        self.lines.push(entity);
        self.lines.len()
    }

    fn point(&mut self, p: [f64; 3]) -> usize {
        let [a, b, c] = p.map(real);
        self.add(format!("CARTESIAN_POINT('',({a},{b},{c}))"))
    }

    fn direction(&mut self, d: [f64; 3]) -> usize {
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let unit = d.map(|v| v / len);
        let key = unit.map(|v| (v * 1e9).round() as i64);
        if let Some(id) = self.directions.get(&key) {
            return *id;
        }
        let [a, b, c] = unit.map(real);
        let id = self.add(format!("DIRECTION('',({a},{b},{c}))"));
        self.directions.insert(key, id);
        id
    }

    fn solid(&mut self, name: &str, brep: &BrepSolid) -> usize {
        let points: Vec<usize> = brep.vertices.iter().map(|p| self.point(*p)).collect();
        let vertices: Vec<usize> = points
            .iter()
            .map(|p| self.add(format!("VERTEX_POINT('',#{p})")))
            .collect();
        let mut edges = Vec::with_capacity(brep.edges.len());
        for (a, b) in &brep.edges {
            let (pa, pb) = (brep.vertices[*a], brep.vertices[*b]);
            let dir = self.direction([pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]]);
            let vector = self.add(format!("VECTOR('',#{dir},1.)"));
            let line = self.add(format!("LINE('',#{},#{vector})", points[*a]));
            edges.push(self.add(format!(
                "EDGE_CURVE('',#{},#{},#{line},.T.)",
                vertices[*a], vertices[*b]
            )));
        }
        let mut faces = Vec::with_capacity(brep.faces.len());
        for face in &brep.faces {
            let origin = self.point(brep.face_origin(face));
            let axis = self.direction(face.normal);
            let reference = self.direction(face.ref_dir);
            let placement = self.add(format!(
                "AXIS2_PLACEMENT_3D('',#{origin},#{axis},#{reference})"
            ));
            let plane = self.add(format!("PLANE('',#{placement})"));
            let mut bounds = Vec::with_capacity(1 + face.inner.len());
            for (k, ring) in std::iter::once(&face.outer)
                .chain(face.inner.iter())
                .enumerate()
            {
                let oriented: Vec<usize> = ring
                    .iter()
                    .map(|(edge, forward)| {
                        self.add(format!(
                            "ORIENTED_EDGE('',*,*,#{},{})",
                            edges[*edge],
                            if *forward { ".T." } else { ".F." }
                        ))
                    })
                    .collect();
                let edge_loop = self.add(format!("EDGE_LOOP('',({}))", refs(&oriented)));
                bounds.push(self.add(if k == 0 {
                    format!("FACE_OUTER_BOUND('',#{edge_loop},.T.)")
                } else {
                    format!("FACE_BOUND('',#{edge_loop},.T.)")
                }));
            }
            faces.push(self.add(format!(
                "ADVANCED_FACE('',({}),#{plane},.T.)",
                refs(&bounds)
            )));
        }
        let shell = self.add(format!("CLOSED_SHELL('',({}))", refs(&faces)));
        self.add(format!("MANIFOLD_SOLID_BREP('{}',#{shell})", esc(name)))
    }
}

fn refs(ids: &[usize]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// STEP REAL literal: always has a decimal point, no `-0.`.
fn real(v: f64) -> String {
    let v = if v.abs() < 1e-12 { 0.0 } else { v };
    let mut s = format!("{v:.6}");
    while s.ends_with('0') {
        s.pop();
    }
    s
}

/// Part 21 strings double single quotes; we also drop non-ASCII.
fn esc(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .collect::<String>()
        .replace('\'', "''")
}

/// Minimal Part 21 reader used to check exported files: every `#n=...;`
/// instance in DATA, keyed by id, with its entity keyword(s) and references.
pub mod check {
    use std::collections::BTreeMap;

    #[derive(Debug)]
    pub struct Instance {
        pub keywords: Vec<String>,
        pub refs: Vec<usize>,
        pub body: String,
    }

    pub fn parse(text: &str) -> Result<BTreeMap<usize, Instance>, String> {
        if !text.starts_with("ISO-10303-21;") || !text.trim_end().ends_with("END-ISO-10303-21;") {
            return Err("missing ISO-10303-21 envelope".into());
        }
        let data = text
            .split("DATA;")
            .nth(1)
            .and_then(|rest| rest.split("ENDSEC;").next())
            .ok_or("missing DATA section")?;
        let mut out = BTreeMap::new();
        for raw in split_statements(data) {
            let stmt = raw.trim();
            if stmt.is_empty() {
                continue;
            }
            let (id, body) = stmt
                .split_once('=')
                .ok_or_else(|| format!("bad instance {stmt}"))?;
            let id: usize = id
                .trim()
                .strip_prefix('#')
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| format!("bad id in {stmt}"))?;
            let body = body.trim().to_string();
            let keywords = keywords(&body);
            let refs = references(&body);
            if out
                .insert(
                    id,
                    Instance {
                        keywords,
                        refs,
                        body,
                    },
                )
                .is_some()
            {
                return Err(format!("duplicate instance #{id}"));
            }
        }
        for (id, inst) in &out {
            for r in &inst.refs {
                if !out.contains_key(r) {
                    return Err(format!("#{id} references missing #{r}"));
                }
            }
        }
        Ok(out)
    }

    fn split_statements(data: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = String::new();
        let mut in_string = false;
        for c in data.chars() {
            if c == '\'' {
                in_string = !in_string;
            }
            if c == ';' && !in_string {
                out.push(std::mem::take(&mut current));
            } else {
                current.push(c);
            }
        }
        out
    }

    fn strip_strings(body: &str) -> String {
        let mut out = String::new();
        let mut in_string = false;
        for c in body.chars() {
            if c == '\'' {
                in_string = !in_string;
                continue;
            }
            if !in_string {
                out.push(c);
            }
        }
        out
    }

    fn keywords(body: &str) -> Vec<String> {
        let plain = strip_strings(body);
        let mut out = Vec::new();
        let mut word = String::new();
        for c in plain.chars() {
            if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' {
                word.push(c);
            } else {
                if c == '(' && !word.is_empty() && word.chars().next().unwrap().is_ascii_uppercase()
                {
                    out.push(word.clone());
                }
                word.clear();
            }
        }
        out
    }

    fn references(body: &str) -> Vec<usize> {
        let plain = strip_strings(body);
        let mut out = Vec::new();
        let bytes = plain.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                if let Ok(n) = plain[start..end].parse() {
                    out.push(n);
                }
                i = end;
            } else {
                i += 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquidcad_core::document::SolidBox;

    #[test]
    fn reals_are_part21_literals() {
        assert_eq!(real(1.0), "1.");
        assert_eq!(real(-0.0), "0.");
        assert_eq!(real(25.4), "25.4");
        assert_eq!(real(-12.7), "-12.7");
    }

    #[test]
    fn single_box_round_trips_through_the_checker() {
        let brep = BrepSolid::from_box(
            SolidBox {
                origin: [0.0; 3],
                size: [1.0, 2.0, 3.0],
            },
            25.4,
        );
        let text = write_ap214(
            &[StepSolid {
                name: "part.o'neil",
                brep: &brep,
                color: [0.5, 0.5, 0.5],
            }],
            &StepMeta {
                file_name: "t.step",
                timestamp: "2026-01-01T00:00:00",
                product_id: "p",
                product_name: "P",
            },
        );
        let parsed = check::parse(&text).unwrap();
        let count = |kw: &str| {
            parsed
                .values()
                .filter(|i| i.keywords.iter().any(|k| k == kw))
                .count()
        };
        assert_eq!(count("MANIFOLD_SOLID_BREP"), 1);
        assert_eq!(count("ADVANCED_FACE"), 6);
        assert_eq!(count("EDGE_CURVE"), 12);
        assert_eq!(count("VERTEX_POINT"), 8);
        assert_eq!(count("STYLED_ITEM"), 1);
        assert!(text.contains("'part.o''neil'"));
        assert!(text.contains("SI_UNIT(.MILLI.,.METRE.)"));
        assert!(text.contains("CARTESIAN_POINT('',(25.4,50.8,76.2))"));
    }
}
