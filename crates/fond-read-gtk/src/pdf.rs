//! The PDF reader: paged and continuous views, drag-to-annotate, text selection, in-document
//! search, undo/redo, and printed-page-number handling.
//!
//! Rendering, text extraction and search come from `fond-doc`; everything here is the GTK
//! layer over it. Persistence goes through [`ReaderHost`] — see `docs/READER-EXTRACTION.md`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, glib, Orientation};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::page_geom::PageGeom;
use crate::{color_swatch, note_edit_widget, popover_button, popover_separator, ReaderHost};

/// Live state of an open PDF reader window.
struct ReaderState {
    pdfium: &'static fond_doc::Pdfium,
    bytes: Vec<u8>,
    /// The document, parsed once and kept open for rendering and page geometry; every
    /// `fond_doc` helper that takes `bytes` re-parses the whole file on each call.
    doc: Option<pdfium_render::prelude::PdfDocument<'static>>,
    page: u16,
    count: u16,
    /// Render width in px = `BASE_WIDTH * zoom`.
    zoom: f64,
    /// This entry's annotation sidecar, loaded once at open and rewritten to disk on every
    /// highlight added. Held here (not re-read from the library each time) so the in-memory
    /// list and the on-screen render never disagree mid-session.
    annotations: fond_bib::AnnotationSidecar,
    /// The current page's rendered pixel size, refreshed by `render()` — the scale a
    /// drag-selected rectangle is converted through when saving a new highlight.
    render_px: (u32, u32),
    /// Each page's box origin/size and `/Rotate`, read on first use — see `PageGeom`.
    page_geoms: RefCell<std::collections::HashMap<u16, Option<PageGeom>>>,
    /// Which kind the next drag creates — set by the mode `DropDown`, defaulting to
    /// `Highlight`. `Note` is never drawn this way (it has no on-page region); it's added
    /// via the separate "Note…" button instead. `None` is the drop-down's "Select text"
    /// entry — a drag copies the covered text to the clipboard instead of saving an
    /// annotation.
    draw_kind: Option<fond_bib::AnnotationKind>,
    /// The most recent "Select text" copy — page (0-based), text, and quadpoints — so the
    /// next note added on that same page can quote it (and carry its real on-page region)
    /// instead of starting blank, and so `render_pdf_page_texture` can keep showing it
    /// highlighted after the drag ends instead of the selection just vanishing (see
    /// `SELECTION_RGBA`). Cleared once consumed by a note, or replaced by the next selection.
    last_selection: Option<(u16, String, Vec<[f64; 8]>)>,
    /// Every match from the last search (empty if none run yet, or the last search found
    /// nothing), and which one is "current" — `render()` blends that one's quads in a
    /// distinct colour when the current page matches, and prev/next-match cycle this index.
    search_matches: Vec<fond_doc::PdfSearchMatch>,
    search_current: usize,
    /// Hex colour (`#rrggbb`) the next drag saves onto its annotation — set by the colour
    /// `DropDown`, defaulting to the original hardcoded amber.
    draw_color: String,
    /// Continuous-scroll mode's state — empty until the mode is toggled on for the first
    /// time (built lazily, not at reader-open, so plain "Read" stays as fast as it always
    /// was). `continuous_pictures[i]` is page `i`'s permanent `Picture` widget (unlike the
    /// paged view's single recycled one); `continuous_offsets[i]` is that page's cumulative
    /// top position in the continuous view, in pixels, for scroll-to-page and for tracking
    /// which page is "current" from scroll position; `continuous_rendered[i]` avoids
    /// re-rendering a page that hasn't changed since it was last drawn.
    continuous_pictures: Vec<gtk4::Picture>,
    continuous_offsets: Vec<f64>,
    continuous_rendered: Vec<bool>,
    /// Jump to a page because a link was followed (records where we came from); set once the
    /// widgets it drives exist.
    link_goto: Option<Rc<dyn Fn(u16)>>,
    /// Pages we followed links away from, most recent last, for the Back button.
    nav_back: Vec<u16>,
    /// Inclusive range of pages currently kept rendered in continuous mode; everything else
    /// is unloaded so a long book at high zoom doesn't hold every page as a texture.
    continuous_window: (u16, u16),
    /// Each page's document-defined `/PageLabels` printed number (`None` where the PDF
    /// doesn't define one, which is most PDFs) — read once at open, since it's an immutable
    /// property of the file. Index `i` (0-based) matches every other page index in this
    /// struct.
    page_labels: Vec<Option<String>>,
    /// Snapshot-based undo/redo: each entry is a full clone of `annotations` taken
    /// immediately before a mutation (drag-created highlight, note added, annotation
    /// deleted or edited). `push_undo_snapshot` is the single place that pushes here and
    /// clears `redo_stack` — every mutation site calls it first. Capped at
    /// `UNDO_HISTORY_LIMIT` so a long session doesn't grow this unbounded.
    undo_stack: Vec<fond_bib::AnnotationSidecar>,
    redo_stack: Vec<fond_bib::AnnotationSidecar>,
    /// Clockwise display rotation in degrees (0/90/180/270) — view-only, single-page mode
    /// only (see `rotate_button`'s wiring): the render pipeline blends annotations in the
    /// PDF's own unrotated coordinate space and only rotates the final pixel buffer for
    /// display, so a rotated page's drag-to-annotate/right-click coordinate math would be
    /// wrong; those are disabled while this is non-zero rather than taught to un-rotate a
    /// click first. Reset to 0 whenever Continuous or Two-page mode is entered, since
    /// neither one's layout math (continuous page offsets computed from the *unrotated*
    /// page size; two-page's facing-page box) accounts for a rotated page either.
    rotation: u16,
    /// Colours inverted for display (a night-reading mode for scanned/white-background
    /// PDFs, whose pixels don't respond to the app's own dark theme) — applied to the same
    /// final pixel buffer `rotation` is, in both paged and continuous mode alike.
    invert_colors: bool,
    /// Bookmarked pages, 1-based (`Annotation.page` numbering) — loaded once at open,
    /// rewritten to the host on every add/remove, same lifecycle as `annotations`. Kept
    /// sorted so the Notes sidebar's "Bookmarks" section lists them in page order.
    bookmarks: Vec<u32>,
}

impl ReaderState {
    fn geom(&self, page: u16) -> Option<PageGeom> {
        if let Some(cached) = self.page_geoms.borrow().get(&page) {
            return *cached;
        }
        let geom = match &self.doc {
            Some(doc) => PageGeom::read_doc(doc, page),
            None => self
                .pdfium
                .load_pdf_from_byte_slice(&self.bytes, None)
                .ok()
                .and_then(|d| PageGeom::read_doc(&d, page)),
        };
        self.page_geoms.borrow_mut().insert(page, geom);
        geom
    }

    /// On-screen point size of `page` — for layout only, so falls back to US Letter.
    fn display_size(&self, page: u16) -> (f32, f32) {
        self.geom(page)
            .map(|g| g.display_size())
            .unwrap_or((612.0, 792.0))
    }
}

/// How many undo steps a PDF reader session keeps before dropping the oldest.
pub(crate) const UNDO_HISTORY_LIMIT: usize = 50;

/// Snapshot `reader`'s current annotations onto the undo stack and clear the redo stack —
/// call this immediately before any mutation to `reader.annotations`, so the mutation can be
/// undone. Standard editor convention: a fresh mutation invalidates any pending redo.
fn push_undo_snapshot(reader: &Rc<RefCell<ReaderState>>) {
    let mut r = reader.borrow_mut();
    let snapshot = r.annotations.clone();
    r.undo_stack.push(snapshot);
    if r.undo_stack.len() > UNDO_HISTORY_LIMIT {
        r.undo_stack.remove(0);
    }
    r.redo_stack.clear();
}

/// Refresh the Undo/Redo header buttons' sensitivity from `reader`'s current stacks — called
/// after every mutation site (both the paged view's and, since `build_continuous_view` builds
/// its own drag handlers, continuous mode's) so the buttons never sit enabled/disabled stale.
fn sync_undo_redo_buttons(
    reader: &Rc<RefCell<ReaderState>>,
    undo_button: &gtk4::Button,
    redo_button: &gtk4::Button,
) {
    let r = reader.borrow();
    undo_button.set_sensitive(!r.undo_stack.is_empty());
    redo_button.set_sensitive(!r.redo_stack.is_empty());
}

/// Parse an annotation's stored `#rrggbb` hex colour into RGBA at the standard highlight
/// alpha (matching the original hardcoded `HIGHLIGHT_RGBA`'s opacity), falling back to that
/// same amber for anything that doesn't parse — a hand-edited sidecar entry, or one written
/// before the colour picker existed and so has no `color` at all.
pub(crate) fn annotation_rgba(hex: Option<&str>) -> [u8; 4] {
    let parsed = hex.and_then(|h| {
        let h = h.trim_start_matches('#');
        if h.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&h[0..2], 16).ok()?;
        let g = u8::from_str_radix(&h[2..4], 16).ok()?;
        let b = u8::from_str_radix(&h[4..6], 16).ok()?;
        Some([r, g, b, HIGHLIGHT_RGBA[3]])
    });
    parsed.unwrap_or(HIGHLIGHT_RGBA)
}

/// Wraps `picture` in an `Overlay` with a semi-transparent `DrawingArea` on top that tracks a
/// live drag rectangle, so a highlight/underline/strikeout is visible *while* it's being
/// dragged instead of only appearing once the drag ends. Returns the overlay (to use in the
/// widget tree in place of `picture`), the preview `DrawingArea` itself (drag handlers call
/// `.queue_draw()` on it after updating the rect), and the shared cell those handlers write
/// into — `Some((x0, y0, x1, y1))` in the picture's own pixel space while dragging, `None`
/// otherwise. The preview doesn't try to match the final narrowed underline/strikeout band —
/// it's just the raw drag rectangle in the current draw colour, which is enough to show what's
/// about to be created.
type PreviewRects = Option<Vec<(f64, f64, f64, f64)>>;
type PreviewCache =
    Rc<RefCell<Option<((u16, i32, i32, i32, i32), std::time::Instant, PreviewRects)>>>;

/// Selection geometry is a full re-read of the PDF, so the live preview recomputes it at most
/// this often while the pointer moves.
const PREVIEW_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(60);

fn paint_preview_rects(cr: &gtk4::cairo::Context, rects: PreviewRects, drag: (f64, f64, f64, f64)) {
    match rects {
        Some(rects) if !rects.is_empty() => {
            for (rx0, ry0, rx1, ry1) in rects {
                cr.rectangle(
                    rx0.min(rx1),
                    ry0.min(ry1),
                    (rx1 - rx0).abs(),
                    (ry1 - ry0).abs(),
                );
            }
        }
        _ => {
            let (x0, y0, x1, y1) = drag;
            cr.rectangle(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
        }
    }
    let _ = cr.fill();
}

/// A drag rectangle in a picture's own pixel space: `(x0, y0, x1, y1)`.
type DragRectCell = Rc<Cell<Option<(f64, f64, f64, f64)>>>;
enum LinkTarget {
    Page(u16),
    Uri(String),
}

fn link_at(r: &ReaderState, page: u16, x: f64, y: f64) -> Option<LinkTarget> {
    use pdfium_render::prelude::{PdfActionType, PdfPoints};
    let doc = r.doc.as_ref()?;
    let pdf_page = doc.pages().get(page).ok()?;
    let links = pdf_page.links();
    let link = links.link_at_point(PdfPoints::new(x as f32), PdfPoints::new(y as f32))?;
    if let Some(dest) = link.destination() {
        return dest.page_index().ok().map(LinkTarget::Page);
    }
    let action = link.action()?;
    match action.action_type() {
        PdfActionType::GoToDestinationInSameDocument => action
            .as_local_destination_action()?
            .destination()
            .ok()?
            .page_index()
            .ok()
            .map(LinkTarget::Page),
        PdfActionType::Uri => action.as_uri_action()?.uri().ok().map(LinkTarget::Uri),
        _ => None,
    }
}

/// If a click at pixel `(px, py)` of a `render_w`×`render_h` render of `page` landed on a
/// link, follow it and report true. Internal links jump (remembering where we were); web
/// links open in the default handler.
fn follow_link(
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    px: f64,
    py: f64,
    render_w: f64,
    render_h: f64,
) -> bool {
    let (target, goto) = {
        let r = reader.borrow();
        if r.rotation != 0 || render_w < 1.0 || render_h < 1.0 {
            return false;
        }
        let Some(geom) = r.geom(page) else {
            return false;
        };
        let (x, y) = geom.px_to_pdf(px, py, render_w, render_h);
        (link_at(&r, page, x, y), r.link_goto.clone())
    };
    match target {
        Some(LinkTarget::Page(p)) => {
            if let Some(goto) = goto {
                goto(p);
            }
            true
        }
        Some(LinkTarget::Uri(uri)) => {
            if uri.starts_with("http://")
                || uri.starts_with("https://")
                || uri.starts_with("mailto:")
            {
                gtk4::UriLauncher::new(&uri).launch(
                    gtk4::Window::NONE,
                    gtk4::gio::Cancellable::NONE,
                    |_| {},
                );
            }
            true
        }
        None => false,
    }
}

/// Late-bound "mark the current selection with colour N" action, filled in once the notes
/// sidebar it refreshes exists.
type QuickMarkSlot = Rc<RefCell<Option<Rc<dyn Fn(usize)>>>>;
/// Self-referential slot for the notes-sidebar rebuild closure — a row's own delete button
/// needs to trigger a fresh rebuild of the list it lives in.
type RebuildNotesCell = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// `page_of` resolves which page this preview belongs to at draw time — a fixed value in
/// continuous-scroll mode (one overlay per page), but the reader's *current* page in paged
/// mode (one recycled overlay, page changes as the user navigates).
fn build_drag_preview_overlay(
    picture: &gtk4::Picture,
    reader: &Rc<RefCell<ReaderState>>,
    page_of: impl Fn() -> u16 + 'static,
) -> (gtk4::Overlay, gtk4::DrawingArea, DragRectCell) {
    let live_rect: DragRectCell = Rc::new(Cell::new(None));

    let preview = gtk4::DrawingArea::new();
    preview.set_can_target(false);
    preview.set_hexpand(true);
    preview.set_vexpand(true);
    {
        let live_rect = live_rect.clone();
        let reader = reader.clone();
        let preview_cache: PreviewCache = Rc::new(RefCell::new(None));
        preview.set_draw_func(move |_area, cr, w, h| {
            let Some((x0, y0, x1, y1)) = live_rect.get() else {
                return;
            };
            let select_mode = reader.borrow().draw_kind.is_none();
            let rgba = if select_mode {
                SELECTION_RGBA
            } else {
                annotation_rgba(Some(&reader.borrow().draw_color))
            };
            cr.set_source_rgba(
                rgba[0] as f64 / 255.0,
                rgba[1] as f64 / 255.0,
                rgba[2] as f64 / 255.0,
                (rgba[3] as f64 / 255.0 * 1.6).min(1.0),
            );

            // Line-aware preview: what a drag from (x0,y0) to (x1,y1) would actually select,
            // not just its own bounding rectangle — matches what `save_drag_annotation`/
            // `copy_drag_selection` compute at drag-end, so the preview doesn't lie about
            // what's about to happen. Falls back to the plain rectangle over blank space
            // (a figure, a margin) where there's no text to hug.
            let page = page_of();
            let key = (
                page,
                (x0 / 4.0) as i32,
                (y0 / 4.0) as i32,
                (x1 / 4.0) as i32,
                (y1 / 4.0) as i32,
            );
            let reuse = {
                let c = preview_cache.borrow();
                c.as_ref().is_some_and(|(k, at, _)| {
                    *k == key || (k.0 == key.0 && at.elapsed() < PREVIEW_MIN_INTERVAL)
                })
            };
            if reuse {
                let rects = preview_cache.borrow().as_ref().and_then(|c| c.2.clone());
                paint_preview_rects(cr, rects, (x0, y0, x1, y1));
                return;
            }
            let line_rects = (w > 0 && h > 0)
                .then(|| {
                    let r = reader.borrow();
                    let geom = r.geom(page)?;
                    let (w, h) = (w as f64, h as f64);
                    let (sx, sy) = geom.px_to_pdf(x0, y0, w, h);
                    let (ex, ey) = geom.px_to_pdf(x1, y1, w, h);
                    let sel = fond_doc::select_text_range(
                        r.pdfium, &r.bytes, page, sx as f32, sy as f32, ex as f32, ey as f32,
                    )
                    .ok()
                    .flatten()?;
                    Some(
                        sel.quads
                            .iter()
                            .map(|q| geom.quad_px_bounds(q, w, h))
                            .collect::<Vec<_>>(),
                    )
                })
                .flatten();

            *preview_cache.borrow_mut() =
                Some((key, std::time::Instant::now(), line_rects.clone()));
            paint_preview_rects(cr, line_rects, (x0, y0, x1, y1));
        });
    }

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(picture));
    overlay.add_overlay(&preview);

    (overlay, preview, live_rect)
}

/// The mark-style `DropDown` beside the colour palette, shared by both readers. "Select
/// text" isn't here — it's the palette's own first button in the PDF reader, and the EPUB
/// reader's native selection is always available.
pub(crate) const MARK_KIND_OPTIONS: [(&str, fond_bib::AnnotationKind); 3] = [
    ("Highlight", fond_bib::AnnotationKind::Highlight),
    ("Underline", fond_bib::AnnotationKind::Underline),
    ("Strikeout", fond_bib::AnnotationKind::Strikeout),
];

const READER_BASE_WIDTH: f64 = 820.0;
/// Amber at ~55% opacity — a highlight tint, not a solid block.
const HIGHLIGHT_RGBA: [u8; 4] = [246, 195, 68, 140];
/// Blue at ~65% opacity — deliberately distinct from `HIGHLIGHT_RGBA` so the current search
/// match reads as "found this" and not as a saved highlight.
const SEARCH_MATCH_RGBA: [u8; 4] = [66, 133, 244, 165];
/// Blue at ~35% opacity, the live drag preview's tint in "Select text" mode — same hue
/// family as `SEARCH_MATCH_RGBA` (a "this is transient, not a saved mark" blue) but its own
/// constant since the two never appear together and may want to diverge later.
const SELECTION_RGBA: [u8; 4] = [66, 133, 244, 90];
/// Below this, a drag reads as a stray click, not an intentional highlight.
const MIN_DRAG_PX: f64 = 6.0;
/// Vertical gap between pages in continuous-scroll mode.
const CONTINUOUS_PAGE_GAP: f64 = 8.0;

/// The pointer shown over the page: an I-beam in "Select text" mode (this is a text
/// selection, not a highlight-drawing gesture), the default arrow otherwise. `None` means
/// "reset to default" — `Widget::set_cursor(None)` is how GTK4 clears a per-widget override.
fn cursor_for_select_mode(select_mode: bool) -> Option<gdk::Cursor> {
    select_mode
        .then(|| gdk::Cursor::from_name("text", None))
        .flatten()
}

/// Rotate an RGBA buffer clockwise by 0/90/180/270 degrees, returning the rotated buffer and
/// its (width, height) — swapped for 90/270. `degrees` outside that set is treated as 0.
fn rotate_rgba(rgba: &[u8], width: u32, height: u32, degrees: u16) -> (Vec<u8>, u32, u32) {
    let w = width as usize;
    let h = height as usize;
    match degrees % 360 {
        90 => {
            let (nw, nh) = (h, w);
            let mut out = vec![0u8; rgba.len()];
            for y in 0..h {
                for x in 0..w {
                    let src = (y * w + x) * 4;
                    let dst = (x * nw + (h - 1 - y)) * 4;
                    out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                }
            }
            (out, nw as u32, nh as u32)
        }
        180 => {
            let mut out = vec![0u8; rgba.len()];
            for y in 0..h {
                for x in 0..w {
                    let src = (y * w + x) * 4;
                    let dst = ((h - 1 - y) * w + (w - 1 - x)) * 4;
                    out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                }
            }
            (out, width, height)
        }
        270 => {
            let (nw, nh) = (h, w);
            let mut out = vec![0u8; rgba.len()];
            for y in 0..h {
                for x in 0..w {
                    let src = (y * w + x) * 4;
                    let dst = ((w - 1 - x) * nw + y) * 4;
                    out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                }
            }
            (out, nw as u32, nh as u32)
        }
        _ => (rgba.to_vec(), width, height),
    }
}

