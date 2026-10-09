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
    let cache_dir = crate::reflow::ocr::cache_dir(&path);
    let mut pending: Vec<fond_doc::PdfSearchMatch> = Vec::new();
    let mut last_sent: Option<Instant> = None;
    let total = doc.pages().len();
    for (index, page) in doc.pages().iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let Ok(text) = page.text() else { continue };
        if text.is_empty() {
            if let Some(found) = recognised_matches(&doc, index as u16, &query, &cache_dir) {
                pending.extend(found);
            }
            continue;
        }
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

/// Matches on a scanned page that was recognised earlier, as quads in PDF user space. `None` when
/// there is nothing recognised for it.
fn recognised_matches(
    doc: &pdfium_render::prelude::PdfDocument<'_>,
    index: u16,
    query: &str,
    cache: &std::path::Path,
) -> Option<Vec<fond_doc::PdfSearchMatch>> {
    let raw = crate::reflow::ocr::cached_page(doc, index, "eng", cache)?;
    let geom = crate::page_geom::PageGeom::read_doc(doc, index)?;
    let (w, h) = geom.display_size();
    let (w, h) = (w as f64, h as f64);
    let spans = recognised_hits(&raw.words, query);
    Some(
        spans
            .into_iter()
            .map(|rects| fond_doc::PdfSearchMatch {
                page: index,
                quads: rects
                    .iter()
                    .map(|b| {
                        let corner = |x: f32, y: f32| geom.px_to_pdf(x as f64, y as f64, w, h);
                        let (tl, tr) = (corner(b[0], b[1]), corner(b[2], b[1]));
                        let (bl, br) = (corner(b[0], b[3]), corner(b[2], b[3]));
                        [tl.0, tl.1, tr.0, tr.1, bl.0, bl.1, br.0, br.1]
                    })
                    .collect(),
            })
            .collect(),
    )
}

/// Where `query` occurs in a page's recognised words (compared ignoring case and spacing, as the
/// text layer is): for each hit, one rectangle per line of words it covers, in page points.
fn recognised_hits(words: &[crate::reflow::model::Word], query: &str) -> Vec<Vec<[f32; 4]>> {
    let needle: String = query
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if needle.is_empty() {
        return Vec::new();
    }
    let mut text = String::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for w in words {
        if !text.is_empty() {
            text.push(' ');
        }
        let start = text.len();
        text.push_str(&w.text.to_lowercase());
        spans.push((start, text.len()));
    }
    let mut hits = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(&needle) {
        let (a, b) = (from + found, from + found + needle.len());
        let covered: Vec<&crate::reflow::model::Word> = words
            .iter()
            .zip(&spans)
            .filter(|(_, (s, e))| *s < b && *e > a)
            .map(|(w, _)| w)
            .collect();
        let mut rects: Vec<[f32; 4]> = Vec::new();
        for w in covered {
            match rects.last_mut() {
                Some(r) if w.centre_y() >= r[1] && w.centre_y() <= r[3] => {
                    r[0] = r[0].min(w.x0);
                    r[1] = r[1].min(w.y0);
                    r[2] = r[2].max(w.x1);
                    r[3] = r[3].max(w.y1);
                }
                _ => rects.push([w.x0, w.y0, w.x1, w.y1]),
            }
        }
        if !rects.is_empty() {
            hits.push(rects);
        }
        from = b.max(from + 1);
        while !text.is_char_boundary(from) {
            from += 1;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflow::model::Word;

    fn word(text: &str, x: f32, y: f32) -> Word {
        Word {
            text: text.into(),
            x0: x,
            y0: y,
            x1: x + 30.0,
            y1: y + 10.0,
            size: 10.0,
            bold: false,
            italic: false,
        }
    }

    #[test]
    fn a_phrase_across_a_line_break_gives_one_rectangle_per_line() {
        let words = [
            word("The", 10.0, 10.0),
            word("Hidden", 50.0, 10.0),
            word("Hand", 10.0, 30.0),
            word("moves.", 50.0, 30.0),
        ];
        let hits = recognised_hits(&words, "hidden  HAND");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].len(), 2);
        assert_eq!(hits[0][0], [50.0, 10.0, 80.0, 20.0]);
        assert_eq!(hits[0][1], [10.0, 30.0, 40.0, 40.0]);
        assert!(recognised_hits(&words, "absent").is_empty());
    }

    #[test]
    fn a_recognised_scan_is_searchable_with_quads_on_the_page() {
        let Ok(pdfium) = crate::pdfium::get() else {
            eprintln!("no PDFium library; skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("pereplyot-ocr-search-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("eng-0.tsv"),
            "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
5\t1\t1\t1\t1\t1\t300\t600\t300\t60\t95\tHello\n\
5\t1\t1\t1\t1\t2\t650\t600\t310\t60\t95\tworld\n",
        )
        .unwrap();
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/blank.pdf");
        let doc = pdfium.load_pdf_from_file(&path, None).unwrap();
        let found = recognised_matches(&doc, 0, "WORLD", &dir).expect("a recognised page");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].quads.len(), 1);
        let q = found[0].quads[0];
        let page = doc.pages().get(0).unwrap();
        assert!(q[0] > 0.0 && (q[0] as f32) < page.width().value);
        assert!(q[1] > 0.0 && (q[1] as f32) < page.height().value);
        assert!(recognised_matches(&doc, 1, "world", &dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
