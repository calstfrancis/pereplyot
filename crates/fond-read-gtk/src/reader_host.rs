//! Shared tab host for the PDF/EPUB readers.
//!
//! By default, "Read" opens a new tab in one shared `Reader` window instead of its own
//! top-level window: before the PDFium singleton fix (see `fond_doc::bind_pdfium`), a second
//! independent reader window was a guaranteed crash the moment either reader closed, and
//! even with that fixed, a window per open document doesn't scale past two or three. A tab
//! can still be popped out to its own standalone window via [`ReaderTab::pop_out`] (wired to
//! a headerbar button by each reader) — deliberately the *only* way to detach a tab, rather
//! than also handling `AdwTabView`'s free-form drag-out-of-the-bar gesture: that gesture
//! moves the page to a brand new `TabView` GTK builds on the fly, which this module has no
//! hook into, so a `ReaderTab` handle (and the `pdf_hash`/`hash`-keyed dedup registry in
//! `lib.rs` built on top of it) would go stale the moment a drag-out happened instead of
//! only on an explicit, fully-tracked `pop_out` call.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

/// GObject data key `on_tab_closed` stashes its closure under, on the `TabPage` itself. Read
/// back by the generic `close-page` handler `new_tab_view` connects once per host window.
const ON_CLOSE_DATA_KEY: &str = "fond-reader-on-close";

/// GObject data key `set_tab_header` stashes a page's header content under (see
/// `TabHeaderContent`) — read back both by the per-window `selected-page` handler
/// `new_tab_view` connects, and by `set_tab_header` itself for an immediate apply when the
/// page setting it is already the selected one.
const HEADER_CONTENT_DATA_KEY: &str = "fond-reader-header-content";

/// GObject data key the three shared header slot boxes (see `HeaderSlots`) are stashed under
/// on the host `adw::Window` itself, so `set_tab_header` can reach them from just a
/// `ReaderTab` without `new_tab_view` having to hand out a second return value everywhere.
const HEADER_SLOTS_DATA_KEY: &str = "fond-reader-header-slots";

/// One tab's contribution to the shared host header: built once by the reader (`pdf.rs`/
/// `epub.rs`) as three independent widgets — normally the same sidebar/undo/redo cluster,
/// document-title widget, and end-of-header controls cluster each reader used to pack into
/// its own nested `HeaderBar` — and handed to [`set_tab_header`] instead of packed directly.
/// Only the *currently selected* tab's content actually lives inside the host header's slot
/// boxes at any moment; switching tabs unparents the outgoing set and reparents this one, so
/// nothing needs rebuilding on every tab switch, just reparenting.
struct TabHeaderContent {
    start: gtk4::Widget,
    title: gtk4::Widget,
    end: gtk4::Widget,
}

/// The three always-present, always-empty-until-filled boxes packed into the host header
/// once, in `new_tab_view` — swapping tabs only ever adds/removes children of these, the
/// boxes themselves never move.
#[derive(Clone)]
struct HeaderSlots {
    start: gtk4::Box,
    title: gtk4::Box,
    end: gtk4::Box,
}

fn clear_box(b: &gtk4::Box) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}

/// Repopulate `slots` from whichever page is currently selected in `view` — called on every
/// `selected-page` change, and once immediately by [`set_tab_header`] when a page sets its
/// header content while already selected (its own first open, most commonly).
fn apply_selected_header(view: &adw::TabView, slots: &HeaderSlots) {
    clear_box(&slots.start);
    clear_box(&slots.title);
    clear_box(&slots.end);
    if let Some(page) = view.selected_page() {
        unsafe {
            if let Some(content) = page.data::<TabHeaderContent>(HEADER_CONTENT_DATA_KEY) {
                let content = content.as_ref();
                slots.start.append(&content.start);
                slots.title.append(&content.title);
                slots.end.append(&content.end);
            }
        }
    }
}

thread_local! {
    // The default shared reader window + its tab view, created lazily on the first reader
    // opened and cleared here as soon as it closes (its last tab closed, or the user closed
    // the window directly) — so the *next* "Read" after every reader was closed starts a
    // fresh window rather than trying to resurrect a dead one.
    static MAIN_HOST: RefCell<Option<(adw::Window, adw::TabView)>> = const { RefCell::new(None) };
    // Optional bottom-right status-bar widget an embedding app can ask every reader host
    // window (the shared one and any popped-out ones) to carry — see `set_host_footer`.
    static HOST_FOOTER: RefCell<Option<Rc<dyn Fn() -> gtk4::Widget>>> = const { RefCell::new(None) };
}

