use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdfium_render::prelude::{PdfSearchDirection, PdfSearchOptions};

pub(super) enum SearchEvent {
    /// More matches, in document order, and how many pages have been scanned so far.
    Matches(Vec<fond_doc::PdfSearchMatch>, u16),
    Done,
}

type Sink = Rc<dyn Fn(SearchEvent)>;

thread_local! {
    static SINKS: RefCell<HashMap<u64, Sink>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A running search. Dropping it, or calling `cancel`, stops the thread and silences its sink.
pub(super) struct SearchHandle {
    id: u64,
    cancel: Arc<AtomicBool>,
}

impl SearchHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        SINKS.with(|s| s.borrow_mut().remove(&self.id));
    }
}

impl Drop for SearchHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Search every page of the PDF at `path` for `query` on a background thread (case-insensitive
/// substring, same as `fond_doc::search_document`), delivering matches to `sink` on the GTK
/// thread in batches so the first hit shows up before the whole book has been scanned.
pub(super) fn spawn(path: PathBuf, query: String, sink: Sink) -> SearchHandle {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    SINKS.with(|s| s.borrow_mut().insert(id, sink));
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let spawned = std::thread::Builder::new()
        .name("pdf-search".into())
        .spawn(move || run(id, path, query, flag));
    if let Err(e) = spawned {
        eprintln!("could not start the search thread: {e}");
        deliver(id, SearchEvent::Done);
    }
    SearchHandle { id, cancel }
}

fn deliver(id: u64, event: SearchEvent) {
    glib::MainContext::default().invoke(move || {
        let sink = SINKS.with(|s| s.borrow().get(&id).cloned());
        if let Some(sink) = sink {
            sink(event);
        }
    });
}

fn run(id: u64, path: PathBuf, query: String, cancel: Arc<AtomicBool>) {
    let _span = crate::perf::span(|| format!("search {query:?} (background)"));
    let finish = |cancelled: bool| {
        if !cancelled {
            deliver(id, SearchEvent::Done);
        }
    };
    if query.trim().is_empty() {
        return finish(false);
    }
    let Ok(pdfium) = crate::pdfium::get() else {
        return finish(false);
    };
    let Ok(doc) = pdfium.load_pdf_from_file(&path, None) else {
        return finish(false);
    };
    let options = PdfSearchOptions::new();
    let mut pending: Vec<fond_doc::PdfSearchMatch> = Vec::new();
    let mut last_sent: Option<Instant> = None;
    let total = doc.pages().len();
    for (index, page) in doc.pages().iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let Ok(text) = page.text() else { continue };
        let Ok(search) = text.search(&query, &options) else {
            continue;
        };
        for segments in search.iter(PdfSearchDirection::SearchForward) {
            let quads: Vec<[f64; 8]> = segments
                .iter()
                .map(|segment| {
                    let b = segment.bounds();
                    [
                        b.left().value as f64,
                        b.top().value as f64,
                        b.right().value as f64,
                        b.top().value as f64,
                        b.left().value as f64,
                        b.bottom().value as f64,
                        b.right().value as f64,
                        b.bottom().value as f64,
                    ]
                })
                .collect();
            if !quads.is_empty() {
                pending.push(fond_doc::PdfSearchMatch {
                    page: index as u16,
                    quads,
                });
            }
        }
        let due = last_sent.map_or(true, |t| t.elapsed() > Duration::from_millis(60));
        if !pending.is_empty() && due {
            last_sent = Some(Instant::now());
            deliver(
                id,
                SearchEvent::Matches(std::mem::take(&mut pending), index as u16 + 1),
            );
        }
    }
    if !pending.is_empty() {
        deliver(id, SearchEvent::Matches(pending, total));
    }
    finish(cancel.load(Ordering::Relaxed));
}
