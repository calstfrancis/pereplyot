use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::page_geom::PageGeom;

/// What reading every page of a document yields: its geometry and its printed page label.
pub(super) struct Scan {
    pub geoms: Vec<Option<PageGeom>>,
    pub labels: Vec<Option<String>>,
}

type Sink = Rc<dyn Fn(Scan)>;

thread_local! {
    static SINKS: RefCell<HashMap<u64, Sink>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Read every page's size and label on a background thread, then call `sink` once on the GTK
/// thread. Doing this per page costs about 0.1 ms each, which added up to over 100 ms of frozen
/// window when opening a 600-page book.
pub(super) fn spawn(path: PathBuf, sink: Sink) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    SINKS.with(|s| s.borrow_mut().insert(id, sink));
    let spawned = std::thread::Builder::new()
        .name("pdf-scan".into())
        .spawn(move || {
            let _span = crate::perf::span(|| "document scan (background)".to_string());
            crate::perf::mark_rss("scan starting");
            let scan = read(&path);
            crate::perf::mark_rss("scan finished");
            glib::MainContext::default().invoke(move || {
                let sink = SINKS.with(|s| s.borrow_mut().remove(&id));
                if let (Some(sink), Some(scan)) = (sink, scan) {
                    sink(scan);
                }
            });
        });
    if let Err(e) = spawned {
        eprintln!("could not start the document scan: {e}");
    }
}

fn read(path: &PathBuf) -> Option<Scan> {
    let pdfium = crate::pdfium::get().ok()?;
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    let count = doc.pages().len();
    let mut geoms = Vec::with_capacity(count as usize);
    let mut labels = Vec::with_capacity(count as usize);
    for index in 0..count {
        geoms.push(PageGeom::read_doc(&doc, index));
        labels.push(
            doc.pages()
                .get(index)
                .ok()
                .and_then(|page| page.label().map(|s| s.to_string())),
        );
    }
    Some(Scan { geoms, labels })
}
