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
mod search;
mod session;
mod sidebar_toggle;
mod text_view;
mod ui;
mod view_modes;
mod zoom;
use ui::PdfUi;
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
    bytes: Vec<u8>,
    /// The document, parsed once and kept open for rendering and page geometry; every
    /// `fond_doc` helper that takes `bytes` re-parses the whole file on each call.
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
    /// While the Text view is showing: where "go to page" navigation is redirected, and how
    /// zoom steps are applied (to the text size instead of the page render).
    text_goto: Option<Rc<dyn Fn(u16)>>,
    text_zoom: Option<Rc<dyn Fn(f64)>>,
    /// Inclusive range of pages currently kept rendered in continuous mode; everything else
    /// is unloaded so a long book at high zoom doesn't hold every page as a texture.
    continuous_window: (u16, u16),
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

    let store = AnnotationStore::new(host, Some(pdf_hash.to_string()));
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
        store,
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
        text_goto: None,
        text_zoom: None,
        page_labels,
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
    let text_toggle = gtk4::ToggleButton::new();
    crate::set_icon_with_fallback(
        &text_toggle,
        &[
            "format-justify-left-symbolic",
            "view-reader-symbolic",
            "text-x-generic-symbolic",
        ],
    );
    text_toggle.set_tooltip_text(Some(
        "Text view — the document's text reflowed: readable by screen readers, resizable, \
         and selectable with the keyboard (Shift+arrows, then 1–4 to mark)",
    ));
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
    header_end.append(&text_toggle);
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
    picture.set_focusable(true);
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
    let reflow_popover: RebuildNotesCell = Rc::new(RefCell::new(None));
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
    let render: Rc<dyn Fn()> = {
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

    // Undo/redo and every other annotation change redraw through the store's change
    // notification; a change doesn't say which page(s) it touched, so redraw everything —
    // cheap even in continuous mode, since each page's blend is just a texture re-render.
    let redraw_for = {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        Rc::new(move |change: &crate::annotation_store::Change| {
            if !continuous_toggle.is_active() {
                render();
            }
            for page in change.pages() {
                render_continuous_page(&reader, page.saturating_sub(1) as u16);
            }
        })
    };
    {
        let store = reader.borrow().store.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        store.subscribe(move |store, change| {
            undo_button.set_sensitive(store.can_undo());
            redo_button.set_sensitive(store.can_redo());
            redraw_for(change);
        });
    }
    let undo = {
        let reader = reader.clone();
        let host = host.clone();
        Rc::new(move || {
            let store = reader.borrow().store.clone();
            match store.undo() {
                Ok(true) => host.notify("Undid last annotation change"),
                Ok(false) => host.notify("Nothing to undo"),
                Err(e) => host.notify(&format!("Could not undo: {e}")),
            }
        })
    };
    let redo = {
        let reader = reader.clone();
        let host = host.clone();
        Rc::new(move || {
            let store = reader.borrow().store.clone();
            match store.redo() {
                Ok(true) => host.notify("Redid annotation change"),
                Ok(false) => host.notify("Nothing to redo"),
                Err(e) => host.notify(&format!("Could not redo: {e}")),
            }
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
    session::install_session(&ui, start_page);
    // @wiring

    warn_if_no_text_layer(host, &reader, blob);
    reader_tab.present();
}
