use super::*;

pub(super) struct CanvasParts {
    pub(super) hint: gtk4::Label,
    pub(super) breadcrumb: gtk4::Label,
    pub(super) picture: gtk4::Picture,
    pub(super) drag_preview: gtk4::DrawingArea,
    pub(super) drag_live_rect: DragRectCell,
    pub(super) right_picture: gtk4::Picture,
    pub(super) right_drag_preview: gtk4::DrawingArea,
    pub(super) right_drag_live_rect: DragRectCell,
    pub(super) scroll: gtk4::ScrolledWindow,
    pub(super) continuous_box: gtk4::Box,
    pub(super) continuous_scroll: gtk4::ScrolledWindow,
    pub(super) view_stack: gtk4::Stack,
    pub(super) view_overlay: gtk4::Overlay,
    pub(super) content: gtk4::Box,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_canvas(reader: &Rc<RefCell<ReaderState>>) -> CanvasParts {
    let reader = reader.clone();
    let hint = gtk4::Label::new(Some(
        "Drag over text to select it, then choose a colour (or press 1–4)",
    ));
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    hint.set_margin_top(4);
    hint.set_margin_bottom(4);

    let picture = gtk4::Picture::new();
    picture.set_halign(gtk4::Align::Center);
    picture.set_valign(gtk4::Align::Start);
    picture.set_can_target(true);
    picture.set_focusable(true);
    picture.set_cursor(cursor_for_select_mode(true).as_ref());
    let (picture_overlay, drag_preview, drag_live_rect) = {
        let reader_for_page = reader.clone();
        build_drag_preview_overlay(&picture, &reader, move || reader_for_page.borrow().page)
    };
    picture_overlay.set_halign(gtk4::Align::Center);
    picture_overlay.set_valign(gtk4::Align::Start);
    // "Two-page" mode's facing page, beside `picture_overlay` in `spread_box`; hidden (and left
    // unrendered) outside that mode. It is the page after the reader's current one, with the
    // same drag, preview and hover overlay as the left page.
    let right_picture = gtk4::Picture::new();
    right_picture.set_halign(gtk4::Align::Center);
    right_picture.set_valign(gtk4::Align::Start);
    right_picture.set_can_target(true);
    right_picture.set_cursor(cursor_for_select_mode(true).as_ref());
    let spread_box = gtk4::Box::new(Orientation::Horizontal, 12);
    spread_box.set_halign(gtk4::Align::Center);
    spread_box.append(&picture_overlay);
    let (right_overlay, right_drag_preview, right_drag_live_rect) = {
        let reader_for_page = reader.clone();
        build_drag_preview_overlay(&right_picture, &reader, move || {
            reader_for_page.borrow().page + 1
        })
    };
    right_overlay.set_halign(gtk4::Align::Center);
    right_overlay.set_valign(gtk4::Align::Start);
    right_overlay.set_visible(false);
    right_picture.set_visible(true);
    spread_box.append(&right_overlay);
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&spread_box));
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);

    // Continuous-scroll mode's surface: an empty Box for now — populated with one Picture
    // per page the first time the mode is toggled on (`build_continuous_view` below), not
    // eagerly here, so a plain "Read" stays exactly as fast as it always was.
    let continuous_box = gtk4::Box::new(Orientation::Vertical, CONTINUOUS_PAGE_GAP as i32);
    continuous_box.set_margin_top(CONTINUOUS_PAGE_GAP as i32);
    continuous_box.set_margin_bottom(CONTINUOUS_PAGE_GAP as i32);
    let continuous_scroll = gtk4::ScrolledWindow::new();
    continuous_scroll.set_child(Some(&continuous_box));
    continuous_scroll.set_vexpand(true);
    continuous_scroll.set_hexpand(true);

    let view_stack = gtk4::Stack::new();
    view_stack.add_named(&scroll, Some("paged"));
    let continuous_overlay = gtk4::Overlay::new();
    continuous_overlay.set_child(Some(&continuous_scroll));
    continuous_overlay.add_overlay(&scrollbar_ticks::build_tick_layer(&reader));
    view_stack.add_named(&continuous_overlay, Some("continuous"));
    view_stack.set_visible_child_name("paged");

    // The hint, and where in the book you are (the outline's section path) at the end of its row.
    let breadcrumb = gtk4::Label::new(None);
    breadcrumb.add_css_class("dim-label");
    breadcrumb.add_css_class("caption");
    breadcrumb.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    breadcrumb.set_max_width_chars(44);
    breadcrumb.set_margin_end(12);
    breadcrumb.set_visible(false);
    hint.set_hexpand(true);
    let hint_row = gtk4::Box::new(Orientation::Horizontal, 0);
    hint_row.append(&hint);
    hint_row.append(&breadcrumb);
    let content = gtk4::Box::new(Orientation::Vertical, 0);
    content.append(&hint_row);
    let view_overlay = gtk4::Overlay::new();
    view_overlay.set_child(Some(&view_stack));
    view_overlay.set_vexpand(true);
    content.append(&view_overlay);
    CanvasParts {
        hint,
        breadcrumb,
        picture,
        drag_preview,
        drag_live_rect,
        right_picture,
        right_drag_preview,
        right_drag_live_rect,
        scroll,
        continuous_box,
        continuous_scroll,
        view_stack,
        view_overlay,
        content,
    }
}