/// Invert an RGBA buffer's colours in place, leaving alpha untouched — a night-reading mode
/// for scanned/white-background pages, which stay bright regardless of the app's own theme
/// since they're just pixels, not something CSS/`adw::StyleManager` can recolour.
fn invert_rgba(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        px[0] = 255 - px[0];
        px[1] = 255 - px[1];
        px[2] = 255 - px[2];
    }
}

fn render_open(r: &ReaderState, page: u16, width: u32) -> Option<fond_doc::RenderedPage> {
    let Some(doc) = &r.doc else {
        return fond_doc::render_page(r.pdfium, &r.bytes, page, width).ok();
    };
    let pdf_page = doc.pages().get(page).ok()?;
    let config = pdfium_render::prelude::PdfRenderConfig::new()
        .set_target_width(width.max(1) as pdfium_render::prelude::Pixels);
    let bitmap = pdf_page.render_with_config(&config).ok()?;
    Some(fond_doc::RenderedPage {
        width: bitmap.width() as u32,
        height: bitmap.height() as u32,
        rgba: bitmap.as_rgba_bytes(),
    })
}

/// Render `page` (0-based) to a ready-to-display texture, with this entry's saved
/// annotations — and, if `page` has the current search match, that match's highlight too —
/// blended in. Shared by both the page-by-page view and continuous-scroll mode so the two
/// can never visually disagree about what a page looks like. Returns the texture, its pixel
/// size, and the page's PDF-point size (the scale a drag-selected rectangle on that page
/// converts through).
fn render_pdf_page_texture(r: &ReaderState, page: u16) -> Option<(gdk::Texture, u32, u32)> {
    let width = (READER_BASE_WIDTH * r.zoom) as u32;
    let geom = r.geom(page);
    let page_pts = geom.map(|g| g.display_size()).unwrap_or((0.0, 0.0));
    let to_display = |quads: &[[f64; 8]]| match geom {
        Some(g) => g.quads_to_display(quads),
        None => quads.to_vec(),
    };
    let mut rp = render_open(r, page, width)?;
    let current_page = page as u32 + 1;

    // A freestanding Note (no quadpoints — added via the "Note…" button on blank page) is
    // already excluded by the `!quadpoints.is_empty()` filter above; a Note created *from a
    // text selection* ("Create note from…", see `show_pdf_context_menu`) does carry real
    // quadpoints and is blended here like a highlight, so it stays visible on the page and
    // not just listed in the sidebar. Each annotation keeps its own colour (from the colour
    // picker at draw time, or the default amber for a Note, which has none), so this blends
    // per-annotation rather than batching every quad on the page into one shared-colour call.
    for a in r
        .annotations
        .annotations
        .iter()
        .filter(|a| a.page == Some(current_page) && !a.quadpoints.is_empty())
    {
        let kind = match a.kind {
            fond_bib::AnnotationKind::Highlight | fond_bib::AnnotationKind::Note => {
                fond_doc::MarkupKind::Highlight
            }
            fond_bib::AnnotationKind::Underline => fond_doc::MarkupKind::Underline,
            fond_bib::AnnotationKind::Strikeout => fond_doc::MarkupKind::Strikeout,
        };
        let items: Vec<(fond_doc::MarkupKind, [f64; 8])> = to_display(&a.quadpoints)
            .into_iter()
            .map(|q| (kind, q))
            .collect();
        fond_doc::blend_annotations(
            &mut rp,
            page_pts.0,
            page_pts.1,
            &items,
            annotation_rgba(a.color.as_deref()),
        );
    }

    // The current search match, if it's on this page — blended in its own colour, on top of
    // any saved highlights, so it reads as "found this" and not as another saved annotation.
    if let Some(current) = r.search_matches.get(r.search_current) {
        if current.page == page {
            fond_doc::blend_highlights(
                &mut rp,
                page_pts.0,
                page_pts.1,
                &to_display(&current.quads),
                SEARCH_MATCH_RGBA,
            );
        }
    }

    // A "Select text" drag's selection, if it's on this page — kept visible after the drag
    // ends (previously it vanished the instant you released the mouse, leaving only the
    // clipboard copy as any trace) until a new selection replaces it or it's consumed by
    // "Create note from…". Uses the live drag-preview's own colour so a selection looks the
    // same while dragging and once settled.
    if let Some((sel_page, _, quads)) = &r.last_selection {
        if *sel_page == page {
            fond_doc::blend_highlights(
                &mut rp,
                page_pts.0,
                page_pts.1,
                &to_display(quads),
                SELECTION_RGBA,
            );
        }
    }

    if r.invert_colors {
        invert_rgba(&mut rp.rgba);
    }
    let (data, out_w, out_h) = if r.rotation != 0 {
        let (rotated, w, h) = rotate_rgba(&rp.rgba, rp.width, rp.height, r.rotation);
        (glib::Bytes::from(&rotated), w, h)
    } else {
        (glib::Bytes::from(&rp.rgba), rp.width, rp.height)
    };
    let texture = gdk::MemoryTexture::new(
        out_w as i32,
        out_h as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &data,
        (out_w * 4) as usize,
    );
    Some((texture.upcast(), out_w, out_h))
}

/// Convert a drag gesture's start/end (widget-local pixel coordinates on `page`'s own
/// render, at that page's own `render_w`×`render_h` scale and `PageGeom`) into
/// PDF-space quadpoints — hugging real text if the drag covers any, same as
/// `select_text_in_rect` documents — and save a new annotation for it. The shared save path
/// for both the page-by-page view's drag handler and continuous-scroll mode's per-page ones,
/// so the two don't duplicate the coordinate math and sidecar write. Returns whether an
/// annotation was actually saved, so the caller knows whether a redraw is needed.
/// The geometry a drag gesture needs converted to PDF-space quadpoints: the page's own
/// rendered pixel size and PDF-point size (its scale), and the drag's start/end in that
/// pixel space. Grouped into one struct rather than eight loose parameters on
/// `save_drag_annotation`.
struct DragGeometry {
    render_w: u32,
    render_h: u32,
    page: Option<PageGeom>,
    start_x: f64,
    start_y: f64,
    end_x: f64,
    end_y: f64,
}

/// The PDF-space points a drag's start/end cover, given its pixel geometry — the inverse of
/// the page's render scale. Order-preserving (unlike a sorted rectangle), since
/// `select_text_range` needs to know which endpoint is the reading-order start. `None` if
/// the geometry is degenerate (zero-sized render, or a page with no reported point size).
fn drag_pdf_points(geom: &DragGeometry) -> Option<((f64, f64), (f64, f64))> {
    let &DragGeometry {
        render_w,
        render_h,
        page,
        start_x,
        start_y,
        end_x,
        end_y,
    } = geom;
    let page = page?;
    if render_w == 0 || render_h == 0 {
        return None;
    }
    let (w, h) = (render_w as f64, render_h as f64);
    Some((
        page.px_to_pdf(start_x, start_y, w, h),
        page.px_to_pdf(end_x, end_y, w, h),
    ))
}

/// The text under `quads` (one per line, user space), read back through PDFium's bounded-
/// text call. `fond_doc::select_text_range`'s own `text` is built from individual glyphs and
/// skips PDFium's generated spaces, so on the many PDFs that position words instead of
/// storing space characters (LaTeX and Typst output, for a start) it runs every word
/// together. Each line's box is shrunk to its middle band so neighbouring lines' glyph boxes,
/// which often overlap vertically, don't leak in.
fn selection_text(r: &ReaderState, page: u16, quads: &[[f64; 8]]) -> Option<String> {
    let document = r.pdfium.load_pdf_from_byte_slice(&r.bytes, None).ok()?;
    let pdf_page = document.pages().get(page).ok()?;
    let text = pdf_page.text().ok()?;
    let lines: Vec<String> = quads
        .iter()
        .map(|q| {
            let (l, rt) = (
                q[0].min(q[2]).min(q[4]).min(q[6]),
                q[0].max(q[2]).max(q[4]).max(q[6]),
            );
            let (b, t) = (
                q[1].min(q[3]).min(q[5]).min(q[7]),
                q[1].max(q[3]).max(q[5]).max(q[7]),
            );
            let dx = ((rt - l) * 0.25).min(0.5);
            let dy = (t - b) * 0.25;
            let rect = pdfium_render::prelude::PdfRect::new_from_values(
                (b + dy) as f32,
                (l + dx) as f32,
                (t - dy) as f32,
                (rt - dx) as f32,
            );
            text.inside_rect(rect)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join(" "))
}

fn copy_to_clipboard(host: &Rc<dyn ReaderHost>, text: &str) {
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(text);
    }
    host.notify("Copied to clipboard");
}

/// Selects the text under a "Select text" mode drag, remembering it (page, text, quadpoints)
/// on `reader` so it stays visibly marked on the page and so the selection popover — and a
/// note added right after — can act on it; see `last_selection`. Returns whether anything was
/// selected, so the caller knows whether to re-render and show the popover.
fn select_drag_text(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    geom: &DragGeometry,
) -> bool {
    let Some((start, end)) = drag_pdf_points(geom) else {
        return false;
    };
    let selection = {
        let r = reader.borrow();
        fond_doc::select_text_range(
            r.pdfium,
            &r.bytes,
            page,
            start.0 as f32,
            start.1 as f32,
            end.0 as f32,
            end.1 as f32,
        )
        .ok()
        .flatten()
    };
    match selection {
        Some(sel) if !sel.text.trim().is_empty() => {
            let text = selection_text(&reader.borrow(), page, &sel.quads).unwrap_or(sel.text);
            reader.borrow_mut().last_selection = Some((page, text, sel.quads));
            true
        }
        _ => {
            host.notify("No text found in selection");
            false
        }
    }
}

/// Everything the selection popover and the 1–4 quick-mark keys need to save a mark.
#[derive(Clone)]
struct MarkCtx {
    host: Rc<dyn ReaderHost>,
    reader: Rc<RefCell<ReaderState>>,
    pdf_hash: String,
    undo_button: gtk4::Button,
    redo_button: gtk4::Button,
    rebuild_notes: Rc<dyn Fn()>,
    reader_window: adw::Window,
    /// Re-render whatever shows the page that was just marked.
    refresh: Rc<dyn Fn()>,
}

/// Turn the current selection into a mark of `kind`, in `color` (hex). Consumes the selection.
fn apply_selection_mark(ctx: &MarkCtx, kind: fond_bib::AnnotationKind, color: &str) -> bool {
    let Some((page, text, quads)) = ctx.reader.borrow_mut().last_selection.take() else {
        return false;
    };
    let annotation = fond_bib::Annotation::drawn(
        kind,
        page as u32 + 1,
        quads,
        Some(text),
        None,
        Some(color.to_string()),
    );
    push_undo_snapshot(&ctx.reader);
    {
        let mut r = ctx.reader.borrow_mut();
        r.annotations.pdf_hash = Some(ctx.pdf_hash.clone());
        r.annotations.upsert(annotation);
    }
    let result = ctx.host.save_annotations(&ctx.reader.borrow().annotations);
    match result {
        Ok(()) => {
            (ctx.refresh)();
            sync_undo_redo_buttons(&ctx.reader, &ctx.undo_button, &ctx.redo_button);
            (ctx.rebuild_notes)();
            ctx.host.notify(match kind {
                fond_bib::AnnotationKind::Underline => "Underline added",
                fond_bib::AnnotationKind::Strikeout => "Strikeout added",
                _ => "Highlight added",
            });
            true
        }
        Err(e) => {
            ctx.host.notify(&e);
            false
        }
    }
}

/// Copy the current selection, optionally as a quotation with its citation in the format last
/// used for export (Typst by default).
fn copy_selection(ctx: &MarkCtx, with_citation: bool) {
    let (page, text) = {
        let r = ctx.reader.borrow();
        match &r.last_selection {
            Some((p, t, _)) => (*p, t.clone()),
            None => return,
        }
    };
    if !with_citation {
        copy_to_clipboard(&ctx.host, &text);
        return;
    }
    let locator = {
        let r = ctx.reader.borrow();
        r.page_labels
            .get(page as usize)
            .and_then(|l| l.clone())
            .unwrap_or_else(|| (page + 1).to_string())
    };
    let snippet = crate::export::cite_snippet(
        crate::export::preferred_format(),
        &text,
        &locator,
        false,
        ctx.host.citation_key().as_deref(),
    );
    copy_to_clipboard(&ctx.host, &snippet);
}

/// The popover offered when a text selection ends: the four colours, underline, strikeout, a
/// note, and copy / copy-with-citation.
fn show_selection_popover(ctx: &MarkCtx, parent: &gtk4::Picture, x: f64, y: f64, page: u16) {
    let popover = gtk4::Popover::new();
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));

    let rows = gtk4::Box::new(Orientation::Vertical, 4);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);

    let colours = gtk4::Box::new(Orientation::Horizontal, 4);
    colours.set_halign(gtk4::Align::Center);
    for (i, color) in crate::palette::HIGHLIGHT_COLORS.iter().enumerate() {
        let button = gtk4::Button::new();
        button.add_css_class("flat");
        button.set_child(Some(&crate::palette::numbered_swatch(color.hex, i + 1)));
        button.set_tooltip_text(Some(&format!(
            "Highlight: {} ({})",
            crate::palette::highlight_label(i),
            i + 1
        )));
        button.update_property(&[gtk4::accessible::Property::Label(&format!(
            "Highlight as {}",
            crate::palette::highlight_label(i)
        ))]);
        let ctx = ctx.clone();
        let popover = popover.clone();
        let hex = color.hex;
        button.connect_clicked(move |_| {
            popover.popdown();
            apply_selection_mark(&ctx, fond_bib::AnnotationKind::Highlight, hex);
        });
        colours.append(&button);
    }
    rows.append(&colours);

    let action = |label: &str, run: Rc<dyn Fn()>| {
        let button = popover_button(label, false);
        let popover = popover.clone();
        button.connect_clicked(move |_| {
            popover.popdown();
            run();
        });
        button
    };
    let default_hex = ctx.reader.borrow().draw_color.clone();
    for (label, kind) in [
        ("Underline", fond_bib::AnnotationKind::Underline),
        ("Strike out", fond_bib::AnnotationKind::Strikeout),
    ] {
        let ctx = ctx.clone();
        let hex = default_hex.clone();
        rows.append(&action(
            label,
            Rc::new(move || {
                apply_selection_mark(&ctx, kind, &hex);
            }),
        ));
    }
    {
        let ctx = ctx.clone();
        rows.append(&action(
            "Add note…",
            Rc::new(move || {
                show_pdf_note_dialog(
                    &ctx.host,
                    &ctx.reader,
                    &ctx.pdf_hash,
                    &ctx.undo_button,
                    &ctx.redo_button,
                    ctx.rebuild_notes.clone(),
                    &ctx.reader_window,
                    ctx.refresh.clone(),
                );
            }),
        ));
    }
    rows.append(&popover_separator());
    {
        let ctx = ctx.clone();
        rows.append(&action(
            "Copy",
            Rc::new(move || copy_selection(&ctx, false)),
        ));
    }
    {
        let ctx = ctx.clone();
        rows.append(&action(
            "Copy with citation",
            Rc::new(move || copy_selection(&ctx, true)),
        ));
    }
    let _ = page;

    popover.set_child(Some(&rows));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

fn save_drag_annotation(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    pdf_hash: &str,
    page: u16,
    geom: DragGeometry,
) -> bool {
    let Some((start, end)) = drag_pdf_points(&geom) else {
        return false;
    };

    let (draw_kind, draw_color) = {
        let r = reader.borrow();
        (r.draw_kind, r.draw_color.clone())
    };
    // "Select text" mode has no `AnnotationKind` to save under — the caller routes that mode
    // to `copy_drag_selection` instead and never reaches here, but guard anyway.
    let Some(draw_kind) = draw_kind else {
        return false;
    };

    // Prefer the actual text under the drag, line-aware (a straight vertical drag through a
    // paragraph selects each in-between line in full, not just the narrow column under the
    // pointer) — over the drag rectangle's own bounding box. Falls back to the plain
    // rectangle when the drag covers no text (a figure, a blank margin).
    let (quads, snippet) = {
        let r = reader.borrow();
        fond_doc::select_text_range(
            r.pdfium,
            &r.bytes,
            page,
            start.0 as f32,
            start.1 as f32,
            end.0 as f32,
            end.1 as f32,
        )
        .ok()
        .flatten()
    }
    .map(|sel| {
        let text = selection_text(&reader.borrow(), page, &sel.quads).unwrap_or(sel.text);
        (sel.quads, Some(text))
    })
    .unwrap_or_else(|| {
        let x0 = start.0.min(end.0);
        let x1 = start.0.max(end.0);
        let y_top = start.1.max(end.1);
        let y_bottom = start.1.min(end.1);
        (
            vec![[x0, y_top, x1, y_top, x0, y_bottom, x1, y_bottom]],
            None,
        )
    });

    let annotation = fond_bib::Annotation::drawn(
        draw_kind,
        page as u32 + 1,
        quads,
        snippet,
        None,
        Some(draw_color),
    );

    push_undo_snapshot(reader);
    {
        let mut r = reader.borrow_mut();
        r.annotations.pdf_hash = Some(pdf_hash.to_string());
        r.annotations.upsert(annotation);
    }

    let write_result = host.save_annotations(&reader.borrow().annotations);
    match write_result {
        Ok(()) => {
            let label = match draw_kind {
                fond_bib::AnnotationKind::Highlight => "Highlight added",
                fond_bib::AnnotationKind::Underline => "Underline added",
                fond_bib::AnnotationKind::Strikeout => "Strikeout added",
                fond_bib::AnnotationKind::Note => "Annotation added",
            };
            host.notify(label);
            true
        }
        Err(e) => {
            host.notify(&e);
            false
        }
    }
}

/// Which annotation (if any) on `page` contains the PDF-space point `(x_pt, y_pt)` — the
/// topmost (most recently added) one whose quad bounding box covers the point, so a
/// right-click over overlapping highlights lands on the one the user most likely means.
/// Returns the annotation's id.
fn annotation_at_pdf_point(
    annotations: &fond_bib::AnnotationSidecar,
    page: u16,
    x_pt: f32,
    y_pt: f32,
) -> Option<String> {
    let page_num = page as u32 + 1;
    annotations
        .annotations
        .iter()
        .rev()
        .find(|a| {
            a.page == Some(page_num)
                && a.quadpoints.iter().any(|q| {
                    let min_x = q[0].min(q[2]).min(q[4]).min(q[6]) as f32;
                    let max_x = q[0].max(q[2]).max(q[4]).max(q[6]) as f32;
                    let min_y = q[1].min(q[3]).min(q[5]).min(q[7]) as f32;
                    let max_y = q[1].max(q[3]).max(q[5]).max(q[7]) as f32;
                    x_pt >= min_x && x_pt <= max_x && y_pt >= min_y && y_pt <= max_y
                })
        })
        .map(|a| a.id.clone())
}

