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

use crate::annotation_store::AnnotationStore;
use crate::page_geom::PageGeom;
use crate::{color_swatch, note_edit_widget, popover_button, popover_separator, ReaderHost};

mod bookmark;
mod context_click;
mod context_menu;
mod continuous;
mod dialogs;
mod drag_gesture;
mod drag_preview;
mod export;
mod keys;
mod mark_layer;
mod open_pdf;
mod scrollbar_ticks;
use open_pdf::OpenedPdf;
mod render_actions;
use render_actions::*;
mod sidebar;
use sidebar::*;
mod page_nav;
use page_nav::*;
mod tool_buttons;
use tool_buttons::*;
mod mark_palette;
use mark_palette::*;
mod header_buttons;
use header_buttons::*;
mod status_bar;
use status_bar::*;
mod canvas;
use canvas::*;
mod link_nav;
mod links;
mod mark_mode;
mod nav;
mod next_page;
mod note_button;
mod notes_toggle;
mod page_click;
mod page_entry;
mod page_number;
mod popout;
mod prev_next;
mod scan;
mod scan_thread;
mod search;
mod search_thread;
mod session;
mod sidebar_toggle;
mod text_view;
mod texture_cache;
mod ui;
mod view_modes;
mod worker;
mod zoom;
use texture_cache::TextureCache;
use ui::PdfUi;
use worker::{Job, RenderKey, RenderWorker, Rendered, Tone};
mod notes;
mod ocr;
mod render;
mod selection;
mod thumbnails;
use context_menu::*;
use continuous::*;
use dialogs::*;
use drag_preview::*;
use links::*;
use nav::*;
use ocr::*;
use render::*;
use selection::*;
use thumbnails::*;

/// Live state of an open PDF reader window.
struct ReaderState {
    pdfium: &'static fond_doc::Pdfium,
    /// The file's bytes, read only if something asks: a few `fond_doc` helpers take raw bytes
    /// and re-parse them on each call. Reading eagerly cost the whole file in memory for every
    /// open document, and a second copy besides.
    bytes: std::cell::OnceCell<Vec<u8>>,
    /// The document, parsed once and kept open for rendering and page geometry, loaded from the
    /// file path so PDFium reads it on demand instead of holding a copy.
    doc: Option<pdfium_render::prelude::PdfDocument<'static>>,
    page: u16,
    count: u16,
    /// Render width in px = `BASE_WIDTH * zoom`.
    zoom: f64,
    /// This document's annotations, with undo/redo and saving; see [`AnnotationStore`].
    store: Rc<AnnotationStore>,
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
    draw_kind: Option<fond_annot::AnnotationKind>,
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
    /// time (built lazily, not at reader-open, so plain "Read" stays as fast as it always was).
    /// `continuous_offsets[i]` is page `i`'s top position in the view, in pixels, for
    /// scroll-to-page and for tracking which page is "current" from scroll position; `continuous`
    /// holds the widgets, which exist only for pages near the viewport.
    continuous: Option<ContinuousPages>,
    continuous_offsets: Vec<f64>,
    /// Jump to a page because a link was followed (records where we came from); set once the
    /// widgets it drives exist.
    link_goto: Option<Rc<dyn Fn(u16)>>,
    /// Pages we followed links away from, most recent last, for the Back button.
    nav_back: Vec<u16>,
    /// While the Text view is showing: where "go to page" navigation is redirected, and how
    /// zoom steps are applied (to the text size instead of the page render).
    text_goto: Option<Rc<dyn Fn(u16)>>,
    text_zoom: Option<Rc<dyn Fn(f64)>>,
    /// Inclusive range of pages that currently have widgets in continuous mode.
    continuous_window: (u16, u16),
    /// Finished page pictures, reused across scrolling, zooming back and forth, and redraws.
    textures: TextureCache,
    /// Whether the background scan has read every page's size yet. Until it has, layout assumes
    /// every page is the size of `layout_page`.
    scanned: bool,
    layout_page: u16,
    /// Where the document lives, for threads that open their own copy.
    path: std::path::PathBuf,
    /// The search in progress, if any; replacing it cancels it.
    search: Option<search_thread::SearchHandle>,
    /// Rasterises pages off the GTK thread.
    worker: Option<RenderWorker>,
    /// Each page's document-defined `/PageLabels` printed number (`None` where the PDF
    /// doesn't define one, which is most PDFs) — read once at open, since it's an immutable
    /// property of the file. Index `i` (0-based) matches every other page index in this
    /// struct.
    page_labels: Vec<Option<String>>,
    /// Clockwise display rotation in degrees (0/90/180/270) — view-only, single-page mode
    /// only (see `rotate_button`'s wiring): the render pipeline blends annotations in the
    /// PDF's own unrotated coordinate space and only rotates the final pixel buffer for
    /// display, so a rotated page's drag-to-annotate/right-click coordinate math would be
    /// wrong; those are disabled while this is non-zero rather than taught to un-rotate a
    /// click first. Reset to 0 whenever Continuous or Two-page mode is entered, since
    /// neither one's layout math (continuous page offsets computed from the *unrotated*
    /// page size; two-page's facing-page box) accounts for a rotated page either.
    rotation: u16,
    /// Dark or sepia page colours (scanned and white-background PDFs are just pixels, so the
    /// app's own dark theme can't reach them) — applied to the same final pixel buffer
    /// `rotation` is, in both paged and continuous mode alike.
    tone: Tone,
    /// Marks on the continuous view's scrollbar edge, repainted when annotations or search
    /// matches change.
    tick_layer: Option<gtk4::DrawingArea>,
    /// The sidebar's thumbnail pictures, filled in as the render thread delivers them.
    thumbnail_pictures: Vec<gtk4::Picture>,
    /// Bookmarked pages, 1-based (`Annotation.page` numbering) — loaded once at open,
    /// rewritten to the host on every add/remove, same lifecycle as `annotations`. Kept
    /// sorted so the Notes sidebar's "Bookmarks" section lists them in page order.
    bookmarks: Vec<u32>,
}

