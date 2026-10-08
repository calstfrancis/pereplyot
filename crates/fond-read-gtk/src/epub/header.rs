use super::*;

pub(super) struct HeaderParts {
    pub(super) sidebar_toggle: gtk4::ToggleButton,
    pub(super) notes_toggle: gtk4::ToggleButton,
    pub(super) undo_button: gtk4::Button,
    pub(super) redo_button: gtk4::Button,
    pub(super) mode_drop: gtk4::DropDown,
    pub(super) palette_choice: Rc<Cell<usize>>,
    pub(super) apply_button: gtk4::Button,
    pub(super) zoom_out_button: gtk4::Button,
    pub(super) zoom_in_button: gtk4::Button,
    pub(super) export_button: gtk4::Button,
    pub(super) popout_button: gtk4::Button,
    pub(super) header_start: gtk4::Box,
    pub(super) header_end: gtk4::Box,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_header(
    has_toc: bool,
    search_toggle: &gtk4::ToggleButton,
    web_view: &webkit6::WebView,
) -> HeaderParts {
    let search_toggle = search_toggle.clone();
    let web_view = web_view.clone();
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
    if !has_toc {
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

    HeaderParts {
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
    }
}