/// The geometry a right-click needs to hit-test against a page's annotations and, if it
/// lands on one, resolve back to PDF space for the popover's own bookkeeping. Grouped like
/// `DragGeometry` for the same reason: fewer loose parameters on `show_pdf_context_menu`.
struct ClickGeometry {
    render_w: u32,
    render_h: u32,
    page: Option<PageGeom>,
    click_x: f64,
    click_y: f64,
}

/// Right-click context menu for the PDF page: if the click landed on an existing
/// highlight/underline/strikeout/note, offers to edit its note text or delete it; otherwise
/// offers to add a new marginal note. Replaces the old "This page" dropdown — editing and
/// deleting now happens at the annotation itself instead of a separate list.
#[allow(clippy::too_many_arguments)]
fn show_pdf_context_menu(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    pdf_hash: &str,
    parent: &gtk4::Picture,
    refresh: Rc<dyn Fn()>,
    rebuild_notes: Rc<dyn Fn()>,
    undo_button: &gtk4::Button,
    redo_button: &gtk4::Button,
    page: u16,
    geom: ClickGeometry,
    reader_window: &adw::Window,
) {
    let ClickGeometry {
        render_w,
        render_h,
        page: page_geom,
        click_x,
        click_y,
    } = geom;

    let hit_id = page_geom
        .filter(|_| render_w > 0 && render_h > 0)
        .map(|g| g.px_to_pdf(click_x, click_y, render_w as f64, render_h as f64))
        .and_then(|(x_pt, y_pt)| {
            annotation_at_pdf_point(&reader.borrow().annotations, page, x_pt as f32, y_pt as f32)
        });

    let popover = gtk4::Popover::new();
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        click_x.round() as i32,
        click_y.round() as i32,
        1,
        1,
    )));
    popover.set_has_arrow(true);

    let rows = gtk4::Box::new(Orientation::Vertical, 2);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);
    rows.set_width_request(240);

    let selection_here = reader
        .borrow()
        .last_selection
        .as_ref()
        .filter(|(sel_page, _, _)| *sel_page == page)
        .map(|(_, text, _)| text.clone());
    if let Some(text) = selection_here.clone() {
        let copy = popover_button("Copy selected text", false);
        let host = host.clone();
        let popover = popover.clone();
        copy.connect_clicked(move |_| {
            copy_to_clipboard(&host, &text);
            popover.popdown();
        });
        rows.append(&copy);
    }

    match hit_id {
        Some(id) => {
            let annotation = reader
                .borrow()
                .annotations
                .annotations
                .iter()
                .find(|a| a.id == id)
                .cloned();
            let Some(annotation) = annotation else {
                return;
            };

            let kind_row = gtk4::Box::new(Orientation::Horizontal, 6);
            if let Some(swatch) = color_swatch(annotation.color.as_deref()) {
                kind_row.append(&swatch);
            }
            let kind_label = gtk4::Label::new(Some(&format!("{:?}", annotation.kind)));
            kind_label.set_xalign(0.0);
            kind_label.add_css_class("dim-label");
            kind_row.append(&kind_label);
            if selection_here.is_some() {
                rows.append(&popover_separator());
            }
            rows.append(&kind_row);

            // Re-read from the page rather than trusting `snippet`, which for annotations
            // saved before `selection_text` existed has its words run together.
            let marked_text = selection_text(&reader.borrow(), page, &annotation.quadpoints)
                .or_else(|| annotation.snippet.clone())
                .filter(|t| !t.trim().is_empty());
            if let Some(text) = marked_text {
                let label = match annotation.kind {
                    fond_bib::AnnotationKind::Underline => "Copy underlined text",
                    fond_bib::AnnotationKind::Strikeout => "Copy struck-out text",
                    fond_bib::AnnotationKind::Note => "Copy noted text",
                    fond_bib::AnnotationKind::Highlight => "Copy highlighted text",
                };
                let copy = popover_button(label, false);
                let host = host.clone();
                let popover = popover.clone();
                copy.connect_clicked(move |_| {
                    copy_to_clipboard(&host, &text);
                    popover.popdown();
                });
                rows.append(&copy);
            }

            let save_note = {
                let host = host.clone();
                let reader = reader.clone();
                let id = id.clone();
                let undo_button = undo_button.clone();
                let redo_button = redo_button.clone();
                let rebuild_notes = rebuild_notes.clone();
                move |text: &str| {
                    let text = text.trim();
                    let current_note = reader
                        .borrow()
                        .annotations
                        .annotations
                        .iter()
                        .find(|a| a.id == id)
                        .and_then(|a| a.note.clone());
                    if current_note.as_deref().unwrap_or("") == text {
                        return;
                    }
                    push_undo_snapshot(&reader);
                    {
                        let mut r = reader.borrow_mut();
                        if let Some(a) = r.annotations.annotations.iter_mut().find(|a| a.id == id) {
                            a.note = (!text.is_empty()).then(|| text.to_string());
                        }
                    }
                    let write_result = host.save_annotations(&reader.borrow().annotations);
                    match write_result {
                        Ok(()) => {
                            sync_undo_redo_buttons(&reader, &undo_button, &redo_button);
                            rebuild_notes();
                        }
                        Err(e) => host.notify(&e),
                    }
                }
            };
            let note_widget =
                note_edit_widget(annotation.note.as_deref(), move |text| save_note(&text));
            rows.append(&note_widget);

            rows.append(&popover_separator());
            let delete_button = popover_button("Delete annotation", true);
            {
                let host = host.clone();
                let reader = reader.clone();
                let refresh = refresh.clone();
                let rebuild_notes = rebuild_notes.clone();
                let popover = popover.clone();
                let id = id.clone();
                let undo_button = undo_button.clone();
                let redo_button = redo_button.clone();
                delete_button.connect_clicked(move |_| {
                    push_undo_snapshot(&reader);
                    reader
                        .borrow_mut()
                        .annotations
                        .annotations
                        .retain(|a| a.id != id);
                    let write_result = host.save_annotations(&reader.borrow().annotations);
                    match write_result {
                        Ok(()) => {
                            refresh();
                            sync_undo_redo_buttons(&reader, &undo_button, &redo_button);
                            rebuild_notes();
                            host.notify("Annotation deleted");
                        }
                        Err(e) => host.notify(&e),
                    }
                    popover.popdown();
                });
            }
            rows.append(&delete_button);
        }
        None => {
            // "Create note from selection…" instead of the generic "Add note here" when
            // there's an active "Select text" drag on this page (see `last_selection`) —
            // same underlying dialog either way (it already pre-fills from the selection and
            // carries its quadpoints when present), just a label that says what's actually
            // about to happen instead of always the generic one.
            let has_selection_here = selection_here.is_some();
            let add_note = popover_button(
                if has_selection_here {
                    "Create note from selection…"
                } else {
                    "Add note here"
                },
                false,
            );
            {
                let host = host.clone();
                let reader = reader.clone();
                let pdf_hash = pdf_hash.to_string();
                let undo_button = undo_button.clone();
                let redo_button = redo_button.clone();
                let popover = popover.clone();
                let rebuild_notes = rebuild_notes.clone();
                let reader_window = reader_window.clone();
                let refresh = refresh.clone();
                add_note.connect_clicked(move |_| {
                    show_pdf_note_dialog(
                        &host,
                        &reader,
                        &pdf_hash,
                        &undo_button,
                        &redo_button,
                        rebuild_notes.clone(),
                        &reader_window,
                        refresh.clone(),
                    );
                    popover.popdown();
                });
            }
            rows.append(&add_note);
        }
    }

    popover.set_child(Some(&rows));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

/// Build continuous-scroll mode's per-page `Picture` widgets, if not already built. A no-op
/// if `reader.continuous_pictures` is already populated (from an earlier toggle-on this
/// session).
///
/// Widget layout (sizes and offsets) is computed eagerly from each page's cheap PDF-point
/// metadata alone — every page gets one *permanent* widget up front, so its drag-to-annotate
/// gesture can capture that page's index directly with no risk of a recycled widget later
/// belonging to a different page (the failure mode a `ListView`-based virtualized version
/// would have to guard against), and so scrolling to any page works immediately. Actually
/// rasterizing each page's texture is the expensive part (PDFium render), so that's deferred
/// to `schedule_continuous_render`, spread one page per idle tick starting from the reader's
/// current page — this used to run inline here, which blocked the whole UI thread for the
/// entire document on every open once continuous mode became the default (previously it only
/// cost anything on an explicit toggle-on, rare enough not to notice).
#[allow(clippy::too_many_arguments)]
fn build_continuous_view(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    pdf_hash: &str,
    continuous_box: &gtk4::Box,
    continuous_scroll: &gtk4::ScrolledWindow,
    undo_button: &gtk4::Button,
    redo_button: &gtk4::Button,
    rebuild_notes: &Rc<dyn Fn()>,
    reader_window: &adw::Window,
) {
    if !reader.borrow().continuous_pictures.is_empty() {
        return;
    }
    let (count, zoom, current_page) = {
        let r = reader.borrow();
        (r.count, r.zoom, r.page)
    };

    let mut pictures = Vec::with_capacity(count as usize);
    let mut offsets = Vec::with_capacity(count as usize + 1);
    let mut y = 0.0f64;

    let select_mode = reader.borrow().draw_kind.is_none();
    for page in 0..count {
        let picture = gtk4::Picture::new();
        picture.set_halign(gtk4::Align::Center);
        picture.set_can_target(true);
        picture.set_cursor(cursor_for_select_mode(select_mode).as_ref());

        // Plausible size from the page's own point dimensions — cheap metadata, not a
        // rasterization — so the layout is correct before this page's texture has rendered.
        let pts = reader.borrow().display_size(page);
        let w = (READER_BASE_WIDTH * zoom) as u32;
        let h = if pts.0 > 0.0 {
            (w as f32 * pts.1 / pts.0) as u32
        } else {
            (w as f32 * 792.0 / 612.0) as u32
        };
        picture.set_size_request(w as i32, h as i32);
        offsets.push(y);
        y += h as f64 + CONTINUOUS_PAGE_GAP;

        let (page_overlay, drag_preview, drag_live_rect) =
            build_drag_preview_overlay(&picture, reader, move || page);
        page_overlay.set_halign(gtk4::Align::Center);

        // Drag-to-annotate on this page's own permanent Picture — `page` is captured by
        // value, so (unlike a recycled `ListView` row) it can never go stale.
        {
            let drag = gtk4::GestureDrag::new();
            let host = host.clone();
            let reader = reader.clone();
            let reader_window = reader_window.clone();
            let pdf_hash = pdf_hash.to_string();
            let this_picture = picture.clone();
            let undo_button = undo_button.clone();
            let redo_button = redo_button.clone();
            let rebuild_notes = rebuild_notes.clone();
            {
                let live_rect = drag_live_rect.clone();
                let drag_preview = drag_preview.clone();
                drag.connect_drag_begin(move |_gesture, start_x, start_y| {
                    live_rect.set(Some((start_x, start_y, start_x, start_y)));
                    drag_preview.queue_draw();
                });
            }
            {
                let live_rect = drag_live_rect.clone();
                let drag_preview = drag_preview.clone();
                drag.connect_drag_update(move |gesture, offset_x, offset_y| {
                    let Some((start_x, start_y)) = gesture.start_point() else {
                        return;
                    };
                    live_rect.set(Some((
                        start_x,
                        start_y,
                        start_x + offset_x,
                        start_y + offset_y,
                    )));
                    drag_preview.queue_draw();
                });
            }
            {
                let live_rect = drag_live_rect.clone();
                let drag_preview = drag_preview.clone();
                drag.connect_drag_end(move |gesture, offset_x, offset_y| {
                    live_rect.set(None);
                    drag_preview.queue_draw();
                    if offset_x.abs() < MIN_DRAG_PX && offset_y.abs() < MIN_DRAG_PX {
                        return;
                    }
                    let Some((start_x, start_y)) = gesture.start_point() else {
                        return;
                    };
                    let end_x = start_x + offset_x;
                    let end_y = start_y + offset_y;
                    let render_w = this_picture.width().max(0) as u32;
                    let render_h = this_picture.height().max(0) as u32;
                    let geom = DragGeometry {
                        render_w,
                        render_h,
                        page: reader.borrow().geom(page),
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                    };
                    if reader.borrow().draw_kind.is_none() {
                        if select_drag_text(&host, &reader, page, &geom) {
                            render_continuous_page(&reader, page);
                            let ctx = MarkCtx {
                                host: host.clone(),
                                reader: reader.clone(),
                                pdf_hash: pdf_hash.clone(),
                                undo_button: undo_button.clone(),
                                redo_button: redo_button.clone(),
                                rebuild_notes: rebuild_notes.clone(),
                                reader_window: reader_window.clone(),
                                refresh: {
                                    let reader = reader.clone();
                                    Rc::new(move || render_continuous_page(&reader, page))
                                },
                            };
                            show_selection_popover(&ctx, &this_picture, end_x, end_y, page);
                        }
                        return;
                    }
                    let saved = save_drag_annotation(&host, &reader, &pdf_hash, page, geom);
                    if saved {
                        render_continuous_page(&reader, page);
                        sync_undo_redo_buttons(&reader, &undo_button, &redo_button);
                        rebuild_notes();
                    }
                });
            }
            picture.add_controller(drag);
        }

        // Right-click: edit/delete the annotation under the cursor, or add a note — same
        // context menu as the paged view, hit-testing against this page's own picture size
        // (each page can differ slightly after per-page render fallbacks).
        {
            let click = gtk4::GestureClick::new();
            click.set_button(gdk::BUTTON_SECONDARY);
            let host = host.clone();
            let reader = reader.clone();
            let pdf_hash = pdf_hash.to_string();
            let this_picture = picture.clone();
            let undo_button = undo_button.clone();
            let redo_button = redo_button.clone();
            let rebuild_notes = rebuild_notes.clone();
            let reader_window = reader_window.clone();
            click.connect_pressed(move |_gesture, _n, x, y| {
                let render_w = this_picture.width().max(0) as u32;
                let render_h = this_picture.height().max(0) as u32;
                let page_geom = reader.borrow().geom(page);
                let refresh: Rc<dyn Fn()> = {
                    let reader = reader.clone();
                    Rc::new(move || render_continuous_page(&reader, page))
                };
                show_pdf_context_menu(
                    &host,
                    &reader,
                    &pdf_hash,
                    &this_picture,
                    refresh,
                    rebuild_notes.clone(),
                    &undo_button,
                    &redo_button,
                    page,
                    ClickGeometry {
                        render_w,
                        render_h,
                        page: page_geom,
                        click_x: x,
                        click_y: y,
                    },
                    &reader_window,
                );
            });
            picture.add_controller(click);
        }

        // Click-to-turn zones — same 20/60/20 split and same-widget coexistence-with-drag
        // reasoning as the paged view's own (see that block's comment); "previous"/"next"
        // here means scrolling to the adjacent page, since there are no prev/next buttons
        // to reuse inside this per-page loop.
        {
            let press_pos: Rc<Cell<Option<(f64, f64)>>> = Rc::new(Cell::new(None));
            let click_nav = gtk4::GestureClick::new();
            click_nav.set_button(gdk::BUTTON_PRIMARY);
            {
                let press_pos = press_pos.clone();
                click_nav.connect_pressed(move |_, _, x, y| press_pos.set(Some((x, y))));
            }
            {
                let this_picture = picture.clone();
                let reader = reader.clone();
                let continuous_scroll = continuous_scroll.clone();
                click_nav.connect_released(move |_, _, x, y| {
                    let Some((sx, sy)) = press_pos.take() else {
                        return;
                    };
                    if (x - sx).abs() > MIN_DRAG_PX || (y - sy).abs() > MIN_DRAG_PX {
                        return;
                    }
                    let w = this_picture.width().max(1) as f64;
                    let h = this_picture.height().max(1) as f64;
                    if follow_link(&reader, page, x, y, w, h) {
                        return;
                    }
                    if x < w * 0.2 {
                        let target = page.saturating_sub(1);
                        scroll_continuous_to_page(&reader, &continuous_scroll, target);
                    } else if x > w * 0.8 {
                        let target = (page + 1).min(count.saturating_sub(1));
                        scroll_continuous_to_page(&reader, &continuous_scroll, target);
                    }
                });
            }
            picture.add_controller(click_nav);
        }

        continuous_box.append(&page_overlay);
        pictures.push(picture);
    }
    offsets.push(y); // sentinel: total content height

    {
        let mut r = reader.borrow_mut();
        r.continuous_pictures = pictures;
        r.continuous_offsets = offsets;
        r.continuous_rendered = vec![false; count as usize];
    }

    refresh_continuous_window(reader, continuous_scroll, Some(current_page));
}

/// Pages kept rendered on each side of the viewport, in viewports.
const CONTINUOUS_KEEP_VIEWPORTS: f64 = 1.5;

/// Decide which pages should be rendered (viewport plus a margin), unload the rest, and
/// render the missing ones one per idle tick, nearest first. `focus_page` overrides the
/// scroll position for the very first build, before GTK has laid anything out.
fn refresh_continuous_window(
    reader: &Rc<RefCell<ReaderState>>,
    scroll: &gtk4::ScrolledWindow,
    focus_page: Option<u16>,
) {
    let adj = scroll.vadjustment();
    let viewport = if adj.page_size() > 1.0 {
        adj.page_size()
    } else {
        1000.0
    };
    let (lo, hi, center) = {
        let r = reader.borrow();
        if r.continuous_offsets.len() < 2 {
            return;
        }
        let (top, center) = match focus_page {
            Some(p) => {
                let t = r.continuous_offsets.get(p as usize).copied().unwrap_or(0.0);
                (t, t + viewport / 2.0)
            }
            None => (adj.value(), adj.value() + viewport / 2.0),
        };
        let keep = viewport * CONTINUOUS_KEEP_VIEWPORTS;
        let lo = continuous_page_at(&r.continuous_offsets, (top - keep).max(0.0));
        let hi = continuous_page_at(&r.continuous_offsets, top + viewport + keep);
        (lo, hi, center)
    };
    let mut missing: Vec<u16> = Vec::new();
    {
        let mut r = reader.borrow_mut();
        r.continuous_window = (lo, hi);
        for page in 0..r.continuous_pictures.len() as u16 {
            let in_window = page >= lo && page <= hi;
            let rendered = r
                .continuous_rendered
                .get(page as usize)
                .copied()
                .unwrap_or(false);
            if in_window && !rendered {
                missing.push(page);
            } else if !in_window && rendered {
                r.continuous_pictures[page as usize].set_paintable(gdk::Paintable::NONE);
                r.continuous_rendered[page as usize] = false;
            }
        }
        let offsets = r.continuous_offsets.clone();
        missing.sort_by(|a, b| {
            let da = (offsets[*a as usize] - center).abs();
            let db = (offsets[*b as usize] - center).abs();
            da.total_cmp(&db)
        });
    }
    if !missing.is_empty() {
        schedule_continuous_render(reader.clone(), missing, 0);
    }
}

/// Rasterize one page of `order`, then yield to the main loop before the next, so filling
/// the window never blocks the UI for more than one page's render. Pages that have since
/// left the window (the user scrolled on) or were already rendered are skipped.
fn schedule_continuous_render(reader: Rc<RefCell<ReaderState>>, order: Vec<u16>, idx: usize) {
    let Some(&page) = order.get(idx) else {
        return;
    };
    let wanted = {
        let r = reader.borrow();
        let (lo, hi) = r.continuous_window;
        r.continuous_pictures.get(page as usize).is_some()
            && page >= lo
            && page <= hi
            && !r
                .continuous_rendered
                .get(page as usize)
                .copied()
                .unwrap_or(true)
    };
    if wanted {
        render_continuous_page(&reader, page);
    }
    glib::idle_add_local_once(move || {
        schedule_continuous_render(reader, order, idx + 1);
    });
}

