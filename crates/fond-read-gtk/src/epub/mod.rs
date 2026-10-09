//! The EPUB reader: chapter navigation, whole-book search, and highlight capture injected
//! into a WebKit view.
//!
//! An EPUB has no fixed page grid, so positions are chapter index plus a scroll fraction
//! within it (`fond_annot::Progress`). Persistence goes through [`ReaderHost`].

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, Orientation};
use libadwaita as adw;
use webkit6::prelude::*;

use super::pdf::{update_bookmark_button, MARK_KIND_OPTIONS};
use crate::annotation_store::{AnnotationStore, Change};
use crate::RebuildCell;
use crate::{color_swatch, note_edit_widget, popover_button, popover_separator, ReaderHost};

mod chapter_load;
mod chapter_nav;
mod chrome;
mod clip_image;
mod footnotes;
mod highlights;
mod keys;
mod open_book;
mod page_label;
mod paged;
mod pages;
mod undo_redo;
mod web_view_setup;
use web_view_setup::*;
mod layout;
use layout::*;
mod header;
use header::*;
mod search_bar;
use search_bar::*;
mod notes;
mod search_wiring;
mod session;
mod toggles;
mod ui;
use ui::EpubUi;
mod search;
use chrome::*;
use highlights::*;
use search::*;

/// Live state of an open EPUB reader window: the chapter list, current position, and this
/// entry's annotation sidecar (loaded once at open and rewritten to disk on every highlight
/// added — the same "hold it in memory, don't re-read the library each time" approach the
/// PDF reader's `ReaderState` uses).
struct EpubReaderState {
    /// Where this EPUB's contents were extracted to (content-addressed by attachment hash,
    /// so a repeat "Read" reuses the extraction rather than re-unzipping every time).
    cache_dir: PathBuf,
    /// Chapter files in reading order, as paths relative to `cache_dir` (same values as
    /// `fond_doc::EpubBook::spine`).
    spine: Vec<String>,
    index: usize,
    store: Rc<AnnotationStore>,
    /// Whole-book plain-text search index, one entry per `spine` chapter — built lazily
    /// (see `epub_chapter_texts`) the first time whole-book search is used, from the
    /// chapters already sitting in `cache_dir` (no re-opening the EPUB zip needed). `None`
    /// until then; cheap to keep in memory afterward for the life of this reader window.
    chapter_texts: Option<Vec<String>>,
    /// Bookmarked chapters (`spine` index) — see the PDF reader's `ReaderState::bookmarks`
    /// for the same idea applied to pages. Loaded once at open, rewritten on every add/
    /// remove, same lifecycle as `annotations`.
    bookmarks: Vec<usize>,
    /// The book's printed pages, if it has any.
    pages: Vec<pages::PageBreak>,
    /// Annotations whose passage could not be found in this edition of the book.
    lost: Vec<String>,
    /// Rebuilds the Notes list when `lost` changes.
    on_lost_changed: Option<Rc<dyn Fn()>>,
    /// Turning pages rather than scrolling.
    paginated: bool,
}

