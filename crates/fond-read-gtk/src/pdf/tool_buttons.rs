use super::*;

pub(super) struct ToolButtonsParts {
    pub(super) zoom_out: gtk4::Button,
    pub(super) zoom_in: gtk4::Button,
    pub(super) zoom_fit_width: gtk4::Button,
    pub(super) zoom_fit_page: gtk4::Button,
    pub(super) rotate_button: gtk4::Button,
    pub(super) invert_button: gtk4::ToggleButton,
    pub(super) note_button: gtk4::Button,
    pub(super) page_num_button: gtk4::Button,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_tool_buttons(
    nav: &gtk4::Box,
    bookmark_button: &gtk4::Button,
    has_native_page_labels: bool,
) -> ToolButtonsParts {
    let nav = nav.clone();
    let bookmark_button = bookmark_button.clone();
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
    invert_button.set_tooltip_text(Some(Tone::Normal.tooltip()));

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
    ToolButtonsParts {
        zoom_out,
        zoom_in,
        zoom_fit_width,
        zoom_fit_page,
        rotate_button,
        invert_button,
        note_button,
        page_num_button,
    }
}
