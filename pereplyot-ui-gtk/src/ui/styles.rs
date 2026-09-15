const GLOBAL_CSS: &str = "\
    .recent-row { \
        padding: 8px 12px; \
        border-radius: 8px; \
    } \
    .recent-row:hover { \
        background: alpha(@window_fg_color, 0.05); \
    } \
    .recent-row .title { \
        font-weight: bold; \
    } \
    .recent-row .subtitle { \
        opacity: 0.6; \
        font-size: 0.85em; \
    } \
    .library-cover-slot { \
        background: alpha(@window_fg_color, 0.08); \
        border-radius: 4px; \
        box-shadow: 0 1px 3px alpha(black, 0.3); \
    } \
    flowboxchild:hover .library-cover-slot { \
        background: alpha(@window_fg_color, 0.12); \
    } \
    .fond-toggle-active { background: transparent; } \
    .fond-toggle-active label { font-weight: 700; color: @window_fg_color; }";

pub fn load_global_css() {
    let css = gtk4::CssProvider::new();
    css.load_from_data(GLOBAL_CSS);
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
