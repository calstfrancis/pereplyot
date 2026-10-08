use super::*;

pub(super) fn install_text_view(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        reader_window,
        reflow_popover,
        prev,
        next,
        page_entry,
        page_of_label,
        bookmark_button,
        continuous_toggle,
        text_toggle,
        hint,
        continuous_scroll,
        view_stack,
        ..
    } = ui.clone();
    // Text view: the document's text reflowed into a text widget (see `pdf_text`).
    {
        let reflow: Rc<RefCell<Option<Rc<crate::pdf_text::ReflowView>>>> =
            Rc::new(RefCell::new(None));
        let host = host.clone();
        let reader = reader.clone();
        let view_stack = view_stack.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let reader_window = reader_window.clone();
        let hint = hint.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        let reflow_popover = reflow_popover.clone();
        text_toggle.connect_toggled(move |btn| {
            if !btn.is_active() {
                {
                    let mut r = reader.borrow_mut();
                    r.text_goto = None;
                    r.text_zoom = None;
                }
                view_stack.set_visible_child_name("continuous");
                let page = reader.borrow().page;
                let reader = reader.clone();
                let continuous_scroll = continuous_scroll.clone();
                glib::idle_add_local_once(move || {
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                });
                hint.set_text("Drag over text to select it, then choose a colour (or press 1–4)");
                return;
            }
            // Page navigation everywhere funnels through the continuous view's scroller, so it
            // stays the mode underneath and the text view takes over its showing.
            if !continuous_toggle.is_active() {
                continuous_toggle.set_active(true);
            }
            let (count, page_labels) = {
                let r = reader.borrow();
                (r.count, r.page_labels.clone())
            };
            let existing = reflow.borrow().clone();
            let view = match existing {
                Some(v) => v,
                None => {
                    let view = crate::pdf_text::ReflowView::new(count);
                    view_stack.add_named(&view.scroll, Some("text"));
                    *reflow.borrow_mut() = Some(view.clone());

                    let make_ctx: Rc<dyn Fn() -> MarkCtx> = {
                        let host = host.clone();
                        let reader = reader.clone();
                        let reader_window = reader_window.clone();
                        Rc::new(move || MarkCtx {
                            host: host.clone(),
                            reader: reader.clone(),
                            reader_window: reader_window.clone(),
                        })
                    };

                    // Follow the user's scrolling so the page counter, bookmarks and
                    // sidebars stay in step.
                    {
                        let reader = reader.clone();
                        let view_for_scroll = view.clone();
                        let page_entry = page_entry.clone();
                        let page_of_label = page_of_label.clone();
                        let prev = prev.clone();
                        let next = next.clone();
                        let bookmark_button = bookmark_button.clone();
                        view.scroll.vadjustment().connect_value_changed(move |_| {
                            if view_for_scroll.loaded_pages() == 0 {
                                return;
                            }
                            let page = view_for_scroll.visible_page();
                            let mut r = reader.borrow_mut();
                            if r.page != page && r.text_goto.is_some() {
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
                            }
                        });
                    }
                    // A selection in the text becomes the thing the 1–4 keys and the popover act
                    // on; refreshed on every cursor move so extending it with Shift+arrows counts.
                    {
                        let reader = reader.clone();
                        let view_for_sel = view.clone();
                        let sync: Rc<dyn Fn()> = Rc::new(move || {
                            if reader.borrow().text_goto.is_none() {
                                return;
                            }
                            let selection = view_for_sel.selection();
                            reader.borrow_mut().last_selection =
                                selection.map(|(page, text)| (page, text, Vec::new()));
                        });
                        let buffer = view.text_view.buffer();
                        {
                            let sync = sync.clone();
                            buffer.connect_has_selection_notify(move |_| sync());
                        }
                        buffer.connect_cursor_position_notify(move |_| sync());
                    }
                    // Mouse: popover when a drag-selection ends. Keyboard: Menu or Enter.
                    {
                        let pointer: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((0.0, 0.0)));
                        let motion = gtk4::EventControllerMotion::new();
                        motion.set_propagation_phase(gtk4::PropagationPhase::Capture);
                        {
                            let pointer = pointer.clone();
                            motion.connect_motion(move |_, x, y| pointer.set((x, y)));
                        }
                        view.text_view.add_controller(motion);

                        let drag = gtk4::GestureDrag::new();
                        drag.set_button(gdk::BUTTON_PRIMARY);
                        drag.set_propagation_phase(gtk4::PropagationPhase::Capture);
                        let make_ctx_drag = make_ctx.clone();
                        let view_drag = view.clone();
                        drag.connect_drag_end(move |_, _, _| {
                            let (x, y) = pointer.get();
                            let make_ctx = make_ctx_drag.clone();
                            let view = view_drag.clone();
                            glib::timeout_add_local_once(
                                std::time::Duration::from_millis(60),
                                move || {
                                    if let Some((page, _)) = view.selection() {
                                        show_selection_popover(
                                            &make_ctx(),
                                            &view.text_view,
                                            x,
                                            y,
                                            page,
                                        );
                                    }
                                },
                            );
                        });
                        view.text_view.add_controller(drag);

                        let make_ctx_key = make_ctx.clone();
                        let view_key = view.clone();
                        *reflow_popover.borrow_mut() = Some(Rc::new(move || {
                            if let Some((page, _)) = view_key.selection() {
                                let (x, y) = view_key.selection_anchor();
                                show_selection_popover(
                                    &make_ctx_key(),
                                    &view_key.text_view,
                                    x as f64,
                                    y as f64,
                                    page,
                                );
                            }
                        }));
                    }
                    view
                }
            };

            // Zoom steps resize the text instead of re-rendering pages; navigation redirects here.
            {
                let view_zoom = view.clone();
                let view_goto = view.clone();
                let reader_for_goto = reader.clone();
                let page_entry = page_entry.clone();
                let page_of_label = page_of_label.clone();
                let prev = prev.clone();
                let next = next.clone();
                let bookmark_button = bookmark_button.clone();
                let mut r = reader.borrow_mut();
                r.text_zoom = Some(Rc::new(move |factor| view_zoom.scale_font(factor)));
                r.text_goto = Some(Rc::new(move |page| {
                    view_goto.scroll_to_page(page);
                    let mut r = reader_for_goto.borrow_mut();
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
                }));
            }
            hint.set_text(
                "Text view: Shift+arrows select, then 1–4 mark it; Ctrl+plus/minus resize the text",
            );

            let reader_for_text = reader.clone();
            let page_labels_text = page_labels.clone();
            view.start_loading(
                Rc::new({
                    let reader = reader_for_text.clone();
                    move |page| {
                        let r = reader.borrow();
                        r.doc
                            .as_ref()
                            .and_then(|d| {
                                let page = d.pages().get(page).ok()?;
                                Some(crate::page_text(&page))
                            })
                            .unwrap_or_default()
                    }
                }),
                Rc::new(move |page| {
                    page_labels_text
                        .get(page as usize)
                        .and_then(|l| l.clone())
                        .unwrap_or_else(|| (page + 1).to_string())
                }),
                Rc::new({
                    let view = view.clone();
                    let reader = reader.clone();
                    move |loaded| {
                        if loaded == count {
                            paint_text_marks(&reader, &view);
                        }
                    }
                }),
            );
            view_stack.set_visible_child_name("text");
            let start_page = reader.borrow().page;
            let view_jump = view.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
                if view_jump.loaded_pages() > start_page {
                    view_jump.scroll_to_page(start_page);
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
            view.text_view.grab_focus();
        });
    }
}
