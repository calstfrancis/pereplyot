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

mod chrome;
mod highlights;
mod keys;
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

    let book = match fond_doc::open_book(blob) {
        Ok(b) => b,
        Err(e) => {
            gtk4::AlertDialog::builder()
                .message("Could not open EPUB")
                .detail(e.to_string())
                .build()
                .show(Some(window));
            return;
        }
    };

    let hex = hash.split_once(':').map(|(_, h)| h).unwrap_or(hash);
    let cache_dir = glib::user_cache_dir()
        .join("pereplyot")
        .join("epub")
        .join(hex);
    if !cache_dir.join(".complete").exists() {
        let partial = cache_dir.with_extension("partial");
        let _ = std::fs::remove_dir_all(&partial);
        let _ = std::fs::remove_dir_all(&cache_dir);
        let extracted = std::fs::create_dir_all(&partial)
            .map_err(|e| e.to_string())
            .and_then(|_| fond_doc::extract_epub(blob, &partial).map_err(|e| e.to_string()))
            .and_then(|_| std::fs::write(partial.join(".complete"), b"").map_err(|e| e.to_string()))
            .and_then(|_| std::fs::rename(&partial, &cache_dir).map_err(|e| e.to_string()));
        if let Err(e) = extracted {
            let _ = std::fs::remove_dir_all(&partial);
            gtk4::AlertDialog::builder()
                .message("Could not open EPUB")
                .detail(e)
                .build()
                .show(Some(window));
            return;
        }
    }

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

    let reader = Rc::new(RefCell::new(EpubReaderState {
        cache_dir,
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
    let bookmark_button = gtk4::Button::new();
    bookmark_button.add_css_class("flat");
    let nav = gtk4::Box::new(Orientation::Horizontal, 6);
    nav.append(&prev);
    nav.append(&chapter_label);
    nav.append(&next);
    nav.append(&bookmark_button);
    update_bookmark_button(
        &bookmark_button,
        reader.borrow().bookmarks.contains(&start_index),
    );

    let web_view = webkit6::WebView::new();
    web_view.set_vexpand(true);
    web_view.set_hexpand(true);
    // Scale text size only, not the page layout/images — "zoom" on a WebView otherwise
    // scales everything, which reads as zooming a picture rather than adjusting font size.
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&web_view) {
        settings.set_zoom_text_only(true);
        // Scripts inside the book never run; the reader's own highlight scripts still do
        // (they go through `evaluate_javascript`, not page markup).
        settings.set_enable_javascript_markup(false);
        settings.set_allow_file_access_from_file_urls(false);
        settings.set_allow_universal_access_from_file_urls(false);
    }
    {
        let cache_root = reader.borrow().cache_dir.clone();
        web_view.connect_decide_policy(move |_view, decision, kind| {
            use webkit6::PolicyDecisionType;
            if kind != PolicyDecisionType::NavigationAction {
                return false;
            }
            let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
                return false;
            };
            let uri = nav
                .navigation_action()
                .and_then(|a| a.request())
                .and_then(|r| r.uri())
                .map(|u| u.to_string())
                .unwrap_or_default();
            let local = gio::File::for_uri(&uri)
                .path()
                .is_some_and(|p| p.starts_with(&cache_root));
            if local {
                return false;
            }
            if uri.starts_with("http://")
                || uri.starts_with("https://")
                || uri.starts_with("mailto:")
            {
                gtk4::UriLauncher::new(&uri).launch(
                    gtk4::Window::NONE,
                    gio::Cancellable::NONE,
                    |_| {},
                );
            }
            decision.ignore();
            true
        });
    }

    let hint = gtk4::Label::new(Some("Select text, then choose a colour or action"));
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    hint.set_margin_top(4);
    hint.set_margin_bottom(4);

    // Search: WebKit's own `FindController` for in-chapter search (highlights/cycles matches
    // in the currently loaded chapter, same as Ctrl+F in a browser) — plus a whole-book mode
    // (`whole_book_toggle`) that searches every chapter's plain-text index
    // (`epub_search_whole_book`) and lists results to jump to, since `FindController` itself
    // only ever sees the one chapter that's actually loaded.
    let search_toggle = gtk4::ToggleButton::new();
    search_toggle.set_icon_name("edit-find-symbolic");
    search_toggle.set_tooltip_text(Some("Search (Ctrl+F)"));
    let search_bar_entry = gtk4::SearchEntry::new();
    search_bar_entry.set_placeholder_text(Some("Search this chapter"));
    search_bar_entry.set_hexpand(true);
    let search_prev = gtk4::Button::from_icon_name("go-up-symbolic");
    search_prev.set_tooltip_text(Some("Previous match"));
    let search_next = gtk4::Button::from_icon_name("go-down-symbolic");
    search_next.set_tooltip_text(Some("Next match"));
    let whole_book_toggle = gtk4::ToggleButton::with_label("Whole book");
    whole_book_toggle.add_css_class("flat");
    whole_book_toggle.set_tooltip_text(Some(
        "Search every chapter instead of just the one currently open",
    ));
    let search_count = gtk4::Label::new(None);
    search_count.add_css_class("dim-label");
    search_count.add_css_class("caption");
    let search_row = gtk4::Box::new(Orientation::Horizontal, 6);
    search_row.set_margin_top(6);
    search_row.set_margin_start(8);
    search_row.set_margin_end(8);
    search_row.append(&search_bar_entry);
    search_row.append(&search_count);
    search_row.append(&search_prev);
    search_row.append(&search_next);
    search_row.append(&whole_book_toggle);

    // Whole-book results: chapter + excerpt, the match bolded via Pango markup. Only shown
    // (and only populated) while `whole_book_toggle` is active.
    let results_list = gtk4::ListBox::new();
    results_list.set_selection_mode(gtk4::SelectionMode::None);
    let results_scroll = gtk4::ScrolledWindow::new();
    results_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    results_scroll.set_min_content_height(160);
    results_scroll.set_max_content_height(240);
    results_scroll.set_propagate_natural_height(true);
    results_scroll.set_child(Some(&results_list));
    let results_revealer = gtk4::Revealer::new();
    results_revealer.set_reveal_child(false);
    results_revealer.set_child(Some(&results_scroll));

    let search_container = gtk4::Box::new(Orientation::Vertical, 0);
    search_container.append(&search_row);
    search_container.append(&results_revealer);
    let search_revealer = gtk4::Revealer::new();
    search_revealer.set_reveal_child(false);
    search_revealer.set_child(Some(&search_container));

    let content = gtk4::Box::new(Orientation::Vertical, 0);
    content.append(&hint);
    content.append(&search_revealer);
    content.append(&web_view);

    // Sidebar toggles (Contents; Notes) — persistent Paned sidebar, not popovers, matching
    // the PDF reader's own house sidebar style (see `show_pdf_reader`'s
    // `sidebar_toggle`/`notes_toggle` pair, and CLAUDE.md's UI standard). `Apply`/mode/
    // colour stay at the end of the header, same relative position "Highlight" used to
    // occupy. Contents is always shown, even when the EPUB has no TOC — disabled with an
    // explanatory tooltip rather than omitted entirely, matching the PDF reader's own fix
    // for the same discoverability problem (a permanently-hidden button was mistaken for a
    // removed one).
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
    if book.toc.is_empty() {
        sidebar_toggle.set_sensitive(false);
        sidebar_toggle.set_tooltip_text(Some("This EPUB has no table of contents"));
    } else {
        sidebar_toggle.set_tooltip_text(Some("Show the table of contents"));
    }
    let notes_toggle = gtk4::ToggleButton::new();
    notes_toggle.set_icon_name("view-list-symbolic");
    notes_toggle.set_tooltip_text(Some("Show notes and highlights"));

    // Undo/redo: same snapshot-based idiom as the PDF reader (see `EpubReaderState`'s
    // `undo_stack`/`redo_stack` and `push_epub_undo_snapshot`).
    let undo_button = gtk4::Button::from_icon_name("edit-undo-symbolic");
    undo_button.set_tooltip_text(Some("Undo (Ctrl+Z)"));
    undo_button.set_sensitive(false);
    let redo_button = gtk4::Button::from_icon_name("edit-redo-symbolic");
    redo_button.set_tooltip_text(Some("Redo (Ctrl+Shift+Z)"));
    redo_button.set_sensitive(false);

    let mode_labels: Vec<&str> = MARK_KIND_OPTIONS.iter().map(|(l, _)| *l).collect();
    let mode_drop = gtk4::DropDown::from_strings(&mode_labels);
    mode_drop.set_tooltip_text(Some("What kind of mark to apply to the selection"));
    let palette_choice: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let palette = {
        let palette_choice = palette_choice.clone();
        crate::palette::palette_widget(false, Some(0), move |choice| {
            palette_choice.set(choice.unwrap_or(0));
        })
    };
    let apply_button = gtk4::Button::with_label("Apply");
    apply_button.set_tooltip_text(Some("Mark the selected text"));

    // Font size: text-only zoom (see `set_zoom_text_only` above), stepped like the PDF
    // reader's own zoom buttons. Not persisted across sessions (unlike window/pane sizing)
    // — a book-length reading choice you're more likely to want to readjust per-book than
    // to lock in globally.
    let font_zoom: Rc<Cell<f64>> = Rc::new(Cell::new(1.0));
    let zoom_out_button = gtk4::Button::from_icon_name("zoom-out-symbolic");
    zoom_out_button.add_css_class("flat");
    zoom_out_button.set_tooltip_text(Some("Smaller text"));
    let zoom_in_button = gtk4::Button::from_icon_name("zoom-in-symbolic");
    zoom_in_button.add_css_class("flat");
    zoom_in_button.set_tooltip_text(Some("Larger text"));
    {
        let web_view = web_view.clone();
        let font_zoom = font_zoom.clone();
        zoom_out_button.connect_clicked(move |_| {
            let z = (font_zoom.get() - 0.1).max(0.5);
            font_zoom.set(z);
            web_view.set_zoom_level(z);
        });
    }
    {
        let web_view = web_view.clone();
        let font_zoom = font_zoom.clone();
        zoom_in_button.connect_clicked(move |_| {
            let z = (font_zoom.get() + 0.1).min(3.0);
            font_zoom.set(z);
            web_view.set_zoom_level(z);
        });
    }

    // Reading theme and font family — independent of the app's own System/Light/Dark
    // toggle, since a WebView's page content doesn't inherit `adw::StyleManager` and people
    // have real preferences about reading typography that don't always track their OS theme
    // (paper-toned "sepia" being the obvious example neither Light nor Dark covers). Applied
    // via a `WebKitUserStyleSheet` at `UserStyleLevel::User` — the highest-priority stylesheet
    // WebKit has, so it overrides the EPUB's own CSS without needing `!important` fragility —
    // registered once on the view's `UserContentManager` and left in place across chapter
    // navigation, rather than re-injected via JS on every `load-changed`.
    let theme_labels = ["Light", "Sepia", "Dark"];
    let theme_drop = gtk4::DropDown::from_strings(&theme_labels);
    theme_drop.set_tooltip_text(Some("Reading theme"));
    let font_labels = ["Default font", "Serif", "Sans-serif"];
    let font_drop = gtk4::DropDown::from_strings(&font_labels);
    font_drop.set_tooltip_text(Some("Font family"));
    {
        let apply_style: Rc<dyn Fn()> = {
            let web_view = web_view.clone();
            let theme_drop = theme_drop.clone();
            let font_drop = font_drop.clone();
            Rc::new(move || {
                apply_epub_style(&web_view, theme_drop.selected(), font_drop.selected())
            })
        };
        {
            let apply_style = apply_style.clone();
            theme_drop.connect_selected_notify(move |_| apply_style());
        }
        {
            let apply_style = apply_style.clone();
            font_drop.connect_selected_notify(move |_| apply_style());
        }
        apply_style();
    }

    let export_button = gtk4::Button::from_icon_name("document-save-symbolic");
    export_button.add_css_class("flat");
    export_button.set_tooltip_text(Some("Export notes & highlights…"));

    // Moves this tab out of the shared "Reader" window into its own standalone one — the
    // only way to detach a tab (see `reader_host`'s module doc for why there's no drag-out-
    // of-the-bar gesture too).
    let popout_button = gtk4::Button::from_icon_name("window-new-symbolic");
    popout_button.add_css_class("flat");
    popout_button.set_tooltip_text(Some("Open in a new window"));

    // Visual order, left to right, in the shared host header: Contents, Search, Undo, Redo
    // (start) … chapter nav (centre) … reading theme, font, Export, font-size, colour
    // palette, Mode, Apply, Open in new window, Notes (end) — unchanged from when these lived in
    // this tab's own `HeaderBar`, just built as plain boxes now and handed to
    // `reader_host::set_tab_header` below instead of packed directly (see the comment above
    // `let prev` for why).
    let header_start = gtk4::Box::new(Orientation::Horizontal, 6);
    header_start.append(&sidebar_toggle);
    header_start.append(&search_toggle);
    header_start.append(&undo_button);
    header_start.append(&redo_button);
    let header_end = gtk4::Box::new(Orientation::Horizontal, 6);
    header_end.append(&theme_drop);
    header_end.append(&font_drop);
    header_end.append(&export_button);
    header_end.append(&zoom_out_button);
    header_end.append(&zoom_in_button);
    header_end.append(&palette);
    header_end.append(&mode_drop);
    header_end.append(&apply_button);
    header_end.append(&popout_button);
    header_end.append(&notes_toggle);

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
    );
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

    // Contents (left) and Notes (right) are now two independent sidebars rather than a
    // shared Stack behind one toggle slot — see `show_pdf_reader`'s matching sidebars for
    // the full rationale (both can be open at once instead of sharing one toggle slot).
    contents_scroll.set_size_request(60, -1);
    notes_scroll.set_size_request(60, -1);

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

    // Status bar: just the reader-host footer (if the embedding app registered one, e.g.
    // Pereplyot's version/changelog button) — this reader otherwise has nothing that
    // belongs at the bottom of the window, unlike the PDF reader's page nav/zoom. Per-tab
    // rather than a second bottom bar added by the host window itself, matching the PDF
    // reader (see its own `show_pdf_reader` for the fuller reasoning).
    if let Some(footer_widget) = crate::reader_host::host_footer_widget() {
        let statusbar = gtk4::Box::new(Orientation::Horizontal, 6);
        statusbar.add_css_class("toolbar");
        statusbar.add_css_class("fond-chrome");
        statusbar.add_css_class("fond-statusbar");
        let spacer = gtk4::Box::new(Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        statusbar.append(&spacer);
        statusbar.append(&footer_widget);
        view.add_bottom_bar(&statusbar);
    }

    let reader_tab = crate::reader_host::open_reader_tab(window, title, &view);
    let reader_window = reader_tab.host_window.clone();
    crate::register_reader(hash, &reader_tab);
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
        let dialog = reader_window.clone();
        export_button.connect_clicked(move |_| {
            export_notes(&host, &reader, &title, &dialog);
        });
    }

    let epub_undo: Rc<dyn Fn()> = {
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
    let epub_redo: Rc<dyn Fn()> = {
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
        let epub_undo = epub_undo.clone();
        undo_button.connect_clicked(move |_| epub_undo());
    }
    {
        let epub_redo = epub_redo.clone();
        redo_button.connect_clicked(move |_| epub_redo());
    }

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
    };
    search_wiring::install_search(&ui);
    keys::install_keys(&ui);
    toggles::install_sidebar_toggles(&ui);

    // Re-apply saved highlights after every chapter load (initial load, TOC jump,
    // prev/next, a notes-sidebar jump — all funnel through `epub_go_to`'s `load_uri`, so
    // one handler here covers all of them), consuming `pending_scroll` if the navigation
    // that triggered this load set one. Also consumes `pending_scroll_percent` (reading-
    // position resume, first load only) and `pending_search` (a whole-book search result's
    // chapter jump, handed to WebKit's own `FindController` once the page is actually there).
    {
        let reader = reader.clone();
        let pending_scroll = pending_scroll.clone();
        let pending_scroll_percent = pending_scroll_percent.clone();
        let pending_search = pending_search.clone();
        web_view.connect_load_changed(move |view, event| {
            if event == webkit6::LoadEvent::Finished {
                let scroll_to = pending_scroll.borrow_mut().take();
                epub_apply_highlights(view, &reader, scroll_to.as_deref());
                if let Some(percent) = pending_scroll_percent.borrow_mut().take() {
                    let script = format!(
                        "window.scrollTo(0, Math.round((document.documentElement.scrollHeight - document.documentElement.clientHeight) * {}));",
                        (percent.min(100) as f64) / 100.0
                    );
                    view.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});
                }
                if let Some(query) = pending_search.borrow_mut().take() {
                    if let Some(fc) = webkit6::prelude::WebViewExt::find_controller(view) {
                        let options = (webkit6::FindOptions::CASE_INSENSITIVE
                            | webkit6::FindOptions::WRAP_AROUND)
                            .bits();
                        fc.search(&query, options, 1000);
                    }
                }
            }
        });
    }

    // Load the first chapter up front (the TOC/prev/next/notes-jump handlers all reuse
    // this same navigation path for consistency, but chapter 0 has to start somewhere).
    let first_chapter = reader.borrow().spine.get(start_index).cloned();
    if let Some(first) = first_chapter {
        epub_go_to(
            &reader,
            &web_view,
            &prev,
            &next,
            &chapter_label,
            &bookmark_button,
            &first,
        );
    }

    {
        let reader = reader.clone();
        let view = web_view.clone();
        let prev_for_handler = prev.clone();
        let next = next.clone();
        let chapter_label = chapter_label.clone();
        let bookmark_button = bookmark_button.clone();
        prev.connect_clicked(move |_| {
            let target = {
                let r = reader.borrow();
                (r.index > 0).then(|| r.spine[r.index - 1].clone())
            };
            if let Some(target) = target {
                epub_go_to(
                    &reader,
                    &view,
                    &prev_for_handler,
                    &next,
                    &chapter_label,
                    &bookmark_button,
                    &target,
                );
            }
        });
    }
    {
        let reader = reader.clone();
        let view = web_view.clone();
        let prev = prev.clone();
        let next_for_handler = next.clone();
        let chapter_label = chapter_label.clone();
        let bookmark_button = bookmark_button.clone();
        next.connect_clicked(move |_| {
            let target = {
                let r = reader.borrow();
                (r.index + 1 < r.spine.len()).then(|| r.spine[r.index + 1].clone())
            };
            if let Some(target) = target {
                epub_go_to(
                    &reader,
                    &view,
                    &prev,
                    &next_for_handler,
                    &chapter_label,
                    &bookmark_button,
                    &target,
                );
            }
        });
    }

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
