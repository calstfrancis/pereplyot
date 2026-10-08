use super::*;

pub(super) struct RenderActionsParts {
    pub(super) render: Rc<dyn Fn()>,
    pub(super) undo: Rc<dyn Fn()>,
    pub(super) redo: Rc<dyn Fn()>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_render_actions(
    bookmark_button: &gtk4::Button,
    continuous_toggle: &gtk4::ToggleButton,
    header_end: &gtk4::Box,
    header_start: &gtk4::Box,
    host: &Rc<dyn ReaderHost>,
    next: &gtk4::Button,
    page_entry: &gtk4::Entry,
    page_of_label: &gtk4::Label,
    picture: &gtk4::Picture,
    prev: &gtk4::Button,
    reader: &Rc<RefCell<ReaderState>>,
    reader_tab: &crate::reader_host::ReaderTab,
    redo_button: &gtk4::Button,
    right_picture: &gtk4::Picture,
    title_widget: &adw::WindowTitle,
    two_page_toggle: &gtk4::ToggleButton,
    undo_button: &gtk4::Button,
) -> RenderActionsParts {
    let bookmark_button = bookmark_button.clone();
    let continuous_toggle = continuous_toggle.clone();
    let header_end = header_end.clone();
    let header_start = header_start.clone();
    let host = host.clone();
    let next = next.clone();
    let page_entry = page_entry.clone();
    let page_of_label = page_of_label.clone();
    let picture = picture.clone();
    let prev = prev.clone();
    let reader = reader.clone();
    let reader_tab = reader_tab.clone();
    let redo_button = redo_button.clone();
    let right_picture = right_picture.clone();
    let title_widget = title_widget.clone();
    let two_page_toggle = two_page_toggle.clone();
    let undo_button = undo_button.clone();
    crate::reader_host::set_tab_header(&reader_tab, header_start, title_widget, header_end);

    // Render the current page into the Picture (via the shared helper both this view and
    // continuous-scroll mode use), and refresh the page label. Also fills `right_picture`
    // with the facing page when Two-page mode is on (hidden otherwise) — every existing
    // caller of `render()` (nav buttons, zoom, search, outline/notes-sidebar jumps, page
    // entry) gets two-page-aware rendering for free this way, with no changes needed at any
    // of those call sites.
    let render: Rc<dyn Fn()> = {
        let reader = reader.clone();
        let picture = picture.clone();
        let right_picture = right_picture.clone();
        let two_page_toggle = two_page_toggle.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        Rc::new(move || {
            let mut r = reader.borrow_mut();
            match render_pdf_page_texture(&r, r.page) {
                Some((texture, w, h)) => {
                    r.render_px = (w, h);
                    picture.set_paintable(Some(&texture));
                    picture.set_size_request(w as i32, h as i32);
                }
                None => picture.set_paintable(gdk::Paintable::NONE),
            }
            if two_page_toggle.is_active() {
                let right_page = r.page + 1;
                if right_page < r.count {
                    match render_pdf_page_texture(&r, right_page) {
                        Some((texture, w, h)) => {
                            right_picture.set_paintable(Some(&texture));
                            right_picture.set_size_request(w as i32, h as i32);
                            right_picture.set_visible(true);
                        }
                        None => right_picture.set_visible(false),
                    }
                } else {
                    // Odd page count: the last spread has no facing page.
                    right_picture.set_visible(false);
                }
            } else {
                right_picture.set_visible(false);
            }
            update_page_display(
                &page_entry,
                &page_of_label,
                &prev,
                &next,
                &bookmark_button,
                r.page,
                r.count,
                &r.page_labels,
                &r.bookmarks,
            );
        })
    };
    render();

    // Undo/redo and every other annotation change redraw through the store's change
    // notification; a change doesn't say which page(s) it touched, so redraw everything —
    // cheap even in continuous mode, since each page's blend is just a texture re-render.
    let redraw_for = {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        Rc::new(move |change: &crate::annotation_store::Change| {
            if !continuous_toggle.is_active() {
                render();
            }
            for page in change.pages() {
                render_continuous_page(&reader, page.saturating_sub(1) as u16);
            }
        })
    };
    {
        let store = reader.borrow().store.clone();
        let undo_button = undo_button.clone();
        let redo_button = redo_button.clone();
        store.subscribe(move |store, change| {
            undo_button.set_sensitive(store.can_undo());
            redo_button.set_sensitive(store.can_redo());
            redraw_for(change);
        });
    }
    let undo = {
        let reader = reader.clone();
        let host = host.clone();
        Rc::new(move || {
            let store = reader.borrow().store.clone();
            match store.undo() {
                Ok(true) => host.notify("Undid last annotation change"),
                Ok(false) => host.notify("Nothing to undo"),
                Err(e) => host.notify(&format!("Could not undo: {e}")),
            }
        })
    };
    let redo = {
        let reader = reader.clone();
        let host = host.clone();
        Rc::new(move || {
            let store = reader.borrow().store.clone();
            match store.redo() {
                Ok(true) => host.notify("Redid annotation change"),
                Ok(false) => host.notify("Nothing to redo"),
                Err(e) => host.notify(&format!("Could not redo: {e}")),
            }
        })
    };
    {
        let undo = undo.clone();
        undo_button.connect_clicked(move |_| undo());
    }
    {
        let redo = redo.clone();
        redo_button.connect_clicked(move |_| redo());
    }
    RenderActionsParts { render, undo, redo }
}
