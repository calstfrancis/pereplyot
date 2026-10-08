use super::*;

pub(super) struct SearchBarParts {
    pub(super) search_toggle: gtk4::ToggleButton,
    pub(super) search_bar_entry: gtk4::SearchEntry,
    pub(super) search_prev: gtk4::Button,
    pub(super) search_next: gtk4::Button,
    pub(super) whole_book_toggle: gtk4::ToggleButton,
    pub(super) search_count: gtk4::Label,
    pub(super) results_list: gtk4::ListBox,
    pub(super) results_revealer: gtk4::Revealer,
    pub(super) search_revealer: gtk4::Revealer,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_search_bar() -> SearchBarParts {
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

    SearchBarParts {
        search_toggle,
        search_bar_entry,
        search_prev,
        search_next,
        whole_book_toggle,
        search_count,
        results_list,
        results_revealer,
        search_revealer,
    }
}
