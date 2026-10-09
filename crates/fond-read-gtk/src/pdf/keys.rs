use super::*;

pub(super) fn install_keys(ui: &PdfUi) {
    let PdfUi {
        reader,
        reader_tab,
        undo,
        redo,
        quick_mark,
        reflow_popover,
        palette,
        view,
        prev,
        next,
        bookmark_button,
        zoom_out,
        zoom_in,
        zoom_fit_width,
        text_toggle,
        search_entry,
        link_back,
        link_forward,
        ..
    } = ui.clone();
    {
        // Registered with the tab host rather than added to `view`: the host catches keys at
        // the window in the capture phase (see `reader_host::set_tab_key_handler`), so
        // navigation works whatever has focus in the reader window, not just widgets inside
        // this tab's own content. Editing `page_entry`/the search entry/a note still needs
        // Left/Right/Home/End/Space for the text cursor, so those pass through below.
        let undo = undo.clone();
        let redo = redo.clone();
        let prev = prev.clone();
        let next = next.clone();
        let reader = reader.clone();
        let view_for_focus = view.clone();
        let bookmark_button = bookmark_button.clone();
        let link_back_key = link_back.clone();
        let link_forward_key = link_forward.clone();
        let zoom_in_key = zoom_in.clone();
        let zoom_out_key = zoom_out.clone();
        let zoom_fit_key = zoom_fit_width.clone();
        let palette_for_keys = palette.clone();
        let quick_mark_for_keys = quick_mark.clone();
        let reflow_popover_for_keys = reflow_popover.clone();
        let text_toggle_key = text_toggle.clone();
        let search_for_keys = search_entry.clone();
        let is_pane = reader.borrow().is_pane;
        let handler = move |keyval: gdk::Key, modifiers: gdk::ModifierType| {
            if !is_pane && focus_in_pane(&view_for_focus) {
                return glib::Propagation::Proceed;
            }
            if (keyval == gdk::Key::z || keyval == gdk::Key::Z)
                && modifiers.contains(gdk::ModifierType::CONTROL_MASK)
            {
                if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
                    redo();
                } else {
                    undo();
                }
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::ALT_MASK)
                && matches!(keyval, gdk::Key::Left | gdk::Key::KP_Left)
                && link_back_key.is_visible()
            {
                link_back_key.emit_clicked();
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::ALT_MASK)
                && matches!(keyval, gdk::Key::Right | gdk::Key::KP_Right)
                && link_forward_key.is_visible()
            {
                link_forward_key.emit_clicked();
                return glib::Propagation::Stop;
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
                    gdk::Key::_0 | gdk::Key::KP_0 => {
                        zoom_fit_key.emit_clicked();
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::f | gdk::Key::F => {
                        search_for_keys.grab_focus();
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            if modifiers.is_empty()
                && matches!(keyval, gdk::Key::t | gdk::Key::T)
                && !view_for_focus
                    .root()
                    .and_then(|root| root.focus())
                    .is_some_and(|w| {
                        w.is::<gtk4::Entry>()
                            || w.is::<gtk4::Text>()
                            || w.is::<gtk4::TextView>() && w.widget_name() != "fond-reflow"
                    })
            {
                text_toggle_key.set_active(!text_toggle_key.is_active());
                return glib::Propagation::Stop;
            }
            let in_reflow = view_for_focus
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|w| w.widget_name() == "fond-reflow");
            if in_reflow {
                if modifiers.is_empty() {
                    let index = match keyval {
                        gdk::Key::_1 => Some(0),
                        gdk::Key::_2 => Some(1),
                        gdk::Key::_3 => Some(2),
                        gdk::Key::_4 => Some(3),
                        _ => None,
                    };
                    if let Some(i) = index {
                        if let Some(mark) = quick_mark_for_keys.borrow().as_ref() {
                            mark(i);
                        }
                        return glib::Propagation::Stop;
                    }
                }
                if matches!(
                    keyval,
                    gdk::Key::Menu | gdk::Key::Return | gdk::Key::KP_Enter
                ) {
                    if let Some(open) = reflow_popover_for_keys.borrow().as_ref() {
                        open();
                        return glib::Propagation::Stop;
                    }
                }
                return glib::Propagation::Proceed;
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
            if modifiers.is_empty() && keyval == gdk::Key::Escape && pin::cancel(&reader) {
                return glib::Propagation::Stop;
            }
            if modifiers.is_empty() && mark_edit::handle_key(&reader, keyval) {
                return glib::Propagation::Stop;
            }
            if crate::focus_owns_activation_keys(&view_for_focus)
                && matches!(
                    keyval,
                    gdk::Key::space
                        | gdk::Key::Up
                        | gdk::Key::Down
                        | gdk::Key::KP_Up
                        | gdk::Key::KP_Down
                )
            {
                return glib::Propagation::Proceed;
            }
            if modifiers.is_empty() {
                let index = match keyval {
                    gdk::Key::_1 => Some(0),
                    gdk::Key::_2 => Some(1),
                    gdk::Key::_3 => Some(2),
                    gdk::Key::_4 => Some(3),
                    _ => None,
                };
                if let Some(i) = index {
                    if reader.borrow().last_selection.is_some() {
                        if let Some(mark) = quick_mark_for_keys.borrow().as_ref() {
                            mark(i);
                            return glib::Propagation::Stop;
                        }
                    }
                    crate::palette::select_color(&palette_for_keys, true, i);
                    return glib::Propagation::Stop;
                }
            }
            // Left/Right, Up/Down, Space/Backspace, and Page_Up/Page_Down reuse the
            // prev/next buttons' own click handlers (continuous-scroll-aware,
            // two-page-spread-aware) via `emit_clicked`, rather than duplicating that logic
            // here. Space/Backspace match the common reader convention (Preview, Acrobat).
            match keyval {
                gdk::Key::Left
                | gdk::Key::Up
                | gdk::Key::Page_Up
                | gdk::Key::BackSpace
                | gdk::Key::KP_Left
                | gdk::Key::KP_Up
                | gdk::Key::KP_Page_Up => {
                    prev.emit_clicked();
                    return glib::Propagation::Stop;
                }
                gdk::Key::Right
                | gdk::Key::Down
                | gdk::Key::Page_Down
                | gdk::Key::space
                | gdk::Key::KP_Right
                | gdk::Key::KP_Down
                | gdk::Key::KP_Page_Down => {
                    next.emit_clicked();
                    return glib::Propagation::Stop;
                }
                gdk::Key::b | gdk::Key::B => {
                    bookmark_button.emit_clicked();
                    return glib::Propagation::Stop;
                }
                gdk::Key::Home | gdk::Key::KP_Home => {
                    jump(&reader, 0, JumpKind::Key);
                    return glib::Propagation::Stop;
                }
                gdk::Key::End | gdk::Key::KP_End => {
                    let last = reader.borrow().count.saturating_sub(1);
                    jump(&reader, last, JumpKind::Key);
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
            glib::Propagation::Proceed
        };
        if is_pane {
            let controller = gtk4::EventControllerKey::new();
            controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
            controller.connect_key_pressed(move |_, key, _, modifiers| handler(key, modifiers));
            ui.view.add_controller(controller);
        } else {
            crate::reader_host::set_tab_key_handler(&reader_tab, handler);
        }
    }
}

/// Whether keyboard focus is inside a split pane, whose own handler should take the key.
fn focus_in_pane(view: &adw::ToolbarView) -> bool {
    let mut widget = view.root().and_then(|root| root.focus());
    while let Some(w) = widget {
        if w.widget_name() == split::PANE_NAME {
            return true;
        }
        widget = w.parent();
    }
    false
}
