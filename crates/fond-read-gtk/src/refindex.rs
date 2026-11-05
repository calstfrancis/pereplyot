//! A document's reference list and where the text cites each entry, built once in the background and
//! kept in the cache (it can always be rebuilt from the file).

use std::path::{Path, PathBuf};

use pereplyot_core::refs::{
    citations_in, cites, context_of, note_cites, parse_note, parse_reference, resolve_notes,
    split_entries, Note, ParsedRef,
};
use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::reflow::citations::all_entries;
use crate::reflow::extract::extract_page;

const VERSION: u32 = 3;
const TAIL_PAGES: u16 = 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cite {
    /// 0-based.
    pub page: u16,
    pub context: String,
    pub locator: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// 0-based page of the entry itself (the page that first cites it, for a note-only source).
    pub page: u16,
    pub bbox: [f32; 4],
    pub parsed: ParsedRef,
    pub cited: Vec<Cite>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RefIndex {
    #[serde(default)]
    pub version: u32,
    pub entries: Vec<Entry>,
    /// The sources came from footnotes, there being no bibliography to read.
    #[serde(default)]
    pub from_notes: bool,
}

fn cache_path(hash: &str) -> PathBuf {
    glib::user_cache_dir()
        .join("pereplyot")
        .join("refs")
        .join(format!("{hash}.json"))
}

pub fn load(hash: &str) -> Option<RefIndex> {
    match fsutil::load_checked(&cache_path(hash), |t| {
        serde_json::from_str::<RefIndex>(t).ok()
    }) {
        fsutil::Loaded::Ok(i) if i.version == VERSION => Some(i),
        _ => None,
    }
}

pub fn save(hash: &str, index: &RefIndex) {
    if let Ok(json) = serde_json::to_string(index) {
        let _ = fsutil::write_atomic(&cache_path(hash), json.as_bytes());
    }
}

/// Blocking: reads the whole file. Run it off the GTK thread.
pub fn build(path: &Path) -> Option<RefIndex> {
    let pdfium = crate::pdfium::get().ok()?;
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    let count = doc.pages().len();
    let first = count.saturating_sub(TAIL_PAGES);
    let tail: Vec<_> = (first..count)
        .filter_map(|i| extract_page(&doc, i))
        .collect();
    let pages: Vec<String> = (0..count)
        .map(|i| {
            doc.pages()
                .get(i)
                .ok()
                .and_then(|p| p.text().ok().map(|t| t.all()))
                .unwrap_or_default()
        })
        .collect();
    let located = all_entries(&tail);
    if std::env::var_os("PP_REFS_DEBUG").is_some() {
        for e in &located {
            eprintln!("RAW p{}: {}", e.page, e.text);
        }
    }
    let mut entries: Vec<Entry> = located
        .into_iter()
        .flat_map(|e| {
            split_entries(&e.text).into_iter().map(move |text| Entry {
                page: e.page,
                bbox: e.bbox,
                parsed: parse_reference(&text),
                cited: Vec::new(),
            })
        })
        .filter(|e| {
            // A heading ("References") has neither a year nor a number to be an entry by.
            (!e.parsed.families.is_empty() || e.parsed.label.is_some())
                && (e.parsed.year.is_some() || e.parsed.label.is_some())
        })
        .collect();
    let mut index = RefIndex {
        version: VERSION,
        ..Default::default()
    };
    if entries.len() >= 3 {
        scan_text(&mut entries, &pages);
        index.entries = entries;
    } else {
        index.entries = from_footnotes(&pages);
        index.from_notes = true;
    }
    Some(index)
}

const HEADINGS: [&str; 7] = [
    "references",
    "bibliography",
    "works cited",
    "literature cited",
    "selected bibliography",
    "sources",
    "reference list",
];

/// The text of the page before the bibliography's heading, if it has one; `None` once the reference
/// list itself has started, where "[1]" is an entry's label and not a citation.
fn before_bibliography(text: &str) -> Option<String> {
    let mut kept = String::new();
    for line in text.lines() {
        let l = line.trim().trim_end_matches(':').to_lowercase();
        if HEADINGS.contains(&l.as_str()) {
            return Some(kept);
        }
        kept.push_str(line);
        kept.push('\n');
    }
    None
}

fn scan_text(entries: &mut [Entry], pages: &[String]) {
    let bibliography_page = entries.iter().map(|e| e.page).min();
    for (p, text) in pages.iter().enumerate() {
        let text = match bibliography_page {
            Some(b) if p as u16 > b => continue,
            Some(b) if p as u16 == b => match before_bibliography(text) {
                Some(body) => body,
                None => continue,
            },
            _ => text.clone(),
        };
        // A citation can wrap across lines; read the page as running text.
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = text.as_str();
        for found in citations_in(text) {
            let context = context_of(text, found.start, found.end);
            for item in &found.items {
                let locator = match item {
                    pereplyot_core::refs::CiteItem::AuthorYear { locator, .. } => locator.clone(),
                    _ => None,
                };
                if let Some(entry) = entries.iter_mut().find(|e| cites(&e.parsed, item)) {
                    // The bibliography's own pages list entries, not citations of them.
                    if entry
                        .cited
                        .iter()
                        .any(|c| c.page as usize == p && c.context == context)
                    {
                        continue;
                    }
                    entry.cited.push(Cite {
                        page: p as u16,
                        context: context.clone(),
                        locator,
                    });
                }
            }
        }
    }
}

/// Footnote-style sources: lines that start with a note number and read as a citation.
fn from_footnotes(pages: &[String]) -> Vec<Entry> {
    let mut notes: Vec<(u16, String, Note)> = Vec::new();
    for (p, text) in pages.iter().enumerate() {
        for line in text.lines() {
            let t = line.trim();
            let digits = t.bytes().take_while(u8::is_ascii_digit).count();
            if !(1..=3).contains(&digits) || t.len() < digits + 8 {
                continue;
            }
            if let Some(note) = parse_note(t) {
                notes.push((p as u16, t.to_string(), note));
            }
        }
    }
    let parsed: Vec<Note> = notes.iter().map(|(_, _, n)| n.clone()).collect();
    let resolved = resolve_notes(&parsed);
    let mut entries: Vec<Entry> = Vec::new();
    for ((page, text, note), res) in notes.iter().zip(resolved) {
        let Some(res) = res else { continue };
        let at = entries.iter().position(|e| note_cites(&e.parsed, &res));
        let at = match at {
            Some(at) => at,
            None => {
                // Only a full note (with its publication details) can stand as the entry.
                if matches!(note, Note::Ibid { .. }) || !text.contains('(') {
                    continue;
                }
                let mut parsed = parse_reference(text);
                if parsed.families.is_empty() {
                    parsed.families = res.families.clone();
                }
                if parsed.title.is_empty() {
                    parsed.title = res.title.clone();
                }
                entries.push(Entry {
                    page: *page,
                    bbox: [0.0; 4],
                    parsed,
                    cited: Vec::new(),
                });
                entries.len() - 1
            }
        };
        entries[at].cited.push(Cite {
            page: *page,
            context: text.clone(),
            locator: res.page.clone(),
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_of(name: &str) -> Option<RefIndex> {
        crate::pdfium::get().ok()?;
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        build(&path)
    }

    #[test]
    fn the_fixture_bibliographies_are_listed_with_their_citations() {
        for name in [
            "cite-authoryear.pdf",
            "cite-numeric.pdf",
            "cite-narrative.pdf",
        ] {
            let Some(index) = index_of(name) else {
                eprintln!("no PDFium library; skipping");
                return;
            };
            eprintln!("{name}: {} entries", index.entries.len());
            for e in &index.entries {
                eprintln!(
                    "  {:?} {:?} {:?} cited {}",
                    e.parsed.families,
                    e.parsed.year,
                    e.parsed.title,
                    e.cited.len()
                );
            }
            assert!(
                index.entries.len() >= 3,
                "{name}: {:?}",
                index.entries.len()
            );
            assert!(
                index.entries.iter().filter(|e| !e.cited.is_empty()).count() >= 2,
                "{name}: too few cited entries"
            );
        }
    }
}