/// Tear down and rebuild continuous-scroll mode's widgets after a zoom change — page pixel
/// sizes all changed, so every offset is stale too. A no-op if continuous mode was never
/// built (the next toggle-on will build fresh at the new zoom already). Simpler than
/// resizing everything in place: zoom changes are infrequent, so paying a full rebuild is a
/// reasonable trade for not having two code paths (initial build vs. resize-in-place) to
/// keep in sync.
#[allow(clippy::too_many_arguments)]
fn rebuild_continuous_view_for_zoom(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    pdf_hash: &str,
    continuous_box: &gtk4::Box,
    continuous_scroll: &gtk4::ScrolledWindow,
    undo_button: &gtk4::Button,
    redo_button: &gtk4::Button,
    rebuild_notes: &Rc<dyn Fn()>,
    reader_window: &adw::Window,
) {
    if reader.borrow().continuous_pictures.is_empty() {
        return;
    }
    while let Some(child) = continuous_box.first_child() {
        continuous_box.remove(&child);
    }
    {
        let mut r = reader.borrow_mut();
        r.continuous_pictures.clear();
        r.continuous_offsets.clear();
        r.continuous_rendered.clear();
    }
    build_continuous_view(
        host,
        reader,
        pdf_hash,
        continuous_box,
        continuous_scroll,
        undo_button,
        redo_button,
        rebuild_notes,
        reader_window,
    );
}

/// Re-render one page's `Picture` in continuous-scroll mode in place (after an annotation on
/// it changed) — its position doesn't move, only its content, so this doesn't touch
/// `continuous_offsets`.
fn render_continuous_page(reader: &Rc<RefCell<ReaderState>>, page: u16) {
    let picture = {
        let r = reader.borrow();
        r.continuous_pictures.get(page as usize).cloned()
    };
    let Some(picture) = picture else {
        return;
    };
    let mut r = reader.borrow_mut();
    if let Some((texture, w, h)) = render_pdf_page_texture(&r, page) {
        picture.set_paintable(Some(&texture));
        picture.set_size_request(w as i32, h as i32);
        if let Some(flag) = r.continuous_rendered.get_mut(page as usize) {
            *flag = true;
        }
    }
}

/// Re-render only the continuous pages that currently hold a texture; the rest pick up the
/// new state whenever they next scroll into the window.
fn rerender_loaded_continuous_pages(reader: &Rc<RefCell<ReaderState>>) {
    let loaded: Vec<u16> = {
        let r = reader.borrow();
        r.continuous_rendered
            .iter()
            .enumerate()
            .filter(|(_, rendered)| **rendered)
            .map(|(i, _)| i as u16)
            .collect()
    };
    for page in loaded {
        render_continuous_page(reader, page);
    }
}

/// Scroll continuous-scroll mode's `ScrolledWindow` so `page` is at the top of the viewport.
fn scroll_continuous_to_page(
    reader: &Rc<RefCell<ReaderState>>,
    scroll: &gtk4::ScrolledWindow,
    page: u16,
) {
    let offset = {
        let r = reader.borrow();
        r.continuous_offsets
            .get(page as usize)
            .copied()
            .unwrap_or(0.0)
    };
    scroll.vadjustment().set_value(offset);
}

/// Which page's span contains vertical position `y` (both in continuous-scroll pixel space)
/// — the last page whose own top offset is at or above `y`. `offsets` is
/// `ReaderState.continuous_offsets`: `count` real page-top offsets plus one trailing
/// sentinel (the total content height), ascending.
fn continuous_page_at(offsets: &[f64], y: f64) -> u16 {
    if offsets.len() < 2 {
        return 0;
    }
    let count = offsets.len() - 1;
    let i = offsets[..count].partition_point(|&o| o <= y);
    i.saturating_sub(1).min(count.saturating_sub(1)) as u16
}

/// Reflect whether the current page is bookmarked on the toggle button's icon/tooltip.
pub(crate) fn update_bookmark_button(button: &gtk4::Button, bookmarked: bool) {
    button.set_icon_name(if bookmarked {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    });
    button.set_tooltip_text(Some(if bookmarked {
        "Remove bookmark (B)"
    } else {
        "Bookmark this page (B)"
    }));
}

/// Refresh the page-number entry/label/prev-next sensitivity for `page` (0-based) — shared
/// by the paged view's `render()` and continuous mode's scroll-position tracker, so the two
/// can't disagree about how the current page is displayed. Shows the document's own printed
/// label when the PDF defines one (`page_labels[page]`), falling back to the raw 1-based
/// file position otherwise — identical to the pre-`/PageLabels`-aware behaviour for the
/// common case of a PDF with no custom numbering.
#[allow(clippy::too_many_arguments)]
fn update_page_display(
    page_entry: &gtk4::Entry,
    page_of_label: &gtk4::Label,
    prev: &gtk4::Button,
    next: &gtk4::Button,
    bookmark_button: &gtk4::Button,
    page: u16,
    count: u16,
    page_labels: &[Option<String>],
    bookmarks: &[u32],
) {
    let raw = (page + 1).to_string();
    let label = page_labels
        .get(page as usize)
        .and_then(|l| l.clone())
        .unwrap_or_else(|| raw.clone());
    page_entry.set_text(&label);
    // Reading-progress indicator: the raw file position already says "how far in", but the
    // percentage reads at a glance without doing the division yourself.
    let percent = if count > 0 {
        ((page as u32 + 1) * 100 / count as u32).min(100)
    } else {
        0
    };
    page_of_label.set_text(&format!("of {count} · {percent}%"));
    // The tooltip always gives the raw file position too — a PDF's `/PageLabels` isn't
    // required to be unique or even present on every page, but the raw number is the one
    // every internal API here (`Annotation.page`, `PdfSearchMatch.page`, `Contents` targets)
    // always means, so it's worth surfacing even when the printed label differs.
    page_entry.set_tooltip_text(Some(&format!("Page {raw} of {count} in the file")));
    prev.set_sensitive(page > 0);
    next.set_sensitive(page + 1 < count);
    update_bookmark_button(bookmark_button, bookmarks.contains(&(page as u32 + 1)));
}

/// Resolve typed text in the page-number entry to a 0-based page index: an exact match
/// against the document's own printed labels first (case-insensitive, since a roman numeral
/// typed in the "wrong" case should still work), falling back to parsing it as a raw 1-based
/// file page number — so typing still works exactly as before on a PDF with no
/// `/PageLabels`, and a user who prefers raw numbers can always use them even on one that
/// has them.
fn find_page_by_label(page_labels: &[Option<String>], text: &str) -> Option<u16> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(idx) = page_labels.iter().position(|l| l.as_deref() == Some(text)) {
        return Some(idx as u16);
    }
    let lower = text.to_lowercase();
    if let Some(idx) = page_labels
        .iter()
        .position(|l| l.as_deref().map(|s| s.to_lowercase()) == Some(lower.clone()))
    {
        return Some(idx as u16);
    }
    text.parse::<u16>().ok().and_then(|n| n.checked_sub(1))
}

