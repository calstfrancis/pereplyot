//! GTK4/libadwaita PDF and EPUB readers, shared across the Fond suite.
//!
//! Rendering, text extraction, search and the annotation schema come from `fond-doc` and
//! `fond-bib`; this crate is the widget layer over them — paged and continuous PDF views,
//! drag-to-annotate, EPUB chapter navigation, undo/redo, and the annotations dialog.
//!
//! The crate knows nothing about how the embedding application identifies a document.
//! Everything it needs comes through [`ReaderHost`]: Kartoteka's implementation resolves a
//! citation key against an open library, and Sputnik's routes a library reading and a local
//! course material to two different places behind the same methods. Nothing here may learn
//! what a citation key is — it is handed a blob path and a host, and that is all it knows.
//!
//! Extracted from Kartoteka's `app_window.rs`; see that repo's `docs/READER-EXTRACTION.md`
//! for the boundary survey this shape came from.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::Orientation;

pub mod annotations;
pub mod epub;
pub mod export;
pub mod fsutil;
pub mod history;
mod page_geom;
pub mod palette;
pub mod pdf;
pub mod reader_host;
pub mod store;

/// A slot holding a "rebuild this list" closure, filled in after the widgets it rebuilds
/// exist. Shared by both readers' notes sidebars.
pub type RebuildCell = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Everything the reader needs from whoever embedded it.
///
/// One instance per open document, constructed by the host. That is what lets the reader
/// stay ignorant of how a document is identified or where its annotations are stored:
/// Kartoteka's implementation closes over an open `Library` and a citation key, and
/// Sputnik's will route a library reading and a local course material to two different
/// places behind the same six methods.
pub trait ReaderHost {
    /// The document's annotation sidecar — an empty one if it has none yet, or if it
    /// could not be read. Infallible by design: the empty fallback has to be built by the
    /// host, because a sidecar is born knowing the key it will be written back to, and that
    /// key is exactly what the reader must not know. A reader that built its own fallback
    /// would save it to the wrong file.
    fn load_annotations(&self) -> fond_bib::AnnotationSidecar;

    /// Persist the sidecar. Called on every annotation add, edit, and delete — the reader
    /// holds the authoritative in-memory copy for the session and rewrites the whole
    /// sidecar each time.
    fn save_annotations(&self, sidecar: &fond_bib::AnnotationSidecar) -> Result<(), String>;

    /// Persist reading position, on reader close. Best-effort: a failure is not surfaced,
    /// since losing a resume position is not worth interrupting a close for.
    fn save_progress(&self, progress: fond_bib::Progress);

    /// The manual printed-page-numbering override, if the user has set one. Consulted only
    /// when the PDF declares no `/PageLabels` of its own, which the reader decides.
    fn page_label_override(&self) -> Option<fond_bib::PageLabelOverride>;

    /// Record (or, with `None`, clear) the manual page-numbering override.
    fn set_page_label_override(&self, value: Option<fond_bib::PageLabelOverride>);

    /// Show a transient confirmation or error, however the host shows those.
    fn notify(&self, message: &str);

    /// Bookmarked pages (PDF, 1-based `Annotation.page` numbering) or chapters (EPUB,
    /// 0-based spine index) — a lightweight "come back to this" marker, distinct from an
    /// annotation. Defaulted to a no-op pair rather than added as a required method: this
    /// trait has three implementors across three repos (Pereplyot, Kartoteka, Sputnik), and
    /// only Pereplyot has bookmarks wired up to real storage so far — the others simply get
    /// an always-empty list and a save that does nothing, which is exactly the pre-bookmarks
    /// behavior they already had, until each one opts in with its own persistence.
    fn load_bookmarks(&self) -> Vec<u32> {
        Vec::new()
    }

    /// Persist the full bookmark list. Called on every add/remove, same as
    /// [`Self::save_annotations`] — the reader holds the authoritative in-memory copy.
    fn save_bookmarks(&self, _pages: &[u32]) {}

    /// The bibliography key to cite this document by in exports — Kartoteka's citation key
    /// when launched from a vault, or whatever the user last entered for a standalone file.
    fn citation_key(&self) -> Option<String> {
        None
    }

    fn set_citation_key(&self, _key: Option<String>) {}

