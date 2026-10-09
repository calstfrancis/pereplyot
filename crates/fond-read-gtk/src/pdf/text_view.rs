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
        let reflow: Rc<RefCell<Option<Rc<crate::reflow::view::ReadingView>>>> =
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
        let host_for_labels = host.clone();
        let host_for_mode = host.clone();
        text_toggle.connect_toggled(move |btn| {
            host_for_mode.set_reading_mode(btn.is_active());
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
                    let labels = page_labels.clone();
                    let view = crate::reflow::view::ReadingView::new(
                        count,
                        Rc::new(move |page| {
                            labels
                                .get(page as usize)
                                .and_then(|l| l.clone())
                                .unwrap_or_else(|| (page + 1).to_string())
                        }),
                    );
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
                            let selection = view_for_sel.selection().map(|(page, text)| {
                                let quads =
                                    rects_to_quads(&reader, page, &view_for_sel.selection_rects());
                                (page, text, quads)
                            });
                            reader.borrow_mut().last_selection = selection;
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
                "Reading mode: Shift+arrows select, then 1–4 mark it; Ctrl+plus/minus resize the text",
            );

            {
                let reader_for_marks = reader.clone();
                let view_for_marks = view.clone();
                *view.on_content.borrow_mut() = Some(Rc::new(move || {
                    paint_text_marks(&reader_for_marks, &view_for_marks);
                }));
                // A mark added, changed, removed or undone repaints here as it does on the page.
                let store = reader.borrow().store.clone();
                let reader_for_store = reader.clone();
                let view_for_store = view.clone();
                store.subscribe(move |_, _| paint_text_marks(&reader_for_store, &view_for_store));
                let host = host.clone();
                let view_for_ocr = view.clone();
                let path_for_ocr = reader.borrow().path.clone();
                *view.on_no_text.borrow_mut() = Some(Rc::new(move || {
                    if crate::reflow::ocr::tesseract().is_none() {
                        host.notify(
                            "This document has no text layer. Install Tesseract to recognise \
                             its text and read it in Reading mode.",
                        );
                        return;
                    }
                    let view = view_for_ocr.clone();
                    let path = path_for_ocr.clone();
                    let host_progress = host.clone();
                    host.notify_action(
                        "This document is a scan with no text layer.",
                        "Recognise text",
                        Rc::new(move || {
                            host_progress.notify(
                                "Recognising text — pages appear as they are done, and the \
                                 results are kept for next time",
                            );
                            view.reset_empty();
                            view.load_with(path.clone(), true);
                        }),
                    );
                }));
                let host_click = host_for_labels.clone();
                let reader_click = reader.clone();
                let text_toggle_click = btn.clone();
                *view.on_page_label_click.borrow_mut() = Some(Rc::new(move |page, parent| {
                    show_page_label_menu(
                        &host_click,
                        &reader_click,
                        &text_toggle_click,
                        page,
                        parent,
                    );
                }));
                view.load(reader.borrow().path.clone());
            }
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
    if host.reading_mode() {
        let text_toggle = text_toggle.clone();
        glib::idle_add_local_once(move || text_toggle.set_active(true));
    }
}

/// The small menu on a page number in the Reading view's left margin: cite the page, or go to the
/// original page image.
fn show_page_label_menu(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    text_toggle: &gtk4::ToggleButton,
    page: u16,
    parent: &gtk4::Widget,
) {
    let popover = gtk4::Popover::new();
    popover.set_parent(parent);
    let rows = gtk4::Box::new(Orientation::Vertical, 2);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);
    let label = {
        let r = reader.borrow();
        r.page_labels
            .get(page as usize)
            .and_then(|l| l.clone())
            .unwrap_or_else(|| (page + 1).to_string())
    };
    let cite = popover_button(&format!("Copy “p. {label}” citation"), false);
    {
        let host = host.clone();
        let popover = popover.clone();
        let label = label.clone();
        cite.connect_clicked(move |_| {
            popover.popdown();
            copy_to_clipboard(&host, &format!("p. {label}"));
        });
    }
    rows.append(&cite);
    let original = popover_button("Show the original page", false);
    {
        let popover = popover.clone();
        let reader = reader.clone();
        let text_toggle = text_toggle.clone();
        original.connect_clicked(move |_| {
            popover.popdown();
            reader.borrow_mut().page = page;
            text_toggle.set_active(false);
        });
    }
    rows.append(&original);
    popover.set_child(Some(&rows));
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}