/// A built-in PDF reader: renders pages with PDFium to RGBA textures, with page navigation,
/// zoom, and click-drag highlighting. No Poppler (GPL) — pure PDFium (BSD), the same binding
/// used for text extraction. Highlights are the on-disk `Annotation` sidecar format `fond-bib`
/// already defines (`annots/<key>.json`) — this is its first writer; until now only PDF
/// import/export touched it.
/// `start_page` is 1-based (matching `Annotation.page`), clamped into range; pass `1` to
/// just open at the first page.
pub fn show_pdf_reader(
    host: &Rc<dyn ReaderHost>,
    window: &adw::ApplicationWindow,
    pdf_hash: &str,
    blob: &std::path::Path,
    title: &str,
    start_page: u32,
) {
    // Already open? Surface it instead of opening a duplicate reader on the same file — two
    // readers on the same document would each keep their own in-memory annotations/progress
    // snapshot and clobber each other's saves. See `crate::OPEN_READERS`.
    if crate::present_existing(pdf_hash) {
        return;
    }
    let bytes = match std::fs::read(blob) {
        Ok(b) => b,
        Err(e) => {
            gtk4::AlertDialog::builder()
                .message("Could not open PDF")
                .detail(e.to_string())
                .build()
                .show(Some(window));
            return;
        }
    };
    let pdfium = match fond_doc::bind_pdfium() {
        Ok(p) => p,
        Err(e) => {
            gtk4::AlertDialog::builder()
                .message("PDF reader unavailable")
                .detail(format!("PDFium could not be loaded: {e}"))
                .build()
                .show(Some(window));
            return;
        }
    };
    let count = fond_doc::page_count(pdfium, &bytes).unwrap_or(1).max(1);
    // Empty for most PDFs — outlines are the exception, not the rule — so the Contents
    // button below only appears when there's actually something to jump to.
    let outline_entries = fond_doc::outline(pdfium, &bytes).unwrap_or_default();
    // Likewise empty for most PDFs (no custom /PageLabels) — falls back to the raw page
    // number wherever it's displayed. When the PDF declares nothing of its own, fall back to
    // a manually-set `page_label_override` on the entry's note (see "Set page numbering…"
    // below) — this is the only way to get printed-page-number navigation on the common case
    // of a scanned or older PDF with no `/PageLabels` dictionary at all.
    let native_page_labels = fond_doc::page_labels(pdfium, &bytes).unwrap_or_default();
    let has_native_page_labels = native_page_labels.iter().any(|l| l.is_some());
    let page_label_override = host.page_label_override();
    let page_labels = if has_native_page_labels {
        native_page_labels
    } else {
        page_label_override
            .map(|ov| ov.apply(count))
            .unwrap_or(native_page_labels)
    };

    let annotations = host.load_annotations();
    let mut bookmarks = host.load_bookmarks();
    bookmarks.sort_unstable();

    let start_page = start_page
        .saturating_sub(1)
        .min(count.saturating_sub(1) as u32) as u16;
    let doc = pdfium.load_pdf_from_byte_vec(bytes.clone(), None).ok();
    let reader = Rc::new(RefCell::new(ReaderState {
        pdfium,
        bytes,
        doc,
        page: start_page,
        count,
        zoom: 1.0,
        annotations,
        render_px: (0, 0),
        page_geoms: RefCell::new(std::collections::HashMap::new()),
        draw_kind: None,
        last_selection: None,
        search_matches: Vec::new(),
        search_current: 0,
        draw_color: crate::palette::HIGHLIGHT_COLORS[0].hex.to_string(),
        continuous_pictures: Vec::new(),
        continuous_offsets: Vec::new(),
        continuous_rendered: Vec::new(),
        continuous_window: (0, 0),
        link_goto: None,
        nav_back: Vec::new(),
        page_labels,
        undo_stack: Vec::new(),
        redo_stack: Vec::new(),
        rotation: 0,
        invert_colors: false,
        bookmarks,
    }));

    let view = adw::ToolbarView::new();
    // No header of this tab's own any more — its controls are handed to the shared host
    // header via `reader_host::set_tab_header` below instead, so there's one header row
    // total rather than the host's own plus a second, per-tab one underneath it. Explicit
    // title widget, not left to the default (the containing window's own title): the reader
    // host window is shared across every open tab now, so its title can't speak for any one
    // document — this used to show "Reader" twice (the tab host's own header fell back to
    // the same window title, since it didn't set a title widget either).
    let title_widget = adw::WindowTitle::new(title, "");

    let prev = gtk4::Button::from_icon_name("go-previous-symbolic");
    prev.add_css_class("flat");
    prev.set_tooltip_text(Some("Previous page"));
    let next = gtk4::Button::from_icon_name("go-next-symbolic");
    next.add_css_class("flat");
    next.set_tooltip_text(Some("Next page"));
    // Shows (and, on Enter, navigates by) the *document's own* printed page number — its
    // `/PageLabels` numbering, e.g. roman-numeral front matter restarting at arabic "1" for
    // the body, rather than always the raw file position.
    // Most PDFs have no `/PageLabels` at all, in which case this just shows the raw number,
    // identical to before.
    let page_entry = gtk4::Entry::new();
    page_entry.set_width_chars(5);
    page_entry.set_max_width_chars(5);
    gtk4::prelude::EntryExt::set_alignment(&page_entry, 0.5);
    let page_of_label = gtk4::Label::new(None);
    page_of_label.add_css_class("dim-label");
    // A lightweight "come back to this" marker, distinct from an annotation — see
    // `ReaderState::bookmarks`. Lives beside page nav (not in the header) since it acts on
    // "the current page", the same thing page nav already shows.
    let bookmark_button = gtk4::Button::new();
    bookmark_button.add_css_class("flat");
    update_bookmark_button(
        &bookmark_button,
        reader.borrow().bookmarks.contains(&(start_page as u32 + 1)),
    );
    // Page nav lives in the bottom status bar (below), not the headerbar's title-widget slot
    // — that slot is left to the default window title (the document's own name) instead.
    let nav = gtk4::Box::new(Orientation::Horizontal, 6);
    nav.append(&prev);
    nav.append(&page_entry);
    nav.append(&page_of_label);
    nav.append(&next);
    nav.append(&bookmark_button);

    let zoom_out = gtk4::Button::from_icon_name("zoom-out-symbolic");
    zoom_out.add_css_class("flat");
    zoom_out.set_tooltip_text(Some("Zoom out"));
    let zoom_in = gtk4::Button::from_icon_name("zoom-in-symbolic");
    zoom_in.add_css_class("flat");
    zoom_in.set_tooltip_text(Some("Zoom in"));
    let zoom_fit_width = gtk4::Button::from_icon_name("view-fullscreen-symbolic");
    zoom_fit_width.add_css_class("flat");
    zoom_fit_width.set_tooltip_text(Some("Zoom to fit width"));
    let zoom_fit_page = gtk4::Button::from_icon_name("zoom-fit-best-symbolic");
    zoom_fit_page.add_css_class("flat");
    zoom_fit_page.set_tooltip_text(Some("Zoom to fit page"));

    // View-only rotation — see `ReaderState::rotation`'s doc comment for why this is
    // single-page-mode only (disabled below whenever Continuous or Two-page is active).
    let rotate_button = gtk4::Button::from_icon_name("object-rotate-right-symbolic");
    rotate_button.add_css_class("flat");
    rotate_button.set_tooltip_text(Some("Rotate page 90°"));

    let invert_button = gtk4::ToggleButton::new();
    invert_button.set_icon_name("weather-clear-night-symbolic");
    invert_button.add_css_class("flat");
    invert_button.set_tooltip_text(Some("Invert colours (for reading at night)"));

    let note_button = gtk4::Button::with_label("Note…");
    note_button.set_tooltip_text(Some("Add a marginal note on the current page"));

    // Lets the current physical page be anchored to its own printed number when the PDF
    // declares no `/PageLabels` of its own (common for scanned or older PDFs) — the manual
    // counterpart to the automatic `/PageLabels` read above. Disabled when the PDF already
    // has native labels, since those are authoritative and an override on top would be
    // silently ignored (see the `page_labels` fallback above) — better to say so up front
    // than let the user set something with no visible effect.
    let page_num_button = gtk4::Button::with_label("Page #…");
    if has_native_page_labels {
        page_num_button.set_sensitive(false);
        page_num_button.set_tooltip_text(Some("This PDF already declares its own page numbers"));
    } else {
        page_num_button.set_tooltip_text(Some("Set the printed page number for this page"));
    }

    // What a drag on the page does: the palette picks Select text or a colour, and the style
    // drop-down picks which kind of mark that colour draws. Both feed one `apply_mode` closure,
    // wired below once `hint`/`picture` exist.
    let mode_change: crate::RebuildCell = Rc::new(RefCell::new(None));
    let style_labels: Vec<&str> = MARK_KIND_OPTIONS.iter().map(|(l, _)| *l).collect();
    let style_drop = gtk4::DropDown::from_strings(&style_labels);
    style_drop.set_tooltip_text(Some("What kind of mark a drag draws"));
    style_drop.set_sensitive(false);
    let palette_choice: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    let palette = {
        let palette_choice = palette_choice.clone();
        let mode_change = mode_change.clone();
        crate::palette::palette_widget(true, None, move |choice| {
            palette_choice.set(choice);
            if let Some(f) = mode_change.borrow().as_ref() {
                f();
            }
        })
    };

    // Always enabled — Thumbnails is always available even for a PDF with no outline
    // (unlike before this toggle covered Contents alone and was disabled without one).
    // Toggles a persistent sidebar (built below, after `render`/`reader` exist) rather than
    // a popover, per CLAUDE.md's house sidebar style: toggle at the *start* of the
    // headerbar, content as a collapsible Paned start-child.
    let sidebar_toggle = gtk4::ToggleButton::new();
    crate::set_icon_with_fallback(
        &sidebar_toggle,
        &[
            "sidebar-show-symbolic",
            "view-sidebar-symbolic",
            "sidebar-expand-left-symbolic",
            "view-list-symbolic",
        ],
    );
    sidebar_toggle.set_tooltip_text(Some("Show contents / thumbnails"));

    // Whole-document notes/highlights list, in a persistent sidebar (built below, alongside
    // Contents) rather than the old per-page "This page" dropdown — readable prose, not just
    // on-page markers, and reachable regardless of which page is current. Editing/deleting
    // an individual annotation now happens by right-clicking it on the page itself.
    let notes_toggle = gtk4::ToggleButton::new();
    notes_toggle.set_icon_name("view-list-symbolic");
    notes_toggle.set_tooltip_text(Some("Show notes and highlights"));

    let continuous_toggle = gtk4::ToggleButton::new();
    crate::set_icon_with_fallback(
        &continuous_toggle,
        &["view-continuous-symbolic", "view-paged-symbolic"],
    );
    continuous_toggle.set_tooltip_text(Some(
        "Continuous — scroll through every page, instead of one page at a time",
    ));
    // Mutually exclusive with `continuous_toggle` (each deactivates the other on activate,
    // wired below) rather than a single 3-way control, so every existing
    // `continuous_toggle.is_active()` check elsewhere keeps meaning exactly what it always
    // did with no changes needed at those call sites. Reuses the single-page view's own
    // `view_stack` child and `render()` (extended to also fill `right_picture`) rather than
    // being a separate mode with its own render path, so navigation/zoom/search/outline/
    // notes-sidebar jumps all stay in sync with two-page mode for free.
    let two_page_toggle = gtk4::ToggleButton::new();
    crate::set_icon_with_fallback(
        &two_page_toggle,
        &["view-dual-symbolic", "view-paged-symbolic"],
    );
    two_page_toggle.set_tooltip_text(Some(
        "Two-page — show two facing pages side by side, like an open book. Drawing a new \
         highlight or note still needs single-page or Continuous mode.",
    ));

    let undo_button = gtk4::Button::from_icon_name("edit-undo-symbolic");
    undo_button.set_tooltip_text(Some("Undo (Ctrl+Z)"));
    undo_button.set_sensitive(false);
    let redo_button = gtk4::Button::from_icon_name("edit-redo-symbolic");
    redo_button.set_tooltip_text(Some("Redo (Ctrl+Shift+Z)"));
    redo_button.set_sensitive(false);

    // Moves this tab out of the shared "Reader" window into its own standalone one — the
    // only way to detach a tab (see `reader_host`'s module doc for why there's no drag-out-
    // of-the-bar gesture too).
    let popout_button = gtk4::Button::from_icon_name("window-new-symbolic");
    popout_button.add_css_class("flat");
    popout_button.set_tooltip_text(Some("Open in a new window"));

    let export_button = gtk4::Button::from_icon_name("document-save-symbolic");
    export_button.add_css_class("flat");
    export_button.set_tooltip_text(Some("Export notes & highlights…"));

    // Visual order, left to right, in the shared host header: sidebar toggle, Undo, Redo
    // (start) … document title (centre) … Two-page, Continuous, colour palette, mark style,
    // Note, Page #, Export, Open in new window, Notes sidebar (end) — unchanged from when
    // these lived in this tab's own `HeaderBar`, just built as plain boxes now and handed to
    // `reader_host::set_tab_header` below instead of packed directly (see the comment on
    // `title_widget` above for why). Thumbnails used to have its own header button opening a
    // popup grid; it's now a tab in the sidebar (built below), alongside Contents.
    let header_start = gtk4::Box::new(Orientation::Horizontal, 6);
    header_start.append(&sidebar_toggle);
    header_start.append(&undo_button);
    header_start.append(&redo_button);
    let header_end = gtk4::Box::new(Orientation::Horizontal, 6);
    header_end.append(&two_page_toggle);
    header_end.append(&continuous_toggle);
    header_end.append(&palette);
    header_end.append(&style_drop);
    let more_button = gtk4::MenuButton::new();
    more_button.set_icon_name("view-more-symbolic");
    more_button.set_tooltip_text(Some("More: note, page numbering, export, new window"));
    more_button.add_css_class("flat");
    {
        let rows = gtk4::Box::new(Orientation::Vertical, 2);
        rows.set_margin_top(6);
        rows.set_margin_bottom(6);
        rows.set_margin_start(6);
        rows.set_margin_end(6);
        let more_popover = gtk4::Popover::new();
        for (label, target) in [
            ("Add note on this page…", note_button.clone()),
            ("Set page numbering…", page_num_button.clone()),
            ("Export notes…", export_button.clone()),
            ("Open in a new window", popout_button.clone()),
        ] {
            let row = popover_button(label, false);
            row.set_sensitive(target.is_sensitive());
            let popover = more_popover.clone();
            row.connect_clicked(move |_| {
                popover.popdown();
                target.emit_clicked();
            });
            rows.append(&row);
        }
        more_popover.set_child(Some(&rows));
        more_button.set_popover(Some(&more_popover));
    }
    header_end.append(&more_button);
    header_end.append(&notes_toggle);

    // Status bar (house style, same classes as the main window's): page nav and search on
    // the left/middle, rotate/invert/zoom on the right, the reader-host footer (if the
    // embedding app registered one) at the far right. Combines what used to be three
    // separate bottom rows — this tab's own nav+zoom bar, its search bar (previously its own
    // row above the page), and the host window's own footer bar below all of it — into one,
    // so the rest of the window is free for the document itself.
    let statusbar = gtk4::Box::new(Orientation::Horizontal, 6);
    statusbar.add_css_class("toolbar");
    statusbar.add_css_class("fond-chrome");
    statusbar.add_css_class("fond-statusbar");

    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search this PDF…"));
    search_entry.set_hexpand(true);
    search_entry.set_max_width_chars(28);
    let search_prev = gtk4::Button::from_icon_name("go-up-symbolic");
    search_prev.add_css_class("flat");
    search_prev.set_tooltip_text(Some("Previous match"));
    search_prev.set_sensitive(false);
    let search_next = gtk4::Button::from_icon_name("go-down-symbolic");
    search_next.add_css_class("flat");
    search_next.set_tooltip_text(Some("Next match"));
    search_next.set_sensitive(false);
    let search_count = gtk4::Label::new(None);
    search_count.add_css_class("dim-label");

    let link_back = gtk4::Button::new();
    link_back.add_css_class("flat");
    link_back.set_visible(false);
    link_back.set_tooltip_text(Some("Return to where you followed a link from (Alt+Left)"));
    statusbar.append(&nav);
    statusbar.append(&link_back);
    statusbar.append(&search_entry);
    statusbar.append(&search_count);
    statusbar.append(&search_prev);
    statusbar.append(&search_next);
    statusbar.append(&rotate_button);
    statusbar.append(&invert_button);
    statusbar.append(&zoom_fit_width);
    statusbar.append(&zoom_fit_page);
    statusbar.append(&zoom_out);
    statusbar.append(&zoom_in);
    if let Some(footer_widget) = crate::reader_host::host_footer_widget() {
        statusbar.append(&footer_widget);
    }
    view.add_bottom_bar(&statusbar);

    let hint = gtk4::Label::new(Some(
        "Drag over text to select it, then choose a colour (or press 1–4)",
    ));
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    hint.set_margin_top(4);
    hint.set_margin_bottom(4);

    let picture = gtk4::Picture::new();
    picture.set_halign(gtk4::Align::Center);
    picture.set_valign(gtk4::Align::Start);
    picture.set_can_target(true);
    picture.set_cursor(cursor_for_select_mode(true).as_ref());
    let (picture_overlay, drag_preview, drag_live_rect) = {
        let reader_for_page = reader.clone();
        build_drag_preview_overlay(&picture, &reader, move || reader_for_page.borrow().page)
    };
    picture_overlay.set_halign(gtk4::Align::Center);
    picture_overlay.set_valign(gtk4::Align::Start);
    // "Two-page" mode's facing page — sits beside `picture_overlay` in `spread_box`, hidden
    // (and left unrendered) outside that mode. View-only for now: no drag/click gesture
    // controllers of its own, so a highlight/note is still made via the left page (or by
    // switching to single-page/Continuous) — `render()` below is what actually keeps this in
    // sync with `picture`, so both stay one page apart with no separate render path to drift.
    let right_picture = gtk4::Picture::new();
    right_picture.set_halign(gtk4::Align::Center);
    right_picture.set_valign(gtk4::Align::Start);
    right_picture.set_visible(false);
    let spread_box = gtk4::Box::new(Orientation::Horizontal, 12);
    spread_box.set_halign(gtk4::Align::Center);
    spread_box.append(&picture_overlay);
    spread_box.append(&right_picture);
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&spread_box));
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);

    // Continuous-scroll mode's surface: an empty Box for now — populated with one Picture
    // per page the first time the mode is toggled on (`build_continuous_view` below), not
    // eagerly here, so a plain "Read" stays exactly as fast as it always was.
    let continuous_box = gtk4::Box::new(Orientation::Vertical, CONTINUOUS_PAGE_GAP as i32);
    continuous_box.set_margin_top(CONTINUOUS_PAGE_GAP as i32);
    continuous_box.set_margin_bottom(CONTINUOUS_PAGE_GAP as i32);
    let continuous_scroll = gtk4::ScrolledWindow::new();
    continuous_scroll.set_child(Some(&continuous_box));
    continuous_scroll.set_vexpand(true);
    continuous_scroll.set_hexpand(true);

    let view_stack = gtk4::Stack::new();
    view_stack.add_named(&scroll, Some("paged"));
    view_stack.add_named(&continuous_scroll, Some("continuous"));
    view_stack.set_visible_child_name("paged");

    let content = gtk4::Box::new(Orientation::Vertical, 0);
    content.append(&hint);
    content.append(&view_stack);
    // `content` is reparented into the sidebar Paned below instead of set directly here —
    // the Notes sidebar (and, when present, Contents) always builds that Paned now, and
    // `Paned::set_end_child` asserts its child has no existing parent.
    let reader_tab = crate::reader_host::open_reader_tab(window, title, &view);
    let reader_window = reader_tab.host_window.clone();
    let quick_mark: QuickMarkSlot = Rc::new(RefCell::new(None));
    crate::register_reader(pdf_hash, &reader_tab);
    crate::label_icon_buttons(&header_start);
    crate::label_icon_buttons(&header_end);
    crate::label_icon_buttons(&statusbar);
    crate::reader_host::set_tab_header(&reader_tab, header_start, title_widget, header_end);

    // Render the current page into the Picture (via the shared helper both this view and
    // continuous-scroll mode use), and refresh the page label. Also fills `right_picture`
    // with the facing page when Two-page mode is on (hidden otherwise) — every existing
    // caller of `render()` (nav buttons, zoom, search, outline/notes-sidebar jumps, page
    // entry) gets two-page-aware rendering for free this way, with no changes needed at any
    // of those call sites.
    let render = {
        let reader = reader.clone();
        let picture = picture.clone();
        let right_picture = right_picture.clone();
        let two_page_toggle = two_page_toggle.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        Rc::new(move || {
            let mut r = reader.borrow_mut();
            match render_pdf_page_texture(&r, r.page) {
                Some((texture, w, h)) => {
                    r.render_px = (w, h);
                    picture.set_paintable(Some(&texture));
                    picture.set_size_request(w as i32, h as i32);
                }
                None => picture.set_paintable(gdk::Paintable::NONE),
            }
            if two_page_toggle.is_active() {
                let right_page = r.page + 1;
                if right_page < r.count {
                    match render_pdf_page_texture(&r, right_page) {
                        Some((texture, w, h)) => {
                            right_picture.set_paintable(Some(&texture));
                            right_picture.set_size_request(w as i32, h as i32);
                            right_picture.set_visible(true);
                        }
                        None => right_picture.set_visible(false),
                    }
                } else {
                    // Odd page count: the last spread has no facing page.
                    right_picture.set_visible(false);
                }
            } else {
                right_picture.set_visible(false);
            }
            update_page_display(
                &page_entry,
                &page_of_label,
                &prev,
                &next,
                &bookmark_button,
                r.page,
                r.count,
                &r.page_labels,
                &r.bookmarks,
            );
        })
    };
    render();

    // Undo/redo: a snapshot doesn't record which page(s) it touched, so pop it and
    // re-render everything — cheap even in continuous mode, since each page's blend is just
    // a texture re-render, not a re-parse of the PDF.
    let rerender_all_pages = {
        let reader = reader.clone();
        let render = render.clone();
        Rc::new(move || {
            render();
            rerender_loaded_continuous_pages(&reader);
        })
    };
    let undo = {
        let reader = reader.clone();
        let host = host.clone();
        let rerender_all_pages = rerender_all_pages.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        Rc::new(move || {
            let popped = {
                let mut r = reader.borrow_mut();
                match r.undo_stack.pop() {
                    Some(prev) => {
                        let current = r.annotations.clone();
                        r.redo_stack.push(current);
                        r.annotations = prev;
                        true
                    }
                    None => false,
                }
            };
            if !popped {
                host.notify("Nothing to undo");
                return;
            }
            let write_result = host.save_annotations(&reader.borrow().annotations);
            match write_result {
                Ok(()) => {
                    rerender_all_pages();
                    host.notify("Undid last annotation change");
                }
                Err(e) => host.notify(&format!("Could not undo: {e}")),
            }
            sync_undo_redo_buttons(&reader, &undo_button, &redo_button);
        })
    };
    let redo = {
        let reader = reader.clone();
        let host = host.clone();
        let rerender_all_pages = rerender_all_pages.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        Rc::new(move || {
            let popped = {
                let mut r = reader.borrow_mut();
                match r.redo_stack.pop() {
                    Some(next) => {
                        let current = r.annotations.clone();
                        r.undo_stack.push(current);
                        r.annotations = next;
                        true
                    }
                    None => false,
                }
            };
            if !popped {
                host.notify("Nothing to redo");
                return;
            }
            let write_result = host.save_annotations(&reader.borrow().annotations);
            match write_result {
                Ok(()) => {
                    rerender_all_pages();
                    host.notify("Redid annotation change");
                }
                Err(e) => host.notify(&format!("Could not redo: {e}")),
            }
            sync_undo_redo_buttons(&reader, &undo_button, &redo_button);
        })
    };
    {
        let undo = undo.clone();
        undo_button.connect_clicked(move |_| undo());
    }
    {
        let redo = redo.clone();
        redo_button.connect_clicked(move |_| redo());
    }
    {
        // Registered with the tab host rather than added to `view`: the host catches keys at
        // the window in the capture phase (see `reader_host::set_tab_key_handler`), so
        // navigation works whatever has focus in the reader window, not just widgets inside
        // this tab's own content. Editing `page_entry`/the search entry/a note still needs
        // Left/Right/Home/End/Space for the text cursor, so those pass through below.
        let undo = undo.clone();
        let redo = redo.clone();
        let prev = prev.clone();
        let next = next.clone();
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let view_for_focus = view.clone();
        let bookmark_button = bookmark_button.clone();
        let link_back_key = link_back.clone();
        let zoom_in_key = zoom_in.clone();
        let zoom_out_key = zoom_out.clone();
        let zoom_fit_key = zoom_fit_width.clone();
        let palette_for_keys = palette.clone();
        let quick_mark_for_keys = quick_mark.clone();
        let search_for_keys = search_entry.clone();
        crate::reader_host::set_tab_key_handler(&reader_tab, move |keyval, modifiers| {
            if (keyval == gdk::Key::z || keyval == gdk::Key::Z)
                && modifiers.contains(gdk::ModifierType::CONTROL_MASK)
            {
                if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
                    redo();
                } else {
                    undo();
                }
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::ALT_MASK)
                && matches!(keyval, gdk::Key::Left | gdk::Key::KP_Left)
                && link_back_key.is_visible()
            {
                link_back_key.emit_clicked();
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                match keyval {
                    gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => {
                        zoom_in_key.emit_clicked();
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::minus | gdk::Key::KP_Subtract => {
                        zoom_out_key.emit_clicked();
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::_0 | gdk::Key::KP_0 => {
                        zoom_fit_key.emit_clicked();
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::f | gdk::Key::F => {
                        search_for_keys.grab_focus();
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            let focus_in_text_entry = view_for_focus
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|w| {
                    w.is::<gtk4::Entry>() || w.is::<gtk4::Text>() || w.is::<gtk4::TextView>()
                });
            if focus_in_text_entry {
                return glib::Propagation::Proceed;
            }
            if crate::focus_owns_activation_keys(&view_for_focus)
                && matches!(
                    keyval,
                    gdk::Key::space
                        | gdk::Key::Up
                        | gdk::Key::Down
                        | gdk::Key::KP_Up
                        | gdk::Key::KP_Down
                )
            {
                return glib::Propagation::Proceed;
            }
            if modifiers.is_empty() {
                let index = match keyval {
                    gdk::Key::_1 => Some(0),
                    gdk::Key::_2 => Some(1),
                    gdk::Key::_3 => Some(2),
                    gdk::Key::_4 => Some(3),
                    _ => None,
                };
                if let Some(i) = index {
                    if reader.borrow().last_selection.is_some() {
                        if let Some(mark) = quick_mark_for_keys.borrow().as_ref() {
                            mark(i);
                            return glib::Propagation::Stop;
                        }
                    }
                    crate::palette::select_color(&palette_for_keys, true, i);
                    return glib::Propagation::Stop;
                }
            }
            // Left/Right, Up/Down, Space/Backspace, and Page_Up/Page_Down reuse the
            // prev/next buttons' own click handlers (continuous-scroll-aware,
            // two-page-spread-aware) via `emit_clicked`, rather than duplicating that logic
            // here. Space/Backspace match the common reader convention (Preview, Acrobat).
            match keyval {
                gdk::Key::Left
                | gdk::Key::Up
                | gdk::Key::Page_Up
                | gdk::Key::BackSpace
                | gdk::Key::KP_Left
                | gdk::Key::KP_Up
                | gdk::Key::KP_Page_Up => {
                    prev.emit_clicked();
                    return glib::Propagation::Stop;
                }
                gdk::Key::Right
                | gdk::Key::Down
                | gdk::Key::Page_Down
                | gdk::Key::space
                | gdk::Key::KP_Right
                | gdk::Key::KP_Down
                | gdk::Key::KP_Page_Down => {
                    next.emit_clicked();
                    return glib::Propagation::Stop;
                }
                gdk::Key::b | gdk::Key::B => {
                    bookmark_button.emit_clicked();
                    return glib::Propagation::Stop;
                }
                gdk::Key::Home | gdk::Key::KP_Home => {
                    if continuous_toggle.is_active() {
                        scroll_continuous_to_page(&reader, &continuous_scroll, 0);
                    } else {
                        reader.borrow_mut().page = 0;
                        render();
                    }
                    return glib::Propagation::Stop;
                }
                gdk::Key::End | gdk::Key::KP_End => {
                    let last = reader.borrow().count.saturating_sub(1);
                    if continuous_toggle.is_active() {
                        scroll_continuous_to_page(&reader, &continuous_scroll, last);
                    } else {
                        reader.borrow_mut().page = last;
                        render();
                    }
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
            glib::Propagation::Proceed
        });
    }

    // Contents/Notes sidebar: persistent (not a popover) so it stays visible while
    // navigating, per Cal's request. Both panels share one Paned start-child slot via a
    // Stack, since only one is useful to see at a time; the two toggles are mutually
    // exclusive (activating one deactivates the other) but each can still be clicked again
    // to close the sidebar entirely, unlike a strict radio-group.
    let contents_scroll = {
        let rows = gtk4::Box::new(Orientation::Vertical, 2);
        rows.set_margin_top(6);
        rows.set_margin_bottom(6);
        rows.set_margin_start(6);
        rows.set_margin_end(6);
        for entry in &outline_entries {
            let label = format!("{}{}", "    ".repeat(entry.depth as usize), entry.title);
            let row = popover_button(&label, false);
            if let Some(lbl) = row.child().and_then(|w| w.downcast::<gtk4::Label>().ok()) {
                lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            }
            if let Some(page) = entry.page {
                let reader = reader.clone();
                let render = render.clone();
                let continuous_toggle = continuous_toggle.clone();
                let continuous_scroll = continuous_scroll.clone();
                row.connect_clicked(move |_| {
                    let target = {
                        let r = reader.borrow();
                        (page.saturating_sub(1)).min(r.count.saturating_sub(1))
                    };
                    if continuous_toggle.is_active() {
                        scroll_continuous_to_page(&reader, &continuous_scroll, target);
                    } else {
                        reader.borrow_mut().page = target;
                        render();
                    }
                });
            } else {
                row.set_sensitive(false);
            }
            rows.append(&row);
        }
        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scroll.set_child(Some(&rows));
        scroll
    };

    // Outline and Thumbnails are two tabs of the one left sidebar (as opposed to Notes,
    // which is its own independent right-hand sidebar — see the comment further down where
    // `notes_paned` is built). A plain Stack + a two-button switcher, not `gtk4::StackSwitcher`
    // or `adw::ViewSwitcher`, to match the rest of this reader's hand-built toggle style and
    // to get `ToggleButton::set_group`'s native radio behaviour (exactly one active, clicking
    // the active one again does nothing) for free.
    let (thumbnails_scroll, trigger_thumbnails) =
        build_thumbnails_sidebar(&reader, &render, &continuous_toggle, &continuous_scroll);

    let outline_tab_toggle = gtk4::ToggleButton::with_label("Outline");
    let thumbnails_tab_toggle = gtk4::ToggleButton::with_label("Thumbnails");
    thumbnails_tab_toggle.set_group(Some(&outline_tab_toggle));
    let sidebar_tabs_row = gtk4::Box::new(Orientation::Horizontal, 0);
    sidebar_tabs_row.add_css_class("linked");
    sidebar_tabs_row.set_margin_top(6);
    sidebar_tabs_row.set_margin_bottom(6);
    sidebar_tabs_row.set_margin_start(6);
    sidebar_tabs_row.set_margin_end(6);
    sidebar_tabs_row.set_halign(gtk4::Align::Center);
    sidebar_tabs_row.append(&outline_tab_toggle);
    sidebar_tabs_row.append(&thumbnails_tab_toggle);

    let sidebar_tab_stack = gtk4::Stack::new();
    sidebar_tab_stack.set_vexpand(true);
    sidebar_tab_stack.add_named(&contents_scroll, Some("outline"));
    sidebar_tab_stack.add_named(&thumbnails_scroll, Some("thumbnails"));

    if outline_entries.is_empty() {
        outline_tab_toggle.set_sensitive(false);
        outline_tab_toggle.set_tooltip_text(Some("This PDF has no table of contents"));
        thumbnails_tab_toggle.set_active(true);
        sidebar_tab_stack.set_visible_child_name("thumbnails");
        trigger_thumbnails();
    } else {
        outline_tab_toggle.set_active(true);
        sidebar_tab_stack.set_visible_child_name("outline");
    }
    {
        let sidebar_tab_stack = sidebar_tab_stack.clone();
        outline_tab_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                sidebar_tab_stack.set_visible_child_name("outline");
            }
        });
    }
    {
        let sidebar_tab_stack = sidebar_tab_stack.clone();
        thumbnails_tab_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                sidebar_tab_stack.set_visible_child_name("thumbnails");
                trigger_thumbnails();
            }
        });
    }

    let sidebar_box = gtk4::Box::new(Orientation::Vertical, 0);
    sidebar_box.append(&sidebar_tabs_row);
    sidebar_box.append(&sidebar_tab_stack);

    // Notes/highlights list: every annotation in the document, readable prose rather than
    // just on-page markers, sorted by page. Rebuilt fresh (`rebuild_notes`, below) whenever
    // shown or whenever an annotation is added/removed elsewhere in the reader, via the
    // `Rc<RefCell<Option<...>>>` indirection so a row's own delete button can trigger a
    // rebuild of the list it lives in.
    let notes_rows = gtk4::Box::new(Orientation::Vertical, 2);
    notes_rows.set_margin_top(6);
    notes_rows.set_margin_bottom(6);
    notes_rows.set_margin_start(6);
    notes_rows.set_margin_end(6);
    let notes_scroll = gtk4::ScrolledWindow::new();
    notes_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    notes_scroll.set_child(Some(&notes_rows));

    let rebuild_notes_cell: RebuildNotesCell = Rc::new(RefCell::new(None));
    {
        let notes_rows = notes_rows.clone();
        let host = host.clone();
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        let bookmark_button = bookmark_button.clone();
        let rebuild_notes_cell_inner = rebuild_notes_cell.clone();
        let builder = move || {
            while let Some(child) = notes_rows.first_child() {
                notes_rows.remove(&child);
            }
            let bookmarks = reader.borrow().bookmarks.clone();
            let mut all: Vec<fond_bib::Annotation> = reader
                .borrow()
                .annotations
                .annotations
                .iter()
                .filter(|a| a.page.is_some())
                .cloned()
                .collect();
            all.sort_by(|a, b| (a.page, &a.created).cmp(&(b.page, &b.created)));
            if all.is_empty() && bookmarks.is_empty() {
                let label = gtk4::Label::new(Some("No bookmarks, notes, or highlights yet"));
                label.add_css_class("dim-label");
                label.set_margin_top(6);
                label.set_margin_bottom(6);
                notes_rows.append(&label);
                return;
            }
            if !bookmarks.is_empty() {
                let heading = gtk4::Label::new(Some("Bookmarks"));
                heading.add_css_class("dim-label");
                heading.add_css_class("caption-heading");
                heading.set_xalign(0.0);
                notes_rows.append(&heading);
                for &page_num in &bookmarks {
                    let printed = reader
                        .borrow()
                        .page_labels
                        .get((page_num as usize).saturating_sub(1))
                        .and_then(|l| l.clone());
                    let page_label = printed.unwrap_or_else(|| page_num.to_string());
                    let row = gtk4::Box::new(Orientation::Horizontal, 6);
                    let label = gtk4::Label::new(Some(&format!("p.{page_label}")));
                    label.set_xalign(0.0);
                    label.set_hexpand(true);
                    row.append(&label);
                    let remove_button = gtk4::Button::from_icon_name("user-trash-symbolic");
                    remove_button.add_css_class("flat");
                    remove_button.set_tooltip_text(Some("Remove bookmark"));
                    row.append(&remove_button);
                    {
                        let reader = reader.clone();
                        let render = render.clone();
                        let continuous_toggle = continuous_toggle.clone();
                        let continuous_scroll = continuous_scroll.clone();
                        crate::make_jump(&label, move || {
                            let target = (page_num.saturating_sub(1))
                                .min(reader.borrow().count.saturating_sub(1) as u32)
                                as u16;
                            if continuous_toggle.is_active() {
                                scroll_continuous_to_page(&reader, &continuous_scroll, target);
                            } else {
                                reader.borrow_mut().page = target;
                                render();
                            }
                        });
                    }
                    {
                        let host = host.clone();
                        let reader = reader.clone();
                        let bookmark_button = bookmark_button.clone();
                        let rebuild_notes_cell = rebuild_notes_cell_inner.clone();
                        remove_button.connect_clicked(move |_| {
                            reader.borrow_mut().bookmarks.retain(|&p| p != page_num);
                            host.save_bookmarks(&reader.borrow().bookmarks);
                            let current = reader.borrow().page as u32 + 1;
                            update_bookmark_button(
                                &bookmark_button,
                                reader.borrow().bookmarks.contains(&current),
                            );
                            if let Some(f) = rebuild_notes_cell.borrow().as_ref() {
                                f();
                            }
                        });
                    }
                    notes_rows.append(&row);
                }
                notes_rows.append(&popover_separator());
            }
            if all.is_empty() {
                return;
            }
            let last = all.len().saturating_sub(1);
            for (i, annotation) in all.into_iter().enumerate() {
                let page_num = annotation.page.unwrap_or(1);
                // Same printed-label lookup the page-number entry itself uses (`page_labels`
                // is 0-based, `page_num` is the annotation's raw 1-based file page).
                let printed = reader
                    .borrow()
                    .page_labels
                    .get((page_num as usize).saturating_sub(1))
                    .and_then(|l| l.clone());
                let page_label = printed.unwrap_or_else(|| page_num.to_string());
                let outer = gtk4::Box::new(Orientation::Vertical, 2);

                let header_box = gtk4::Box::new(Orientation::Horizontal, 6);
                if let Some(swatch) = color_swatch(annotation.color.as_deref()) {
                    header_box.append(&swatch);
                }
                let header_label =
                    gtk4::Label::new(Some(&format!("p.{page_label} — {:?}", annotation.kind)));
                header_label.set_xalign(0.0);
                header_label.set_hexpand(true);
                header_label.add_css_class("dim-label");
                header_label.add_css_class("caption-heading");
                header_box.append(&header_label);
                let delete_button = gtk4::Button::from_icon_name("user-trash-symbolic");
                delete_button.add_css_class("flat");
                delete_button.set_tooltip_text(Some("Delete this annotation"));
                header_box.append(&delete_button);
                outer.append(&header_box);

                {
                    let reader = reader.clone();
                    let render = render.clone();
                    let continuous_toggle = continuous_toggle.clone();
                    let continuous_scroll = continuous_scroll.clone();
                    crate::make_jump(&header_label, move || {
                        let target = (page_num.saturating_sub(1))
                            .min(reader.borrow().count.saturating_sub(1) as u32)
                            as u16;
                        if continuous_toggle.is_active() {
                            scroll_continuous_to_page(&reader, &continuous_scroll, target);
                        } else {
                            reader.borrow_mut().page = target;
                            render();
                        }
                    });
                }

                if let Some(snippet) = &annotation.snippet {
                    let snippet_label = gtk4::Label::new(Some(snippet));
                    snippet_label.set_xalign(0.0);
                    snippet_label.set_wrap(true);
                    snippet_label.add_css_class("dim-label");
                    snippet_label.add_css_class("caption");
                    outer.append(&snippet_label);
                }

                let save_note = {
                    let host = host.clone();
                    let reader = reader.clone();
                    let id = annotation.id.clone();
                    let undo_button = undo_button.clone();
                    let redo_button = redo_button.clone();
                    move |text: &str| {
                        let text = text.trim();
                        let current_note = reader
                            .borrow()
                            .annotations
                            .annotations
                            .iter()
                            .find(|a| a.id == id)
                            .and_then(|a| a.note.clone());
                        if current_note.as_deref().unwrap_or("") == text {
                            return;
                        }
                        push_undo_snapshot(&reader);
                        {
                            let mut r = reader.borrow_mut();
                            if let Some(a) =
                                r.annotations.annotations.iter_mut().find(|a| a.id == id)
                            {
                                a.note = (!text.is_empty()).then(|| text.to_string());
                            }
                        }
                        let write_result = host.save_annotations(&reader.borrow().annotations);
                        match write_result {
                            Ok(()) => {
                                sync_undo_redo_buttons(&reader, &undo_button, &redo_button);
                            }
                            Err(e) => host.notify(&e),
                        }
                    }
                };
                let note_widget =
                    note_edit_widget(annotation.note.as_deref(), move |text| save_note(&text));
                outer.append(&note_widget);

                {
                    let host = host.clone();
                    let reader = reader.clone();
                    let render = render.clone();
                    let id = annotation.id.clone();
                    let undo_button = undo_button.clone();
                    let redo_button = redo_button.clone();
                    let rebuild_notes_cell = rebuild_notes_cell_inner.clone();
                    delete_button.connect_clicked(move |_| {
                        push_undo_snapshot(&reader);
                        reader
                            .borrow_mut()
                            .annotations
                            .annotations
                            .retain(|a| a.id != id);
                        let write_result = host.save_annotations(&reader.borrow().annotations);
                        match write_result {
                            Ok(()) => {
                                render();
                                render_continuous_page(
                                    &reader,
                                    (page_num.saturating_sub(1)) as u16,
                                );
                                sync_undo_redo_buttons(&reader, &undo_button, &redo_button);
                                host.notify("Annotation deleted");
                                if let Some(f) = rebuild_notes_cell.borrow().as_ref() {
                                    f();
                                }
                            }
                            Err(e) => host.notify(&e),
                        }
                    });
                }

                notes_rows.append(&outer);
                if i != last {
                    notes_rows.append(&popover_separator());
                }
            }
        };
        *rebuild_notes_cell.borrow_mut() = Some(Rc::new(builder));
    }
    let rebuild_notes: Rc<dyn Fn()> = {
        let cell = rebuild_notes_cell.clone();
        Rc::new(move || {
            let f = cell.borrow().clone();
            if let Some(f) = f {
                f();
            }
        })
    };

    {
        let host = host.clone();
        let reader = reader.clone();
        let render = render.clone();
        let pdf_hash = pdf_hash.to_string();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        let rebuild_notes = rebuild_notes.clone();
        let reader_window = reader_window.clone();
        *quick_mark.borrow_mut() = Some(Rc::new(move |i: usize| {
            let page = reader.borrow().last_selection.as_ref().map(|(p, _, _)| *p);
            let ctx = MarkCtx {
                host: host.clone(),
                reader: reader.clone(),
                pdf_hash: pdf_hash.clone(),
                undo_button: undo_button.clone(),
                redo_button: redo_button.clone(),
                rebuild_notes: rebuild_notes.clone(),
                reader_window: reader_window.clone(),
                refresh: {
                    let reader = reader.clone();
                    let render = render.clone();
                    Rc::new(move || {
                        render();
                        if let Some(p) = page {
                            render_continuous_page(&reader, p);
                        }
                    })
                },
            };
            if let Some(color) = crate::palette::HIGHLIGHT_COLORS.get(i) {
                apply_selection_mark(&ctx, fond_bib::AnnotationKind::Highlight, color.hex);
            }
        }));
    }

    {
        let reader_for_goto = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let link_back = link_back.clone();
        let jump_to = {
            let reader = reader.clone();
            Rc::new(move |page: u16| {
                if continuous_toggle.is_active() {
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                } else {
                    reader.borrow_mut().page = page;
                    render();
                }
            })
        };
        {
            let jump_to = jump_to.clone();
            let link_back = link_back.clone();
            reader.borrow_mut().link_goto = Some(Rc::new(move |page| {
                let from = {
                    let mut r = reader_for_goto.borrow_mut();
                    let from = r.page;
                    r.nav_back.push(from);
                    from
                };
                link_back.set_label(&format!("← Back to p. {}", from + 1));
                link_back.set_visible(true);
                jump_to(page);
            }));
        }
        let reader = reader.clone();
        let link_back_inner = link_back.clone();
        link_back.connect_clicked(move |_| {
            let dest = reader.borrow_mut().nav_back.pop();
            if let Some(page) = dest {
                jump_to(page);
            }
            match reader.borrow().nav_back.last() {
                Some(p) => link_back_inner.set_label(&format!("← Back to p. {}", p + 1)),
                None => link_back_inner.set_visible(false),
            }
        });
    }

    // Contents (left, itself split into Outline/Thumbnails tabs — built above) and Notes
    // (right) are two independent sidebars rather than a shared Stack behind one toggle slot
    // — Cal asked for Notes on its own right-hand sidebar so it can stay open alongside
    // Contents instead of the two forcing a choice between them. Width is user-adjustable
    // via each Paned handle; start at a reasonable default but let it be dragged down to a
    // slim strip.
    sidebar_box.set_size_request(60, -1);
    notes_scroll.set_size_request(60, -1);

    // Notes sidebar: its own Paned wrapping `content`, so it sits on the right of the
    // document regardless of whether Contents is open on the left. `content` (the actual
    // document view) is the resizing child; the notes list is the fixed-width one — sized
    // from the paned's current allocation the moment it's first shown, since a plain
    // constant would be wrong for whatever the window's actual width happens to be (unlike
    // the left Contents sidebar's fixed 220, which is measured from the start edge and so
    // stays correct regardless of window width).
    let notes_paned = gtk4::Paned::new(Orientation::Horizontal);
    notes_paned.set_start_child(Some(&content));
    notes_paned.set_resize_start_child(true);
    notes_paned.set_shrink_start_child(false);
    notes_paned.set_end_child(gtk4::Widget::NONE);
    notes_paned.set_resize_end_child(false);
    notes_paned.set_shrink_end_child(true);
    notes_paned.set_vexpand(true);
    notes_paned.set_hexpand(true);

    let paned = gtk4::Paned::new(Orientation::Horizontal);
    paned.set_start_child(gtk4::Widget::NONE);
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(true);
    paned.set_end_child(Some(&notes_paned));
    paned.set_vexpand(true);
    paned.set_hexpand(true);
    paned.set_position(220);
    view.set_content(Some(&paned));

    {
        let paned = paned.clone();
        let sidebar_box = sidebar_box.clone();
        sidebar_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                paned.set_start_child(Some(&sidebar_box));
            } else {
                paned.set_start_child(gtk4::Widget::NONE);
            }
        });
    }
    {
        let notes_paned = notes_paned.clone();
        let notes_scroll = notes_scroll.clone();
        let rebuild_notes = rebuild_notes.clone();
        const NOTES_SIDEBAR_WIDTH: i32 = 300;
        notes_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                rebuild_notes();
                let available = notes_paned.width();
                let total = if available > 0 { available } else { 900 };
                notes_paned.set_position((total - NOTES_SIDEBAR_WIDTH).max(200));
                notes_paned.set_end_child(Some(&notes_scroll));
            } else {
                notes_paned.set_end_child(gtk4::Widget::NONE);
            }
        });
    }

    // Click-drag on the page creates a highlight: the dragged rectangle (in the render's own
    // pixel grid — the Picture is size-requested to exactly that, so widget-local coordinates
    // from the gesture already are that grid) converts to PDF-space quadpoints via the current
    // page's point size, and is appended to the sidecar and written straight to disk.
    {
        let drag = gtk4::GestureDrag::new();
        let reader = reader.clone();
        let render = render.clone();
        let reader_window_for_drag = reader_window.clone();
        let picture_for_popover = picture.clone();
        let host = host.clone();
        let pdf_hash = pdf_hash.to_string();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        let rebuild_notes = rebuild_notes.clone();
        {
            let live_rect = drag_live_rect.clone();
            let drag_preview = drag_preview.clone();
            drag.connect_drag_begin(move |_gesture, start_x, start_y| {
                live_rect.set(Some((start_x, start_y, start_x, start_y)));
                drag_preview.queue_draw();
            });
        }
        {
            let live_rect = drag_live_rect.clone();
            let drag_preview = drag_preview.clone();
            drag.connect_drag_update(move |gesture, offset_x, offset_y| {
                let Some((start_x, start_y)) = gesture.start_point() else {
                    return;
                };
                live_rect.set(Some((
                    start_x,
                    start_y,
                    start_x + offset_x,
                    start_y + offset_y,
                )));
                drag_preview.queue_draw();
            });
        }
        {
            let live_rect = drag_live_rect.clone();
            let drag_preview = drag_preview.clone();
            drag.connect_drag_end(move |gesture, offset_x, offset_y| {
                live_rect.set(None);
                drag_preview.queue_draw();
                if offset_x.abs() < MIN_DRAG_PX && offset_y.abs() < MIN_DRAG_PX {
                    return;
                }
                if reader.borrow().rotation != 0 {
                    // See `ReaderState::rotation`'s doc comment — the coordinate math below
                    // assumes an unrotated page, and the rotate control is meant to be
                    // disabled outside single-page mode anyway, so this should be
                    // unreachable via the UI; guarded regardless since a stray drag
                    // finishing mid-toggle is cheap to rule out.
                    host.notify("Rotate back to 0° to annotate or select text");
                    return;
                }
                let Some((start_x, start_y)) = gesture.start_point() else {
                    return;
                };
                let end_x = start_x + offset_x;
                let end_y = start_y + offset_y;

                let (page, render_w, render_h, page_geom) = {
                    let r = reader.borrow();
                    (r.page, r.render_px.0, r.render_px.1, r.geom(r.page))
                };
                let geom = DragGeometry {
                    render_w,
                    render_h,
                    page: page_geom,
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                };
                if reader.borrow().draw_kind.is_none() {
                    if select_drag_text(&host, &reader, page, &geom) {
                        render();
                        render_continuous_page(&reader, page);
                        let ctx = MarkCtx {
                            host: host.clone(),
                            reader: reader.clone(),
                            pdf_hash: pdf_hash.clone(),
                            undo_button: undo_button.clone(),
                            redo_button: redo_button.clone(),
                            rebuild_notes: rebuild_notes.clone(),
                            reader_window: reader_window_for_drag.clone(),
                            refresh: {
                                let reader = reader.clone();
                                let render = render.clone();
                                Rc::new(move || {
                                    render();
                                    render_continuous_page(&reader, page);
                                })
                            },
                        };
                        show_selection_popover(&ctx, &picture_for_popover, end_x, end_y, page);
                    }
                    return;
                }
                let saved = save_drag_annotation(&host, &reader, &pdf_hash, page, geom);
                if saved {
                    render();
                    // Keep continuous mode's copy of this page in sync too, in case it was
                    // already built from an earlier toggle-on and the user drew this
                    // highlight after switching back to the paged view.
                    render_continuous_page(&reader, page);
                    sync_undo_redo_buttons(&reader, &undo_button, &redo_button);
                    rebuild_notes();
                }
            });
        }
        picture.add_controller(drag);
    }

    // Right-click: edit or delete the annotation under the cursor, or add a marginal note
    // if the click landed on blank page — replaces the old "This page" dropdown, which
    // listed the same actions in a fixed menu instead of at the annotation itself.
    {
        let click = gtk4::GestureClick::new();
        click.set_button(gdk::BUTTON_SECONDARY);
        let host = host.clone();
        let reader = reader.clone();
        let render = render.clone();
        let pdf_hash = pdf_hash.to_string();
        let picture_for_menu = picture.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        let rebuild_notes = rebuild_notes.clone();
        let dialog_for_menu = reader_window.clone();
        click.connect_pressed(move |_gesture, _n, x, y| {
            if reader.borrow().rotation != 0 {
                host.notify("Rotate back to 0° to edit annotations");
                return;
            }
            let (page, render_w, render_h, page_geom) = {
                let r = reader.borrow();
                (r.page, r.render_px.0, r.render_px.1, r.geom(r.page))
            };
            let refresh: Rc<dyn Fn()> = {
                let reader = reader.clone();
                let render = render.clone();
                Rc::new(move || {
                    render();
                    let page = reader.borrow().page;
                    render_continuous_page(&reader, page);
                })
            };
            show_pdf_context_menu(
                &host,
                &reader,
                &pdf_hash,
                &picture_for_menu,
                refresh,
                rebuild_notes.clone(),
                &undo_button,
                &redo_button,
                page,
                ClickGeometry {
                    render_w,
                    render_h,
                    page: page_geom,
                    click_x: x,
                    click_y: y,
                },
                &dialog_for_menu,
            );
        });
        picture.add_controller(click);
    }

    // Click-to-turn zones: a plain (non-dragging) click in the leftmost/rightmost 20% of the
    // page turns to the previous/next page — the middle 60% is left alone so a normal
    // highlight drag or text click isn't at risk of being misread as page navigation.
    // Coexists with the drag-to-annotate `GestureDrag` above (both see the same primary-
    // button sequence, un-grouped, which is the standard GTK4 way for a click and a drag
    // gesture to share one widget) — a real drag past `MIN_DRAG_PX` already makes this
    // handler's own displacement check a no-op, the same threshold the drag handler itself
    // uses to decide whether anything was actually drawn.
    {
        let press_pos: Rc<Cell<Option<(f64, f64)>>> = Rc::new(Cell::new(None));
        let click_nav = gtk4::GestureClick::new();
        click_nav.set_button(gdk::BUTTON_PRIMARY);
        {
            let press_pos = press_pos.clone();
            click_nav.connect_pressed(move |_, _, x, y| press_pos.set(Some((x, y))));
        }
        {
            let picture_for_nav = picture.clone();
            let reader_for_nav = reader.clone();
            let prev = prev.clone();
            let next = next.clone();
            click_nav.connect_released(move |_, _, x, y| {
                let Some((sx, sy)) = press_pos.take() else {
                    return;
                };
                if (x - sx).abs() > MIN_DRAG_PX || (y - sy).abs() > MIN_DRAG_PX {
                    return;
                }
                let w = picture_for_nav.width().max(1) as f64;
                let h = picture_for_nav.height().max(1) as f64;
                let page = reader_for_nav.borrow().page;
                if follow_link(&reader_for_nav, page, x, y, w, h) {
                    return;
                }
                if x < w * 0.2 {
                    prev.emit_clicked();
                } else if x > w * 0.8 {
                    next.emit_clicked();
                }
            });
        }
        picture.add_controller(click_nav);
    }

    {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let two_page_toggle = two_page_toggle.clone();
        prev.connect_clicked(move |_| {
            if continuous_toggle.is_active() {
                let target = reader.borrow().page.saturating_sub(1);
                scroll_continuous_to_page(&reader, &continuous_scroll, target);
                return;
            }
            // Two-page mode steps by a whole spread, not one page, so Prev/Next always land
            // back on a left-hand page.
            let step = if two_page_toggle.is_active() { 2 } else { 1 };
            {
                let mut r = reader.borrow_mut();
                r.page = r.page.saturating_sub(step);
            }
            render();
        });
    }
    {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let two_page_toggle = two_page_toggle.clone();
        next.connect_clicked(move |_| {
            if continuous_toggle.is_active() {
                let target = {
                    let r = reader.borrow();
                    (r.page + 1).min(r.count.saturating_sub(1))
                };
                scroll_continuous_to_page(&reader, &continuous_scroll, target);
                return;
            }
            let step = if two_page_toggle.is_active() { 2 } else { 1 };
            {
                let mut r = reader.borrow_mut();
                if r.page + step < r.count {
                    r.page += step;
                }
            }
            render();
        });
    }
    // Every zoom-changing control (in/out, fit-width, fit-page) funnels through one
    // debounced `request_zoom`, factored out of what used to be two near-identical
    // zoom_in/zoom_out handlers. Two things this buys beyond de-duplication:
    //
    // - Debounce: rapid clicking coalesces into one render+continuous-rebuild ~150ms after
    //   the last click, instead of one full cycle per click.
    // - Deferred continuous rebuild: `rebuild_continuous_view_for_zoom` re-renders every
    //   page in the document (spread across idle ticks — see `build_continuous_view`'s own
    //   doc comment). Paying that cost on every zoom change even while continuous mode
    //   isn't the visible view was pure wasted background work; now it only rebuilds
    //   immediately when continuous mode is actually on-screen, and otherwise just clears
    //   the stale state so the *next* toggle-to-continuous rebuilds fresh at the new zoom.
    let pending_zoom: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let zoom_debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let zoom_baseline = {
        let reader = reader.clone();
        let pending_zoom = pending_zoom.clone();
        move || pending_zoom.get().unwrap_or_else(|| reader.borrow().zoom)
    };
    let request_zoom: Rc<dyn Fn(f64)> = {
        let host = host.clone();
        let reader = reader.clone();
        let render = render.clone();
        let pdf_hash = pdf_hash.to_string();
        let continuous_box = continuous_box.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        let rebuild_notes = rebuild_notes.clone();
        let dialog = reader_window.clone();
        let pending_zoom = pending_zoom.clone();
        let zoom_debounce = zoom_debounce.clone();
        Rc::new(move |target: f64| {
            pending_zoom.set(Some(target.clamp(0.35, 4.0)));
            if let Some(id) = zoom_debounce.borrow_mut().take() {
                id.remove();
            }
            let host = host.clone();
            let reader = reader.clone();
            let render = render.clone();
            let pdf_hash = pdf_hash.clone();
            let continuous_box = continuous_box.clone();
            let continuous_toggle = continuous_toggle.clone();
            let continuous_scroll = continuous_scroll.clone();
            let undo_button = undo_button.clone();
            let redo_button = redo_button.clone();
            let rebuild_notes = rebuild_notes.clone();
            let dialog = dialog.clone();
            let pending_zoom = pending_zoom.clone();
            let zoom_debounce_slot = zoom_debounce.clone();
            let id = glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
                zoom_debounce_slot.borrow_mut().take();
                let Some(new_zoom) = pending_zoom.take() else {
                    return glib::ControlFlow::Break;
                };
                reader.borrow_mut().zoom = new_zoom;
                render();
                if continuous_toggle.is_active() {
                    rebuild_continuous_view_for_zoom(
                        &host,
                        &reader,
                        &pdf_hash,
                        &continuous_box,
                        &continuous_scroll,
                        &undo_button,
                        &redo_button,
                        &rebuild_notes,
                        &dialog,
                    );
                    let page = reader.borrow().page;
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                } else if !reader.borrow().continuous_pictures.is_empty() {
                    while let Some(child) = continuous_box.first_child() {
                        continuous_box.remove(&child);
                    }
                    let mut r = reader.borrow_mut();
                    r.continuous_pictures.clear();
                    r.continuous_offsets.clear();
                    r.continuous_rendered.clear();
                }
                glib::ControlFlow::Break
            });
            *zoom_debounce.borrow_mut() = Some(id);
        })
    };
    for target in [scroll.clone(), continuous_scroll.clone()] {
        let wheel = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        wheel.set_propagation_phase(gtk4::PropagationPhase::Capture);
        {
            let request_zoom = request_zoom.clone();
            let zoom_baseline = zoom_baseline.clone();
            wheel.connect_scroll(move |ctl, _dx, dy| {
                if !ctl
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                {
                    return glib::Propagation::Proceed;
                }
                request_zoom(zoom_baseline() * (1.0 - dy * 0.1));
                glib::Propagation::Stop
            });
        }
        target.add_controller(wheel);

        let pinch = gtk4::GestureZoom::new();
        pinch.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let start_zoom = Rc::new(Cell::new(1.0f64));
        {
            let start_zoom = start_zoom.clone();
            let zoom_baseline = zoom_baseline.clone();
            pinch.connect_begin(move |_, _| start_zoom.set(zoom_baseline()));
        }
        {
            let request_zoom = request_zoom.clone();
            pinch.connect_scale_changed(move |_, scale| request_zoom(start_zoom.get() * scale));
        }
        target.add_controller(pinch);
    }
    {
        let request_zoom = request_zoom.clone();
        let zoom_baseline = zoom_baseline.clone();
        zoom_in.connect_clicked(move |_| request_zoom(zoom_baseline() * 1.25));
    }
    {
        let request_zoom = request_zoom.clone();
        let zoom_baseline = zoom_baseline.clone();
        zoom_out.connect_clicked(move |_| request_zoom(zoom_baseline() / 1.25));
    }
    {
        let request_zoom = request_zoom.clone();
        let scroll = scroll.clone();
        zoom_fit_width.connect_clicked(move |_| {
            let viewport_width = scroll.width().max(1) as f64;
            request_zoom(viewport_width / READER_BASE_WIDTH);
        });
    }
    {
        let request_zoom = request_zoom.clone();
        let reader = reader.clone();
        let scroll = scroll.clone();
        zoom_fit_page.connect_clicked(move |_| {
            let viewport_width = scroll.width().max(1) as f64;
            let viewport_height = scroll.height().max(1) as f64;
            let page_pts = {
                let r = reader.borrow();
                r.display_size(r.page)
            };
            let fit_width_zoom = viewport_width / READER_BASE_WIDTH;
            let aspect = if page_pts.0 > 0.0 {
                page_pts.1 as f64 / page_pts.0 as f64
            } else {
                792.0 / 612.0
            };
            let fit_height_zoom = viewport_height / (READER_BASE_WIDTH * aspect);
            request_zoom(fit_width_zoom.min(fit_height_zoom));
        });
    }
    {
        let reader = reader.clone();
        let render = render.clone();
        rotate_button.connect_clicked(move |_| {
            {
                let mut r = reader.borrow_mut();
                r.rotation = (r.rotation + 90) % 360;
            }
            render();
        });
    }
    {
        let reader = reader.clone();
        let render = render.clone();
        invert_button.connect_toggled(move |btn| {
            reader.borrow_mut().invert_colors = btn.is_active();
            render();
            // `render()` alone only updates the paged view's (possibly hidden) Picture —
            // Continuous mode has its own per-page Pictures with their own last-rendered
            // textures, so they need their own refresh or an inverted toggle would silently
            // do nothing while Continuous (the default mode) is what's actually on screen.
            rerender_loaded_continuous_pages(&reader);
        });
    }
    {
        // Rotation is view-only and only meaningful in single-page mode — see
        // `ReaderState::rotation`'s doc comment. Disabled (rather than just made a no-op)
        // whenever Continuous or Two-page is active, so the limitation is visible instead
        // of a click silently doing nothing; any existing non-zero rotation is cleared on
        // the way in, since neither mode's own layout math accounts for it.
        let reader = reader.clone();
        let render = render.clone();
        let rotate_button = rotate_button.clone();
        let two_page_toggle_for_rotate = two_page_toggle.clone();
        continuous_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                reader.borrow_mut().rotation = 0;
                rotate_button.set_sensitive(false);
                rotate_button.set_tooltip_text(Some("Switch to single-page view to rotate"));
            } else if !two_page_toggle_for_rotate.is_active() {
                rotate_button.set_sensitive(true);
                rotate_button.set_tooltip_text(Some("Rotate page 90°"));
            }
            render();
        });
    }
    {
        let reader = reader.clone();
        let rotate_button = rotate_button.clone();
        let continuous_toggle_for_rotate = continuous_toggle.clone();
        two_page_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                reader.borrow_mut().rotation = 0;
                rotate_button.set_sensitive(false);
                rotate_button.set_tooltip_text(Some("Switch to single-page view to rotate"));
            } else if !continuous_toggle_for_rotate.is_active() {
                rotate_button.set_sensitive(true);
                rotate_button.set_tooltip_text(Some("Rotate page 90°"));
            }
        });
    }
    // Tracks which page is "current" from scroll position alone — connected once, works
    // regardless of whether continuous mode has been built yet (an empty `continuous_offsets`
    // just makes `continuous_page_at` a no-op returning 0). Keeps `r.page`/the page label/
    // prev-next sensitivity live while scrolling, the same things the paged view's `render()`
    // updates on navigation, so "This page"/Progress/Contents-jump-target stay correct no
    // matter which view is currently visible.
    {
        let reader = reader.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        let window_debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        let scroll_for_window = continuous_scroll.clone();
        let reader_for_window = reader.clone();
        continuous_scroll
            .vadjustment()
            .connect_value_changed(move |adj| {
                if let Some(id) = window_debounce.borrow_mut().take() {
                    id.remove();
                }
                {
                    let reader = reader_for_window.clone();
                    let scroll = scroll_for_window.clone();
                    let slot = window_debounce.clone();
                    let id = glib::timeout_add_local_once(
                        std::time::Duration::from_millis(80),
                        move || {
                            slot.borrow_mut().take();
                            refresh_continuous_window(&reader, &scroll, None);
                        },
                    );
                    *window_debounce.borrow_mut() = Some(id);
                }
                let mut r = reader.borrow_mut();
                if r.continuous_offsets.len() < 2 {
                    return;
                }
                let page = continuous_page_at(&r.continuous_offsets, adj.value());
                r.page = page;
                update_page_display(
                    &page_entry,
                    &page_of_label,
                    &prev,
                    &next,
                    &bookmark_button,
                    page,
                    r.count,
                    &r.page_labels,
                    &r.bookmarks,
                );
            });
    }
    {
        let host = host.clone();
        let reader = reader.clone();
        let render = render.clone();
        let pdf_hash = pdf_hash.to_string();
        let continuous_box = continuous_box.clone();
        let continuous_scroll = continuous_scroll.clone();
        let view_stack = view_stack.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        let rebuild_notes = rebuild_notes.clone();
        let dialog = reader_window.clone();
        let two_page_toggle = two_page_toggle.clone();
        continuous_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                two_page_toggle.set_active(false);
                build_continuous_view(
                    &host,
                    &reader,
                    &pdf_hash,
                    &continuous_box,
                    &continuous_scroll,
                    &undo_button,
                    &redo_button,
                    &rebuild_notes,
                    &dialog,
                );
                view_stack.set_visible_child_name("continuous");
                // Deferred to the next idle cycle: the page widgets `build_continuous_view`
                // just added haven't been through a layout pass yet at this point, so the
                // ScrolledWindow's vadjustment `upper` bound is still whatever it was before
                // (typically 0, on the very first activation) — setting the scroll position
                // synchronously here gets silently clamped back to the top. Found live: after
                // resuming a saved page, the page indicator correctly read e.g. "6 of 10" but
                // the visible content was still page 1, every time — continuous mode is the
                // default, so this broke "resume where I left off" for every document.
                let page = reader.borrow().page;
                let reader = reader.clone();
                let continuous_scroll = continuous_scroll.clone();
                glib::idle_add_local_once(move || {
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                });
            } else {
                view_stack.set_visible_child_name("paged");
                render();
            }
        });
        // Continuous scrolling is the default reading mode; `set_active` fires the handler
        // above, which builds the continuous view and switches the stack to it.
        continuous_toggle.set_active(true);
    }
    {
        let render = render.clone();
        let view_stack = view_stack.clone();
        let continuous_toggle = continuous_toggle.clone();
        two_page_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                // Deactivating Continuous (if it was on) runs its own handler above, which
                // switches `view_stack` to "paged" — the single view both single-page and
                // two-page mode share — before `render()` fills `right_picture` too.
                continuous_toggle.set_active(false);
                view_stack.set_visible_child_name("paged");
            }
            render();
        });
    }
    // Typing a page number (the document's own printed label, or a raw file page number —
    // see `find_page_by_label`) and pressing Enter jumps there, navigating by the printed
    // page number rather than always the raw file position.
    {
        let reader = reader.clone();
        let render = render.clone();
        let host = host.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        page_entry.clone().connect_activate(move |entry| {
            let text = entry.text();
            let target = {
                let r = reader.borrow();
                find_page_by_label(&r.page_labels, &text)
            };
            match target {
                Some(page) if page < reader.borrow().count => {
                    if continuous_toggle.is_active() {
                        scroll_continuous_to_page(&reader, &continuous_scroll, page);
                    } else {
                        reader.borrow_mut().page = page;
                        render();
                    }
                }
                _ => {
                    host.notify("No such page");
                    // Revert to the current page's actual label/number rather than leaving
                    // the entry showing whatever unresolvable text was typed.
                    let (page, count) = {
                        let r = reader.borrow();
                        (r.page, r.count)
                    };
                    let labels = reader.borrow().page_labels.clone();
                    let bookmarks = reader.borrow().bookmarks.clone();
                    update_page_display(
                        &page_entry,
                        &page_of_label,
                        &prev,
                        &next,
                        &bookmark_button,
                        page,
                        count,
                        &labels,
                        &bookmarks,
                    );
                }
            }
        });
    }
    {
        let reader = reader.clone();
        let hint = hint.clone();
        let picture = picture.clone();
        let style_drop_for_mode = style_drop.clone();
        let palette_choice = palette_choice.clone();
        let apply_mode: Rc<dyn Fn()> = Rc::new(move || {
            let style_drop = &style_drop_for_mode;
            let choice = palette_choice.get();
            let kind = choice.map(|_| {
                MARK_KIND_OPTIONS
                    .get(style_drop.selected() as usize)
                    .map(|(_, k)| *k)
                    .unwrap_or(fond_bib::AnnotationKind::Highlight)
            });
            style_drop.set_sensitive(choice.is_some());
            {
                let mut r = reader.borrow_mut();
                r.draw_kind = kind;
                if let Some(color) = choice.and_then(|i| crate::palette::HIGHLIGHT_COLORS.get(i)) {
                    r.draw_color = color.hex.to_string();
                }
            }
            let cursor = cursor_for_select_mode(kind.is_none());
            picture.set_cursor(cursor.as_ref());
            for p in &reader.borrow().continuous_pictures {
                p.set_cursor(cursor.as_ref());
            }
            let text = match kind {
                None => "Drag over text to select it, then choose a colour (or press 1–4)",
                Some(fond_bib::AnnotationKind::Highlight) => "Drag over text to highlight it",
                Some(fond_bib::AnnotationKind::Underline) => "Drag over text to underline it",
                Some(fond_bib::AnnotationKind::Strikeout) => "Drag over text to strike it out",
                Some(fond_bib::AnnotationKind::Note) => "Drag over the page",
            };
            hint.set_text(text);
        });
        {
            let apply_mode = apply_mode.clone();
            style_drop.connect_selected_notify(move |_| apply_mode());
        }
        *mode_change.borrow_mut() = Some(apply_mode);
    }
    {
        let host = host.clone();
        let reader = reader.clone();
        let pdf_hash = pdf_hash.to_string();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        let rebuild_notes = rebuild_notes.clone();
        let dialog = reader_window.clone();
        let refresh: Rc<dyn Fn()> = {
            let reader = reader.clone();
            let render = render.clone();
            Rc::new(move || {
                render();
                let page = reader.borrow().page;
                render_continuous_page(&reader, page);
            })
        };
        note_button.connect_clicked(move |_| {
            show_pdf_note_dialog(
                &host,
                &reader,
                &pdf_hash,
                &undo_button,
                &redo_button,
                rebuild_notes.clone(),
                &dialog,
                refresh.clone(),
            );
        });
    }
    {
        let host = host.clone();
        let reader = reader.clone();
        let rebuild_notes = rebuild_notes.clone();
        bookmark_button.connect_clicked(move |btn| {
            let page_num = reader.borrow().page as u32 + 1;
            let now_bookmarked = {
                let mut r = reader.borrow_mut();
                if let Some(pos) = r.bookmarks.iter().position(|&p| p == page_num) {
                    r.bookmarks.remove(pos);
                    false
                } else {
                    r.bookmarks.push(page_num);
                    r.bookmarks.sort_unstable();
                    true
                }
            };
            host.save_bookmarks(&reader.borrow().bookmarks);
            update_bookmark_button(btn, now_bookmarked);
            rebuild_notes();
        });
    }
    {
        let host = host.clone();
        let reader = reader.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        let dialog = reader_window.clone();
        page_num_button.connect_clicked(move |_| {
            show_page_number_dialog(
                &host,
                &reader,
                &page_entry,
                &page_of_label,
                &prev,
                &next,
                &bookmark_button,
                &dialog,
            );
        });
    }
    {
        let window = window.clone();
        let pdf_hash = pdf_hash.to_string();
        let reader_tab = reader_tab.clone();
        popout_button.connect_clicked(move |_| {
            let new_tab = reader_tab.pop_out(&window);
            crate::register_reader(&pdf_hash, &new_tab);
        });
    }
    {
        let host = host.clone();
        let reader = reader.clone();
        let title = title.to_string();
        let dialog = reader_window.clone();
        export_button.connect_clicked(move |_| {
            export_notes(&host, &reader, &title, &dialog);
        });
    }
    // Search: run on Enter (not per-keystroke — PDFium re-searches every page each time, not
    // worth doing on every character typed), jumping straight to the first match's page.
    // Prev/Next cycle `search_current` with wraparound; the count label and match highlight
    // (blended in `render()`, a distinct colour from saved highlights) follow along.
    // Jump to `page` after a search match changes: in continuous mode, scroll there and
    // re-render both it and the previous match's page (to clear that page's now-stale match
    // tint — cheap no-op if continuous mode was never built); in paged mode, the shared
    // `render()` already re-blends the match into whichever page it lands on.
    let goto_search_match = {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        Rc::new(move |page: u16, previous_page: Option<u16>| {
            if continuous_toggle.is_active() {
                scroll_continuous_to_page(&reader, &continuous_scroll, page);
                render_continuous_page(&reader, page);
                if let Some(prev_page) = previous_page {
                    if prev_page != page {
                        render_continuous_page(&reader, prev_page);
                    }
                }
            } else {
                render();
            }
        })
    };
    let run_search = {
        let reader = reader.clone();
        let goto_search_match = goto_search_match.clone();
        let search_prev = search_prev.clone();
        let search_next = search_next.clone();
        let search_count = search_count.clone();
        Rc::new(move |query: &str| {
            let matches = {
                let r = reader.borrow();
                fond_doc::search_document(r.pdfium, &r.bytes, query).unwrap_or_default()
            };
            let count = matches.len();
            let first_page = matches.first().map(|m| m.page);
            let previous_page = {
                let r = reader.borrow();
                r.search_matches.get(r.search_current).map(|m| m.page)
            };
            {
                let mut r = reader.borrow_mut();
                r.search_matches = matches;
                r.search_current = 0;
                if let Some(page) = first_page {
                    r.page = page;
                }
            }
            search_prev.set_sensitive(count > 0);
            search_next.set_sensitive(count > 0);
            search_count.set_text(&match (count, query.trim().is_empty()) {
                (0, true) => String::new(),
                (0, false) => "No matches".to_string(),
                (n, _) => format!("1 of {n}"),
            });
            if let Some(page) = first_page {
                goto_search_match(page, previous_page);
            }
        })
    };
    {
        let run_search = run_search.clone();
        search_entry.connect_activate(move |entry| run_search(&entry.text()));
    }
    {
        // Clear stale results (and the match highlight) as soon as the box is emptied,
        // rather than leaving them stuck until another search is run.
        let reader = reader.clone();
        let render = render.clone();
        let search_prev = search_prev.clone();
        let search_next = search_next.clone();
        let search_count = search_count.clone();
        search_entry.connect_search_changed(move |entry| {
            if entry.text().is_empty() {
                let cleared_page = {
                    let mut r = reader.borrow_mut();
                    let page = r.search_matches.get(r.search_current).map(|m| m.page);
                    r.search_matches.clear();
                    page
                };
                search_prev.set_sensitive(false);
                search_next.set_sensitive(false);
                search_count.set_text("");
                render();
                if let Some(page) = cleared_page {
                    render_continuous_page(&reader, page);
                }
            }
        });
    }
    {
        let reader = reader.clone();
        let goto_search_match = goto_search_match.clone();
        let search_count = search_count.clone();
        search_prev.connect_clicked(move |_| {
            let (previous_page, page) = {
                let mut r = reader.borrow_mut();
                if r.search_matches.is_empty() {
                    return;
                }
                let previous_page = r.search_matches[r.search_current].page;
                r.search_current = if r.search_current == 0 {
                    r.search_matches.len() - 1
                } else {
                    r.search_current - 1
                };
                let page = r.search_matches[r.search_current].page;
                r.page = page;
                search_count.set_text(&format!(
                    "{} of {}",
                    r.search_current + 1,
                    r.search_matches.len()
                ));
                (previous_page, page)
            };
            goto_search_match(page, Some(previous_page));
        });
    }
    {
        let reader = reader.clone();
        let goto_search_match = goto_search_match.clone();
        let search_count = search_count.clone();
        search_next.connect_clicked(move |_| {
            let (previous_page, page) = {
                let mut r = reader.borrow_mut();
                if r.search_matches.is_empty() {
                    return;
                }
                let previous_page = r.search_matches[r.search_current].page;
                r.search_current = (r.search_current + 1) % r.search_matches.len();
                let page = r.search_matches[r.search_current].page;
                r.page = page;
                search_count.set_text(&format!(
                    "{} of {}",
                    r.search_current + 1,
                    r.search_matches.len()
                ));
                (previous_page, page)
            };
            goto_search_match(page, Some(previous_page));
        });
    }

    // Save the current page back to the entry's Progress on close, so the next "Read" opens
    // where this session left off — the PDF-reader half of Tier 2a. `page`/`count` snapshot
    // out of `reader` up front since the RefCell isn't needed once we're just writing to the
    // library.
    let closed = Rc::new(Cell::new(false));
    {
        let host = host.clone();
        let reader = reader.clone();
        let closed = closed.clone();
        let last_saved = Cell::new(start_page);
        glib::timeout_add_local(std::time::Duration::from_secs(5), move || {
            if closed.get() {
                return glib::ControlFlow::Break;
            }
            let (page, count) = {
                let r = reader.borrow();
                (r.page, r.count)
            };
            if page != last_saved.get() {
                last_saved.set(page);
                host.save_progress(fond_bib::Progress {
                    page: page as u32 + 1,
                    of: count as u32,
                    chapter_percent: None,
                });
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let host = host.clone();
        let pdf_hash = pdf_hash.to_string();
        let reader = reader.clone();
        crate::reader_host::on_tab_closed(&reader_tab, move || {
            closed.set(true);
            let (page, count) = {
                let r = reader.borrow();
                (r.page as u32 + 1, r.count as u32)
            };
            host.save_progress(fond_bib::Progress {
                page,
                of: count,
                chapter_percent: None,
            });
            crate::unregister_window(&pdf_hash);
        });
    }

    reader_tab.present();
}

/// Thumbnail width for the sidebar list — deliberately much smaller than the reader's own
/// `READER_BASE_WIDTH`, since this is a jump-to-page aid, not a reading surface.
const THUMBNAIL_GRID_WIDTH: u32 = 140;

/// Builds the Thumbnails tab of the Contents sidebar: every page as a small thumbnail in a
/// vertical list, for jumping around a long document visually instead of by number —
/// Okular/Acrobat's "page overview" mode, docked instead of a popup. Thumbnails are plain
/// unannotated renders (no highlight/search blending — this is for navigation, not a
/// miniature reading view).
///
/// Rendering is deferred: this only builds the row widgets (cheap) and hands back a trigger
/// closure that does the actual rasterizing, lazily and only once — call it when the tab is
/// first shown (see the `Thumbnails` toggle below), not at reader-open time, so a document
/// nobody ever opens the Thumbnails tab for never pays the render cost. Once triggered, pages
/// rasterize nearest-to-current-page first, one per idle tick — same spreading idiom as
/// continuous mode's own `schedule_continuous_render`, so it doesn't block the UI even on a
/// long document.
fn build_thumbnails_sidebar(
    reader: &Rc<RefCell<ReaderState>>,
    render: &Rc<impl Fn() + 'static>,
    continuous_toggle: &gtk4::ToggleButton,
    continuous_scroll: &gtk4::ScrolledWindow,
) -> (gtk4::ScrolledWindow, Rc<dyn Fn()>) {
    let count = reader.borrow().count;

    let rows = gtk4::Box::new(Orientation::Vertical, 8);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);

    let mut pictures = Vec::with_capacity(count as usize);
    for page in 0..count {
        let picture = gtk4::Picture::new();
        picture.set_size_request(-1, 128);
        picture.set_content_fit(gtk4::ContentFit::Contain);

        let card = gtk4::Box::new(Orientation::Vertical, 2);
        card.append(&picture);
        let label = gtk4::Label::new(Some(&(page + 1).to_string()));
        label.add_css_class("caption");
        label.add_css_class("dim-label");
        card.append(&label);

        let button = gtk4::Button::new();
        button.add_css_class("flat");
        button.set_child(Some(&card));

        {
            let reader = reader.clone();
            let render = render.clone();
            let continuous_toggle = continuous_toggle.clone();
            let continuous_scroll = continuous_scroll.clone();
            button.connect_clicked(move |_| {
                if continuous_toggle.is_active() {
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                } else {
                    reader.borrow_mut().page = page;
                    render();
                }
            });
        }

        rows.append(&button);
        pictures.push(picture);
    }

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_child(Some(&rows));

    let built = Rc::new(Cell::new(false));
    let trigger: Rc<dyn Fn()> = {
        let reader = reader.clone();
        let built = built.clone();
        let scroll = scroll.clone();
        Rc::new(move || {
            if built.replace(true) {
                return;
            }
            let current_page = reader.borrow().page;
            let mut order: Vec<u16> = (0..count).collect();
            order.sort_by_key(|&p| (p as i32 - current_page as i32).unsigned_abs());
            schedule_thumbnail_render(reader.clone(), pictures.clone(), order, 0);

            // Jump the list roughly to the current page on first show, rather than always
            // opening at the top — exact scrolling would need each row's real allocation,
            // which isn't settled yet the same frame the tab becomes visible, so this is an
            // approximation (fraction of the way through the document), good enough for a
            // jump-near-here aid.
            if count > 0 {
                let fraction = current_page as f64 / count as f64;
                let scroll = scroll.clone();
                glib::idle_add_local_once(move || {
                    let adj = scroll.vadjustment();
                    adj.set_value(fraction * adj.upper());
                });
            }
        })
    };

    (scroll, trigger)
}

/// One tick of `show_thumbnail_grid`'s lazy rasterization — see that function's doc comment.
fn schedule_thumbnail_render(
    reader: Rc<RefCell<ReaderState>>,
    pictures: Vec<gtk4::Picture>,
    order: Vec<u16>,
    idx: usize,
) {
    let Some(&page) = order.get(idx) else {
        return;
    };
    if let Some(picture) = pictures.get(page as usize) {
        let r = reader.borrow();
        if let Some(rp) = render_open(&r, page, THUMBNAIL_GRID_WIDTH) {
            let data = glib::Bytes::from(&rp.rgba);
            let texture = gdk::MemoryTexture::new(
                rp.width as i32,
                rp.height as i32,
                gdk::MemoryFormat::R8g8b8a8,
                &data,
                (rp.width * 4) as usize,
            );
            picture.set_paintable(Some(&texture));
        }
    }
    glib::idle_add_local_once(move || {
        schedule_thumbnail_render(reader, pictures, order, idx + 1);
    });
}

/// Export every bookmark and annotation — format, grouping and citation key chosen in a
/// dialog (see [`crate::export`]).
fn export_notes(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    title: &str,
    reader_window: &adw::Window,
) {
    let (items, bookmarks) = {
        let r = reader.borrow();
        let items = crate::export::items_from_sidecar(&r.annotations, &r.page_labels, &|_| None);
        let bookmarks = r
            .bookmarks
            .iter()
            .map(|&p| {
                let label = r
                    .page_labels
                    .get((p as usize).saturating_sub(1))
                    .and_then(|l| l.clone())
                    .unwrap_or_else(|| p.to_string());
                format!("p. {label}")
            })
            .collect();
        (items, bookmarks)
    };
    crate::export::show_export_dialog(host, reader_window, title, items, bookmarks);
}

/// A small modal that anchors the reader's *current* physical page to its own printed page
/// number — the manual counterpart to the automatic `/PageLabels` read in `show_pdf_reader`,
/// for a PDF that declares no page labels of its own (see `fond_bib::PageLabelOverride`).
/// Leaving the entry blank and confirming clears any existing override, reverting to raw file
/// page numbers. Writes straight to `notes/<key>.md` (same `load_note`/`write_note` pattern
/// as `Progress`) and updates the live reader in place, so the change is visible immediately
/// without reopening the document.
#[allow(clippy::too_many_arguments)]
fn show_page_number_dialog(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    page_entry: &gtk4::Entry,
    page_of_label: &gtk4::Label,
    prev: &gtk4::Button,
    next: &gtk4::Button,
    bookmark_button: &gtk4::Button,
    reader_window: &adw::Window,
) {
    let (page, count) = {
        let r = reader.borrow();
        (r.page, r.count)
    };

    let dialog = adw::Window::new();
    dialog.set_title(Some("Set page numbering"));
    dialog.set_modal(true);
    // Modal against the reader window, not the main library window — see the same note on
    // `show_pdf_note_dialog`.
    dialog.set_transient_for(Some(reader_window));
    dialog.set_default_size(380, -1);

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.add_css_class("fond-chrome");
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    let set_button = gtk4::Button::with_label("Set");
    set_button.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&set_button);
    view.add_top_bar(&header);

    let content = gtk4::Box::new(Orientation::Vertical, 8);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    let hint = gtk4::Label::new(Some(&format!(
        "This is file page {} of {count}. What number is printed on it? Later pages count up \
         from here; earlier ones are left unlabeled.",
        page + 1
    )));
    hint.add_css_class("dim-label");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    let entry = gtk4::Entry::builder()
        .placeholder_text("e.g. 1 — leave blank to clear")
        .activates_default(true)
        .build();
    content.append(&entry);
    content.append(&hint);
    view.set_content(Some(&content));
    dialog.set_content(Some(&view));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let host = host.clone();
        let dialog = dialog.clone();
        let reader = reader.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        set_button.connect_clicked(move |_| {
            let text = entry.text().trim().to_string();
            let override_value = if text.is_empty() {
                None
            } else {
                match text.parse::<i64>() {
                    Ok(n) => Some(fond_bib::PageLabelOverride {
                        start_page: page as u32 + 1,
                        start_label: n,
                    }),
                    Err(_) => {
                        host.notify("Enter a whole number, or leave blank to clear");
                        return;
                    }
                }
            };

            host.set_page_label_override(override_value);

            let new_labels = override_value.map(|ov| ov.apply(count)).unwrap_or_default();
            reader.borrow_mut().page_labels = new_labels.clone();
            let bookmarks = reader.borrow().bookmarks.clone();
            update_page_display(
                &page_entry,
                &page_of_label,
                &prev,
                &next,
                &bookmark_button,
                page,
                count,
                &new_labels,
                &bookmarks,
            );
            host.notify(if override_value.is_some() {
                "Page numbering set"
            } else {
                "Page numbering cleared"
            });
            dialog.close();
        });
    }

    dialog.present();
}

