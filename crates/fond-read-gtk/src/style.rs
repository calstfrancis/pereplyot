//! The few CSS rules the readers' own widgets need, installed once for the display. Colours are
//! libadwaita's named ones, so they follow the theme and the accent.

use std::cell::Cell;

thread_local! {
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn ensure() {
    if INSTALLED.with(|i| i.replace(true)) {
        return;
    }
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let css = gtk4::CssProvider::new();
    css.load_from_data(
        ".outline-current { font-weight: 700; background-color: alpha(@accent_bg_color, 0.16); } \
         .reading-note-active { background-color: alpha(@accent_bg_color, 0.14); border-radius: 6px; } \
         .pinned-figure { background-color: @card_bg_color; border-radius: 8px; \
                          box-shadow: 0 2px 10px alpha(black, 0.35); } \
         .figure-grip { background-color: alpha(@window_fg_color, 0.06); border-radius: 8px 8px 0 0; } \
         .reading-position { background-color: @accent_bg_color; } \
         .reading-position-label { background-color: @accent_bg_color; color: @accent_fg_color; \
                                   border-radius: 10px; padding: 1px 10px; }",
    );
    gtk4::style_context_add_provider_for_display(
        &display,
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
