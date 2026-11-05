//! Saved synthesis matrices (`matrices/<stem>.json`): their columns and your notes, never copies of quotes.

use std::path::PathBuf;

use fond_read_gtk::fsutil;
use pereplyot_core::matrix::Def;

fn dir() -> PathBuf {
    crate::paths::data_dir().join("matrices")
}

fn path_of(stem: &str) -> PathBuf {
    dir().join(format!("{stem}.json"))
}

/// A file name for a matrix called `title`.
pub fn stem_of(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "matrix".to_string()
    } else {
        s
    }
}

/// Every saved matrix, by stem and title.
pub fn list() -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = entries
        .flatten()
        .filter_map(|e| {
            let stem = e.path().file_stem()?.to_string_lossy().into_owned();
            let def = load(&stem)?;
            Some((stem, def.title))
        })
        .collect();
    out.sort_by_key(|(_, t)| t.to_lowercase());
    out
}

pub fn load(stem: &str) -> Option<Def> {
    match fsutil::load_checked(&path_of(stem), |t| serde_json::from_str::<Def>(t).ok()) {
        fsutil::Loaded::Ok(d) => Some(d),
        fsutil::Loaded::Corrupt {
            recovered_from_backup,
            ..
        } => recovered_from_backup,
        fsutil::Loaded::Missing => None,
    }
}

pub fn save(stem: &str, def: &Def) {
    if let Ok(json) = serde_json::to_string_pretty(def) {
        let _ = fsutil::write_atomic(&path_of(stem), json.as_bytes());
    }
}

/// "Method; Date of Q: q-date, dating" → columns; a column with no tags is tagged by its own name.
pub fn parse_columns(text: &str) -> Vec<pereplyot_core::matrix::Column> {
    text.split(';')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let (name, tags) = match part.split_once(':') {
                Some((n, t)) => (
                    n.trim(),
                    t.split(',')
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty())
                        .collect(),
                ),
                None => (
                    part,
                    vec![part
                        .to_lowercase()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join("-")],
                ),
            };
            (!name.is_empty()).then(|| pereplyot_core::matrix::Column {
                name: name.to_string(),
                tags,
            })
        })
        .collect()
}

pub fn columns_text(def: &Def) -> String {
    def.columns
        .iter()
        .map(|c| format!("{}: {}", c.name, c.tags.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_read_with_or_without_tags() {
        let cols = parse_columns("Method; Date of Q: q-date, dating ;;");
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].tags, ["method"]);
        assert_eq!(cols[1].name, "Date of Q");
        assert_eq!(cols[1].tags, ["q-date", "dating"]);
        let def = Def {
            columns: cols,
            ..Default::default()
        };
        assert_eq!(parse_columns(&columns_text(&def)), def.columns);
    }

    #[test]
    fn a_title_becomes_a_file_name() {
        assert_eq!(stem_of("Q & the sources!"), "q-the-sources");
        assert_eq!(stem_of("???"), "matrix");
    }
}
