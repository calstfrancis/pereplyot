use super::*;

pub(super) struct SidebarParts {
    pub(super) sidebar_box: gtk4::Box,
    pub(super) notes_rows: gtk4::Box,
    pub(super) notes_scroll: gtk4::ScrolledWindow,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_sidebar(
    continuous_scroll: &gtk4::ScrolledWindow,
    continuous_toggle: &gtk4::ToggleButton,
    outline_entries: &[fond_doc::PdfOutlineEntry],
    reader: &Rc<RefCell<ReaderState>>,
    render: &Rc<dyn Fn()>,
) -> SidebarParts {
    let render = render.clone();
    let continuous_scroll = continuous_scroll.clone();
    let continuous_toggle = continuous_toggle.clone();
    let reader = reader.clone();
    // Contents/Notes sidebar: persistent (not a popover) so it stays visible while
    // navigating, per Cal's request. Both panels share one Paned start-child slot via a
    // Stack, since only one is useful to see at a time; the two toggles are mutually
    // exclusive (activating one deactivates the other) but each can still be clicked again
    // to close the sidebar entirely, unlike a strict radio-group.
    let contents_scroll = {
        let rows = gtk4::Box::new(Orientation::Vertical, 2);
        rows.set_margin_top(6);
        rows.set_margin_bottom(6);
        rows.set_margin_start(6);
        rows.set_margin_end(6);
        for entry in outline_entries {
            let label = format!("{}{}", "    ".repeat(entry.depth as usize), entry.title);
            let row = popover_button(&label, false);
            if let Some(lbl) = row.child().and_then(|w| w.downcast::<gtk4::Label>().ok()) {
                lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            }
            if let Some(page) = entry.page {
                let reader = reader.clone();
                let render = render.clone();
                let continuous_toggle = continuous_toggle.clone();
                let continuous_scroll = continuous_scroll.clone();
                row.connect_clicked(move |_| {
                    let target = {
                        let r = reader.borrow();
                        (page.saturating_sub(1)).min(r.count.saturating_sub(1))
                    };
                    if continuous_toggle.is_active() {
                        scroll_continuous_to_page(&reader, &continuous_scroll, target);
                    } else {
                        reader.borrow_mut().page = target;
                        render();
                    }
                });
            } else {
                row.set_sensitive(false);
            }
            rows.append(&row);
        }
        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scroll.set_child(Some(&rows));
        scroll
    };

    // Outline and Thumbnails are two tabs of the one left sidebar (as opposed to Notes,
    // which is its own independent right-hand sidebar — see the comment further down where
    // `notes_paned` is built). A plain Stack + a two-button switcher, not `gtk4::StackSwitcher`
    // or `adw::ViewSwitcher`, to match the rest of this reader's hand-built toggle style and
    // to get `ToggleButton::set_group`'s native radio behaviour (exactly one active, clicking
    // the active one again does nothing) for free.
    let (thumbnails_scroll, trigger_thumbnails) =
        build_thumbnails_sidebar(&reader, &render, &continuous_toggle, &continuous_scroll);

    let outline_tab_toggle = gtk4::ToggleButton::with_label("Outline");
    let thumbnails_tab_toggle = gtk4::ToggleButton::with_label("Thumbnails");
    thumbnails_tab_toggle.set_group(Some(&outline_tab_toggle));
    let sidebar_tabs_row = gtk4::Box::new(Orientation::Horizontal, 0);
    sidebar_tabs_row.add_css_class("linked");
    sidebar_tabs_row.set_margin_top(6);
    sidebar_tabs_row.set_margin_bottom(6);
    sidebar_tabs_row.set_margin_start(6);
    sidebar_tabs_row.set_margin_end(6);
    sidebar_tabs_row.set_halign(gtk4::Align::Center);
    sidebar_tabs_row.append(&outline_tab_toggle);
    sidebar_tabs_row.append(&thumbnails_tab_toggle);

    let sidebar_tab_stack = gtk4::Stack::new();
    sidebar_tab_stack.set_vexpand(true);
    sidebar_tab_stack.add_named(&contents_scroll, Some("outline"));
    sidebar_tab_stack.add_named(&thumbnails_scroll, Some("thumbnails"));

    if outline_entries.is_empty() {
        outline_tab_toggle.set_sensitive(false);
        outline_tab_toggle.set_tooltip_text(Some("This PDF has no table of contents"));
        thumbnails_tab_toggle.set_active(true);
        sidebar_tab_stack.set_visible_child_name("thumbnails");
        trigger_thumbnails();
    } else {
        outline_tab_toggle.set_active(true);
        sidebar_tab_stack.set_visible_child_name("outline");
    }
    {
        let sidebar_tab_stack = sidebar_tab_stack.clone();
        outline_tab_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                sidebar_tab_stack.set_visible_child_name("outline");
            }
        });
    }
    {
        let sidebar_tab_stack = sidebar_tab_stack.clone();
        thumbnails_tab_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                sidebar_tab_stack.set_visible_child_name("thumbnails");
                trigger_thumbnails();
            }
        });
    }

    let sidebar_box = gtk4::Box::new(Orientation::Vertical, 0);
    sidebar_box.append(&sidebar_tabs_row);
    sidebar_box.append(&sidebar_tab_stack);

    // Notes/highlights list: every annotation in the document, readable prose rather than
    // just on-page markers, sorted by page. Rebuilt fresh (`rebuild_notes`, below) whenever
    // shown or whenever an annotation is added/removed elsewhere in the reader, via the
    // `Rc<RefCell<Option<...>>>` indirection so a row's own delete button can trigger a
    // rebuild of the list it lives in.
    let notes_rows = gtk4::Box::new(Orientation::Vertical, 2);
    notes_rows.set_margin_top(6);
    notes_rows.set_margin_bottom(6);
    notes_rows.set_margin_start(6);
    notes_rows.set_margin_end(6);
    let notes_scroll = gtk4::ScrolledWindow::new();
    notes_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    notes_scroll.set_child(Some(&notes_rows));

    SidebarParts {
        sidebar_box,
        notes_rows,
        notes_scroll,
    }
}
