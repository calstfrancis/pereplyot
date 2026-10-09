//! Keeps the text index (`fond_read_gtk::index`) up to date with the documents Pereplyot knows:
//! everything in History and the Library, indexed one document at a time off the main thread.

use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;

use fond_read_gtk::history::DocKind;
use fond_read_gtk::index::{self, DocEntry};

use crate::library::Library;

#[derive(Debug, Clone)]
pub struct Source {
    pub hash: String,
    pub path: PathBuf,
    pub kind: DocKind,
    pub title: String,
}

/// The documents worth indexing: History and the Library, those still where they were.
pub fn sources(library: &Library) -> Vec<Source> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for e in fond_read_gtk::history::load() {
        if seen.insert(e.hash.clone()) && e.path.is_file() {
            out.push(Source {
                hash: e.hash,
                path: e.path,
                kind: e.kind,
                title: e.title,
            });
        }
    }
    for e in library.entries() {
        if seen.insert(e.hash.clone()) && e.path.is_file() {
            out.push(Source {
                hash: e.hash.clone(),
                path: e.path.clone(),
                kind: e.kind,
                title: e.title.clone(),
            });
        }
    }
    out
}

thread_local! {
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// Index whatever is missing, calling `progress(done, total)` as it goes and `finished(any
/// changed)` at the end. A run already going makes this a no-op.
pub fn refresh(
    sources: Vec<Source>,
    progress: impl Fn(usize, usize) + 'static,
    finished: impl Fn(bool) + 'static,
) {
    if RUNNING.with(|r| r.replace(true)) {
        return;
    }
    glib::spawn_future_local(async move {
        let dir = index::dir();
        let wanted: Vec<_> = sources
            .iter()
            .map(|s| (s.hash.clone(), s.path.clone(), s.kind, s.title.clone()))
            .collect();
        let todo: Vec<_> = index::missing(&dir, &wanted).into_iter().cloned().collect();
        let total = todo.len();
        for (i, (hash, path, kind, title)) in todo.into_iter().enumerate() {
            progress(i, total);
            let dir = dir.clone();
            let _ = gio::spawn_blocking(move || {
                let (units, labels, chapters) = index::extract(&path, kind).unwrap_or_default();
                let entry = DocEntry {
                    hash,
                    title,
                    path,
                    kind,
                    labels,
                    chapters,
                };
                let _ = index::store(&dir, entry, &units);
            })
            .await;
        }
        let keep: HashSet<String> = wanted.iter().map(|w| w.0.clone()).collect();
        let dropped = {
            let dir = dir.clone();
            gio::spawn_blocking(move || {
                let before = index::load_catalogue(&dir).len();
                index::retain(&dir, &|e| keep.contains(&e.hash));
                before != index::load_catalogue(&dir).len()
            })
            .await
            .unwrap_or(false)
        };
        RUNNING.with(|r| r.set(false));
        progress(total, total);
        finished(total > 0 || dropped);
    });
}

/// Throw the cache away so the next refresh reads every document again.
pub fn clear() {
    let _ = std::fs::remove_dir_all(index::dir());
}