    /// Like [`Self::notify`], with one button. Hosts that can't show buttons just notify.
    fn notify_action(&self, message: &str, _label: &str, _action: Rc<dyn Fn()>) {
        self.notify(message);
    }

    /// Open another document (for instance the OCR'd copy of this one) in the host app.
    fn open_document(&self, _path: &std::path::Path) {}
}

thread_local! {
    /// Reader tabs currently open, keyed by the document's content hash — so this is really
    /// "the same file", not "the same entry".
    ///
    /// Opening a second reader on a document already open would give each one an
    /// independent in-memory sidecar snapshot, and both rewrite the whole sidecar on every
    /// save, so the last close would silently discard the other's annotations. Instead the
    /// second attempt surfaces the tab that is already open.
    ///
    /// Process-global rather than per-library: entries are removed only on a reader's own
    /// tab closing, never on a library switch, which matches the behaviour this replaced
    /// (`AppState.open_readers`). GTK is single-threaded, so a `thread_local` is the whole
    /// of the synchronisation story.
    static OPEN_READERS: RefCell<HashMap<String, reader_host::ReaderTab>> =
        RefCell::new(HashMap::new());

    /// Callbacks registered via [`on_all_readers_closed`].
    static ON_ALL_CLOSED: RefCell<Vec<Rc<dyn Fn()>>> = RefCell::new(Vec::new());
}

/// Register a callback to run whenever the last open reader tab/window closes (across the
/// shared default host and any popped-out ones). Meant for a host that has no persistent
/// window of its own to fall back to once nothing is being read — Pereplyot's launcher, for
/// one, which otherwise stays alive (invisibly, if it was never actually shown) purely
/// because the `ApplicationWindow` GTK tracks for auto-quit is the launcher, not the reader.
/// Kartoteka and Sputnik have no use for this — their own main window is always there
/// regardless of any reader closing.
pub fn on_all_readers_closed(cb: impl Fn() + 'static) {
    ON_ALL_CLOSED.with(|c| c.borrow_mut().push(Rc::new(cb)));
}

/// The reader tab already open on `hash`, if there is one.
pub fn existing_reader(hash: &str) -> Option<reader_host::ReaderTab> {
    OPEN_READERS.with(|r| r.borrow().get(hash).cloned())
}

pub fn any_reader_open() -> bool {
    OPEN_READERS.with(|r| !r.borrow().is_empty())
}

/// Surface the reader tab already open on `hash`, reporting whether there was one.
pub fn present_existing(hash: &str) -> bool {
    match existing_reader(hash) {
        Some(tab) => {
            tab.present();
            true
        }
        None => false,
    }
}

pub fn register_reader(hash: &str, tab: &reader_host::ReaderTab) {
    OPEN_READERS.with(|r| r.borrow_mut().insert(hash.to_string(), tab.clone()));
}

pub fn unregister_window(hash: &str) {
    let now_empty = OPEN_READERS.with(|r| {
        let mut r = r.borrow_mut();
        r.remove(hash);
        r.is_empty()
    });
    if now_empty {
        ON_ALL_CLOSED.with(|c| {
            for cb in c.borrow().iter() {
                cb();
            }
        });
    }
}

/// A flat, left-aligned popover row. Deliberately a copy of `app_window`'s helper of the
/// same name rather than a shared import: this module is on its way into its own crate, and
/// an eighteen-line button helper is not worth a dependency back on the application. If the
/// two ever need to differ, they already can.
pub fn popover_button(label: &str, destructive: bool) -> gtk4::Button {
    let button = gtk4::Button::new();
    button.add_css_class("flat");
    if destructive {
        button.add_css_class("destructive-action");
    }
    let lbl = gtk4::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_halign(gtk4::Align::Start);
    button.set_child(Some(&lbl));
    button
}

/// The margined rule between logical groups of popover rows. Copied for the same reason as
/// [`popover_button`].
pub fn popover_separator() -> gtk4::Separator {
    let sep = gtk4::Separator::new(Orientation::Horizontal);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    sep
}

