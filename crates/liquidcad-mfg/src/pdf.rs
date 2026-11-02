//! PDF 1.4 writer for [`Sheet`]. Uses the base-14 Helvetica faces so the
//! file needs no embedded fonts; widths come from the Adobe AFM metrics so
//! centred and right-aligned text lands where the sheet model says.

use crate::sheet::{Anchor, Prim, Sheet};
use std::fmt::Write as _;
use std::io::Write as _;

pub fn render(sheet: &Sheet, creation_date: &str) -> Vec<u8> {
    let mut c = String::new();
    c.push_str("1 J 1 j\n");
    for item in &sheet.items {
        match item {
            Prim::Line {
                a,
                b,
                weight,
                color,
                dash,
            } => {
                let _ = writeln!(
                    c,
                    "{} {} {} RG {} w {} {} {} m {} {} l S{}",
                    f(color[0]),
                    f(color[1]),
                    f(color[2]),
                    n(*weight),
                    if *dash { "[4 2] 0 d" } else { "[] 0 d" },
                    n(a[0]),
                    n(a[1]),
                    n(b[0]),
                    n(b[1]),
                    if *dash { " [] 0 d" } else { "" }
                );
            }
            Prim::Rect {
                x,
                y,
                w,
                h,
                weight,
                stroke,
                fill,
            } => {
                if let Some(fc) = fill {
                    let _ = write!(c, "{} {} {} rg ", f(fc[0]), f(fc[1]), f(fc[2]));
                }
                if let Some(sc) = stroke {
                    let _ = write!(
                        c,
                        "{} {} {} RG {} w ",
                        f(sc[0]),
                        f(sc[1]),
                        f(sc[2]),
                        n(*weight)
                    );
                }
                let op = match (fill.is_some(), stroke.is_some()) {
                    (true, true) => "B",
                    (true, false) => "f",
                    (false, true) => "S",
                    (false, false) => "n",
                };
                let _ = writeln!(c, "{} {} {} {} re {op}", n(*x), n(*y), n(*w), n(*h));
            }
            Prim::Text {
                at,
                size,
                text,
                color,
                anchor,
                bold,
                rotate,
            } => {
                let text = ascii(text);
                let width = text_width(&text, *size, *bold);
                let shift = match anchor {
                    Anchor::Start => 0.0,
                    Anchor::Middle => -width * 0.5,
                    Anchor::End => -width,
                };
                let (sin, cos) = rotate.to_radians().sin_cos();
                let x = at[0] + shift * cos;
                let y = at[1] + shift * sin;
                let _ = writeln!(
                    c,
                    "BT /{} {} Tf {} {} {} rg {} {} {} {} {} {} Tm ({}) Tj ET",
                    if *bold { "F2" } else { "F1" },
                    n(*size),
                    f(color[0]),
                    f(color[1]),
                    f(color[2]),
                    n(cos),
                    n(sin),
                    n(-sin),
                    n(cos),
                    n(x),
                    n(y),
                    escape(&text)
                );
            }
        }
    }

    let stream = c.into_bytes();
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> /ProcSet [/PDF /Text] >> >>",
            n(sheet.width),
            n(sheet.height)
        )
        .into_bytes(),
        {
            let mut obj = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
            obj.extend_from_slice(&stream);
            obj.extend_from_slice(b"\nendstream");
            obj
        },
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec(),
        format!(
            "<< /Title ({}) /Producer (liquidCAD {}) /Creator (liquidCAD) /CreationDate ({}) >>",
            escape(&ascii(&sheet.title)),
            env!("CARGO_PKG_VERSION"),
            escape(creation_date)
        )
        .into_bytes(),
    ];

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        let _ = writeln!(pdf, "{} 0 obj", i + 1);
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let _ = write!(pdf, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = writeln!(pdf, "{offset:010} 00000 n ");
    }
    let _ = write!(
        pdf,
        "trailer\n<< /Size {} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    pdf
}

fn n(v: f64) -> String {
    let v = if v.abs() < 1e-9 { 0.0 } else { v };
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.to_string()
    }
}

fn f(v: f32) -> String {
    n(f64::from(v.clamp(0.0, 1.0)))
}

fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| if (' '..='~').contains(&c) { c } else { '?' })
        .collect()
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

/// Helvetica / Helvetica-Bold advance widths for ASCII 32..=126 (1/1000 em).
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

pub fn text_width(text: &str, size: f64, bold: bool) -> f64 {
    let table = if bold { &HELV_BOLD } else { &HELV };
    let units: u32 = text
        .bytes()
        .map(|b| match b {
            32..=126 => u32::from(table[(b - 32) as usize]),
            _ => 556,
        })
        .sum();
    f64::from(units) / 1000.0 * size
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet;
    use liquidcad_core::{rebuild, templates::modular_dock};

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn xref_offsets_point_at_objects() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let pdf = render(&sheet::build(&doc, "2026-11-01"), "D:20261101000000Z");
        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let tail = &pdf[find(&pdf, b"startxref\n").unwrap() + 10..];
        let startxref: usize = std::str::from_utf8(&tail[..find(tail, b"\n").unwrap()])
            .unwrap()
            .parse()
            .unwrap();
        assert!(pdf[startxref..].starts_with(b"xref\n0 8\n"));
        let table = std::str::from_utf8(&pdf[startxref..]).unwrap();
        for (i, line) in table.lines().skip(3).take(7).enumerate() {
            assert_eq!(line.len(), 19, "xref rows are 20 bytes with the newline");
            let offset: usize = line[..10].parse().unwrap();
            let expected = format!("{} 0 obj", i + 1);
            assert!(
                pdf[offset..].starts_with(expected.as_bytes()),
                "object {} not at {offset}",
                i + 1
            );
        }
        let len_at = find(&pdf, b"/Length ").unwrap() + 8;
        let len_end = len_at + find(&pdf[len_at..], b" ").unwrap();
        let length: usize = std::str::from_utf8(&pdf[len_at..len_end])
            .unwrap()
            .parse()
            .unwrap();
        let start = find(&pdf, b"stream\n").unwrap() + 7;
        assert!(pdf[start + length..].starts_with(b"\nendstream"));
    }

    #[test]
    fn widths_match_afm() {
        assert_eq!(text_width("A", 1000.0, false), 667.0);
        assert_eq!(text_width("i", 10.0, false), 2.22);
        assert_eq!(text_width("W", 1000.0, true), 944.0);
        assert_eq!(text_width("~", 1000.0, false), 584.0);
    }
}
