use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn install_notes_sidebar(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
    prev: &gtk4::Button,
    next: &gtk4::Button,
    chapter_label: &gtk4::Label,
    bookmark_button: &gtk4::Button,
    pending_scroll: &Rc<RefCell<Option<String>>>,
    notes_rows: &gtk4::Box,
) -> (Rc<dyn Fn()>, Rc<Cell<bool>>) {
    let rebuild_notes_cell: RebuildCell = Rc::new(RefCell::new(None));
    let quiet_notes = Rc::new(Cell::new(false));
    {
        let notes_rows = notes_rows.clone();
        let host = host.clone();
        let reader = reader.clone();
        let view = web_view.clone();
        let prev = prev.clone();
        let next = next.clone();
        let chapter_label = chapter_label.clone();
        let bookmark_button = bookmark_button.clone();
        let pending_scroll = pending_scroll.clone();
        let quiet_notes = quiet_notes.clone();
        let rebuild_notes_cell_inner = rebuild_notes_cell.clone();
        let builder = move || {
            while let Some(child) = notes_rows.first_child() {
                notes_rows.remove(&child);
            }
            let mut all: Vec<fond_annot::Annotation> = reader
                .borrow()
                .store
                .sidecar()
                .annotations
                .iter()
                .filter(|a| a.chapter.is_some())
                .cloned()
                .collect();
            all.sort_by_key(|a| {
                let r = reader.borrow();
                let spine_pos = a
                    .chapter
                    .as_deref()
                    .and_then(|c| r.spine.iter().position(|p| p == c))
                    .unwrap_or(usize::MAX);
                (spine_pos, a.created.clone())
            });
            let bookmarks = reader.borrow().bookmarks.clone();
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
                for &chapter_index in &bookmarks {
                    let row = gtk4::Box::new(Orientation::Horizontal, 6);
                    let label = gtk4::Label::new(Some(&format!("Chapter {}", chapter_index + 1)));
                    label.set_xalign(0.0);
                    label.set_hexpand(true);
                    row.append(&label);
                    let remove_button = gtk4::Button::from_icon_name("user-trash-symbolic");
                    remove_button.add_css_class("flat");
                    remove_button.set_tooltip_text(Some("Remove bookmark"));
                    row.append(&remove_button);
                    {
                        let reader = reader.clone();
                        let view = view.clone();
                        let prev = prev.clone();
                        let next = next.clone();
                        let chapter_label = chapter_label.clone();
                        let bookmark_button = bookmark_button.clone();
                        crate::make_jump(&label, move || {
                            let target = reader
                                .borrow()
                                .spine
                                .get(chapter_index)
                                .cloned()
                                .unwrap_or_default();
                            epub_go_to(
                                &reader,
                                &view,
                                &prev,
                                &next,
                                &chapter_label,
                                &bookmark_button,
                                &target,
                            );
                        });
                    }
                    {
                        let host = host.clone();
                        let reader = reader.clone();
                        let bookmark_button = bookmark_button.clone();
                        let rebuild_notes_cell = rebuild_notes_cell_inner.clone();
                        remove_button.connect_clicked(move |_| {
                            reader
                                .borrow_mut()
                                .bookmarks
                                .retain(|&c| c != chapter_index);
                            let saved: Vec<u32> = reader
                                .borrow()
                                .bookmarks
                                .iter()
                                .map(|&c| c as u32)
                                .collect();
                            host.save_bookmarks(&saved);
                            let current = reader.borrow().index;
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
                let Some(chapter) = annotation.chapter.clone() else {
                    continue;
                };
                let chapter_num = reader
                    .borrow()
                    .spine
                    .iter()
                    .position(|p| p == &chapter)
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let kind_label = match annotation.kind {
                    fond_annot::AnnotationKind::Highlight => "Highlight",
                    fond_annot::AnnotationKind::Underline => "Underline",
                    fond_annot::AnnotationKind::Strikeout => "Strikeout",
                    fond_annot::AnnotationKind::Note => "Note",
                    // AnnotationKind is non_exhaustive from fond-core's next rev
                    _ => "Annotation",
                };
                let outer = gtk4::Box::new(Orientation::Vertical, 2);

                let header_box = gtk4::Box::new(Orientation::Horizontal, 6);
                if let Some(swatch) = color_swatch(annotation.color.as_deref()) {
                    header_box.append(&swatch);
                }
                let place = match crate::page_label_of(&annotation) {
                    Some(label) => format!("p. {label}"),
                    None => format!("Ch. {chapter_num}"),
                };
                let header_label = gtk4::Label::new(Some(&format!("{place} — {kind_label}")));
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
                    let view = view.clone();
                    let prev = prev.clone();
                    let next = next.clone();
                    let chapter_label = chapter_label.clone();
                    let bookmark_button = bookmark_button.clone();
                    let pending_scroll = pending_scroll.clone();
                    let id = annotation.id.clone();
                    let chapter = chapter.clone();
                    crate::make_jump(&header_label, move || {
                        *pending_scroll.borrow_mut() = Some(id.clone());
                        epub_go_to(
                            &reader,
                            &view,
                            &prev,
                            &next,
                            &chapter_label,
                            &bookmark_button,
                            &chapter,
                        );
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