/// A small multi-line note editor, sharing the "save on focus-leave, no explicit Save
/// button" behavior every inline editor in this app already uses, but — unlike the plain
/// `gtk4::Entry` every note field used until now — able to hold more than one line. Every
/// other reader worth comparing against (Kindle, Apple Books, Foliate) treats a margin note
/// as a small paragraph, not a single sentence; a single-line `Entry` silently ate anything
/// past the first Enter (which just defocused the field instead of inserting a newline).
/// `gtk4::TextView` has no native placeholder, so a dim label is manually shown/hidden over
/// it via `TextBuffer::connect_changed`.
pub fn note_edit_widget(initial: Option<&str>, on_save: impl Fn(String) + 'static) -> gtk4::Widget {
    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
    text_view.set_top_margin(6);
    text_view.set_bottom_margin(6);
    text_view.set_left_margin(8);
    text_view.set_right_margin(8);

    let buffer = text_view.buffer();
    if let Some(text) = initial {
        buffer.set_text(text);
    }

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_min_content_height(48);
    scroll.set_max_content_height(140);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&text_view));

    let frame = gtk4::Frame::new(None);
    frame.set_child(Some(&scroll));

    let placeholder = gtk4::Label::new(Some("No note"));
    placeholder.add_css_class("dim-label");
    placeholder.set_halign(gtk4::Align::Start);
    placeholder.set_valign(gtk4::Align::Start);
    placeholder.set_margin_top(6);
    placeholder.set_margin_start(8);
    placeholder.set_can_target(false);
    placeholder.set_visible(buffer.char_count() == 0);

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&frame));
    overlay.add_overlay(&placeholder);

    {
        let placeholder = placeholder.clone();
        buffer.connect_changed(move |buf| {
            placeholder.set_visible(buf.char_count() == 0);
        });
    }

    let focus = gtk4::EventControllerFocus::new();
    {
        let buffer = buffer.clone();
        focus.connect_leave(move |_| {
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            on_save(text.trim().to_string());
        });
    }
    text_view.add_controller(focus);

    overlay.upcast()
}

/// A small colored square showing a highlight/underline/strikeout's color, for annotation
/// list rows (the notes sidebar, the annotations dialog) — those rows showed only `{:?}`
/// kind text before this, with no visual link back to what's actually marked on the page,
/// unlike every comparable reader's notebook/highlights view (Kindle, Apple Books), which
/// leads with color since that's usually how a reader actually remembers a passage. `None`
/// (a margin note, or an annotation predating the color picker) draws nothing.
pub fn color_swatch(hex: Option<&str>) -> Option<gtk4::Widget> {
    let hex = hex?.to_string();
    let swatch = gtk4::DrawingArea::new();
    swatch.set_content_width(12);
    swatch.set_content_height(12);
    swatch.set_valign(gtk4::Align::Center);
    let tooltip = palette::label_for_hex(&hex).unwrap_or_else(|| hex.clone());
    swatch.set_tooltip_text(Some(&tooltip));
    swatch.set_draw_func(move |_, cr, w, h| {
        let [r, g, b, _] = pdf::annotation_rgba(Some(&hex));
        cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        cr.rectangle(0.0, 0.0, w as f64, h as f64);
        let _ = cr.fill();
    });
    Some(swatch.upcast())
}

/// Set a button's icon via a themed-icon fallback chain instead of a single icon name, so a
/// missing name in whatever icon theme is active shows the *next* choice instead of GTK's
/// generic "missing image" glyph — reported live as a plain question mark on the PDF/EPUB
/// readers' Contents sidebar toggle (`sidebar-show-symbolic`, not guaranteed present in every
/// icon theme Pereplyot might run under). `names` should be ordered most- to least-specific.
pub fn set_icon_with_fallback(button: &impl IsA<gtk4::Button>, names: &[&str]) {
    let icon = gtk4::gio::ThemedIcon::from_names(names);
    let image = gtk4::Image::from_gicon(&icon);
    button.set_child(Some(&image));
}

fn is_icon_only(button: &gtk4::Widget) -> bool {
    match button.downcast_ref::<gtk4::Button>() {
        Some(b) => b.label().is_none() && !b.child().is_some_and(|c| c.is::<gtk4::Label>()),
        None => button
            .downcast_ref::<gtk4::ToggleButton>()
            .is_some_and(|b| {
                b.label().is_none() && !b.child().is_some_and(|c| c.is::<gtk4::Label>())
            }),
    }
}

