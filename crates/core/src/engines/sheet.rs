//! Spreadsheets: read with calamine / csv, write CSV, TSV, XLSX, JSON, HTML
//! and Markdown tables.

use crate::kind::ext;
use crate::output::stem;
use crate::{output_path, Options, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
}

impl Cell {
    fn text(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            Cell::Text(s) => s.clone(),
            Cell::Number(n) => n.to_string(),
            Cell::Bool(b) => b.to_string(),
        }
    }

    fn parse(s: &str) -> Cell {
        let t = s.trim();
        if t.is_empty() {
            Cell::Empty
        } else if let Ok(n) = t.parse::<f64>() {
            // Keep leading zeros (postcodes, ids) as text.
            if t.len() > 1 && t.starts_with('0') && !t.starts_with("0.") {
                Cell::Text(s.into())
            } else {
                Cell::Number(n)
            }
        } else {
            Cell::Text(s.into())
        }
    }
}

pub type Rows = Vec<Vec<Cell>>;

pub struct Sheet {
    pub name: String,
    pub rows: Rows,
}

pub fn can_read(e: &str) -> bool {
    matches!(e, "xlsx" | "xlsm" | "xls" | "xlsb" | "ods" | "csv" | "tsv")
}

pub fn read(path: &Path) -> Result<Vec<Sheet>> {
    let e = ext(path);
    if e == "csv" || e == "tsv" {
        let delimiter = if e == "tsv" {
            b'\t'
        } else {
            sniff_delimiter(path)?
        };
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(delimiter)
            .from_path(path)
            .map_err(anyhow::Error::from)?;
        let mut rows = Vec::new();
        for record in reader.records() {
            let record = record.map_err(anyhow::Error::from)?;
            rows.push(record.iter().map(Cell::parse).collect());
        }
        return Ok(vec![Sheet {
            name: stem(path),
            rows,
        }]);
    }
    use calamine::{open_workbook_auto, Data, Reader};
    let mut wb = open_workbook_auto(path).map_err(anyhow::Error::from)?;
    let mut sheets = Vec::new();
    for name in wb.sheet_names() {
        let range = wb.worksheet_range(&name).map_err(anyhow::Error::from)?;
        let rows = range
            .rows()
            .map(|r| {
                r.iter()
                    .map(|c| match c {
                        Data::Empty => Cell::Empty,
                        Data::String(s) => Cell::Text(s.clone()),
                        Data::Float(f) => Cell::Number(*f),
                        Data::Int(i) => Cell::Number(*i as f64),
                        Data::Bool(b) => Cell::Bool(*b),
                        other => Cell::Text(other.to_string()),
                    })
                    .collect()
            })
            .collect();
        sheets.push(Sheet { name, rows });
    }
    Ok(sheets)
}

/// European exports often use `;`. Pick whichever of `,;` appears most in the first line.
fn sniff_delimiter(path: &Path) -> Result<u8> {
    use std::io::BufRead;
    let mut first = String::new();
    std::io::BufReader::new(std::fs::File::open(path)?).read_line(&mut first)?;
    let commas = first.matches(',').count();
    let semis = first.matches(';').count();
    Ok(if semis > commas { b';' } else { b',' })
}

fn write_delimited(rows: &Rows, delimiter: u8, out: &Path) -> Result<()> {
    let mut w = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_path(out)
        .map_err(anyhow::Error::from)?;
    for row in rows {
        w.write_record(row.iter().map(Cell::text))
            .map_err(anyhow::Error::from)?;
    }
    w.flush()?;
    Ok(())
}

fn write_json(rows: &Rows, out: &Path) -> Result<()> {
    use serde_json::{Map, Value};
    let Some((header, body)) = rows.split_first() else {
        std::fs::write(out, "[]\n")?;
        return Ok(());
    };
    let keys: Vec<String> = header
        .iter()
        .enumerate()
        .map(|(i, c)| match c.text() {
            s if s.is_empty() => format!("column{}", i + 1),
            s => s,
        })
        .collect();
    let records: Vec<Value> = body
        .iter()
        .map(|row| {
            let mut obj = Map::new();
            for (i, key) in keys.iter().enumerate() {
                let v = match row.get(i).unwrap_or(&Cell::Empty) {
                    Cell::Empty => Value::Null,
                    Cell::Text(s) => Value::String(s.clone()),
                    Cell::Number(n) => serde_json::Number::from_f64(*n)
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                    Cell::Bool(b) => Value::Bool(*b),
                };
                obj.insert(key.clone(), v);
            }
            Value::Object(obj)
        })
        .collect();
    std::fs::write(
        out,
        serde_json::to_string_pretty(&records).map_err(anyhow::Error::from)? + "\n",
    )?;
    Ok(())
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn write_html(sheets: &[&Sheet], out: &Path) -> Result<()> {
    let mut html = String::from("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><style>table{border-collapse:collapse;margin:1em 0}td,th{border:1px solid #ccc;padding:4px 8px}</style></head><body>\n");
    for sheet in sheets {
        html.push_str(&format!("<h2>{}</h2>\n<table>\n", esc_html(&sheet.name)));
        for (i, row) in sheet.rows.iter().enumerate() {
            let tag = if i == 0 { "th" } else { "td" };
            html.push_str("<tr>");
            for c in row {
                html.push_str(&format!("<{tag}>{}</{tag}>", esc_html(&c.text())));
            }
            html.push_str("</tr>\n");
        }
        html.push_str("</table>\n");
    }
    html.push_str("</body></html>\n");
    std::fs::write(out, html)?;
    Ok(())
}

fn write_markdown(rows: &Rows, out: &Path) -> Result<()> {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut md = String::new();
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<String> = (0..width)
            .map(|j| {
                row.get(j)
                    .map(Cell::text)
                    .unwrap_or_default()
                    .replace('|', "\\|")
                    .replace('\n', " ")
            })
            .collect();
        md.push_str(&format!("| {} |\n", cells.join(" | ")));
        if i == 0 {
            md.push_str(&format!("|{}\n", " --- |".repeat(width)));
        }
    }
    std::fs::write(out, md)?;
    Ok(())
}

