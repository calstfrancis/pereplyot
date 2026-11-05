//! The synthesis matrix: sources down the side, your questions across the top, and in each cell the marks
//! from that source about that question. An empty cell is shown empty, because what a source says
//! nothing about is half of what the matrix is for.

use std::collections::BTreeMap;

use fond_annot::export::typst_escape;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// Marks carrying any of these tags go in this column.
    pub tags: Vec<String>,
}

/// What is stored of a matrix: its columns and your own notes, never copies of the quotes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Def {
    pub title: String,
    /// The Library shelf the sources come from; none means every document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shelf: Option<String>,
    pub columns: Vec<Column>,
    /// A note per cell, keyed "hash|column name".
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<String, String>,
}

pub fn note_key(hash: &str, column: &str) -> String {
    format!("{hash}|{column}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub hash: String,
    pub title: String,
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub hash: String,
    pub id: String,
    /// The printed page, as cited.
    pub label: String,
    pub text: String,
    pub note: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cell {
    pub marks: Vec<Mark>,
    pub note: String,
}

impl Cell {
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty() && self.note.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub source: Source,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matrix {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

fn normal(tag: &str) -> String {
    tag.trim().trim_start_matches('#').to_lowercase()
}

pub fn build(def: &Def, sources: &[Source], marks: &[Mark]) -> Matrix {
    let rows = sources
        .iter()
        .map(|source| {
            let cells = def
                .columns
                .iter()
                .map(|col| {
                    let wanted: Vec<String> = col.tags.iter().map(|t| normal(t)).collect();
                    let mut in_cell: Vec<Mark> = marks
                        .iter()
                        .filter(|m| m.hash == source.hash)
                        .filter(|m| m.tags.iter().any(|t| wanted.contains(&normal(t))))
                        .cloned()
                        .collect();
                    in_cell.sort_by(|a, b| a.label.cmp(&b.label));
                    Cell {
                        marks: in_cell,
                        note: def
                            .notes
                            .get(&note_key(&source.hash, &col.name))
                            .cloned()
                            .unwrap_or_default(),
                    }
                })
                .collect();
            Row {
                source: source.clone(),
                cells,
            }
        })
        .collect();
    Matrix {
        title: def.title.clone(),
        columns: def.columns.iter().map(|c| c.name.clone()).collect(),
        rows,
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cell_plain(cell: &Cell) -> String {
    let mut parts: Vec<String> = cell
        .marks
        .iter()
        .map(|m| {
            let loc = if m.label.is_empty() {
                String::new()
            } else {
                format!(" (p. {})", m.label)
            };
            format!("“{}”{loc}", one_line(&m.text))
        })
        .collect();
    if !cell.note.trim().is_empty() {
        parts.push(one_line(&cell.note));
    }
    parts.join(" ; ")
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn to_csv(m: &Matrix) -> String {
    let mut out = String::new();
    let head: Vec<String> = std::iter::once("Source".to_string())
        .chain(m.columns.iter().cloned())
        .map(|f| csv_field(&f))
        .collect();
    out.push_str(&head.join(","));
    out.push('\n');
    for row in &m.rows {
        let fields: Vec<String> = std::iter::once(row.source.title.clone())
            .chain(row.cells.iter().map(cell_plain))
            .map(|f| csv_field(&f))
            .collect();
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn to_markdown(m: &Matrix) -> String {
    let mut out = String::new();
    if !m.title.is_empty() {
        out.push_str(&format!("# {}\n\n", m.title));
    }
    out.push_str(&format!(
        "| Source | {} |\n",
        m.columns
            .iter()
            .map(|c| md_cell(c))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    out.push_str(&format!(
        "|{}|\n",
        vec!["---"; m.columns.len() + 1].join("|")
    ));
    for row in &m.rows {
        let cells: Vec<String> = row.cells.iter().map(|c| md_cell(&cell_plain(c))).collect();
        out.push_str(&format!(
            "| {} | {} |\n",
            md_cell(&row.source.title),
            cells.join(" | ")
        ));
    }
    out
}

/// A Typst table. With `cite`, a source that has a bibliography key is cited by it (`@key`), so the
/// document needs a bibliography; without, sources are named.
pub fn to_typst(m: &Matrix, cite: bool) -> String {
    let mut out = String::new();
    if !m.title.is_empty() {
        out.push_str(&format!("= {}\n\n", typst_escape(&m.title)));
    }
    let cols = m.columns.len() + 1;
    out.push_str(&format!(
        "#table(\n  columns: (auto, {}),\n  inset: 6pt,\n  align: top,\n",
        vec!["1fr"; m.columns.len()].join(", ")
    ));
    out.push_str("  [*Source*], ");
    for c in &m.columns {
        out.push_str(&format!("[*{}*], ", typst_escape(c)));
    }
    out.push('\n');
    for row in &m.rows {
        let who = match (&row.source.key, cite) {
            (Some(k), true) if !k.is_empty() => format!("@{k}"),
            _ => typst_escape(&row.source.title),
        };
        out.push_str(&format!("  [{who}],\n"));
        for cell in &row.cells {
            if cell.is_empty() {
                out.push_str("  table.cell(fill: luma(240))[],\n");
                continue;
            }
            let mut body = String::new();
            for mk in &cell.marks {
                let loc = if mk.label.is_empty() {
                    String::new()
                } else {
                    format!(" (p. {})", typst_escape(&mk.label))
                };
                body.push_str(&format!("“{}”{loc} \\ ", typst_escape(&one_line(&mk.text))));
            }
            if !cell.note.trim().is_empty() {
                body.push_str(&format!("_{}_", typst_escape(&one_line(&cell.note))));
            }
            let body = body.trim_end_matches(" \\ ");
            out.push_str(&format!("  [{body}],\n"));
        }
    }
    let _ = cols;
    out.push_str(")\n");
    out
}

#[cfg(test)]
mod tests;
