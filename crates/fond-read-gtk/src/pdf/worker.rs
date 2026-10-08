use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// What a rendered page depends on. Two jobs with the same key produce the same pixels.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) struct RenderKey {
    pub page: u16,
    /// Device pixels (logical width × the widget's scale factor) of the unrotated page.
    pub width: u32,
    pub rotation: u16,
    pub invert: bool,
    /// Hash of the overlay below, so a changed highlight is a different picture.
    pub overlay: u64,
}

impl RenderKey {
    fn same_view(&self, other: &RenderKey) -> bool {
        self.width == other.width && self.rotation == other.rotation && self.invert == other.invert
    }
}

/// The marks drawn over a page, already in the page's displayed point space.
#[derive(Clone, Default)]
pub(super) struct Overlay {
    pub page_pts: (f32, f32),
    pub marks: Vec<(fond_doc::MarkupKind, Vec<[f64; 8]>, [u8; 4])>,
    pub highlights: Vec<(Vec<[f64; 8]>, [u8; 4])>,
}

pub(super) struct Job {
    pub key: RenderKey,
    pub overlay: Overlay,
    /// Lower runs first; distance from the page being read.
    pub priority: u32,
}

pub(super) struct Rendered {
    pub key: RenderKey,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

struct Queue {
    jobs: Vec<Job>,
    in_flight: Option<RenderKey>,
    shutdown: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

type Sink = Rc<dyn Fn(Rendered)>;

thread_local! {
    static SINKS: RefCell<HashMap<u64, Sink>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Renders pages on its own thread, newest view first, and delivers each result back on the GTK
/// thread. The thread opens its own copy of the document from the file path, so the reader's
/// main-thread document is never shared.
pub(super) struct RenderWorker {
    id: u64,
    shared: Arc<Shared>,
}

impl RenderWorker {
    pub fn spawn(path: PathBuf) -> RenderWorker {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                jobs: Vec::new(),
                in_flight: None,
                shutdown: false,
            }),
            wake: Condvar::new(),
        });
        let for_thread = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("pdf-render".into())
            .spawn(move || run(id, path, for_thread));
        if let Err(e) = spawned {
            eprintln!("could not start the render thread: {e}");
        }
        RenderWorker { id, shared }
    }

    /// Where finished pages go. Called on the GTK thread.
    pub fn set_sink(&self, sink: Sink) {
        SINKS.with(|s| s.borrow_mut().insert(self.id, sink));
    }

    pub fn submit(&self, job: Job) {
        let mut q = self.shared.queue.lock().unwrap();
        if q.in_flight.as_ref() == Some(&job.key) || q.jobs.iter().any(|j| j.key == job.key) {
            return;
        }
        // A new view (zoom, rotation, scale, colours) makes everything queued for the old one
        // pointless, and a newer picture of the same page replaces an older one.
        q.jobs
            .retain(|j| j.key.same_view(&job.key) && j.key.page != job.key.page);
        q.jobs.push(job);
        self.shared.wake.notify_one();
    }
}

impl Drop for RenderWorker {
    fn drop(&mut self) {
        SINKS.with(|s| s.borrow_mut().remove(&self.id));
        let mut q = self.shared.queue.lock().unwrap();
        q.shutdown = true;
        q.jobs.clear();
        self.shared.wake.notify_one();
    }
}

fn run(id: u64, path: PathBuf, shared: Arc<Shared>) {
    let Ok(pdfium) = crate::pdfium::get() else {
        return;
    };
    let doc = match pdfium.load_pdf_from_file(&path, None) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("render thread could not open {}: {e}", path.display());
            return;
        }
    };
    loop {
        let job = {
            let mut q = shared.queue.lock().unwrap();
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(i) = q
                    .jobs
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, j)| j.priority)
                    .map(|(i, _)| i)
                {
                    let job = q.jobs.swap_remove(i);
                    q.in_flight = Some(job.key.clone());
                    break job;
                }
                q = shared.wake.wait(q).unwrap();
            }
        };
        let span = crate::perf::span(|| format!("render page {}", job.key.page + 1));
        let rendered = render(&doc, job);
        drop(span);
        shared.queue.lock().unwrap().in_flight = None;
        if let Some(done) = rendered {
            glib::MainContext::default().invoke(move || {
                let sink = SINKS.with(|s| s.borrow().get(&id).cloned());
                if let Some(sink) = sink {
                    sink(done);
                }
            });
        }
    }
}

fn render(doc: &pdfium_render::prelude::PdfDocument<'_>, job: Job) -> Option<Rendered> {
    use pdfium_render::prelude::{PdfRenderConfig, Pixels};
    let Job { key, overlay, .. } = job;
    let page = doc.pages().get(key.page).ok()?;
    let config = PdfRenderConfig::new().set_target_width(key.width.max(1) as Pixels);
    let bitmap = page.render_with_config(&config).ok()?;
    let mut rp = fond_doc::RenderedPage {
        width: bitmap.width() as u32,
        height: bitmap.height() as u32,
        rgba: bitmap.as_rgba_bytes(),
    };
    let (pw, ph) = overlay.page_pts;
    for (kind, quads, rgba) in &overlay.marks {
        let items: Vec<(fond_doc::MarkupKind, [f64; 8])> =
            quads.iter().map(|q| (*kind, *q)).collect();
        fond_doc::blend_annotations(&mut rp, pw, ph, &items, *rgba);
    }
    for (quads, rgba) in &overlay.highlights {
        fond_doc::blend_highlights(&mut rp, pw, ph, quads, *rgba);
    }
    if key.invert {
        super::render::invert_rgba(&mut rp.rgba);
    }
    let (rgba, width, height) = if key.rotation != 0 {
        super::render::rotate_rgba(&rp.rgba, rp.width, rp.height, key.rotation)
    } else {
        (rp.rgba, rp.width, rp.height)
    };
    Some(Rendered {
        key,
        rgba,
        width,
        height,
    })
}
