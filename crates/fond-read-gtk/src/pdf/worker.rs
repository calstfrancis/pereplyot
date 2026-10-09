use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// How a page's colours are changed for reading: left alone, darkened without turning pictures
/// into negatives, or warmed like old paper.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub(super) enum Tone {
    #[default]
    Normal,
    Dark,
    Sepia,
}

impl Tone {
    pub fn next(self) -> Tone {
        match self {
            Tone::Normal => Tone::Dark,
            Tone::Dark => Tone::Sepia,
            Tone::Sepia => Tone::Normal,
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Tone::Normal => "Page colours: normal (click for dark)",
            Tone::Dark => "Page colours: dark (click for sepia)",
            Tone::Sepia => "Page colours: sepia (click for normal)",
        }
    }
}

/// What a rendered page depends on. Two jobs with the same key produce the same pixels.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) struct RenderKey {
    pub page: u16,
    /// Device pixels (logical width × the widget's scale factor) of the unrotated page.
    pub width: u32,
    pub rotation: u16,
    pub tone: Tone,
    /// A small page for the sidebar, not a reading surface: queued behind every page job and
    /// never discarded in favour of one.
    pub thumb: bool,
    /// Device width of the whole page at the current zoom. Equal to `width` except for a tile's
    /// low-resolution backdrop, which is drawn smaller than the zoom it belongs to.
    pub zoom_w: u32,
    /// Which tile of the page, for very large renders drawn in pieces (see `tiles.rs`).
    pub tile: Option<(u16, u16)>,
    /// An arbitrary region of the page (x, y, width, height in device pixels at `zoom_w`), drawn
    /// for a hover preview or a pinned figure.
    pub crop: Option<[i32; 4]>,
}

impl RenderKey {
    /// Pages and their tiles, thumbnails and crops queue apart, so one never discards another.
    fn class(&self) -> u8 {
        if self.thumb {
            1
        } else if self.crop.is_some() {
            2
        } else {
            0
        }
    }

    fn same_view(&self, other: &RenderKey) -> bool {
        self.zoom_w == other.zoom_w && self.rotation == other.rotation && self.tone == other.tone
    }
}

pub(super) struct Job {
    pub key: RenderKey,
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
        q.jobs.retain(|j| {
            j.key.class() != job.key.class()
                || (j.key.same_view(&job.key)
                    && (j.key.page != job.key.page
                        || j.key.tile != job.key.tile
                        || j.key.crop != job.key.crop))
        });
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

/// The part of `page` drawn `zoom_w` device pixels wide that starts at (x, y) and is `w`×`h` pixels.
fn region<'p>(
    page: &'p pdfium_render::prelude::PdfPage<'_>,
    zoom_w: u32,
    (x, y, w, h): (u32, u32, u32, u32),
) -> Option<pdfium_render::prelude::PdfBitmap<'p>> {
    use pdfium_render::prelude::{PdfPoints, PdfRenderConfig, Pixels};
    let scale = zoom_w as f32 / page.width().value.max(1.0);
    let config = PdfRenderConfig::new()
        .set_fixed_width(w as Pixels)
        .set_fixed_height(h as Pixels)
        .scale_page_by_factor(scale)
        .translate(
            PdfPoints::new(-(x as f32) / scale),
            PdfPoints::new(-(y as f32) / scale),
        )
        .ok()?;
    page.render_with_config(&config).ok()
}

fn render(doc: &pdfium_render::prelude::PdfDocument<'_>, job: Job) -> Option<Rendered> {
    use pdfium_render::prelude::{PdfRenderConfig, Pixels};
    let Job { key, .. } = job;
    let page = doc.pages().get(key.page).ok()?;
    let bitmap = match key.tile {
        None if key.crop.is_none() => {
            let config = PdfRenderConfig::new().set_target_width(key.width.max(1) as Pixels);
            page.render_with_config(&config).ok()?
        }
        None => {
            let [x, y, w, h] = key.crop?;
            region(
                &page,
                key.zoom_w,
                (
                    x.max(0) as u32,
                    y.max(0) as u32,
                    w.max(1) as u32,
                    h.max(1) as u32,
                ),
            )?
        }
        Some((col, row)) => {
            let (x0, y0) = (
                col as u32 * super::tiles::TILE_PX,
                row as u32 * super::tiles::TILE_PX,
            );
            let scale = key.zoom_w as f32 / page.width().value.max(1.0);
            let full_h = (page.height().value * scale).round() as u32;
            if x0 >= key.zoom_w || y0 >= full_h {
                return None;
            }
            let tw = super::tiles::TILE_PX.min(key.zoom_w - x0);
            let th = super::tiles::TILE_PX.min(full_h - y0);
            region(&page, key.zoom_w, (x0, y0, tw, th))?
        }
    };
    let mut rgba = bitmap.as_rgba_bytes();
    super::render::apply_tone(&mut rgba, key.tone);
    let (width, height) = (bitmap.width() as u32, bitmap.height() as u32);
    let (rgba, width, height) = if key.rotation != 0 {
        super::render::rotate_rgba(&rgba, width, height, key.rotation)
    } else {
        (rgba, width, height)
    };
    Some(Rendered {
        key,
        rgba,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfium_render::prelude::*;

    fn rgba_of(doc: &PdfDocument<'_>, key: RenderKey) -> Option<Rendered> {
        render(doc, Job { key, priority: 0 })
    }

    #[test]
    fn a_tile_matches_the_same_region_of_a_full_render() {
        let Ok(pdfium) = crate::pdfium::get() else {
            eprintln!("no PDFium library; skipping");
            return;
        };
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/plain.pdf");
        let doc = pdfium.load_pdf_from_file(&path, None).unwrap();
        let width = 2400u32;
        let full_key = RenderKey {
            page: 0,
            width,
            rotation: 0,
            tone: Tone::Normal,
            thumb: false,
            zoom_w: width,
            tile: None,
            crop: None,
        };
        let full = rgba_of(&doc, full_key.clone()).unwrap();
        let tile = rgba_of(
            &doc,
            RenderKey {
                tile: Some((1, 0)),
                crop: None,
                ..full_key
            },
        )
        .unwrap();
        let (x0, y0) = (
            super::super::tiles::TILE_PX as usize,
            super::super::tiles::TILE_PX as usize,
        );
        let mut differing = 0usize;
        let mut total = 0usize;
        for y in 0..tile.height as usize {
            for x in 0..tile.width as usize {
                let a = &tile.rgba[(y * tile.width as usize + x) * 4..][..3];
                let b = &full.rgba[((y0 + y) * full.width as usize + x0 + x) * 4..][..3];
                total += 1;
                if a.iter()
                    .zip(b)
                    .any(|(p, q)| (*p as i32 - *q as i32).abs() > 40)
                {
                    differing += 1;
                }
            }
        }
        assert!(total > 0);
        let inked = tile.rgba.chunks_exact(4).filter(|p| p[0] < 100).count();
        assert!(inked > 100, "the tile is blank, so this proves nothing");
        assert!(
            (differing as f64) < total as f64 * 0.01,
            "{differing} of {total} pixels differ"
        );
    }
}