impl ReaderState {
    fn bytes(&self) -> &[u8] {
        self.bytes
            .get_or_init(|| std::fs::read(&self.path).unwrap_or_default())
    }

    fn geom(&self, page: u16) -> Option<PageGeom> {
        if let Some(cached) = self.page_geoms.borrow().get(&page) {
            return *cached;
        }
        let _span = crate::perf::span(|| format!("geometry page {}", page + 1));
        let geom = match &self.doc {
            Some(doc) => PageGeom::read_doc(doc, page),
            None => self
                .pdfium
                .load_pdf_from_byte_slice(self.bytes(), None)
                .ok()
                .and_then(|d| PageGeom::read_doc(&d, page)),
        };
        self.page_geoms.borrow_mut().insert(page, geom);
        geom
    }

    /// The size layout should give `page`: its real size once the scan has run, and until then
    /// the starting page's size for every page, so opening never reads them all up front.
    fn layout_size(&self, page: u16) -> (f32, f32) {
        if self.scanned {
            self.display_size(page)
        } else {
            self.display_size(self.layout_page)
        }
    }

    /// On-screen point size of `page` — for layout only, so falls back to US Letter.
    fn display_size(&self, page: u16) -> (f32, f32) {
        self.geom(page)
            .map(|g| g.display_size())
            .unwrap_or((612.0, 792.0))
    }
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

/// Late-bound "mark the current selection with colour N" action, filled in once the notes
/// sidebar it refreshes exists.
type QuickMarkSlot = Rc<RefCell<Option<Rc<dyn Fn(usize)>>>>;
/// Self-referential slot for the notes-sidebar rebuild closure — a row's own delete button
/// needs to trigger a fresh rebuild of the list it lives in.
type RebuildNotesCell = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// The mark-style `DropDown` beside the colour palette, shared by both readers. "Select
/// text" isn't here — it's the palette's own first button in the PDF reader, and the EPUB
/// reader's native selection is always available.
pub(crate) const MARK_KIND_OPTIONS: [(&str, fond_annot::AnnotationKind); 3] = [
    ("Highlight", fond_annot::AnnotationKind::Highlight),
    ("Underline", fond_annot::AnnotationKind::Underline),
    ("Strikeout", fond_annot::AnnotationKind::Strikeout),
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
    crate::perf::watch_main_loop();
    crate::perf::mark("show_pdf_reader start");
    let open_span = crate::perf::span(|| "open_pdf".to_string());
    let Some(OpenedPdf {
        reader,
        outline_entries,
        has_native_page_labels,
        start_page,
    }) = open_pdf::open_pdf(host, window, pdf_hash, blob, start_page)
    else {
        return;
    };
    drop(open_span);

    let PageNavParts {
        view,
        title_widget,
        prev,
        next,
        page_entry,
        page_of_label,
        bookmark_button,
        nav,
    } = page_nav::build_page_nav(&reader, start_page, title);
    let ToolButtonsParts {
        zoom_out,
        zoom_in,
        zoom_fit_width,
        zoom_fit_page,
        rotate_button,
        invert_button,
        note_button,
        page_num_button,
    } = tool_buttons::build_tool_buttons(&nav, &bookmark_button, has_native_page_labels);

    let MarkPaletteParts {
        mode_change,
        style_drop,
        palette_choice,
        palette,
    } = mark_palette::build_mark_palette();
    let HeaderButtonsParts {
        sidebar_toggle,
        notes_toggle,
        continuous_toggle,
        text_toggle,
        two_page_toggle,
        undo_button,
        redo_button,
        popout_button,
        export_button,
        header_start,
        header_end,
        more_button,
    } = header_buttons::build_header_buttons(&note_button, &page_num_button, &palette, &style_drop);
    header_end.append(&more_button);
    let StatusBarParts {
        statusbar,
        search_entry,
        search_prev,
        search_next,
        search_count,
        link_back,
    } = status_bar::build_status_bar(
        &header_end,
        &invert_button,
        &rotate_button,
        &zoom_fit_page,
        &zoom_fit_width,
        &zoom_in,
        &zoom_out,
        &notes_toggle,
        &nav,
    );
    view.add_bottom_bar(&statusbar);

    let CanvasParts {
        hint,
        picture,
        drag_preview,
        drag_live_rect,
        right_picture,
        scroll,
        continuous_box,
        continuous_scroll,
        view_stack,
        content,
    } = canvas::build_canvas(&reader);
    // `content` is reparented into the sidebar Paned below instead of set directly here —
    // the Notes sidebar (and, when present, Contents) always builds that Paned now, and
    // `Paned::set_end_child` asserts its child has no existing parent.
    let reader_tab = crate::reader_host::open_reader_tab(window, title, &view);
    let reader_window = reader_tab.host_window.clone();
    let quick_mark: QuickMarkSlot = Rc::new(RefCell::new(None));
    let reflow_popover: RebuildNotesCell = Rc::new(RefCell::new(None));
    crate::register_reader(pdf_hash, &reader_tab);
    crate::label_icon_buttons(&header_start);
    crate::label_icon_buttons(&header_end);
    crate::label_icon_buttons(&statusbar);
    let RenderActionsParts { render, undo, redo } = render_actions::build_render_actions(
        &bookmark_button,
        &continuous_toggle,
        &header_end,
        &header_start,
        host,
        &next,
        &page_entry,
        &page_of_label,
        &picture,
        &prev,
        &reader,
        &reader_tab,
        &redo_button,
        &right_picture,
        &title_widget,
        &two_page_toggle,
        &undo_button,
    );

    let SidebarParts {
        sidebar_box,
        notes_rows,
        notes_scroll,
    } = sidebar::build_sidebar(
        &continuous_scroll,
        &continuous_toggle,
        &outline_entries,
        &reader,
        &render,
    );
    let (rebuild_notes, quiet_notes) = notes::install_notes_sidebar(
        host,
        &reader,
        &render,
        &continuous_toggle,
        &continuous_scroll,
        &bookmark_button,
        &notes_rows,
    );
    {
        let store = reader.borrow().store.clone();
        let rebuild_notes = rebuild_notes.clone();
        let quiet_notes = quiet_notes.clone();
        store.subscribe(move |_, _| {
            if !quiet_notes.get() {
                rebuild_notes();
            }
        });
    }

    {
        let host = host.clone();
        let reader = reader.clone();
        let reader_window = reader_window.clone();
        *quick_mark.borrow_mut() = Some(Rc::new(move |i: usize| {
            let ctx = MarkCtx {
                host: host.clone(),
                reader: reader.clone(),
                reader_window: reader_window.clone(),
            };
            if let Some(color) = crate::palette::HIGHLIGHT_COLORS.get(i) {
                apply_selection_mark(&ctx, fond_annot::AnnotationKind::Highlight, color.hex);
            }
        }));
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
    let ui = ui::PdfUi {
        host: host.clone(),
        reader: reader.clone(),
        reader_tab: reader_tab.clone(),
        reader_window: reader_window.clone(),
        render: render.clone(),
        undo: undo.clone(),
        redo: redo.clone(),
        quick_mark: quick_mark.clone(),
        reflow_popover: reflow_popover.clone(),
        mode_change: mode_change.clone(),
        palette_choice: palette_choice.clone(),
        palette: palette.clone(),
        style_drop: style_drop.clone(),
        rebuild_notes: rebuild_notes.clone(),
        view: view.clone(),
        prev: prev.clone(),
        next: next.clone(),
        page_entry: page_entry.clone(),
        page_of_label: page_of_label.clone(),
        bookmark_button: bookmark_button.clone(),
        zoom_out: zoom_out.clone(),
        zoom_in: zoom_in.clone(),
        zoom_fit_width: zoom_fit_width.clone(),
        zoom_fit_page: zoom_fit_page.clone(),
        rotate_button: rotate_button.clone(),
        invert_button: invert_button.clone(),
        note_button: note_button.clone(),
        page_num_button: page_num_button.clone(),
        sidebar_toggle: sidebar_toggle.clone(),
        notes_toggle: notes_toggle.clone(),
        continuous_toggle: continuous_toggle.clone(),
        text_toggle: text_toggle.clone(),
        two_page_toggle: two_page_toggle.clone(),
        popout_button: popout_button.clone(),
        export_button: export_button.clone(),
        search_entry: search_entry.clone(),
        search_prev: search_prev.clone(),
        search_next: search_next.clone(),
        search_count: search_count.clone(),
        link_back: link_back.clone(),
        hint: hint.clone(),
        picture: picture.clone(),
        scroll: scroll.clone(),
        continuous_box: continuous_box.clone(),
        continuous_scroll: continuous_scroll.clone(),
        view_stack: view_stack.clone(),
        notes_scroll: notes_scroll.clone(),
        notes_paned: notes_paned.clone(),
        paned: paned.clone(),
        pdf_hash: pdf_hash.to_string(),
        title: title.to_string(),
        sidebar_box: sidebar_box.clone(),
        drag_preview: drag_preview.clone(),
        drag_live_rect: drag_live_rect.clone(),
        window: window.clone(),
    };
    keys::install_keys(&ui);
    text_view::install_text_view(&ui);
    link_nav::install_link_nav(&ui);
    sidebar_toggle::install_sidebar_toggle(&ui);
    notes_toggle::install_notes_toggle(&ui);
    drag_gesture::install_drag_gesture(&ui);
    context_click::install_context_click(&ui);
    page_click::install_page_click(&ui);
    prev_next::install_prev_next(&ui);
    next_page::install_next_page(&ui);
    zoom::install_zoom(&ui);
    view_modes::install_view_modes(&ui);
    page_entry::install_page_entry(&ui);
    mark_mode::install_mark_mode(&ui);
    note_button::install_note_button(&ui);
    bookmark::install_bookmark(&ui);
    page_number::install_page_number(&ui);
    popout::install_popout(&ui);
    export::install_export(&ui);
    search::install_search(&ui);
    scan::install_scan(&ui);
    session::install_session(&ui, start_page);
    // @wiring

    warn_if_no_text_layer(host, &reader, blob);
    crate::perf::mark_rss("pdf reader presented");
    reader_tab.present();
}