/// A built-in EPUB reader: renders each chapter with WebKitGTK (`webkit6`), which — unlike
/// PDFium for the PDF reader — handles an XHTML+CSS chapter's layout, images, and text
/// selection natively, so this only has to handle chapter/TOC navigation plus highlighting.
/// `hash` is the attachment's content hash (`blake3:…`), used to pick a stable extraction
/// cache directory — the EPUB zip is extracted to disk once per unique file (not re-unzipped
/// on every "Read") so chapter-relative links to sibling images/CSS resolve the normal
/// browser way over `file://`, rather than needing a custom URI scheme handler.
///
/// Highlighting (M5-SPEC.md 5C) reuses the `WebView`'s own native text selection — the user
/// drags to select the ordinary browser way, then "Highlight" captures it via
/// `EPUB_CAPTURE_SELECTION_JS` and saves a `fond_annot::Annotation::drawn_epub` (chapter +
/// snippet + context; no page/quadpoints — there's no fixed page grid to hang those on) into
/// the same `annots/<key>.json` sidecar the PDF reader writes. Applying saved highlights back
/// onto the page is a live DOM search-and-wrap (`epub_apply_highlights`) run after every
/// chapter load, not a raster blend like the PDF reader's `blend_highlights` — there's no
/// bitmap to blend into here, WebKit owns the actual rendering.
///
/// `start_annotation_id`, if given, opens on that annotation's chapter and scrolls it into
/// view once highlights are applied — the EPUB equivalent of the PDF reader's `start_page`.
/// `start_progress`, if given (and `start_annotation_id` isn't — a specific annotation jump
/// always wins), resumes at the entry's saved reading position: `progress.page - 1` as the
/// starting chapter index, then `progress.chapter_percent` (if set) as a scroll-fraction
/// restore once that chapter finishes loading. The EPUB equivalent of the PDF reader's own
/// `start_page` resume, using the chapter+percent shape `fond_annot::Progress` gained for it.
#[allow(clippy::too_many_arguments)]
pub fn show_epub_reader(
    host: &Rc<dyn ReaderHost>,
    window: &adw::ApplicationWindow,
    hash: &str,
    blob: &std::path::Path,
    title: &str,
    start_annotation_id: Option<&str>,
    start_progress: Option<fond_annot::Progress>,
) {
    // Already open? Surface it instead of opening a duplicate reader on the same file — see
    // the identical check (and `crate::OPEN_READERS`'s doc comment) in `show_pdf_reader`.
    if crate::present_existing(hash) {
        return;
    }

    let Some((book, cache_dir)) = open_book::open_book_cached(blob, hash, window) else {
        return;
    };

    let store = AnnotationStore::new(host, None);

    let start_index = start_annotation_id
        .and_then(|id| store.get(id))
        .and_then(|a| a.chapter)
        .and_then(|chapter| book.spine.iter().position(|p| *p == chapter))
        .or_else(|| {
            start_progress.and_then(|p| {
                let idx = (p.page as usize).saturating_sub(1);
                (idx < book.spine.len()).then_some(idx)
            })
        })
        .unwrap_or(0);
    // Only restore the scroll-within-chapter fraction when it's actually this chapter we're
    // opening on — a stale percent from a since-shrunk book, or a jump that landed on a
    // different chapter than `start_progress` recorded, would scroll to the wrong spot.
    let start_percent = start_annotation_id
        .is_none()
        .then(|| start_progress.filter(|p| (p.page as usize).saturating_sub(1) == start_index))
        .flatten()
        .and_then(|p| p.chapter_percent);

    let mut bookmarks: Vec<usize> = host
        .load_bookmarks()
        .into_iter()
        .map(|p| p as usize)
        .collect();
    bookmarks.sort_unstable();

    let printed_pages = pages::read(&cache_dir, &book.spine);
    let reader = Rc::new(RefCell::new(EpubReaderState {
        cache_dir,
        pages: printed_pages,
        lost: Vec::new(),
        on_lost_changed: None,
        paginated: host.epub_paginated(),
        spine: book.spine,
        index: start_index,
        store,
        chapter_texts: None,
        bookmarks,
    }));

    let view = adw::ToolbarView::new();
    // No header of this tab's own any more — its controls are handed to the shared host
    // header via `reader_host::set_tab_header` below instead, matching the PDF reader (see
    // its own `show_pdf_reader` for the fuller explanation), so there's one header row total
    // rather than the host's own plus a second, per-tab one underneath it.

    let prev = gtk4::Button::from_icon_name("go-previous-symbolic");
    prev.set_tooltip_text(Some("Previous chapter"));
    let next = gtk4::Button::from_icon_name("go-next-symbolic");
    next.set_tooltip_text(Some("Next chapter"));
    let chapter_label = gtk4::Label::new(None);
    chapter_label.add_css_class("dim-label");
    let page_label = gtk4::Label::new(None);
    page_label.add_css_class("heading");
    page_label.set_tooltip_text(Some("The printed page you are on"));
    page_label.set_visible(false);
    let bookmark_button = gtk4::Button::new();
    bookmark_button.add_css_class("flat");
    let nav = gtk4::Box::new(Orientation::Horizontal, 6);
    nav.append(&prev);
    nav.append(&page_label);
    nav.append(&chapter_label);
    nav.append(&next);
    nav.append(&bookmark_button);
    update_bookmark_button(
        &bookmark_button,
        reader.borrow().bookmarks.contains(&start_index),
    );

    let WebViewSetupParts { web_view } = web_view_setup::build_web_view_setup(&reader);

    let hint = gtk4::Label::new(Some("Select text, then choose a colour or action"));
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    hint.set_margin_top(4);
    hint.set_margin_bottom(4);

    let SearchBarParts {
        search_toggle,
        search_bar_entry,
        search_prev,
        search_next,
        whole_book_toggle,
        search_count,
        results_list,
        results_revealer,
        search_revealer,
    } = search_bar::build_search_bar();
    let content = gtk4::Box::new(Orientation::Vertical, 0);
    content.append(&hint);
    content.append(&search_revealer);
    content.append(&web_view);

    let has_toc = !book.toc.is_empty();
    let HeaderParts {
        sidebar_toggle,
        notes_toggle,
        undo_button,
        redo_button,
        mode_drop,
        palette_choice,
        apply_button,
        zoom_out_button,
        zoom_in_button,
        export_button,
        popout_button,
        header_start,
        header_end,
    } = header::build_header(has_toc, &search_toggle, &web_view);
    // Contents sidebar — always built, even for an EPUB with no TOC (an empty, unreachable
    // panel behind a disabled toggle, per the doc comment above).
    let contents_scroll = build_contents_sidebar(
        &book.toc,
        &reader,
        &web_view,
        &prev,
        &next,
        &chapter_label,
        &bookmark_button,
    );

    // Notes/highlights sidebar: every annotation on this EPUB, in reading order, readable
    // prose rather than just an in-text mark — same pattern as the PDF reader's own notes
    // sidebar (`show_pdf_reader`), rebuilt via the same self-referential-cell idiom so a
    // row's own delete button can trigger a fresh rebuild of the list it lives in.
    let notes_rows = gtk4::Box::new(Orientation::Vertical, 2);
    notes_rows.set_margin_top(6);
    notes_rows.set_margin_bottom(6);
    notes_rows.set_margin_start(6);
    notes_rows.set_margin_end(6);
    let notes_scroll = gtk4::ScrolledWindow::new();
    notes_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    notes_scroll.set_child(Some(&notes_rows));

    // Pending scroll target for the *next* chapter load — set right before calling
    // `epub_go_to` by anything that wants the freshly-loaded chapter to scroll to a
    // specific annotation (the initial `start_annotation_id`, or a notes-sidebar jump);
    // left `None` for plain prev/next/TOC navigation, which just lands at the top.
    let pending_scroll: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(start_annotation_id.map(|s| s.to_string())));

    // Reading-position resume: consumed by the very first `load-changed` finish (see below),
    // never set again afterward, so it can't fight a later prev/next/TOC/search jump.
    let pending_scroll_percent: Rc<RefCell<Option<u8>>> = Rc::new(RefCell::new(start_percent));

    // Whole-book search: set right before navigating to a result's chapter, so the
    // `load-changed` handler below can hand the query to WebKit's `FindController` once that
    // chapter has actually finished loading — highlighting/scrolling to the match the same
    // way in-chapter search already does, just arriving at the right chapter first.
    let pending_search: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let (rebuild_notes, quiet_notes) = notes::install_notes_sidebar(
        host,
        &reader,
        &web_view,
        &prev,
        &next,
        &chapter_label,
        &bookmark_button,
        &pending_scroll,
        &notes_rows,
        crate::notebook_ui::DocRef {
            hash: hash.to_string(),
            title: title.to_string(),
        },
    );
    reader.borrow_mut().on_lost_changed = Some(rebuild_notes.clone());
    {
        let store = reader.borrow().store.clone();
        let reader_weak = Rc::downgrade(&reader);
        let view = web_view.clone();
        let rebuild_notes = rebuild_notes.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        let quiet_notes = quiet_notes.clone();
        store.subscribe(move |store, change| {
            undo_button.set_sensitive(store.can_undo());
            redo_button.set_sensitive(store.can_redo());
            let Some(reader) = reader_weak.upgrade() else {
                return;
            };
            let scroll_to = match change {
                Change::Added(a) => Some(a.id.as_str()),
                _ => None,
            };
            epub_apply_highlights(&view, &reader, scroll_to);
            if !quiet_notes.get() {
                rebuild_notes();
            }
        });
    }
    {
        let host = host.clone();
        let reader = reader.clone();
        let rebuild_notes = rebuild_notes.clone();
        bookmark_button.connect_clicked(move |btn| {
            let chapter_index = reader.borrow().index;
            let now_bookmarked = {
                let mut r = reader.borrow_mut();
                if let Some(pos) = r.bookmarks.iter().position(|&c| c == chapter_index) {
                    r.bookmarks.remove(pos);
                    false
                } else {
                    r.bookmarks.push(chapter_index);
                    r.bookmarks.sort_unstable();
                    true
                }
            };
            let saved: Vec<u32> = reader
                .borrow()
                .bookmarks
                .iter()
                .map(|&c| c as u32)
                .collect();
            host.save_bookmarks(&saved);
            update_bookmark_button(btn, now_bookmarked);
            rebuild_notes();
        });
    }

    let paged = paged::build(
        host,
        &reader,
        &web_view,
        &prev,
        &next,
        &pending_scroll_percent,
    );
    let notebook_toggle = gtk4::ToggleButton::new();
    notebook_toggle.add_css_class("flat");
    crate::set_icon_with_fallback(
        &notebook_toggle,
        &["accessories-text-editor-symbolic", "document-edit-symbolic"],
    );
    notebook_toggle.set_tooltip_text(Some(
        "Notebook (N) — a page of your own writing beside the book; drag highlights from the \
         Notes list into it and they keep their source and page",
    ));
    let LayoutParts {
        notes_paned,
        paned,
        notebook_paned,
    } = layout::build_layout(
        &content,
        &contents_scroll,
        &notes_scroll,
        &view,
        &[
            paged.toggle.clone().upcast(),
            paged.spread.clone().upcast(),
            notebook_toggle.clone().upcast(),
        ],
    );
    crate::notebook_ui::bind_toggle(&notebook_toggle, &notebook_paned);
    let reader_tab = crate::reader_host::open_reader_tab(window, title, &view);
    let reader_window = reader_tab.host_window.clone();
    crate::register_reader(hash, &reader_tab);
    {
        let reader = reader.clone();
        let view = web_view.clone();
        let prev = prev.clone();
        let next = next.clone();
        let chapter_label = chapter_label.clone();
        let bookmark_button = bookmark_button.clone();
        let pending_scroll = pending_scroll.clone();
        crate::register_jump(
            hash,
            Rc::new(move |_page, id| {
                let Some(id) = id else { return };
                let chapter = reader.borrow().store.get(id).and_then(|a| a.chapter);
                let Some(chapter) = chapter else { return };
                *pending_scroll.borrow_mut() = Some(id.to_string());
                epub_go_to(
                    &reader,
                    &view,
                    &prev,
                    &next,
                    &chapter_label,
                    &bookmark_button,
                    &chapter,
                );
            }),
        );
    }
    crate::label_icon_buttons(&header_start);
    crate::label_icon_buttons(&header_end);
    crate::label_icon_buttons(&nav);
    crate::reader_host::set_tab_header(&reader_tab, header_start, nav, header_end);

    {
        let window = window.clone();
        let hash = hash.to_string();
        let reader_tab = reader_tab.clone();
        popout_button.connect_clicked(move |_| {
            let new_tab = reader_tab.pop_out(&window);
            crate::register_reader(&hash, &new_tab);
        });
    }
    {
        let host = host.clone();
        let reader = reader.clone();
        let title = title.to_string();
        let hash = hash.to_string();
        let dialog = reader_window.clone();
        export_button.connect_clicked(move |_| {
            export_notes(&host, &reader, &title, &hash, &dialog);
        });
    }

    let (epub_undo, epub_redo) =
        undo_redo::build_undo_redo(host, &reader, &undo_button, &redo_button);

    let ui = ui::EpubUi {
        reader: reader.clone(),
        web_view: web_view.clone(),
        prev: prev.clone(),
        next: next.clone(),
        chapter_label: chapter_label.clone(),
        bookmark_button: bookmark_button.clone(),
        contents_scroll: contents_scroll.clone(),
        notes_scroll: notes_scroll.clone(),
        notes_toggle: notes_toggle.clone(),
        notes_paned: notes_paned.clone(),
        paned: paned.clone(),
        sidebar_toggle: sidebar_toggle.clone(),
        pending_search: pending_search.clone(),
        reader_tab: reader_tab.clone(),
        rebuild_notes: rebuild_notes.clone(),
        results_list: results_list.clone(),
        results_revealer: results_revealer.clone(),
        search_bar_entry: search_bar_entry.clone(),
        search_count: search_count.clone(),
        search_next: search_next.clone(),
        search_prev: search_prev.clone(),
        search_revealer: search_revealer.clone(),
        search_toggle: search_toggle.clone(),
        whole_book_toggle: whole_book_toggle.clone(),
        zoom_in_button: zoom_in_button.clone(),
        zoom_out_button: zoom_out_button.clone(),
        epub_undo: epub_undo.clone(),
        epub_redo: epub_redo.clone(),
        page_turn: paged.turn.clone(),
        notebook_toggle: notebook_toggle.clone(),
    };
    search_wiring::install_search(&ui);
    clip_image::install(host, &reader, &web_view);
    {
        let reader = reader.clone();
        let view = web_view.clone();
        let prev = prev.clone();
        let next = next.clone();
        let chapter_label = chapter_label.clone();
        let bookmark_button = bookmark_button.clone();
        footnotes::install(
            &web_view,
            &reader.clone(),
            Rc::new(move |target| {
                epub_go_to(
                    &reader,
                    &view,
                    &prev,
                    &next,
                    &chapter_label,
                    &bookmark_button,
                    target,
                )
            }),
        );
    }
    page_label::install_page_label(&reader, &web_view, &page_label);
    keys::install_keys(&ui);
    toggles::install_sidebar_toggles(&ui);

    chapter_load::install_chapter_load(
        &reader,
        &web_view,
        &pending_scroll,
        &pending_scroll_percent,
        &pending_search,
    );

    chapter_nav::install_chapter_nav(
        &reader,
        &web_view,
        &prev,
        &next,
        &chapter_label,
        &bookmark_button,
        start_index,
    );

    let apply_mark = build_apply_mark(host, &reader, &web_view);
    {
        let apply_mark = apply_mark.clone();
        let mode_drop = mode_drop.clone();
        let palette_choice = palette_choice.clone();
        apply_button.connect_clicked(move |_| {
            let kind = MARK_KIND_OPTIONS
                .get(mode_drop.selected() as usize)
                .map(|(_, k)| *k)
                .unwrap_or(fond_annot::AnnotationKind::Highlight);
            let color = crate::palette::HIGHLIGHT_COLORS
                .get(palette_choice.get())
                .map(|c| c.hex.to_string());
            apply_mark(kind, color, None);
        });
    }

    install_selection_popover(host, &reader, &web_view, &apply_mark, &reader_window);

    session::install_progress_saving(host, hash, &reader, &web_view, &reader_tab, start_percent);

    reader_tab.present();
}
