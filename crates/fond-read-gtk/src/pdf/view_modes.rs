use super::*;

pub(super) fn install_view_modes(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        reader_window,
        render,
        prev,
        next,
        page_entry,
        page_of_label,
        bookmark_button,
        rotate_button,
        invert_button,
        continuous_toggle,
        two_page_toggle,
        continuous_box,
        continuous_scroll,
        view_stack,
        ..
    } = ui.clone();
    {
        let reader = reader.clone();
        let render = render.clone();
        rotate_button.connect_clicked(move |_| {
            {
                let mut r = reader.borrow_mut();
                r.rotation = (r.rotation + 90) % 360;
            }
            render();
        });
    }
    {
        let reader = reader.clone();
        let render = render.clone();
        invert_button.connect_clicked(move |btn| {
            let tone = {
                let mut r = reader.borrow_mut();
                r.tone = r.tone.next();
                r.tone
            };
            btn.set_active(tone != Tone::Normal);
            btn.set_tooltip_text(Some(tone.tooltip()));
            render();
            // `render()` alone only updates the paged view's (possibly hidden) Picture —
            // Continuous mode has its own per-page Pictures with their own last-rendered
            // textures, so they need their own refresh or an inverted toggle would silently
            // do nothing while Continuous (the default mode) is what's actually on screen.
            rerender_loaded_continuous_pages(&reader);
        });
    }
    {
        // Rotation is view-only and only meaningful in single-page mode — see
        // `ReaderState::rotation`'s doc comment. Disabled (rather than just made a no-op)
        // whenever Continuous or Two-page is active, so the limitation is visible instead
        // of a click silently doing nothing; any existing non-zero rotation is cleared on
        // the way in, since neither mode's own layout math accounts for it.
        let reader = reader.clone();
        let render = render.clone();
        let rotate_button = rotate_button.clone();
        let two_page_toggle_for_rotate = two_page_toggle.clone();
        continuous_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                reader.borrow_mut().rotation = 0;
                rotate_button.set_sensitive(false);
                rotate_button.set_tooltip_text(Some("Switch to single-page view to rotate"));
            } else if !two_page_toggle_for_rotate.is_active() {
                rotate_button.set_sensitive(true);
                rotate_button.set_tooltip_text(Some("Rotate page 90°"));
            }
            render();
        });
    }
    {
        let reader = reader.clone();
        let rotate_button = rotate_button.clone();
        let continuous_toggle_for_rotate = continuous_toggle.clone();
        two_page_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                reader.borrow_mut().rotation = 0;
                rotate_button.set_sensitive(false);
                rotate_button.set_tooltip_text(Some("Switch to single-page view to rotate"));
            } else if !continuous_toggle_for_rotate.is_active() {
                rotate_button.set_sensitive(true);
                rotate_button.set_tooltip_text(Some("Rotate page 90°"));
            }
        });
    }
    // Tracks which page is "current" from scroll position alone — connected once, works
    // regardless of whether continuous mode has been built yet (an empty `continuous_offsets`
    // just makes `continuous_page_at` a no-op returning 0). Keeps `r.page`/the page label/
    // prev-next sensitivity live while scrolling, the same things the paged view's `render()`
    // updates on navigation, so "This page"/Progress/Contents-jump-target stay correct no
    // matter which view is currently visible.
    {
        let reader = reader.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        let window_debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        let scroll_for_window = continuous_scroll.clone();
        let reader_for_window = reader.clone();
        continuous_scroll
            .vadjustment()
            .connect_value_changed(move |adj| {
                if let Some(id) = window_debounce.borrow_mut().take() {
                    id.remove();
                }
                {
                    let reader = reader_for_window.clone();
                    let scroll = scroll_for_window.clone();
                    let slot = window_debounce.clone();
                    let id = glib::timeout_add_local_once(
                        std::time::Duration::from_millis(80),
                        move || {
                            slot.borrow_mut().take();
                            refresh_continuous_window(&reader, &scroll, None);
                        },
                    );
                    *window_debounce.borrow_mut() = Some(id);
                }
                let mut r = reader.borrow_mut();
                if r.continuous_offsets.len() < 2 {
                    return;
                }
                let page = continuous_page_at(&r.continuous_offsets, adj.value());
                r.page = page;
                update_page_display(
                    &page_entry,
                    &page_of_label,
                    &prev,
                    &next,
                    &bookmark_button,
                    page,
                    r.count,
                    &r.page_labels,
                    &r.bookmarks,
                );
            });
    }
    {
        let host = host.clone();
        let reader = reader.clone();
        let render = render.clone();
        let continuous_box = continuous_box.clone();
        let continuous_scroll = continuous_scroll.clone();
        let view_stack = view_stack.clone();
        let dialog = reader_window.clone();
        let two_page_toggle = two_page_toggle.clone();
        continuous_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                two_page_toggle.set_active(false);
                build_continuous_view(&host, &reader, &continuous_box, &continuous_scroll, &dialog);
                view_stack.set_visible_child_name("continuous");
                // Deferred to the next idle cycle: the page widgets `build_continuous_view`
                // just added haven't been through a layout pass yet at this point, so the
                // ScrolledWindow's vadjustment `upper` bound is still whatever it was before
                // (typically 0, on the very first activation) — setting the scroll position
                // synchronously here gets silently clamped back to the top. Found live: after
                // resuming a saved page, the page indicator correctly read e.g. "6 of 10" but
                // the visible content was still page 1, every time — continuous mode is the
                // default, so this broke "resume where I left off" for every document.
                let page = reader.borrow().page;
                let reader = reader.clone();
                let continuous_scroll = continuous_scroll.clone();
                glib::idle_add_local_once(move || {
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                });
            } else {
                view_stack.set_visible_child_name("paged");
                render();
            }
        });
        // Continuous scrolling is the default reading mode; `set_active` fires the handler
        // above, which builds the continuous view and switches the stack to it.
        continuous_toggle.set_active(true);
    }
    {
        let render = render.clone();
        let view_stack = view_stack.clone();
        let continuous_toggle = continuous_toggle.clone();
        two_page_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                // Deactivating Continuous (if it was on) runs its own handler above, which
                // switches `view_stack` to "paged" — the single view both single-page and
                // two-page mode share — before `render()` fills `right_picture` too.
                continuous_toggle.set_active(false);
                view_stack.set_visible_child_name("paged");
            }
            render();
        });
    }
}
