use super::*;
use crate::reflow::citations::{citation_at, find_entry};
use crate::reflow::extract::extract_page;
use crate::reflow::model::RawPage;

const HOVER_DELAY: std::time::Duration = std::time::Duration::from_millis(380);
/// The preview's size on screen, in logical pixels.
const PREVIEW_WIDTH: f64 = 460.0;
const MAX_PREVIEW_HEIGHT: f64 = 300.0;
/// How far the pointer may wander from where it rested before the preview goes away.
const STAY_RADIUS: f64 = 36.0;
/// How many pages at the end of the document are searched for the bibliography.
const BIBLIOGRAPHY_PAGES: u16 = 40;

enum Bibliography {
    NotStarted,
    Loading,
    Ready(Rc<Vec<RawPage>>),
}

struct Pending {
    key: RenderKey,
    parent: gtk4::Widget,
    at: (f64, f64),
    caption: String,
}

/// The state of the hover preview: what has been asked for, what is showing, and what has been
/// read of the document to answer with.
pub(super) struct PreviewState {
    token: u64,
    pending: Option<Pending>,
    popover: Option<gtk4::Popover>,
    shown_at: Option<(f64, f64)>,
    words: Vec<(u16, Rc<RawPage>)>,
    bibliography: Bibliography,
}

impl Default for PreviewState {
    fn default() -> Self {
        PreviewState {
            token: 0,
            pending: None,
            popover: None,
            shown_at: None,
            words: Vec::new(),
            bibliography: Bibliography::NotStarted,
        }
    }
}

/// What to show: a stretch of a page, in page points as displayed (origin top left).
struct Target {
    page: u16,
    region: [f32; 4],
    caption: String,
}

fn hide(reader: &Rc<RefCell<ReaderState>>) {
    let popover = {
        let mut r = reader.borrow_mut();
        r.previews.token += 1;
        r.previews.pending = None;
        r.previews.shown_at = None;
        r.previews.popover.take()
    };
    if let Some(popover) = popover {
        popover.popdown();
        glib::idle_add_local_once(move || popover.unparent());
    }
}

/// The words of `page`, read once and kept.
fn words_of(reader: &Rc<RefCell<ReaderState>>, page: u16) -> Option<Rc<RawPage>> {
    if let Some((_, w)) = reader
        .borrow()
        .previews
        .words
        .iter()
        .find(|(p, _)| *p == page)
    {
        return Some(w.clone());
    }
    let raw = {
        let r = reader.borrow();
        extract_page(r.doc.as_ref()?, page)?
    };
    let raw = Rc::new(raw);
    let mut r = reader.borrow_mut();
    r.previews.words.push((page, raw.clone()));
    if r.previews.words.len() > 8 {
        r.previews.words.remove(0);
    }
    Some(raw)
}

/// The last pages of the document, read in the background the first time they are wanted.
fn bibliography(reader: &Rc<RefCell<ReaderState>>) -> Option<Rc<Vec<RawPage>>> {
    match &reader.borrow().previews.bibliography {
        Bibliography::Ready(pages) => return Some(pages.clone()),
        Bibliography::Loading => return None,
        Bibliography::NotStarted => {}
    }
    let path = {
        let mut r = reader.borrow_mut();
        r.previews.bibliography = Bibliography::Loading;
        r.path.clone()
    };
    let reader = reader.clone();
    glib::spawn_future_local(async move {
        let pages = gtk4::gio::spawn_blocking(move || tail_pages(&path))
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        reader.borrow_mut().previews.bibliography = Bibliography::Ready(Rc::new(pages));
    });
    None
}

fn tail_pages(path: &std::path::Path) -> Option<Vec<RawPage>> {
    let pdfium = crate::pdfium::get().ok()?;
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    let count = doc.pages().len();
    let first = count.saturating_sub(BIBLIOGRAPHY_PAGES);
    Some(
        (first..count)
            .filter_map(|i| extract_page(&doc, i))
            .collect(),
    )
}