/// A small modal for adding a freestanding marginal note (`AnnotationKind::Note`) to the
/// PDF reader's *current* page — unlike Highlight/Underline/Strikeout, a note isn't tied to
/// a drawn region, so there's no drag gesture for it, just this prompt. Saves straight into
/// `reader`'s in-memory sidecar and to disk, the same `annots/<key>.json` the drag gesture
/// writes. When opened right after a "Select text" drag on this page, the note carries that
/// selection's real quadpoints (see `last_selection`), so — unlike a plain marginal note on
/// blank page — it does need a re-render (`refresh`) afterward to show up on the page.
#[allow(clippy::too_many_arguments)]
fn show_pdf_note_dialog(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    pdf_hash: &str,
    undo_button: &gtk4::Button,
    redo_button: &gtk4::Button,
    rebuild_notes: Rc<dyn Fn()>,
    reader_window: &adw::Window,
    refresh: Rc<dyn Fn()>,
) {
    let current_page = reader.borrow().page as u32 + 1;

    let dialog = adw::Window::new();
    dialog.set_title(Some(&format!("Note on page {current_page}")));
    dialog.set_modal(true);
    // Modal against the reader window itself, not the main library window — otherwise
    // opening this from within the reader leaves the reader interactive but blocks the
    // library behind it, which is backwards and is what "the reader blocks the library"
    // reports were actually seeing (the reader's own top-level window was never modal).
    dialog.set_transient_for(Some(reader_window));
    dialog.set_default_size(420, 260);

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.add_css_class("fond-chrome");
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    let save = gtk4::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&save);
    view.add_top_bar(&header);

    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_margin_top(8);
    text_view.set_margin_bottom(8);
    text_view.set_margin_start(8);
    text_view.set_margin_end(8);

    // Pre-fill with the last "Select text" copy, quoted with its page number, if it was made
    // on this same page — consumed either way so a stale selection from another page doesn't
    // linger into some later, unrelated note. Its quadpoints (if any) carry over onto the
    // created annotation too, so the note anchors to — and stays visibly marked at — the
    // actual selected text on the page, rather than being a page-only marginal note.
    let (selection_quads, selection_quote) = {
        let mut r = reader.borrow_mut();
        match r.last_selection.take() {
            Some((sel_page, text, quads)) if sel_page == r.page => (quads, Some(text)),
            _ => (Vec::new(), None),
        }
    };

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&text_view));
    let body = gtk4::Box::new(Orientation::Vertical, 0);
    if let Some(quote) = &selection_quote {
        let quote_label = gtk4::Label::new(Some(quote));
        quote_label.set_wrap(true);
        quote_label.set_xalign(0.0);
        quote_label.set_selectable(true);
        quote_label.add_css_class("dim-label");
        quote_label.set_margin_top(8);
        quote_label.set_margin_start(12);
        quote_label.set_margin_end(12);
        body.append(&quote_label);
    }
    body.append(&scrolled);
    view.set_content(Some(&body));
    text_view.grab_focus();
    dialog.set_content(Some(&view));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let host = host.clone();
        let host = host.clone();
        let reader = reader.clone();
        let pdf_hash = pdf_hash.to_string();
        let text_view = text_view.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        save.connect_clicked(move |_| {
            let buffer = text_view.buffer();
            let text = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .trim()
                .to_string();
            if text.is_empty() {
                host.notify("Note is empty");
                return;
            }

            let has_region = !selection_quads.is_empty();
            let annotation = fond_bib::Annotation::drawn(
                fond_bib::AnnotationKind::Note,
                current_page,
                selection_quads.clone(),
                selection_quote.clone(),
                Some(text),
                None,
            );
            push_undo_snapshot(&reader);
            {
                let mut r = reader.borrow_mut();
                r.annotations.pdf_hash = Some(pdf_hash.clone());
                r.annotations.upsert(annotation);
            }
            let write_result = host.save_annotations(&reader.borrow().annotations);
            match write_result {
                Ok(()) => {
                    if has_region {
                        refresh();
                    }
                    sync_undo_redo_buttons(&reader, &undo_button, &redo_button);
                    rebuild_notes();
                    host.notify("Note added");
                    dialog.close();
                }
                Err(e) => host.notify(&e),
            }
        });
    }

    dialog.present();
}
