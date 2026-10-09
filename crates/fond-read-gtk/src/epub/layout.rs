use super::*;

pub(super) struct LayoutParts {
    pub(super) notes_paned: gtk4::Paned,
    pub(super) paned: gtk4::Paned,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_layout(
    content: &gtk4::Box,
    contents_scroll: &gtk4::ScrolledWindow,
    notes_scroll: &gtk4::ScrolledWindow,
    view: &adw::ToolbarView,
    status_items: &[gtk4::Widget],
) -> LayoutParts {
    let content = content.clone();
    let contents_scroll = contents_scroll.clone();
    let notes_scroll = notes_scroll.clone();
    let view = view.clone();
    // Contents (left) and Notes (right) are now two independent sidebars rather than a
    // shared Stack behind one toggle slot — see `show_pdf_reader`'s matching sidebars for
    // the full rationale (both can be open at once instead of sharing one toggle slot).
    contents_scroll.set_size_request(60, -1);
    notes_scroll.set_size_request(60, -1);

    let notes_paned = gtk4::Paned::new(Orientation::Horizontal);
    notes_paned.set_start_child(Some(&content));
    notes_paned.set_resize_start_child(true);
    notes_paned.set_shrink_start_child(false);
    notes_paned.set_end_child(gtk4::Widget::NONE);
    notes_paned.set_resize_end_child(false);
    notes_paned.set_shrink_end_child(true);
    notes_paned.set_vexpand(true);
    notes_paned.set_hexpand(true);

    let paned = gtk4::Paned::new(Orientation::Horizontal);
    paned.set_start_child(gtk4::Widget::NONE);
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(true);
    paned.set_end_child(Some(&notes_paned));
    paned.set_vexpand(true);
    paned.set_hexpand(true);
    paned.set_position(220);
    view.set_content(Some(&paned));

    // Status bar: just the reader-host footer (if the embedding app registered one, e.g.
    // Pereplyot's version/changelog button) — this reader otherwise has nothing that
    // belongs at the bottom of the window, unlike the PDF reader's page nav/zoom. Per-tab
    // rather than a second bottom bar added by the host window itself, matching the PDF
    // reader (see its own `show_pdf_reader` for the fuller reasoning).
    let statusbar = gtk4::Box::new(Orientation::Horizontal, 6);
    statusbar.add_css_class("toolbar");
    statusbar.add_css_class("fond-chrome");
    statusbar.add_css_class("fond-statusbar");
    for item in status_items {
        statusbar.append(item);
    }
    let spacer = gtk4::Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    statusbar.append(&spacer);
    if let Some(footer_widget) = crate::reader_host::host_footer_widget() {
        statusbar.append(&footer_widget);
    }
    view.add_bottom_bar(&statusbar);

    LayoutParts { notes_paned, paned }
}
