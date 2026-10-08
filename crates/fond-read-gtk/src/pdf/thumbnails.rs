use super::*;

/// Thumbnail width for the sidebar list — deliberately much smaller than the reader's own
/// `READER_BASE_WIDTH`, since this is a jump-to-page aid, not a reading surface.
pub(super) const THUMBNAIL_GRID_WIDTH: u32 = 140;

/// Builds the Thumbnails tab of the Contents sidebar: every page as a small thumbnail in a
/// vertical list, for jumping around a long document visually instead of by number —
/// Okular/Acrobat's "page overview" mode, docked instead of a popup. Thumbnails are plain
/// unannotated renders (no highlight/search blending — this is for navigation, not a
/// miniature reading view).
///
/// Rendering is deferred: this only builds the row widgets (cheap) and hands back a trigger
/// closure that does the actual rasterizing, lazily and only once — call it when the tab is
/// first shown (see the `Thumbnails` toggle below), not at reader-open time, so a document
/// nobody ever opens the Thumbnails tab for never pays the render cost. Once triggered, pages
/// rasterize nearest-to-current-page first, one per idle tick — same spreading idiom as
/// continuous mode's own `schedule_continuous_render`, so it doesn't block the UI even on a
/// long document.
pub(super) fn build_thumbnails_sidebar(
    reader: &Rc<RefCell<ReaderState>>,
    render: &Rc<impl Fn() + 'static>,
    continuous_toggle: &gtk4::ToggleButton,
    continuous_scroll: &gtk4::ScrolledWindow,
) -> (gtk4::ScrolledWindow, Rc<dyn Fn()>) {
    let count = reader.borrow().count;

    let rows = gtk4::Box::new(Orientation::Vertical, 8);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);

    let mut pictures = Vec::with_capacity(count as usize);
    for page in 0..count {
        let picture = gtk4::Picture::new();
        picture.set_size_request(-1, 128);
        picture.set_content_fit(gtk4::ContentFit::Contain);

        let card = gtk4::Box::new(Orientation::Vertical, 2);
        card.append(&picture);
        let label = gtk4::Label::new(Some(&(page + 1).to_string()));
        label.add_css_class("caption");
        label.add_css_class("dim-label");
        card.append(&label);

        let button = gtk4::Button::new();
        button.add_css_class("flat");
        button.set_child(Some(&card));

        {
            let reader = reader.clone();
            let render = render.clone();
            let continuous_toggle = continuous_toggle.clone();
            let continuous_scroll = continuous_scroll.clone();
            button.connect_clicked(move |_| {
                if continuous_toggle.is_active() {
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                } else {
                    reader.borrow_mut().page = page;
                    render();
                }
            });
        }

        rows.append(&button);
        pictures.push(picture);
    }

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_child(Some(&rows));

    let built = Rc::new(Cell::new(false));
    let trigger: Rc<dyn Fn()> = {
        let reader = reader.clone();
        let built = built.clone();
        let scroll = scroll.clone();
        Rc::new(move || {
            if built.replace(true) {
                return;
            }
            let current_page = reader.borrow().page;
            let mut order: Vec<u16> = (0..count).collect();
            order.sort_by_key(|&p| (p as i32 - current_page as i32).unsigned_abs());
            schedule_thumbnail_render(reader.clone(), pictures.clone(), order, 0);

            // Jump the list roughly to the current page on first show, rather than always
            // opening at the top — exact scrolling would need each row's real allocation,
            // which isn't settled yet the same frame the tab becomes visible, so this is an
            // approximation (fraction of the way through the document), good enough for a
            // jump-near-here aid.
            if count > 0 {
                let fraction = current_page as f64 / count as f64;
                let scroll = scroll.clone();
                glib::idle_add_local_once(move || {
                    let adj = scroll.vadjustment();
                    adj.set_value(fraction * adj.upper());
                });
            }
        })
    };

    (scroll, trigger)
}

/// One tick of `show_thumbnail_grid`'s lazy rasterization — see that function's doc comment.
pub(super) fn schedule_thumbnail_render(
    reader: Rc<RefCell<ReaderState>>,
    pictures: Vec<gtk4::Picture>,
    order: Vec<u16>,
    idx: usize,
) {
    let Some(&page) = order.get(idx) else {
        return;
    };
    if let Some(picture) = pictures.get(page as usize) {
        let r = reader.borrow();
        if let Some(rp) = render_open(&r, page, THUMBNAIL_GRID_WIDTH) {
            let data = glib::Bytes::from(&rp.rgba);
            let texture = gdk::MemoryTexture::new(
                rp.width as i32,
                rp.height as i32,
                gdk::MemoryFormat::R8g8b8a8,
                &data,
                (rp.width * 4) as usize,
            );
            picture.set_paintable(Some(&texture));
        }
    }
    glib::idle_add_local_once(move || {
        schedule_thumbnail_render(reader, pictures, order, idx + 1);
    });
}
