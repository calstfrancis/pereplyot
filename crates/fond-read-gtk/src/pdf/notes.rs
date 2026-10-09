use super::*;

pub(super) fn install_notes_sidebar(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    bookmark_button: &gtk4::Button,
    notes_rows: &gtk4::Box,
) -> (Rc<dyn Fn()>, Rc<Cell<bool>>) {
    let rebuild_notes_cell: RebuildNotesCell = Rc::new(RefCell::new(None));
    let quiet_notes = Rc::new(Cell::new(false));
    {
        let notes_rows = notes_rows.clone();
        let host = host.clone();
        let reader = reader.clone();
        let bookmark_button = bookmark_button.clone();
        let rebuild_notes_cell_inner = rebuild_notes_cell.clone();
        let quiet_notes = quiet_notes.clone();
        let builder = move || {
            while let Some(child) = notes_rows.first_child() {
                notes_rows.remove(&child);
            }
            let bookmarks = reader.borrow().bookmarks.clone();
            let mut all: Vec<fond_annot::Annotation> = reader
                .borrow()
                .store
                .sidecar()
                .annotations
                .iter()
                .filter(|a| a.page.is_some())
                .cloned()
                .collect();
            all.sort_by(|a, b| (a.page, &a.created).cmp(&(b.page, &b.created)));
            if all.is_empty() && bookmarks.is_empty() {
                let label = gtk4::Label::new(Some("No bookmarks, notes, or highlights yet"));
                label.add_css_class("dim-label");
                label.set_margin_top(6);
                label.set_margin_bottom(6);
                notes_rows.append(&label);
                return;
            }
            if !bookmarks.is_empty() {
                let heading = gtk4::Label::new(Some("Bookmarks"));
                heading.add_css_class("dim-label");
                heading.add_css_class("caption-heading");
                heading.set_xalign(0.0);
                notes_rows.append(&heading);
                for &page_num in &bookmarks {
                    let printed = reader
                        .borrow()
                        .page_labels
                        .get((page_num as usize).saturating_sub(1))
                        .and_then(|l| l.clone());
                    let page_label = printed.unwrap_or_else(|| page_num.to_string());
                    let row = gtk4::Box::new(Orientation::Horizontal, 6);
                    let label = gtk4::Label::new(Some(&format!("p.{page_label}")));
                    label.set_xalign(0.0);
                    label.set_hexpand(true);
                    row.append(&label);
                    let remove_button = gtk4::Button::from_icon_name("user-trash-symbolic");
                    remove_button.add_css_class("flat");
                    remove_button.set_tooltip_text(Some("Remove bookmark"));
                    row.append(&remove_button);
                    {
                        let reader = reader.clone();
                        crate::make_jump(&label, move || {
                            let target = (page_num.saturating_sub(1))
                                .min(reader.borrow().count.saturating_sub(1) as u32)
                                as u16;
                            jump(&reader, target, JumpKind::Annotation);
                        });
                    }
                    {
                        let host = host.clone();
                        let reader = reader.clone();
                        let bookmark_button = bookmark_button.clone();
                        let rebuild_notes_cell = rebuild_notes_cell_inner.clone();
                        remove_button.connect_clicked(move |_| {
                            reader.borrow_mut().bookmarks.retain(|&p| p != page_num);
                            host.save_bookmarks(&reader.borrow().bookmarks);
                            let current = reader.borrow().page as u32 + 1;
                            update_bookmark_button(
                                &bookmark_button,
                                reader.borrow().bookmarks.contains(&current),
                            );
                            if let Some(f) = rebuild_notes_cell.borrow().as_ref() {
                                f();
                            }
                        });
                    }
                    notes_rows.append(&row);
                }
                notes_rows.append(&popover_separator());
            }
            if all.is_empty() {
                return;
            }
            let last = all.len().saturating_sub(1);
            for (i, annotation) in all.into_iter().enumerate() {
                let page_num = annotation.page.unwrap_or(1);
                // Same printed-label lookup the page-number entry itself uses (`page_labels`
                // is 0-based, `page_num` is the annotation's raw 1-based file page).
                let printed = reader
                    .borrow()
                    .page_labels
                    .get((page_num as usize).saturating_sub(1))
                    .and_then(|l| l.clone());
                let page_label = printed.unwrap_or_else(|| page_num.to_string());
                let outer = gtk4::Box::new(Orientation::Vertical, 2);

                let header_box = gtk4::Box::new(Orientation::Horizontal, 6);
                if let Some(swatch) = color_swatch(annotation.color.as_deref()) {
                    header_box.append(&swatch);
                }
                let header_label =
                    gtk4::Label::new(Some(&format!("p.{page_label} — {:?}", annotation.kind)));
                header_label.set_xalign(0.0);
                header_label.set_hexpand(true);
                header_label.add_css_class("dim-label");
                header_label.add_css_class("caption-heading");
                header_box.append(&header_label);
                let delete_button = gtk4::Button::from_icon_name("user-trash-symbolic");
                delete_button.add_css_class("flat");
                delete_button.set_tooltip_text(Some("Delete this annotation"));
                header_box.append(&delete_button);
                outer.append(&header_box);

                {
                    let reader = reader.clone();
                    crate::make_jump(&header_label, move || {
                        let target = (page_num.saturating_sub(1))
                            .min(reader.borrow().count.saturating_sub(1) as u32)
                            as u16;
                        jump(&reader, target, JumpKind::Annotation);
                    });
                }

                if let Some(snippet) = &annotation.snippet {
                    let snippet_label = gtk4::Label::new(Some(snippet));
                    snippet_label.set_xalign(0.0);
                    snippet_label.set_wrap(true);
                    snippet_label.add_css_class("dim-label");
                    snippet_label.add_css_class("caption");
                    outer.append(&snippet_label);
                }

                let save_note = {
                    let host = host.clone();
                    let reader = reader.clone();
                    let quiet_notes = quiet_notes.clone();
                    let id = annotation.id.clone();
                    move |text: &str| {
                        let text = text.trim();
                        let store = reader.borrow().store.clone();
                        quiet_notes.set(true);
                        let result = store.update(&id, |a| {
                            a.note = (!text.is_empty()).then(|| text.to_string());
                        });
                        quiet_notes.set(false);
                        if let Err(e) = result {
                            host.notify(&e);
                        }
                    }
                };
                let note_widget =
                    note_edit_widget(annotation.note.as_deref(), move |text| save_note(&text));
                outer.append(&note_widget);

                {
                    let host = host.clone();
                    let reader = reader.clone();
                    let id = annotation.id.clone();
                    delete_button.connect_clicked(move |_| {
                        let store = reader.borrow().store.clone();
                        match store.remove(&id) {
                            Ok(_) => host.notify("Annotation deleted"),
                            Err(e) => host.notify(&e),
                        }
                    });
                }

                notes_rows.append(&outer);
                if i != last {
                    notes_rows.append(&popover_separator());
                }
            }
        };
        *rebuild_notes_cell.borrow_mut() = Some(Rc::new(builder));
    }
    let rebuild_notes: Rc<dyn Fn()> = {
        let cell = rebuild_notes_cell.clone();
        Rc::new(move || {
            let f = cell.borrow().clone();
            if let Some(f) = f {
                f();
            }
        })
    };
    (rebuild_notes, quiet_notes)
}
