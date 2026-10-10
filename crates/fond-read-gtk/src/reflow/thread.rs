//! Laying a document out on a background thread, handing finished items to the GTK thread as
//! they are ready so the first pages can be read while the rest are still being worked on.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use super::extract::extract_page;
use super::layout::{body_size, detect_furniture, Assembler};
use super::model::{Item, RawPage};
use super::ocr;

pub enum ReflowEvent {
    /// More of the flow, in order, with how many pages are laid out so far.
    Items(Vec<Item>, u16),
    Done,
}

type Sink = Rc<dyn Fn(ReflowEvent)>;

thread_local! {
    static SINKS: RefCell<HashMap<u64, Sink>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A running layout. Dropping it stops the thread and silences its sink.
pub struct ReflowHandle {
    id: u64,
    cancel: Arc<AtomicBool>,
}

impl Drop for ReflowHandle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        SINKS.with(|s| s.borrow_mut().remove(&self.id));
    }
}

/// Start laying the document at `path` out. With `recognise`, pages that have no text layer are
/// run through Tesseract (when it is installed) and laid out like any other.
pub fn spawn(path: PathBuf, recognise: bool, sink: Sink) -> ReflowHandle {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    SINKS.with(|s| s.borrow_mut().insert(id, sink));
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let spawned = std::thread::Builder::new()
        .name("pdf-reflow".into())
        .spawn(move || run(id, path, recognise, flag));
    if let Err(e) = spawned {
        eprintln!("could not start the reflow thread: {e}");
        deliver(id, ReflowEvent::Done);
    }
    ReflowHandle { id, cancel }
}

fn deliver(id: u64, event: ReflowEvent) {
    glib::MainContext::default().invoke(move || {
        let sink = SINKS.with(|s| s.borrow().get(&id).cloned());
        if let Some(sink) = sink {
            sink(event);
        }
    });
}

/// How many pages from the start, and spread through the rest, are read first to find the running
/// headers and the body size.
const SAMPLE_HEAD: u16 = 30;
const SAMPLE_SPREAD: u16 = 30;

/// A page with fewer words than this has no usable text layer.
const MIN_WORDS: usize = 3;

fn run(id: u64, path: PathBuf, recognise: bool, cancel: Arc<AtomicBool>) {
    let _span = crate::perf::span(|| "reflow (background)".to_string());
    let Ok(pdfium) = crate::pdfium::get() else {
        return deliver(id, ReflowEvent::Done);
    };
    let Ok(doc) = pdfium.load_pdf_from_file(&path, None) else {
        return deliver(id, ReflowEvent::Done);
    };
    let total = doc.pages().len();
    let recogniser = recognise.then(ocr::tesseract).flatten();
    let cache_dir = ocr::cache_dir(&path);
    let language = ocr::language();
    let read_page = |index: u16| -> Option<RawPage> {
        let page = extract_page(&doc, index).filter(|p| p.words.len() >= MIN_WORDS);
        match (&page, &recogniser) {
            (None, Some(_)) if index == 0 => None,
            (None, Some(program)) => ocr::ocr_page(&doc, index, program, &language, &cache_dir),
            (None, None) => ocr::cached_page(&doc, index, &language, &cache_dir),
            _ => page,
        }
    };
    let mut cache: HashMap<u16, RawPage> = HashMap::new();
    let mut sample_ids: Vec<u16> = (0..total.min(SAMPLE_HEAD)).collect();
    if total > SAMPLE_HEAD {
        let rest = total - SAMPLE_HEAD;
        let step = (rest / SAMPLE_SPREAD).max(1);
        sample_ids.extend((SAMPLE_HEAD..total).step_by(step as usize));
    }
    let mut sample: Vec<RawPage> = Vec::new();
    for index in sample_ids {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if let Some(page) = read_page(index) {
            sample.push(page.clone());
            cache.insert(index, page);
        }
    }
    let endnotes = super::endnotes::find(total, &|i| extract_page(&doc, i));
    let notes_pages = endnotes.as_ref().map(|e| e.pages.clone());
    let mut asm =
        Assembler::new(detect_furniture(&sample), body_size(&sample)).with_endnotes(endnotes);
    drop(sample);
    for index in 0..total {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if notes_pages.as_ref().is_some_and(|r| r.contains(&index)) {
            continue;
        }
        let page = cache.remove(&index).or_else(|| extract_page(&doc, index));
        if let Some(page) = page {
            asm.push_page(&page);
        } else if index == 0 {
            if let Some(geom) = crate::page_geom::PageGeom::read_doc(&doc, 0) {
                let (w, h) = geom.display_size();
                asm.push_title_page(0, w, h);
            }
        }
        let ready = asm.take_ready(index);
        if !ready.is_empty() {
            deliver(id, ReflowEvent::Items(ready, index));
        }
    }
    let rest = asm.finish();
    if !rest.is_empty() {
        deliver(id, ReflowEvent::Items(rest, total));
    }
    deliver(id, ReflowEvent::Done);
}