fn sync_accessible_label(widget: &gtk4::Widget) {
    let Some(tip) = widget.tooltip_text() else {
        return;
    };
    let first = tip.lines().next().unwrap_or("");
    let name = match first.rfind(" (") {
        Some(i) if first.ends_with(')') => &first[..i],
        _ => first,
    };
    if !name.is_empty() {
        widget.update_property(&[gtk4::accessible::Property::Label(name)]);
    }
}

/// Give every icon-only button under `root` an accessible name taken from its tooltip, and
/// keep it in step when the tooltip changes (the bookmark star, for one). Screen readers
/// otherwise announce these as unnamed buttons.
pub fn label_icon_buttons(root: &impl IsA<gtk4::Widget>) {
    let root = root.as_ref();
    if (root.is::<gtk4::Button>() || root.is::<gtk4::ToggleButton>()) && is_icon_only(root) {
        sync_accessible_label(root);
        root.connect_notify_local(Some("tooltip-text"), |w, _| sync_accessible_label(w));
    }
    if root.is::<gtk4::DropDown>() {
        sync_accessible_label(root);
    }
    let mut child = root.first_child();
    while let Some(c) = child {
        label_icon_buttons(&c);
        child = c.next_sibling();
    }
}

/// Whether keyboard focus is on a control that owns Space/Enter/arrow keys itself (a button,
/// drop-down or list row), so reader-wide page shortcuts must leave those keys alone.
pub fn focus_owns_activation_keys(root: &impl IsA<gtk4::Widget>) -> bool {
    let Some(focus) = root.as_ref().root().and_then(|r| r.focus()) else {
        return false;
    };
    let mut w = Some(focus);
    while let Some(cur) = w {
        if cur.is::<gtk4::Button>()
            || cur.is::<gtk4::ToggleButton>()
            || cur.is::<gtk4::DropDown>()
            || cur.is::<gtk4::ListBoxRow>()
            || cur.is::<gtk4::CheckButton>()
        {
            return true;
        }
        w = cur.parent();
    }
    false
}

/// Turn `label` (already appended to a horizontal/vertical `Box`) into the content of a flat,
/// keyboard-focusable button in the same place, running `on_click` on click, Enter or Space.
/// Sidebar jump targets used to be bare labels with a click gesture, unreachable without a mouse.
pub fn make_jump(label: &gtk4::Label, on_click: impl Fn() + 'static) {
    let Some(parent) = label.parent().and_then(|p| p.downcast::<gtk4::Box>().ok()) else {
        return;
    };
    let prev = label.prev_sibling();
    let hexpand = gtk4::prelude::WidgetExt::hexpands(label);
    parent.remove(label);
    let button = gtk4::Button::new();
    button.add_css_class("flat");
    button.set_hexpand(hexpand);
    button.set_child(Some(label));
    parent.insert_child_after(&button, prev.as_ref());
    button.connect_clicked(move |_| on_click());
}

/// A small modal for typing a note, optionally showing the quoted text it's about.
pub fn note_dialog(
    parent: &impl IsA<gtk4::Window>,
    quote: Option<&str>,
    on_save: impl Fn(String) + 'static,
) {
    use libadwaita as adw;
    use libadwaita::prelude::*;
    let dialog = adw::Window::new();
    dialog.set_title(Some("Add note"));
    dialog.set_modal(true);
    dialog.set_transient_for(Some(parent));
    dialog.set_default_size(420, 280);
    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    let save = gtk4::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&save);
    view.add_top_bar(&header);
    let body = gtk4::Box::new(Orientation::Vertical, 0);
    if let Some(q) = quote {
        let label = gtk4::Label::new(Some(q));
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.set_selectable(true);
        label.add_css_class("dim-label");
        label.set_margin_top(8);
        label.set_margin_start(12);
        label.set_margin_end(12);
        body.append(&label);
    }
    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_margin_top(8);
    text_view.set_margin_bottom(8);
    text_view.set_margin_start(8);
    text_view.set_margin_end(8);
    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&text_view));
    body.append(&scrolled);
    view.set_content(Some(&body));
    dialog.set_content(Some(&view));
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let text_view = text_view.clone();
        save.connect_clicked(move |_| {
            let buffer = text_view.buffer();
            let text = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .trim()
                .to_string();
            if !text.is_empty() {
                on_save(text);
                dialog.close();
            }
        });
    }
    dialog.present();
    text_view.grab_focus();
}
