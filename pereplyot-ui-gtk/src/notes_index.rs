//! Search across every annotation stored locally, for the launcher's Notes tab.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::library::Library;
use fond_read_gtk::history::DocKind;

fn annotations_dir() -> PathBuf {
    glib::user_data_dir().join("pereplyot").join("annotations")
}

#[derive(Debug, Clone)]
pub struct NoteHit {
    pub path: Option<PathBuf>,
    pub doc_title: String,
    pub location: String,
    pub page: Option<u32>,
    pub annotation_id: String,
    pub color: Option<String>,
    pub snippet: Option<String>,
    pub note: Option<String>,
}

pub const MAX_HITS: usize = 300;

/// Title/path per content hash, from History and the Library.
fn known_documents(library: &Library) -> HashMap<String, (String, PathBuf, DocKind)> {
    let mut map = HashMap::new();
    for e in fond_read_gtk::history::load() {
        map.insert(e.hash, (e.title, e.path, e.kind));
    }
    for e in library.entries() {
        map.insert(e.hash.clone(), (e.title.clone(), e.path.clone(), e.kind));
    }
    map
}

fn matches(hit: &NoteHit, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    [
        Some(hit.doc_title.as_str()),
        hit.snippet.as_deref(),
        hit.note.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|t| t.to_lowercase().contains(needle))
}

/// Annotations matching `query` (case-insensitive over title, quote and note), optionally only
/// those in colour `color_hex`, newest documents first.
pub fn search(library: &Library, query: &str, color_hex: Option<&str>) -> Vec<NoteHit> {
    let needle = query.trim().to_lowercase();
    let docs = known_documents(library);
    let Ok(dir) = fs::read_dir(annotations_dir()) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(hash) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(sidecar) = fond_annot::AnnotationSidecar::parse(&text, &path) else {
            continue;
        };
        let doc = docs.get(&hash);
        let doc_title = doc
            .map(|d| d.0.clone())
            .unwrap_or_else(|| format!("Unknown document ({})", &hash[..hash.len().min(8)]));
        for a in sidecar.annotations {
            if let Some(want) = color_hex {
                if !a
                    .color
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(want))
                {
                    continue;
                }
            }
            let location = match (a.page, a.chapter.as_deref()) {
                (Some(p), _) => format!("p. {p}"),
                (None, Some(c)) => std::path::Path::new(c)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| c.to_string()),
                _ => String::new(),
            };
            let hit = NoteHit {
                path: doc.map(|d| d.1.clone()),
                doc_title: doc_title.clone(),
                location,
                page: a.page,
                annotation_id: a.id,
                color: a.color,
                snippet: a.snippet,
                note: a.note,
            };
            if matches(&hit, &needle) {
                hits.push(hit);
            }
        }
    }
    hits.sort_by(|a, b| {
        a.doc_title
            .to_lowercase()
            .cmp(&b.doc_title.to_lowercase())
            .then(a.page.cmp(&b.page))
    });
    hits.truncate(MAX_HITS);
    hits
}