/// Excel sheet names: ≤ 31 chars, none of `[]:*?/\`, unique.
fn sheet_name(raw: &str, taken: &mut Vec<String>) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| !"[]:*?/\\".contains(*c))
        .take(31)
        .collect();
    let base = if clean.is_empty() {
        "Sheet".to_string()
    } else {
        clean
    };
    let mut name = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
        let suffix = format!(" {n}");
        name = format!(
            "{}{suffix}",
            base.chars().take(31 - suffix.len()).collect::<String>()
        );
        n += 1;
    }
    taken.push(name.clone());
    name
}

fn write_xlsx(sheets: &[(String, &Rows)], out: &Path) -> Result<()> {
    use rust_xlsxwriter::Workbook;
    let x = |e: rust_xlsxwriter::XlsxError| anyhow::anyhow!("{e}");
    let mut wb = Workbook::new();
    let mut taken = Vec::new();
    for (name, rows) in sheets {
        let ws = wb.add_worksheet();
        ws.set_name(sheet_name(name, &mut taken)).map_err(x)?;
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                let (r, c) = (r as u32, c as u16);
                match cell {
                    Cell::Empty => {}
                    Cell::Text(s) => {
                        ws.write_string(r, c, s).map_err(x)?;
                    }
                    Cell::Number(n) => {
                        ws.write_number(r, c, *n).map_err(x)?;
                    }
                    Cell::Bool(b) => {
                        ws.write_boolean(r, c, *b).map_err(x)?;
                    }
                }
            }
        }
    }
    wb.save(out).map_err(x)?;
    Ok(())
}

pub fn convert(input: &Path, target: &str, opts: &Options) -> Result<Vec<PathBuf>> {
    let sheets = read(input)?;
    if target == "xlsx" {
        let out = output_path(input, "", "xlsx", opts);
        let named: Vec<(String, &Rows)> =
            sheets.iter().map(|s| (s.name.clone(), &s.rows)).collect();
        write_xlsx(&named, &out)?;
        return Ok(vec![out]);
    }
    if target == "html" {
        let out = output_path(input, "", "html", opts);
        write_html(&sheets.iter().collect::<Vec<_>>(), &out)?;
        return Ok(vec![out]);
    }
    // One file per sheet for the flat formats.
    let many = sheets.len() > 1;
    let mut outputs = Vec::new();
    for sheet in &sheets {
        let suffix = if many {
            format!(" - {}", sheet.name.replace('/', "-"))
        } else {
            String::new()
        };
        let out = output_path(input, &suffix, target, opts);
        match target {
            "csv" => write_delimited(&sheet.rows, b',', &out)?,
            "tsv" => write_delimited(&sheet.rows, b'\t', &out)?,
            "json" => write_json(&sheet.rows, &out)?,
            "md" => write_markdown(&sheet.rows, &out)?,
            other => return Err(anyhow::anyhow!("cannot write {other} spreadsheets").into()),
        }
        outputs.push(out);
    }
    Ok(outputs)
}

/// Combine every sheet of every input into one workbook.
pub fn merge(inputs: &[PathBuf], opts: &Options) -> Result<PathBuf> {
    let mut all = Vec::new();
    for input in inputs {
        for sheet in read(input)? {
            let name = if ext(input) == "csv" || ext(input) == "tsv" {
                stem(input)
            } else {
                sheet.name.clone()
            };
            all.push((name, sheet.rows));
        }
    }
    let out = output_path(&inputs[0], "-merged", "xlsx", opts);
    let refs: Vec<(String, &Rows)> = all.iter().map(|(n, r)| (n.clone(), r)).collect();
    write_xlsx(&refs, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_round_trips_through_xlsx() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("data.csv");
        std::fs::write(&csv, "name;age;zip\nAda;36;01234\nLinus;54;\n").unwrap();
        let opts = Options::default();
        let xlsx = convert(&csv, "xlsx", &opts).unwrap().remove(0);
        let sheets = read(&xlsx).unwrap();
        assert_eq!(sheets[0].rows[1][1], Cell::Number(36.0));
        assert_eq!(sheets[0].rows[1][2], Cell::Text("01234".into()));
        let json = convert(&xlsx, "json", &opts).unwrap().remove(0);
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(json).unwrap()).unwrap();
        assert_eq!(v[0]["name"], "Ada");
        assert_eq!(v[1]["zip"], serde_json::Value::Null);
        for t in ["tsv", "md", "html"] {
            assert!(convert(&xlsx, t, &opts).unwrap()[0].exists());
        }
        let merged = merge(&[csv.clone(), xlsx.clone()], &opts).unwrap();
        assert_eq!(read(&merged).unwrap().len(), 2);
    }

    #[test]
    fn sheet_names_are_valid_and_unique() {
        let mut taken = Vec::new();
        assert_eq!(sheet_name("a/b", &mut taken), "ab");
        assert_eq!(sheet_name("AB", &mut taken), "AB 2");
        assert_eq!(sheet_name(&"x".repeat(40), &mut taken).len(), 31);
    }
}
