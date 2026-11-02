//! Fabrication outputs: BOM CSV, drawing sheet (PDF and on-screen),
//! stock cut planning, and manufacturing validation.

pub mod cutlist;
pub mod pdf;
pub mod sheet;

use liquidcad_core::document::Document;
use std::path::Path;

pub use cutlist::{CutPlan, StockGroup};
pub use sheet::Sheet;

pub fn write_csv(document: &Document, path: &Path) -> std::io::Result<u64> {
    std::fs::write(path, to_csv(document))?;
    Ok(std::fs::metadata(path)?.len())
}

/// RFC 4180 CSV, one row per BOM line.
pub fn to_csv(document: &Document) -> String {
    let mut out = String::from(
        "line,item,kind,material,size,cut_length_in,qty,unit_weight_lb,total_weight_lb,notes,component_ids\r\n",
    );
    for (i, line) in document.bom.iter().enumerate() {
        let fields = [
            (i + 1).to_string(),
            line.item.clone(),
            line.kind.clone(),
            line.material.clone(),
            line.size.clone(),
            line.cut_length_in
                .map(|v| format!("{v:.3}"))
                .unwrap_or_default(),
            line.qty.to_string(),
            format!("{:.3}", line.unit_weight_lb),
            format!("{:.3}", line.total_weight_lb),
            line.notes.clone(),
            line.component_ids.join(" "),
        ];
        out.push_str(
            &fields
                .iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn drawing(document: &Document) -> Sheet {
    sheet::build(
        document,
        &chrono::Local::now().format("%Y-%m-%d").to_string(),
    )
}

pub fn write_pdf(document: &Document, path: &Path) -> std::io::Result<u64> {
    let date = chrono::Utc::now().format("D:%Y%m%d%H%M%SZ").to_string();
    std::fs::write(path, pdf::render(&drawing(document), &date))?;
    Ok(std::fs::metadata(path)?.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquidcad_core::{rebuild, templates::modular_dock};

    fn parse_csv(text: &str) -> Vec<Vec<String>> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, quoted) {
                ('"', true) if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                ('"', _) => quoted = !quoted,
                (',', false) => row.push(std::mem::take(&mut field)),
                ('\r', false) => {}
                ('\n', false) => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                _ => field.push(c),
            }
        }
        rows
    }

    #[test]
    fn csv_round_trips_and_matches_bom() {
        let document = rebuild(&modular_dock(), 1).unwrap();
        let rows = parse_csv(&to_csv(&document));
        assert_eq!(rows.len(), document.bom.len() + 1);
        assert!(rows.iter().all(|r| r.len() == 11));
        let floats = rows.iter().find(|r| r[2] == "float_module").unwrap();
        assert_eq!(floats[6], "32");
        let total: usize = rows[1..]
            .iter()
            .map(|r| r[6].parse::<usize>().unwrap())
            .sum();
        assert_eq!(total, document.components.len());
    }

    #[test]
    fn csv_quotes_awkward_fields() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("6\" tube"), "\"6\"\" tube\"");
        assert_eq!(csv_field("plain"), "plain");
    }
}
