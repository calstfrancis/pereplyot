use super::*;

pub(super) fn install_keys(ui: &EpubUi) {
    let EpubUi {
        web_view,
        prev,
        next,
        bookmark_button,
        reader_tab,
        search_toggle,
        zoom_in_button,
        zoom_out_button,
        epub_undo,
        epub_redo,
        page_turn,
        reader,
        ..
    } = ui;
    {
        // Registered with the tab host, which catches keys at the window in the capture
        // phase (see `reader_host::set_tab_key_handler`) — ahead of the WebView (which owns
        // focus whenever the reader isn't showing chrome) and any focused header control,
        // either of which would otherwise consume Left/Right for in-page scroll or focus
        // navigation. The search entry still needs normal Left/Right/cursor behavior while
        // it has focus, so that's explicitly passed through below rather than intercepted.
        let epub_undo = epub_undo.clone();
        let epub_redo = epub_redo.clone();
        let search_toggle = search_toggle.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        let view_for_focus = web_view.clone();
        let page_turn = page_turn.clone();
        let reader = reader.clone();
        let zoom_in_key = zoom_in_button.clone();
        let zoom_out_key = zoom_out_button.clone();
        crate::reader_host::set_tab_key_handler(reader_tab, move |keyval, modifiers| {
            if (keyval == gdk::Key::z || keyval == gdk::Key::Z)
                && modifiers.contains(gdk::ModifierType::CONTROL_MASK)
            {
                if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
                    epub_redo();
                } else {
                    epub_undo();
                }
                return glib::Propagation::Stop;
            }
            if keyval == gdk::Key::f && modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                search_toggle.set_active(!search_toggle.is_active());
                return glib::Propagation::Stop;
            }
            if keyval == gdk::Key::Escape && search_toggle.is_active() {
                search_toggle.set_active(false);
                return glib::Propagation::Stop;
            }
            let focus_in_text_entry = view_for_focus
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|w| {
                    w.is::<gtk4::Entry>() || w.is::<gtk4::Text>() || w.is::<gtk4::TextView>()
                });
            if focus_in_text_entry {
                return glib::Propagation::Proceed;
            }
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                match keyval {
                    gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => {
                        zoom_in_key.emit_clicked();
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::minus | gdk::Key::KP_Subtract => {
                        zoom_out_key.emit_clicked();
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            // Paginated: the keys turn pages, and a chapter ends only when its pages do.
            if reader.borrow().paginated && !modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
                match keyval {
                    gdk::Key::Right
                    | gdk::Key::KP_Right
                    | gdk::Key::Page_Down
                    | gdk::Key::KP_Page_Down => {
                        page_turn(1);
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::space if !shift => {
                        page_turn(1);
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::Left
                    | gdk::Key::KP_Left
                    | gdk::Key::Page_Up
                    | gdk::Key::KP_Page_Up => {
                        page_turn(-1);
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::space => {
                        page_turn(-1);
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            // Prev/next chapter — reuses the prev/next buttons' own handlers via
            // `emit_clicked` rather than duplicating their chapter-boundary logic.
            match keyval {
                gdk::Key::Left | gdk::Key::KP_Left => {
                    prev.emit_clicked();
                    return glib::Propagation::Stop;
                }
                gdk::Key::Right | gdk::Key::KP_Right => {
                    next.emit_clicked();
                    return glib::Propagation::Stop;
                }
                gdk::Key::b | gdk::Key::B => {
                    bookmark_button.emit_clicked();
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
            glib::Propagation::Proceed
        });
    }
}