/// Reserve a widget to show at the bottom right of every reader host window's status bar —
/// built fresh per window via `factory`, since the same widget can't live in two windows at
/// once (the shared host and any windows popped out via [`ReaderTab::pop_out`] each need
/// their own instance). This crate is shared across Pereplyot, Kartoteka, and Sputnik, and
/// only Pereplyot wants this (its version/changelog button — Kartoteka and Sputnik already
/// show their own on their own main window instead, and this crate has no way to build an
/// app-specific `CARGO_PKG_VERSION`/`CHANGELOG.md` button itself), so the slot defaults to
/// unset and the status bar simply isn't shown at all unless an app opts in. Call this once,
/// before opening the first reader tab — the host window is built lazily on first use and
/// its bottom bar isn't rebuilt afterward.
pub fn set_host_footer(factory: impl Fn() -> gtk4::Widget + 'static) {
    HOST_FOOTER.with(|cell| *cell.borrow_mut() = Some(Rc::new(factory)));
}

/// Build a fresh instance of the reserved footer widget (see `set_host_footer`), if the
/// embedding app registered one. Each reader appends this into its own per-tab bottom status
/// bar rather than the host window adding a second, separate bottom bar of its own — one
/// merged status bar per tab instead of the host's status bar stacking below each reader's.
pub fn host_footer_widget() -> Option<gtk4::Widget> {
    HOST_FOOTER.with(|cell| cell.borrow().as_ref().map(|f| f()))
}

/// A handle to one reader's tab: parent window for its own nested dialogs/popovers, and a
/// way to close or re-present just this tab without knowing whether it's living in the
/// shared host window or one popped out on its own.
#[derive(Clone)]
pub struct ReaderTab {
    pub host_window: adw::Window,
    tab_view: adw::TabView,
    page: adw::TabPage,
}

impl ReaderTab {
    /// Select this tab and raise its window.
    pub fn present(&self) {
        self.tab_view.set_selected_page(&self.page);
        self.host_window.present();
    }

    /// Close just this tab (confirms immediately — there is nothing here to confirm).
    pub fn close(&self) {
        self.tab_view.close_page(&self.page);
    }

    /// Move this tab out of its current window into a brand-new standalone one — the
    /// explicit counterpart to dragging a tab out of the bar. Returns the new handle; the
    /// old one still refers to the same page but is no longer useful for `present`/`close`
    /// once the transfer completes (the page's tab view has changed).
    pub fn pop_out(&self, parent: &adw::ApplicationWindow) -> ReaderTab {
        let (new_host, new_view) = new_tab_view(parent);
        self.tab_view.transfer_page(&self.page, &new_view, 0);
        new_host.present();
        ReaderTab {
            host_window: new_host,
            tab_view: new_view,
            page: self.page.clone(),
        }
    }
}

/// Flip the host window between fullscreen and normal, swapping the header button's icon
/// and tooltip to match — `adw::Window` (like `gtk4::Window`) tracks fullscreen state itself
/// via `is_fullscreen`, so this just reads it back rather than keeping a separate bool.
fn toggle_fullscreen(host: &adw::Window, button: &gtk4::Button) {
    if host.is_fullscreen() {
        host.unfullscreen();
        button.set_icon_name("view-fullscreen-symbolic");
        button.set_tooltip_text(Some("Fullscreen (F11)"));
    } else {
        host.fullscreen();
        button.set_icon_name("view-restore-symbolic");
        button.set_tooltip_text(Some("Leave fullscreen (F11)"));
    }
}

