use super::*;

/// The geometry a right-click needs to hit-test against a page's annotations and, if it
/// lands on one, resolve back to PDF space for the popover's own bookkeeping. Grouped like
/// `DragGeometry` for the same reason: fewer loose parameters on `show_pdf_context_menu`.
pub(super) struct ClickGeometry {
    pub(super) render_w: u32,
    pub(super) render_h: u32,
    pub(super) page: Option<PageGeom>,
    pub(super) click_x: f64,
    pub(super) click_y: f64,
}

/// Right-click context menu for the PDF page: if the click landed on an existing
/// highlight/underline/strikeout/note, offers to edit its note text or delete it; otherwise
/// offers to add a new marginal note. Replaces the old "This page" dropdown — editing and
/// deleting now happens at the annotation itself instead of a separate list.
#[allow(clippy::too_many_arguments)]
pub(super) fn show_pdf_context_menu(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    parent: &gtk4::Picture,
    page: u16,
    geom: ClickGeometry,
    reader_window: &adw::Window,
) {
    let ClickGeometry {
        render_w,
        render_h,
        page: page_geom,
        click_x,
        click_y,
    } = geom;

    let hit_id = page_geom
        .filter(|_| render_w > 0 && render_h > 0)
        .map(|g| g.px_to_pdf(click_x, click_y, render_w as f64, render_h as f64))
        .and_then(|(x_pt, y_pt)| {
            annotation_at_pdf_point(
                &reader.borrow().store.sidecar(),
                page,
                x_pt as f32,
                y_pt as f32,
            )
        });

    let popover = gtk4::Popover::new();
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        click_x.round() as i32,
        click_y.round() as i32,
        1,
        1,
    )));
    popover.set_has_arrow(true);

    let rows = gtk4::Box::new(Orientation::Vertical, 2);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);
    rows.set_width_request(240);

    let selection_here = reader
        .borrow()
        .last_selection
        .as_ref()
        .filter(|(sel_page, _, _)| *sel_page == page)
        .map(|(_, text, _)| text.clone());
    if let Some(text) = selection_here.clone() {
        let copy = popover_button("Copy selected text", false);
        let host = host.clone();
        let popover = popover.clone();
        copy.connect_clicked(move |_| {
            copy_to_clipboard(&host, &text);
            popover.popdown();
        });
        rows.append(&copy);
    }

    match hit_id {
        Some(id) => {
            let annotation = reader
                .borrow()
                .store
                .sidecar()
                .annotations
                .iter()
                .find(|a| a.id == id)
                .cloned();
            let Some(annotation) = annotation else {
                return;
            };

            let kind_row = gtk4::Box::new(Orientation::Horizontal, 6);
            if let Some(swatch) = color_swatch(annotation.color.as_deref()) {
                kind_row.append(&swatch);
            }
            let kind_label = gtk4::Label::new(Some(&format!("{:?}", annotation.kind)));
            kind_label.set_xalign(0.0);
            kind_label.add_css_class("dim-label");
            kind_row.append(&kind_label);
            if selection_here.is_some() {
                rows.append(&popover_separator());
            }
            rows.append(&kind_row);

            // Re-read from the page rather than trusting `snippet`, which for annotations
            // saved before `selection_text` existed has its words run together.
            let marked_text = selection_text(&reader.borrow(), page, &annotation.quadpoints)
                .or_else(|| annotation.snippet.clone())
                .filter(|t| !t.trim().is_empty());
            if let Some(text) = marked_text {
                let label = match annotation.kind {
                    fond_annot::AnnotationKind::Underline => "Copy underlined text",
                    fond_annot::AnnotationKind::Strikeout => "Copy struck-out text",
                    fond_annot::AnnotationKind::Note => "Copy noted text",
                    _ => "Copy highlighted text",
                };
                let copy = popover_button(label, false);
                let host = host.clone();
                let popover = popover.clone();
                copy.connect_clicked(move |_| {
                    copy_to_clipboard(&host, &text);
                    popover.popdown();
                });
                rows.append(&copy);
            }

            let save_note = {
                let host = host.clone();
                let reader = reader.clone();
                let id = id.clone();
                move |text: &str| {
                    let text = text.trim();
                    let store = reader.borrow().store.clone();
                    let result = store.update(&id, |a| {
                        a.note = (!text.is_empty()).then(|| text.to_string());
                    });
                    if let Err(e) = result {
                        host.notify(&e);
                    }
                }
            };
            let note_widget =
                note_edit_widget(annotation.note.as_deref(), move |text| save_note(&text));
            rows.append(&note_widget);

            rows.append(&popover_separator());
            let delete_button = popover_button("Delete annotation", true);
            {
                let host = host.clone();
                let reader = reader.clone();
                let popover = popover.clone();
                let id = id.clone();
                delete_button.connect_clicked(move |_| {
                    let store = reader.borrow().store.clone();
                    match store.remove(&id) {
                        Ok(_) => host.notify("Annotation deleted"),
                        Err(e) => host.notify(&e),
                    }
                    popover.popdown();
                });
            }
            rows.append(&delete_button);
        }
        None => {
            // "Create note from selection…" instead of the generic "Add note here" when
            // there's an active "Select text" drag on this page (see `last_selection`) —
            // same underlying dialog either way (it already pre-fills from the selection and
            // carries its quadpoints when present), just a label that says what's actually
            // about to happen instead of always the generic one.
            let has_selection_here = selection_here.is_some();
            let add_note = popover_button(
                if has_selection_here {
                    "Create note from selection…"
                } else {
                    "Add note here"
                },
                false,
            );
            {
                let host = host.clone();
                let reader = reader.clone();
                let popover = popover.clone();
                let reader_window = reader_window.clone();
                add_note.connect_clicked(move |_| {
                    show_pdf_note_dialog(&host, &reader, &reader_window);
                    popover.popdown();
                });
            }
            rows.append(&add_note);
        }
    }

    popover.set_child(Some(&rows));
    let parent = parent.clone();
    popover.connect_closed(move |p| {
        p.unparent();
        crate::grab_focus_keeping_scroll(&parent);
    });
    popover.popup();
}