/// What the pointer at `(x, y)` on a `w`×`h` page picture is over: a link to somewhere in the
/// document, or a citation whose bibliography entry can be found.
fn resolve(
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Option<Target> {
    let (geom, dest) = {
        let r = reader.borrow();
        if r.rotation != 0 || w < 1.0 || h < 1.0 {
            return None;
        }
        let geom = r.geom(page)?;
        let (px, py) = geom.px_to_pdf(x, y, w, h);
        (geom, link_destination(&r, page, px, py))
    };
    if let Some(dest) = dest {
        let (dw, dh) = {
            let r = reader.borrow();
            r.geom(dest.page)?.display_size()
        };
        let geom = reader.borrow().geom(dest.page)?;
        let (top, left) = match dest.point {
            Some((px, py)) if py < f64::MAX / 2.0 => {
                let (sx, sy) = geom.pdf_to_px(px, py, dw as f64, dh as f64);
                (sy as f32, sx as f32)
            }
            _ => (0.0, 0.0),
        };
        let span_w = 430.0f32.min(dw);
        let span_h = (span_w * 0.5).min(dh);
        let x0 = (left - 30.0).clamp(0.0, (dw - span_w).max(0.0));
        let y0 = (top - 18.0).clamp(0.0, (dh - span_h).max(0.0));
        let label = reader
            .borrow()
            .page_labels
            .get(dest.page as usize)
            .and_then(|l| l.clone())
            .unwrap_or_else(|| (dest.page + 1).to_string());
        return Some(Target {
            page: dest.page,
            region: [x0, y0, x0 + span_w, y0 + span_h],
            caption: format!("p. {label}"),
        });
    }
    let (dw, dh) = geom.display_size();
    let raw = words_of(reader, page)?;
    let citation = citation_at(&raw, (x * dw as f64 / w) as f32, (y * dh as f64 / h) as f32)?;
    let pages = bibliography(reader)?;
    let entry = find_entry(&pages, &citation)?;
    let (edw, edh) = reader.borrow().geom(entry.page)?.display_size();
    let b = entry.bbox;
    let x0 = (b[0] - 14.0).max(0.0);
    let x1 = (b[2] + 14.0).min(edw).max(x0 + 160.0);
    let y0 = (b[1] - 2.0).max(0.0);
    let y1 = (b[3] + 3.0).min(edh);
    let label = reader
        .borrow()
        .page_labels
        .get(entry.page as usize)
        .and_then(|l| l.clone())
        .unwrap_or_else(|| (entry.page + 1).to_string());
    Some(Target {
        page: entry.page,
        region: [x0, y0, x1, y1],
        caption: format!("Reference, p. {label}"),
    })
}

/// Ask the render thread for the picture of `target`; `deliver` shows it when it arrives.
fn request(
    reader: &Rc<RefCell<ReaderState>>,
    parent: &gtk4::Widget,
    at: (f64, f64),
    target: Target,
) {
    let scale = parent.scale_factor().max(1) as f64;
    let key = {
        let r = reader.borrow();
        let Some(geom) = r.geom(target.page) else {
            return;
        };
        let (dw, _) = geom.display_size();
        let [x0, y0, x1, y1] = target.region;
        let region_w = (x1 - x0).max(40.0) as f64;
        let out_w = PREVIEW_WIDTH * scale;
        let per_pt = out_w / region_w;
        let out_h = ((y1 - y0) as f64 * per_pt)
            .min(MAX_PREVIEW_HEIGHT * scale)
            .max(24.0);
        let zoom_w = (dw as f64 * per_pt).round() as u32;
        RenderKey {
            page: target.page,
            width: zoom_w,
            rotation: 0,
            tone: r.tone,
            thumb: false,
            zoom_w,
            tile: None,
            crop: Some([
                (x0 as f64 * per_pt).round() as i32,
                (y0 as f64 * per_pt).round() as i32,
                out_w.round() as i32,
                out_h.round() as i32,
            ]),
        }
    };
    let submitted = {
        let mut r = reader.borrow_mut();
        r.previews.pending = Some(Pending {
            key: key.clone(),
            parent: parent.clone(),
            at,
            caption: target.caption,
        });
        if let Some(worker) = &r.worker {
            worker.submit(Job { key, priority: 0 });
            true
        } else {
            false
        }
    };
    if !submitted {
        reader.borrow_mut().previews.pending = None;
    }
}

/// A finished crop from the render thread: show it if it is still what was asked for.
pub(super) fn deliver(reader: &Rc<RefCell<ReaderState>>, done: Rendered) {
    let pending = {
        let mut r = reader.borrow_mut();
        match &r.previews.pending {
            Some(p) if p.key == done.key => r.previews.pending.take(),
            _ => None,
        }
    };
    let Some(pending) = pending else {
        return;
    };
    let scale = pending.parent.scale_factor().max(1);
    let stride = done.width as usize * 4;
    let (width, height) = (done.width as i32, done.height as i32);
    let texture = gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(done.rgba),
        stride,
    );
    let picture = gtk4::Picture::for_paintable(&texture);
    picture.set_size_request(width / scale, height / scale);
    picture.set_can_shrink(false);
    let caption = gtk4::Label::new(Some(&pending.caption));
    caption.add_css_class("caption");
    caption.add_css_class("dim-label");
    caption.set_xalign(0.0);
    let column = gtk4::Box::new(Orientation::Vertical, 4);
    column.set_margin_top(6);
    column.set_margin_bottom(6);
    column.set_margin_start(6);
    column.set_margin_end(6);
    column.append(&picture);
    column.append(&caption);
    let popover = gtk4::Popover::new();
    popover.set_child(Some(&column));
    popover.set_autohide(false);
    popover.set_can_target(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_parent(&pending.parent);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        pending.at.0.round() as i32,
        pending.at.1.round() as i32 + 6,
        1,
        1,
    )));
    popover.popup();
    let old = {
        let mut r = reader.borrow_mut();
        r.previews.shown_at = Some(pending.at);
        r.previews.popover.replace(popover)
    };
    if let Some(old) = old {
        old.popdown();
        glib::idle_add_local_once(move || old.unparent());
    }
}