/// Build a reader host window: an `adw::Window` with a header bar, a tab bar, and an
/// `adw::TabView` filling the rest — used both for the shared default window and for every
/// popped-out one, so both look and behave identically.
fn new_tab_view(parent: &adw::ApplicationWindow) -> (adw::Window, adw::TabView) {
    let host = adw::Window::new();
    host.set_title(Some("Reader"));
    host.set_transient_for(Some(parent));
    host.set_default_size(1000, 820);

    let tab_view = adw::TabView::new();
    let tab_bar = adw::TabBar::new();
    tab_bar.set_view(Some(&tab_view));

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.add_css_class("fond-chrome");

    let maximize_button = gtk4::Button::from_icon_name("window-maximize-symbolic");
    maximize_button.add_css_class("flat");
    maximize_button.set_tooltip_text(Some("Maximize window"));
    {
        let host = host.clone();
        maximize_button.connect_clicked(move |_| host.maximize());
    }
    header.pack_end(&maximize_button);

    let fullscreen_button = gtk4::Button::from_icon_name("view-fullscreen-symbolic");
    fullscreen_button.add_css_class("flat");
    fullscreen_button.set_tooltip_text(Some("Fullscreen (F11)"));
    {
        let host = host.clone();
        let button = fullscreen_button.clone();
        fullscreen_button.connect_clicked(move |_| toggle_fullscreen(&host, &button));
    }
    header.pack_end(&fullscreen_button);

    // The currently-selected tab's own controls (sidebar/undo/redo, document title, and its
    // end-of-header cluster) — each reader hands these to `set_tab_header` instead of
    // packing them into a nested `HeaderBar` of its own, so there is exactly one header row
    // total instead of the host's window-level row plus a second, per-tab one underneath it.
    // The boxes below are the only thing ever packed here; swapping tabs only reparents their
    // children (`apply_selected_header`), never touches these three.
    let header_start = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let header_title = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let header_end = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    header.pack_start(&header_start);
    header.set_title_widget(Some(&header_title));
    // Packed after maximize/fullscreen above, so (pack_end's reversed order) it lands just
    // left of them — window-level controls stay outermost, tab controls stay inward.
    header.pack_end(&header_end);
    let header_slots = HeaderSlots {
        start: header_start,
        title: header_title,
        end: header_end,
    };
    unsafe {
        host.set_data(HEADER_SLOTS_DATA_KEY, header_slots.clone());
    }

    {
        let host_for_key = host.clone();
        let button = fullscreen_button.clone();
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, keyval, _keycode, _modifiers| {
            if keyval == gtk4::gdk::Key::F11 {
                toggle_fullscreen(&host_for_key, &button);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        host.add_controller(key_controller);
    }

    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&tab_bar);

    {
        let slots = header_slots.clone();
        tab_view.connect_notify_local(Some("selected-page"), move |view, _| {
            apply_selected_header(view, &slots);
        });
    }

    toolbar.set_content(Some(&tab_view));
    host.set_content(Some(&toolbar));

    // No `create-window` handler: dragging a tab out of the bar is left as GTK's default
    // (refused, snaps back) rather than spawning a window — see the module doc for why.

    // The last tab leaving this view (closed, or transferred out via drag/`pop_out`) closes
    // the now-empty window instead of leaving a stray blank "Reader" window behind.
    {
        let host_weak = host.downgrade();
        tab_view.connect_page_detached(move |view, _page, _position| {
            if view.n_pages() == 0 {
                if let Some(host) = host_weak.upgrade() {
                    host.close();
                }
            }
        });
    }

    // One generic close confirmation for every page this view will ever hold, rather than
    // each reader connecting (and having to reconnect on every `pop_out`/drag-detach) its
    // own: `on_tab_closed` stashes its cleanup closure on the `TabPage` itself, which travels
    // with the page across `transfer_page` calls, so looking it up here keeps working no
    // matter which window/view the page has moved through since it was opened. Always
    // confirms immediately — there is never anything here worth blocking a close over.
    tab_view.connect_close_page(move |view, page| {
        unsafe {
            if let Some(on_close) = page.data::<Rc<dyn Fn()>>(ON_CLOSE_DATA_KEY) {
                (on_close.as_ref())();
            }
        }
        view.close_page_finish(page, true);
        glib::Propagation::Stop
    });

    // If this is (or was) the shared default host, forget it on close so the next reader
    // opened builds a fresh one rather than reusing a window that's going away. A no-op for
    // a popped-out window, which was never stored here.
    {
        let tab_view_for_close = tab_view.clone();
        host.connect_close_request(move |window| {
            MAIN_HOST.with(|cell| {
                let mut slot = cell.borrow_mut();
                if slot.as_ref().map(|(h, _)| h == window).unwrap_or(false) {
                    *slot = None;
                }
            });
            // Closing the *window* (its own close button, or the app quitting) never fires
            // `TabView`'s `close-page` signal — that only runs for an explicit tab-close
            // (its own close button, `ReaderTab::close`, or a keyboard shortcut). Without
            // this, `on_tab_closed`'s hooks (save reading progress, unregister the open
            // reader) silently never ran for the single most common way anyone actually
            // closes a reader. Found 2026-09-22 verifying progress round-trips correctly
            // through a `HostOverride` — this doc comment on `on_tab_closed` claimed the
            // cascade already happened here; it didn't.
            let n = tab_view_for_close.n_pages();
            for i in 0..n {
                let Some(page) = tab_view_for_close
                    .pages()
                    .item(i as u32)
                    .and_downcast::<adw::TabPage>()
                else {
                    continue;
                };
                unsafe {
                    if let Some(on_close) = page.data::<Rc<dyn Fn()>>(ON_CLOSE_DATA_KEY) {
                        (on_close.as_ref())();
                    }
                }
            }
            glib::Propagation::Proceed
        });
    }

    (host, tab_view)
}

/// Open `content` as a new tab in the shared reader window — creating it if this is the
/// first reader opened, or if the previous shared window has since closed — and select it.
/// `title` labels the tab. Does not present the window itself: callers still finish wiring
/// up the reader's own signal handlers afterward, exactly as when each reader was its own
/// top-level window, so presenting is left to the caller's own final `ReaderTab::present()`
/// once that's done.
pub fn open_reader_tab(
    parent: &adw::ApplicationWindow,
    title: &str,
    content: &impl IsA<gtk4::Widget>,
) -> ReaderTab {
    let (host_window, tab_view) = MAIN_HOST.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some(existing) = slot.clone() {
            return existing;
        }
        let created = new_tab_view(parent);
        *slot = Some(created.clone());
        created
    });

    let page = tab_view.append(content);
    page.set_title(title);
    tab_view.set_selected_page(&page);

    ReaderTab {
        host_window,
        tab_view,
        page,
    }
}

