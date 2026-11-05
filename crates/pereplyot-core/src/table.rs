//! A table read out of the words in a clipped region: rows by where the lines sit, columns by the vertical
//! channels no row writes across. The picture stays the original; this is the data beside it.

use fond_annot::export::typst_escape;

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// The cells of the table the words form, or `None` when they do not form one (a single column or row).
pub fn grid(words: &[Word]) -> Option<Vec<Vec<String>>> {
    let words: Vec<&Word> = words.iter().filter(|w| !w.text.trim().is_empty()).collect();
    if words.len() < 4 {
        return None;
    }
    let height = median(words.iter().map(|w| (w.y1 - w.y0).abs()).collect()).max(1.0);

    // Columns: merge every word's horizontal extent, and cut where a clear channel remains.
    let mut spans: Vec<(f32, f32)> = words
        .iter()
        .map(|w| (w.x0.min(w.x1), w.x0.max(w.x1)))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut columns: Vec<(f32, f32)> = Vec::new();
    for (a, b) in spans {
        match columns.last_mut() {
            Some(last) if a - last.1 < 1.3 * height => last.1 = last.1.max(b),
            _ => columns.push((a, b)),
        }
    }
    if columns.len() < 2 {
        return None;
    }

    // Rows: words whose centres sit on about the same line.
    let mut order: Vec<&Word> = words.clone();
    order.sort_by(|a, b| ((a.y0 + a.y1) / 2.0).total_cmp(&((b.y0 + b.y1) / 2.0)));
    let mut rows: Vec<Vec<&Word>> = Vec::new();
    for w in order {
        let centre = (w.y0 + w.y1) / 2.0;
        match rows.last_mut() {
            Some(row) => {
                let mean = row.iter().map(|r| (r.y0 + r.y1) / 2.0).sum::<f32>() / row.len() as f32;
                if (centre - mean).abs() <= 0.6 * height {
                    row.push(w);
                } else {
                    rows.push(vec![w]);
                }
            }
            None => rows.push(vec![w]),
        }
    }
    if rows.len() < 2 {
        return None;
    }

    let mut cells: Vec<Vec<String>> = rows
        .iter_mut()
        .map(|row| {
            row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
            let mut cells = vec![String::new(); columns.len()];
            for w in row.iter() {
                let centre = (w.x0 + w.x1) / 2.0;
                let at = columns
                    .iter()
                    .position(|(a, b)| centre >= *a && centre <= *b)
                    .unwrap_or(0);
                if !cells[at].is_empty() {
                    cells[at].push(' ');
                }
                cells[at].push_str(w.text.trim());
            }
            cells
        })
        .collect();
    // Drop columns nothing was written in.
    let keep: Vec<bool> = (0..columns.len())
        .map(|c| cells.iter().any(|r| !r[c].is_empty()))
        .collect();
    for row in &mut cells {
        let mut k = keep.iter();
        row.retain(|_| *k.next().unwrap_or(&true));
    }
    (cells.first().map_or(0, Vec::len) >= 2).then_some(cells)
}

pub fn to_tsv(cells: &[Vec<String>]) -> String {
    cells
        .iter()
        .map(|r| {
            r.iter()
                .map(|c| c.replace(['\t', '\n'], " "))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn to_csv(cells: &[Vec<String>]) -> String {
    cells
        .iter()
        .map(|r| r.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// A Typst `#table`, the first row as the header.
pub fn to_typst(cells: &[Vec<String>]) -> String {
    let n = cells.first().map_or(0, Vec::len);
    let mut out = format!("#table(\n  columns: {n},\n  inset: 5pt,\n");
    for (i, row) in cells.iter().enumerate() {
        let line: Vec<String> = row
            .iter()
            .map(|c| {
                if i == 0 {
                    format!("[*{}*]", typst_escape(c))
                } else {
                    format!("[{}]", typst_escape(c))
                }
            })
            .collect();
        out.push_str(&format!("  {},\n", line.join(", ")));
    }
    out.push_str(")\n");
    out
}

#[cfg(test)]
mod tests;