/// Hovering a link, or a citation the text names, shows what it leads to in a small popover; the
/// pointer resting still for a moment is what asks.
pub(super) fn install(
    overlay: &gtk4::Overlay,
    reader: &Rc<RefCell<ReaderState>>,
    page_of: Rc<dyn Fn() -> u16>,
) {
    let motion = gtk4::EventControllerMotion::new();
    motion.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let reader = reader.clone();
        let overlay = overlay.clone();
        motion.connect_motion(move |ctl, x, y| {
            let pressed = ctl
                .current_event_state()
                .contains(gdk::ModifierType::BUTTON1_MASK);
            let stays = reader
                .borrow()
                .previews
                .shown_at
                .is_some_and(|(sx, sy)| (sx - x).hypot(sy - y) < STAY_RADIUS);
            if pressed || !stays {
                hide(&reader);
            }
            if pressed || stays {
                return;
            }
            let token = reader.borrow().previews.token;
            let reader = reader.clone();
            let overlay = overlay.clone();
            let page = page_of();
            glib::timeout_add_local_once(HOVER_DELAY, move || {
                if reader.borrow().previews.token != token {
                    return;
                }
                let (w, h) = (overlay.width() as f64, overlay.height() as f64);
                if let Some(target) = resolve(&reader, page, x, y, w, h) {
                    request(&reader, overlay.upcast_ref(), (x, y), target);
                }
            });
        });
    }
    {
        let reader = reader.clone();
        motion.connect_leave(move |_| hide(&reader));
    }
    overlay.add_controller(motion);
    let click = gtk4::GestureClick::new();
    click.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let reader = reader.clone();
        click.connect_pressed(move |_, _, _, _| hide(&reader));
    }
    overlay.add_controller(click);
}