/// Run `on_close` once, when this tab is closed for good — its own tab-close button,
/// keyboard shortcut, or its host window closing (`new_tab_view`'s `close-request` handler
/// runs every open page's hook directly, since `TabView`'s own `close-page` signal doesn't
/// fire for a whole-window close). Runs as the close is confirmed, before anything is torn
/// down, so it's safe to touch the tab's own still-live widgets from `on_close` (e.g.
/// reading a WebView's scroll position) — the same timing a plain top-level reader window's
/// `close-request` gave each reader before tabs existed. See `new_tab_view`'s
/// `close-page`/`close-request` handlers for how this is actually invoked.
pub fn on_tab_closed(tab: &ReaderTab, on_close: impl Fn() + 'static) {
    let boxed: Rc<dyn Fn()> = Rc::new(on_close);
    unsafe {
        tab.page.set_data(ON_CLOSE_DATA_KEY, boxed);
    }
}

/// Give this tab its header content — the sidebar/undo/redo cluster, the document-title
/// widget, and the end-of-header controls cluster a reader used to pack into its own nested
/// `HeaderBar` — for the shared host header to show whenever this tab is selected (see
/// `TabHeaderContent`). Stored on the page itself, like `on_tab_closed`'s hook, so it keeps
/// working across `pop_out`/drag-detach without needing to be set again. If this tab is
/// already the selected one (true for a freshly opened tab, since `open_reader_tab` selects
/// it before the caller gets a chance to call this), applies immediately rather than waiting
/// for a `selected-page` change that may never come.
pub fn set_tab_header(
    tab: &ReaderTab,
    start: impl IsA<gtk4::Widget>,
    title: impl IsA<gtk4::Widget>,
    end: impl IsA<gtk4::Widget>,
) {
    let content = TabHeaderContent {
        start: start.upcast(),
        title: title.upcast(),
        end: end.upcast(),
    };
    unsafe {
        tab.page.set_data(HEADER_CONTENT_DATA_KEY, content);
    }
    if tab.tab_view.selected_page().as_ref() == Some(&tab.page) {
        unsafe {
            if let Some(slots) = tab.host_window.data::<HeaderSlots>(HEADER_SLOTS_DATA_KEY) {
                apply_selected_header(&tab.tab_view, slots.as_ref());
            }
        }
    }
}
