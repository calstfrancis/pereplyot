use super::*;

pub(super) struct PageNavParts {
    pub(super) view: adw::ToolbarView,
    pub(super) title_widget: adw::WindowTitle,
    pub(super) prev: gtk4::Button,
    pub(super) next: gtk4::Button,
    pub(super) page_entry: gtk4::Entry,
    pub(super) page_of_label: gtk4::Label,
    pub(super) bookmark_button: gtk4::Button,
    pub(super) nav: gtk4::Box,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_page_nav(
    reader: &Rc<RefCell<ReaderState>>,
    start_page: u16,
    title: &str,
) -> PageNavParts {
    let reader = reader.clone();
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
    PageNavParts {
        view,
        title_widget,
        prev,
        next,
        page_entry,
        page_of_label,
        bookmark_button,
        nav,
    }
}
