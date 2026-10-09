use super::*;

pub(super) struct StatusBarParts {
    pub(super) statusbar: gtk4::Box,
    pub(super) search_entry: gtk4::SearchEntry,
    pub(super) search_prev: gtk4::Button,
    pub(super) search_next: gtk4::Button,
    pub(super) search_count: gtk4::Label,
    pub(super) link_back: gtk4::Button,
    pub(super) link_forward: gtk4::Button,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_status_bar(
    header_end: &gtk4::Box,
    text_toggle: &gtk4::ToggleButton,
    invert_button: &gtk4::ToggleButton,
    rotate_button: &gtk4::Button,
    zoom_fit_page: &gtk4::Button,
    zoom_fit_width: &gtk4::Button,
    zoom_in: &gtk4::Button,
    zoom_out: &gtk4::Button,
    notes_toggle: &gtk4::ToggleButton,
    nav: &gtk4::Box,
) -> StatusBarParts {
    let notes_toggle = notes_toggle.clone();
    let nav = nav.clone();
    let header_end = header_end.clone();
    let text_toggle = text_toggle.clone();
    let invert_button = invert_button.clone();
    let rotate_button = rotate_button.clone();
    let zoom_fit_page = zoom_fit_page.clone();
    let zoom_fit_width = zoom_fit_width.clone();
    let zoom_in = zoom_in.clone();
    let zoom_out = zoom_out.clone();
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
    let link_forward = gtk4::Button::new();
    link_forward.add_css_class("flat");
    link_forward.set_visible(false);
    link_forward.set_tooltip_text(Some("Go forward again after going back (Alt+Right)"));
    statusbar.append(&link_back);
    statusbar.append(&link_forward);
    statusbar.append(&search_entry);
    statusbar.append(&search_count);
    statusbar.append(&search_prev);
    statusbar.append(&search_next);
    let typography_button = crate::typography::button();
    typography_button.set_visible(false);
    {
        let typography_button = typography_button.clone();
        text_toggle.connect_toggled(move |t| {
            typography_button.set_visible(t.is_active());
            if let Some(label) = t.child().and_then(|c| c.downcast::<gtk4::Label>().ok()) {
                let text = if t.is_active() {
                    "<b>Reading</b>"
                } else {
                    "Reading"
                };
                label.set_markup(text);
            }
        });
    }
    statusbar.append(&text_toggle);
    statusbar.append(&typography_button);
    statusbar.append(&rotate_button);
    statusbar.append(&invert_button);
    statusbar.append(&zoom_fit_width);
    statusbar.append(&zoom_fit_page);
    statusbar.append(&zoom_out);
    statusbar.append(&zoom_in);
    if let Some(footer_widget) = crate::reader_host::host_footer_widget() {
        statusbar.append(&footer_widget);
    }
    StatusBarParts {
        statusbar,
        search_entry,
        search_prev,
        search_next,
        search_count,
        link_back,
        link_forward,
    }
}
