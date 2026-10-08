use super::*;

pub(super) struct HeaderButtonsParts {
    pub(super) sidebar_toggle: gtk4::ToggleButton,
    pub(super) notes_toggle: gtk4::ToggleButton,
    pub(super) continuous_toggle: gtk4::ToggleButton,
    pub(super) text_toggle: gtk4::ToggleButton,
    pub(super) two_page_toggle: gtk4::ToggleButton,
    pub(super) undo_button: gtk4::Button,
    pub(super) redo_button: gtk4::Button,
    pub(super) popout_button: gtk4::Button,
    pub(super) export_button: gtk4::Button,
    pub(super) header_start: gtk4::Box,
    pub(super) header_end: gtk4::Box,
    pub(super) more_button: gtk4::MenuButton,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_header_buttons(
    note_button: &gtk4::Button,
    page_num_button: &gtk4::Button,
    palette: &gtk4::Box,
    style_drop: &gtk4::DropDown,
) -> HeaderButtonsParts {
    let note_button = note_button.clone();
    let page_num_button = page_num_button.clone();
    let palette = palette.clone();
    let style_drop = style_drop.clone();
    // Always enabled — Thumbnails is always available even for a PDF with no outline
    // (unlike before this toggle covered Contents alone and was disabled without one).
    // Toggles a persistent sidebar (built below, after `render`/`reader` exist) rather than
    // a popover, per CLAUDE.md's house sidebar style: toggle at the *start* of the
    // headerbar, content as a collapsible Paned start-child.
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
    sidebar_toggle.set_tooltip_text(Some("Show contents / thumbnails"));

    // Whole-document notes/highlights list, in a persistent sidebar (built below, alongside
    // Contents) rather than the old per-page "This page" dropdown — readable prose, not just
    // on-page markers, and reachable regardless of which page is current. Editing/deleting
    // an individual annotation now happens by right-clicking it on the page itself.
    let notes_toggle = gtk4::ToggleButton::new();
    notes_toggle.set_icon_name("view-list-symbolic");
    notes_toggle.set_tooltip_text(Some("Show notes and highlights"));

    let continuous_toggle = gtk4::ToggleButton::new();
    crate::set_icon_with_fallback(
        &continuous_toggle,
        &["view-continuous-symbolic", "view-paged-symbolic"],
    );
    continuous_toggle.set_tooltip_text(Some(
        "Continuous — scroll through every page, instead of one page at a time",
    ));
    // Mutually exclusive with `continuous_toggle` (each deactivates the other on activate,
    // wired below) rather than a single 3-way control, so every existing
    // `continuous_toggle.is_active()` check elsewhere keeps meaning exactly what it always
    // did with no changes needed at those call sites. Reuses the single-page view's own
    // `view_stack` child and `render()` (extended to also fill `right_picture`) rather than
    // being a separate mode with its own render path, so navigation/zoom/search/outline/
    // notes-sidebar jumps all stay in sync with two-page mode for free.
    let text_toggle = gtk4::ToggleButton::new();
    crate::set_icon_with_fallback(
        &text_toggle,
        &[
            "format-justify-left-symbolic",
            "view-reader-symbolic",
            "text-x-generic-symbolic",
        ],
    );
    text_toggle.set_tooltip_text(Some(
        "Text view — the document's text reflowed: readable by screen readers, resizable, \
         and selectable with the keyboard (Shift+arrows, then 1–4 to mark)",
    ));
    let two_page_toggle = gtk4::ToggleButton::new();
    crate::set_icon_with_fallback(
        &two_page_toggle,
        &["view-dual-symbolic", "view-paged-symbolic"],
    );
    two_page_toggle.set_tooltip_text(Some(
        "Two-page — show two facing pages side by side, like an open book. Drawing a new \
         highlight or note still needs single-page or Continuous mode.",
    ));

    let undo_button = gtk4::Button::from_icon_name("edit-undo-symbolic");
    undo_button.set_tooltip_text(Some("Undo (Ctrl+Z)"));
    undo_button.set_sensitive(false);
    let redo_button = gtk4::Button::from_icon_name("edit-redo-symbolic");
    redo_button.set_tooltip_text(Some("Redo (Ctrl+Shift+Z)"));
    redo_button.set_sensitive(false);

    // Moves this tab out of the shared "Reader" window into its own standalone one — the
    // only way to detach a tab (see `reader_host`'s module doc for why there's no drag-out-
    // of-the-bar gesture too).
    let popout_button = gtk4::Button::from_icon_name("window-new-symbolic");
    popout_button.add_css_class("flat");
    popout_button.set_tooltip_text(Some("Open in a new window"));

    let export_button = gtk4::Button::from_icon_name("document-save-symbolic");
    export_button.add_css_class("flat");
    export_button.set_tooltip_text(Some("Export notes & highlights…"));

    // Visual order, left to right, in the shared host header: sidebar toggle, Undo, Redo
    // (start) … document title (centre) … Two-page, Continuous, colour palette, mark style,
    // Note, Page #, Export, Open in new window, Notes sidebar (end) — unchanged from when
    // these lived in this tab's own `HeaderBar`, just built as plain boxes now and handed to
    // `reader_host::set_tab_header` below instead of packed directly (see the comment on
    // `title_widget` above for why). Thumbnails used to have its own header button opening a
    // popup grid; it's now a tab in the sidebar (built below), alongside Contents.
    let header_start = gtk4::Box::new(Orientation::Horizontal, 6);
    header_start.append(&sidebar_toggle);
    header_start.append(&undo_button);
    header_start.append(&redo_button);
    let header_end = gtk4::Box::new(Orientation::Horizontal, 6);
    header_end.append(&two_page_toggle);
    header_end.append(&continuous_toggle);
    header_end.append(&text_toggle);
    header_end.append(&palette);
    header_end.append(&style_drop);
    let more_button = gtk4::MenuButton::new();
    more_button.set_icon_name("view-more-symbolic");
    more_button.set_tooltip_text(Some("More: note, page numbering, export, new window"));
    more_button.add_css_class("flat");
    {
        let rows = gtk4::Box::new(Orientation::Vertical, 2);
        rows.set_margin_top(6);
        rows.set_margin_bottom(6);
        rows.set_margin_start(6);
        rows.set_margin_end(6);
        let more_popover = gtk4::Popover::new();
        for (label, target) in [
            ("Add note on this page…", note_button.clone()),
            ("Set page numbering…", page_num_button.clone()),
            ("Export notes…", export_button.clone()),
            ("Open in a new window", popout_button.clone()),
        ] {
            let row = popover_button(label, false);
            row.set_sensitive(target.is_sensitive());
            let popover = more_popover.clone();
            row.connect_clicked(move |_| {
                popover.popdown();
                target.emit_clicked();
            });
            rows.append(&row);
        }
        more_popover.set_child(Some(&rows));
        more_button.set_popover(Some(&more_popover));
    }
    HeaderButtonsParts {
        sidebar_toggle,
        notes_toggle,
        continuous_toggle,
        text_toggle,
        two_page_toggle,
        undo_button,
        redo_button,
        popout_button,
        export_button,
        header_start,
        header_end,
        more_button,
    }
}
