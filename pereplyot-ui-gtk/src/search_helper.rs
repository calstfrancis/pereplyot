//! `pereplyot-gtk --search-json QUERY…`: search the documents Pereplyot knows and the
//! annotations made in them, and print the hits as one line of JSON, for the Porog launcher
//! (`flatpak run --command=pereplyot-search …`). No window, no network and no writes.
//!
//! Each hit carries the `args` that reopen it, exactly as Porog hands them back to
//! `flatpak run io.github.calstfrancis.Pereplyot`: the file, plus `--annotation=ID` for an
//! annotation. Failures are printed as `{"schema":1,"error":"…"}` with exit code 0.

use std::path::Path;

use crate::library::Library;
use crate::notes_index::{self, Filter};

/// Version of the JSON this prints. Bumped only for a change an older Porog cannot ignore.
const SCHEMA: u32 = 1;
/// Most hits one answer carries; Porog shows a handful.
const LIMIT: usize = 8;
/// Most documents among them, so a common word does not crowd the annotations out.
const DOCUMENT_LIMIT: usize = 4;
/// Longest title a hit carries; a long highlighted passage is cut at a word.
const TITLE_CHARS: usize = 90;

pub fn run(query: &[String]) {
    println!("{}", answer(query));
}

fn answer(query: &[String]) -> String {
    let text = query.join(" ");
    let text = text.trim();
    if text.is_empty() {
        return serde_json::json!({ "schema": SCHEMA, "hits": [] }).to_string();
    }
    serde_json::json!({ "schema": SCHEMA, "hits": hits(text) }).to_string()
}

fn hits(text: &str) -> Vec<serde_json::Value> {
    let library = Library::load();
    let needle = text.to_lowercase();

    let mut documents: Vec<_> = notes_index::known_documents(&library)
        .into_values()
        .filter(|(title, path, _)| title.to_lowercase().contains(&needle) && path.exists())
        .collect();
    documents.sort_by_key(|(title, _, _)| title.to_lowercase());
    documents.truncate(DOCUMENT_LIMIT);

    let mut out: Vec<serde_json::Value> = documents
        .into_iter()
        .map(|(title, path, _)| hit(&title, "Document", &path, None))
        .collect();

    let filter = Filter {
        text: text.to_string(),
        limit: Some(LIMIT),
        ..Filter::default()
    };
    for note in notes_index::search_with(&library, &filter) {
        if out.len() >= LIMIT {
            break;
        }
        let Some(path) = note.path.as_ref().filter(|p| p.exists()) else {
            continue;
        };
        let words = note
            .note
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .or(note.snippet.as_deref())
            .unwrap_or("");
        let detail = [note.doc_title.as_str(), note.location.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        out.push(hit(words, &detail, path, Some(&note.annotation_id)));
    }
    out
}

fn hit(title: &str, detail: &str, path: &Path, annotation: Option<&str>) -> serde_json::Value {
    let mut args = vec![path.to_string_lossy().into_owned()];
    if let Some(id) = annotation {
        args.push(format!("--annotation={id}"));
    }
    serde_json::json!({ "title": shorten(title), "detail": detail, "args": args })
}

/// One line, at most [`TITLE_CHARS`] long, cut at a word with an ellipsis.
fn shorten(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= TITLE_CHARS {
        return one_line;
    }
    let cut: String = one_line.chars().take(TITLE_CHARS).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_kept_and_long_text_is_cut_at_a_word() {
        assert_eq!(shorten("A  short\nline"), "A short line");
        let long = "word ".repeat(40);
        let cut = shorten(&long);
        assert!(cut.ends_with("word…"));
        assert!(cut.chars().count() <= TITLE_CHARS + 1);
    }

    #[test]
    fn an_annotation_hit_reopens_the_file_at_the_annotation() {
        let h = hit("A quote", "Doc · p. 3", Path::new("/a/b.pdf"), Some("hl-1"));
        assert_eq!(h["args"], serde_json::json!(["/a/b.pdf", "--annotation=hl-1"]));
        let d = hit("Doc", "Document", Path::new("/a/b.pdf"), None);
        assert_eq!(d["args"], serde_json::json!(["/a/b.pdf"]));
    }
}
