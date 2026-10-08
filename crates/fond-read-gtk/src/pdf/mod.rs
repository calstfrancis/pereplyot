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

mod context_menu;
mod continuous;
mod dialogs;
mod drag_preview;
mod links;
mod nav;
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
        let reflow_popover_for_keys = reflow_popover.clone();
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
            let in_reflow = view_for_focus
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|w| w.widget_name() == "fond-reflow");
            if in_reflow {
                if modifiers.is_empty() {
                    let index = match keyval {
                        gdk::Key::_1 => Some(0),
                        gdk::Key::_2 => Some(1),
                        gdk::Key::_3 => Some(2),
                        gdk::Key::_4 => Some(3),
                        _ => None,
                    };
                    if let Some(i) = index {
                        if let Some(mark) = quick_mark_for_keys.borrow().as_ref() {
                            mark(i);
                        }
                        return glib::Propagation::Stop;
                    }
                }
                if matches!(
                    keyval,
                    gdk::Key::Menu | gdk::Key::Return | gdk::Key::KP_Enter
                ) {
                    if let Some(open) = reflow_popover_for_keys.borrow().as_ref() {
                        open();
                        return glib::Propagation::Stop;
                    }
                }
                return glib::Propagation::Proceed;
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
    let quiet_notes = Rc::new(Cell::new(false));
    {
        let notes_rows = notes_rows.clone();
        let host = host.clone();
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let bookmark_button = bookmark_button.clone();
        let rebuild_notes_cell_inner = rebuild_notes_cell.clone();
        let quiet_notes = quiet_notes.clone();
        let builder = move || {
            while let Some(child) = notes_rows.first_child() {
                notes_rows.remove(&child);
            }
            let bookmarks = reader.borrow().bookmarks.clone();
            let mut all: Vec<fond_annot::Annotation> = reader
                .borrow()
                .store
                .sidecar()
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
                    let quiet_notes = quiet_notes.clone();
                    let id = annotation.id.clone();
                    move |text: &str| {
                        let text = text.trim();
                        let store = reader.borrow().store.clone();
                        quiet_notes.set(true);
                        let result = store.update(&id, |a| {
                            a.note = (!text.is_empty()).then(|| text.to_string());
                        });
                        quiet_notes.set(false);
                        if let Err(e) = result {
                            host.notify(&e);
                        }
                    }
                };
                let note_widget =
                    note_edit_widget(annotation.note.as_deref(), move |text| save_note(&text));
                outer.append(&note_widget);

                {
                    let host = host.clone();
                    let reader = reader.clone();
                    let id = annotation.id.clone();
                    delete_button.connect_clicked(move |_| {
                        let store = reader.borrow().store.clone();
                        match store.remove(&id) {
                            Ok(_) => host.notify("Annotation deleted"),
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

    // Text view: the document's text reflowed into a text widget (see `pdf_text`).
    {
        let reflow: Rc<RefCell<Option<Rc<crate::pdf_text::ReflowView>>>> =
            Rc::new(RefCell::new(None));
        let host = host.clone();
        let reader = reader.clone();
        let view_stack = view_stack.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let reader_window = reader_window.clone();
        let hint = hint.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        let reflow_popover = reflow_popover.clone();
        text_toggle.connect_toggled(move |btn| {
            if !btn.is_active() {
                {
                    let mut r = reader.borrow_mut();
                    r.text_goto = None;
                    r.text_zoom = None;
                }
                view_stack.set_visible_child_name("continuous");
                let page = reader.borrow().page;
                let reader = reader.clone();
                let continuous_scroll = continuous_scroll.clone();
                glib::idle_add_local_once(move || {
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                });
                hint.set_text("Drag over text to select it, then choose a colour (or press 1–4)");
                return;
            }
            // Page navigation everywhere funnels through the continuous view's scroller, so it
            // stays the mode underneath and the text view takes over its showing.
            if !continuous_toggle.is_active() {
                continuous_toggle.set_active(true);
            }
            let (count, page_labels) = {
                let r = reader.borrow();
                (r.count, r.page_labels.clone())
            };
            let existing = reflow.borrow().clone();
            let view = match existing {
                Some(v) => v,
                None => {
                    let view = crate::pdf_text::ReflowView::new(count);
                    view_stack.add_named(&view.scroll, Some("text"));
                    *reflow.borrow_mut() = Some(view.clone());

                    let make_ctx: Rc<dyn Fn() -> MarkCtx> = {
                        let host = host.clone();
                        let reader = reader.clone();
                        let reader_window = reader_window.clone();
                        Rc::new(move || MarkCtx {
                            host: host.clone(),
                            reader: reader.clone(),
                            reader_window: reader_window.clone(),
                        })
                    };

                    // Follow the user's scrolling so the page counter, bookmarks and
                    // sidebars stay in step.
                    {
                        let reader = reader.clone();
                        let view_for_scroll = view.clone();
                        let page_entry = page_entry.clone();
                        let page_of_label = page_of_label.clone();
                        let prev = prev.clone();
                        let next = next.clone();
                        let bookmark_button = bookmark_button.clone();
                        view.scroll.vadjustment().connect_value_changed(move |_| {
                            if view_for_scroll.loaded_pages() == 0 {
                                return;
                            }
                            let page = view_for_scroll.visible_page();
                            let mut r = reader.borrow_mut();
                            if r.page != page && r.text_goto.is_some() {
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
                            }
                        });
                    }
                    // A selection in the text becomes the thing the 1–4 keys and the popover act
                    // on; refreshed on every cursor move so extending it with Shift+arrows counts.
                    {
                        let reader = reader.clone();
                        let view_for_sel = view.clone();
                        let sync: Rc<dyn Fn()> = Rc::new(move || {
                            if reader.borrow().text_goto.is_none() {
                                return;
                            }
                            let selection = view_for_sel.selection();
                            reader.borrow_mut().last_selection =
                                selection.map(|(page, text)| (page, text, Vec::new()));
                        });
                        let buffer = view.text_view.buffer();
                        {
                            let sync = sync.clone();
                            buffer.connect_has_selection_notify(move |_| sync());
                        }
                        buffer.connect_cursor_position_notify(move |_| sync());
                    }
                    // Mouse: popover when a drag-selection ends. Keyboard: Menu or Enter.
                    {
                        let pointer: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((0.0, 0.0)));
                        let motion = gtk4::EventControllerMotion::new();
                        motion.set_propagation_phase(gtk4::PropagationPhase::Capture);
                        {
                            let pointer = pointer.clone();
                            motion.connect_motion(move |_, x, y| pointer.set((x, y)));
                        }
                        view.text_view.add_controller(motion);

                        let drag = gtk4::GestureDrag::new();
                        drag.set_button(gdk::BUTTON_PRIMARY);
                        drag.set_propagation_phase(gtk4::PropagationPhase::Capture);
                        let make_ctx_drag = make_ctx.clone();
                        let view_drag = view.clone();
                        drag.connect_drag_end(move |_, _, _| {
                            let (x, y) = pointer.get();
                            let make_ctx = make_ctx_drag.clone();
                            let view = view_drag.clone();
                            glib::timeout_add_local_once(
                                std::time::Duration::from_millis(60),
                                move || {
                                    if let Some((page, _)) = view.selection() {
                                        show_selection_popover(
                                            &make_ctx(),
                                            &view.text_view,
                                            x,
                                            y,
                                            page,
                                        );
                                    }
                                },
                            );
                        });
                        view.text_view.add_controller(drag);

                        let make_ctx_key = make_ctx.clone();
                        let view_key = view.clone();
                        *reflow_popover.borrow_mut() = Some(Rc::new(move || {
                            if let Some((page, _)) = view_key.selection() {
                                let (x, y) = view_key.selection_anchor();
                                show_selection_popover(
                                    &make_ctx_key(),
                                    &view_key.text_view,
                                    x as f64,
                                    y as f64,
                                    page,
                                );
                            }
                        }));
                    }
                    view
                }
            };

            // Zoom steps resize the text instead of re-rendering pages; navigation redirects here.
            {
                let view_zoom = view.clone();
                let view_goto = view.clone();
                let reader_for_goto = reader.clone();
                let page_entry = page_entry.clone();
                let page_of_label = page_of_label.clone();
                let prev = prev.clone();
                let next = next.clone();
                let bookmark_button = bookmark_button.clone();
                let mut r = reader.borrow_mut();
                r.text_zoom = Some(Rc::new(move |factor| view_zoom.scale_font(factor)));
                r.text_goto = Some(Rc::new(move |page| {
                    view_goto.scroll_to_page(page);
                    let mut r = reader_for_goto.borrow_mut();
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
                }));
            }
            hint.set_text(
                "Text view: Shift+arrows select, then 1–4 mark it; Ctrl+plus/minus resize the text",
            );

            let reader_for_text = reader.clone();
            let page_labels_text = page_labels.clone();
            view.start_loading(
                Rc::new({
                    let reader = reader_for_text.clone();
                    move |page| {
                        let r = reader.borrow();
                        r.doc
                            .as_ref()
                            .and_then(|d| {
                                let page = d.pages().get(page).ok()?;
                                Some(crate::page_text(&page))
                            })
                            .unwrap_or_default()
                    }
                }),
                Rc::new(move |page| {
                    page_labels_text
                        .get(page as usize)
                        .and_then(|l| l.clone())
                        .unwrap_or_else(|| (page + 1).to_string())
                }),
                Rc::new({
                    let view = view.clone();
                    let reader = reader.clone();
                    move |loaded| {
                        if loaded == count {
                            paint_text_marks(&reader, &view);
                        }
                    }
                }),
            );
            view_stack.set_visible_child_name("text");
            let start_page = reader.borrow().page;
            let view_jump = view.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
                if view_jump.loaded_pages() > start_page {
                    view_jump.scroll_to_page(start_page);
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
            view.text_view.grab_focus();
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
                            reader_window: reader_window_for_drag.clone(),
                        };
                        show_selection_popover(&ctx, &picture_for_popover, end_x, end_y, page);
                    }
                    return;
                }
                save_drag_annotation(&host, &reader, page, geom);
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
        let picture_for_menu = picture.clone();
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
            show_pdf_context_menu(
                &host,
                &reader,
                &picture_for_menu,
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
        let continuous_box = continuous_box.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let dialog = reader_window.clone();
        let pending_zoom = pending_zoom.clone();
        let zoom_debounce = zoom_debounce.clone();
        Rc::new(move |target: f64| {
            let text_zoom = reader.borrow().text_zoom.clone();
            if let Some(text_zoom) = text_zoom {
                let base = reader.borrow().zoom.max(0.01);
                text_zoom(target / base);
                return;
            }
            pending_zoom.set(Some(target.clamp(0.35, 4.0)));
            if let Some(id) = zoom_debounce.borrow_mut().take() {
                id.remove();
            }
            let host = host.clone();
            let reader = reader.clone();
            let render = render.clone();
            let continuous_box = continuous_box.clone();
            let continuous_toggle = continuous_toggle.clone();
            let continuous_scroll = continuous_scroll.clone();
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
                        &continuous_box,
                        &continuous_scroll,
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
        let continuous_box = continuous_box.clone();
        let continuous_scroll = continuous_scroll.clone();
        let view_stack = view_stack.clone();
        let dialog = reader_window.clone();
        let two_page_toggle = two_page_toggle.clone();
        continuous_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                two_page_toggle.set_active(false);
                build_continuous_view(&host, &reader, &continuous_box, &continuous_scroll, &dialog);
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
                    .unwrap_or(fond_annot::AnnotationKind::Highlight)
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
                Some(fond_annot::AnnotationKind::Highlight) => "Drag over text to highlight it",
                Some(fond_annot::AnnotationKind::Underline) => "Drag over text to underline it",
                Some(fond_annot::AnnotationKind::Strikeout) => "Drag over text to strike it out",
                Some(_) => "Drag over the page",
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
        let dialog = reader_window.clone();
        note_button.connect_clicked(move |_| {
            show_pdf_note_dialog(&host, &reader, &dialog);
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
        let reader_tab = reader_tab.clone();
        let pdf_hash = pdf_hash.to_string();
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
                host.save_progress(fond_annot::Progress {
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
        let reader = reader.clone();
        let pdf_hash = pdf_hash.to_string();
        crate::reader_host::on_tab_closed(&reader_tab, move || {
            closed.set(true);
            let (page, count) = {
                let r = reader.borrow();
                (r.page as u32 + 1, r.count as u32)
            };
            host.save_progress(fond_annot::Progress {
                page,
                of: count,
                chapter_percent: None,
            });
            reader.borrow().store.clear_listeners();
            crate::unregister_window(&pdf_hash);
        });
    }

    warn_if_no_text_layer(host, &reader, blob);
    reader_tab.present();
}
