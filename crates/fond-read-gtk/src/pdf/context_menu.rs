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

    let click_point = page_geom
        .filter(|_| render_w > 0 && render_h > 0)
        .map(|g| g.px_to_pdf(click_x, click_y, render_w as f64, render_h as f64))
        .map(|(x, y)| [x, y]);
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

            if shapes::shape_of(&annotation) != shapes::Shape::Sticky {
                rows.append(&colour_row(reader, &popover, &annotation));
            }
            if shapes::shape_of(&annotation) == shapes::Shape::Text
                && annotation.kind != fond_annot::AnnotationKind::Note
            {
                rows.append(&kind_buttons(reader, &popover, &annotation));
            }
            let overlapping = overlapping_ids(&reader.borrow(), page, &annotation);
            if !overlapping.is_empty() {
                let merge = popover_button(
                    &format!(
                        "Merge with {} overlapping mark{}",
                        overlapping.len(),
                        if overlapping.len() == 1 { "" } else { "s" }
                    ),
                    false,
                );
                let host = host.clone();
                let reader = reader.clone();
                let popover = popover.clone();
                let id = id.clone();
                merge.connect_clicked(move |_| {
                    popover.popdown();
                    merge_marks(&host, &reader, page, &id, &overlapping);
                });
                rows.append(&merge);
            }

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
            rows.append(&tag_chips(reader, &popover, &annotation));

            let doc = reader.borrow().doc_ref.clone();
            if let Some(doc) = doc {
                let label = annotation
                    .page
                    .and_then(|p| {
                        reader
                            .borrow()
                            .page_labels
                            .get((p as usize).saturating_sub(1))
                            .cloned()
                            .flatten()
                    })
                    .unwrap_or_else(|| annotation.page.unwrap_or(0).to_string());
                rows.append(&popover_separator());
                let to_notebook = popover_button("Add to notebook", false);
                {
                    let host = host.clone();
                    let popover = popover.clone();
                    let quote = crate::notebook_ui::quote_of(&doc, &label, &annotation);
                    to_notebook.connect_clicked(move |_| {
                        popover.popdown();
                        crate::notebook_ui::add_to_notebook(quote.clone(), &|m| host.notify(m));
                    });
                }
                rows.append(&to_notebook);
                let pending = crate::connections::pending();
                let end = crate::notebook_ui::end_of(&doc, &label, &annotation);
                let connect_label = match &pending {
                    Some(p) if p.is(&end.hash, &end.id) => "Cancel connecting".to_string(),
                    Some(p) => format!("Connect to {}", p.whence()),
                    None => "Connect to another annotation…".to_string(),
                };
                let connect = popover_button(&connect_label, false);
                {
                    let host = host.clone();
                    let popover = popover.clone();
                    let window = reader_window.clone();
                    connect.connect_clicked(move |_| {
                        popover.popdown();
                        let host = host.clone();
                        crate::notebook_ui::connect::press(
                            Some(window.upcast_ref::<gtk4::Window>()),
                            end.clone(),
                            Rc::new(move |m| host.notify(m)),
                        );
                    });
                }
                rows.append(&connect);
            }

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
                    "Add a note here"
                },
                false,
            );
            {
                let host = host.clone();
                let reader = reader.clone();
                let popover = popover.clone();
                let reader_window = reader_window.clone();
                add_note.connect_clicked(move |_| {
                    show_pdf_note_dialog(&host, &reader, &reader_window, click_point);
                    popover.popdown();
                });
            }
            rows.append(&add_note);
        }
    }

    let pin_row = popover_button("Pin a region…", false);
    {
        let reader = reader.clone();
        let popover = popover.clone();
        pin_row.connect_clicked(move |_| {
            popover.popdown();
            pin::begin(&reader);
        });
    }
    rows.append(&popover_separator());
    rows.append(&pin_row);

    popover.set_child(Some(&rows));
    let parent = parent.clone();
    popover.connect_closed(move |p| {
        p.unparent();
        crate::grab_focus_keeping_scroll(&parent);
    });
    popover.popup();
}

/// A row of the palette's colours; choosing one recolours the mark.
fn colour_row(
    reader: &Rc<RefCell<ReaderState>>,
    popover: &gtk4::Popover,
    annotation: &fond_annot::Annotation,
) -> gtk4::Widget {
    let row = gtk4::Box::new(Orientation::Horizontal, 2);
    row.set_margin_top(2);
    for colour in crate::palette::HIGHLIGHT_COLORS.iter() {
        let button = gtk4::Button::new();
        button.add_css_class("flat");
        button.set_tooltip_text(Some(colour.hex));
        if let Some(swatch) = color_swatch(Some(colour.hex)) {
            button.set_child(Some(&swatch));
        }
        if annotation
            .color
            .as_deref()
            .is_some_and(|c| c.eq_ignore_ascii_case(colour.hex))
        {
            button.add_css_class("suggested-action");
        }
        let (reader, popover, id, hex) = (
            reader.clone(),
            popover.clone(),
            annotation.id.clone(),
            colour.hex.to_string(),
        );
        button.connect_clicked(move |_| {
            popover.popdown();
            let store = reader.borrow().store.clone();
            let _ = store.update(&id, |a| a.color = Some(hex.clone()));
        });
        row.append(&button);
    }
    row.upcast()
}

/// Highlight, Underline and Strikeout as buttons; choosing one changes the mark's kind.
fn kind_buttons(
    reader: &Rc<RefCell<ReaderState>>,
    popover: &gtk4::Popover,
    annotation: &fond_annot::Annotation,
) -> gtk4::Widget {
    let row = gtk4::Box::new(Orientation::Horizontal, 0);
    row.add_css_class("linked");
    row.set_margin_top(2);
    for (label, kind) in MARK_KIND_OPTIONS.iter().take(3) {
        let button = gtk4::ToggleButton::with_label(label);
        button.set_active(annotation.kind == *kind);
        let (reader, popover, id, kind) = (
            reader.clone(),
            popover.clone(),
            annotation.id.clone(),
            *kind,
        );
        button.connect_clicked(move |_| {
            popover.popdown();
            let store = reader.borrow().store.clone();
            let _ = store.update(&id, |a| a.kind = kind);
        });
        row.append(&button);
    }
    row.upcast()
}

fn bounds_of(quads: &[[f64; 8]]) -> Option<[f64; 4]> {
    let mut out: Option<[f64; 4]> = None;
    for q in quads {
        let xs = [q[0], q[2], q[4], q[6]];
        let ys = [q[1], q[3], q[5], q[7]];
        let b = [
            xs.iter().cloned().fold(f64::INFINITY, f64::min),
            ys.iter().cloned().fold(f64::INFINITY, f64::min),
            xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        ];
        out = Some(match out {
            Some(o) => [
                o[0].min(b[0]),
                o[1].min(b[1]),
                o[2].max(b[2]),
                o[3].max(b[3]),
            ],
            None => b,
        });
    }
    out
}

fn quads_touch(a: &[[f64; 8]], b: &[[f64; 8]]) -> bool {
    a.iter().any(|qa| {
        let ra = bounds_of(std::slice::from_ref(qa));
        b.iter().any(|qb| {
            let rb = bounds_of(std::slice::from_ref(qb));
            matches!((ra, rb), (Some(x), Some(y))
                if x[0] <= y[2] && y[0] <= x[2] && x[1] <= y[3] && y[1] <= x[3])
        })
    })
}

/// Other text marks on `page` whose lines cross those of `annotation`.
fn overlapping_ids(r: &ReaderState, page: u16, annotation: &fond_annot::Annotation) -> Vec<String> {
    if annotation.quadpoints.is_empty() {
        return Vec::new();
    }
    r.store
        .sidecar()
        .annotations
        .iter()
        .filter(|a| {
            a.id != annotation.id
                && a.page == Some(page as u32 + 1)
                && shapes::shape_of(a) == shapes::Shape::Text
                && quads_touch(&annotation.quadpoints, &a.quadpoints)
        })
        .map(|a| a.id.clone())
        .collect()
}

/// Fold the marks `others` into `id`: all their lines, their notes and tags, one mark.
fn merge_marks(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    id: &str,
    others: &[String],
) {
    let store = reader.borrow().store.clone();
    let mut quads: Vec<[f64; 8]> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    for a in std::iter::once(id.to_string())
        .chain(others.iter().cloned())
        .filter_map(|i| store.get(&i))
    {
        for q in &a.quadpoints {
            if !quads.contains(q) {
                quads.push(*q);
            }
        }
        if let Some(n) = a.note.as_deref().filter(|n| !n.trim().is_empty()) {
            if !notes.iter().any(|x| x == n) {
                notes.push(n.to_string());
            }
        }
        for t in a.explicit_tags() {
            if !tags.contains(&t) {
                tags.push(t);
            }
        }
    }
    quads.sort_by(|a, b| {
        let key = |q: &[f64; 8]| (-q[1].max(q[3]).max(q[5]).max(q[7]), q[0].min(q[2]));
        key(a)
            .partial_cmp(&key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let snippet = selection_text(&reader.borrow(), page, &quads);
    let merged = store.group(|| {
        store.update(id, |a| {
            a.quadpoints = quads.clone();
            if snippet.is_some() {
                a.snippet = snippet.clone();
            }
            a.note = (!notes.is_empty()).then(|| notes.join("\n\n"));
            a.set_explicit_tags(&tags);
        })?;
        for other in others {
            let _ = store.remove(other);
        }
        Ok::<(), String>(())
    });
    match merged {
        Ok(()) => host.notify("Marks merged (Ctrl+Z undoes it in one step)"),
        Err(e) => host.notify(&e),
    }
}

/// The tags already in use, to put on this mark with one click (a new tag is typed as #tag in
/// the note), and the ones it has, to take off.
fn tag_chips(
    reader: &Rc<RefCell<ReaderState>>,
    popover: &gtk4::Popover,
    annotation: &fond_annot::Annotation,
) -> gtk4::Widget {
    let column = gtk4::Box::new(Orientation::Vertical, 2);
    let mine = annotation.explicit_tags();
    let in_use = notes::all_tags(&reader.borrow().store.sidecar());
    let chips: Vec<(String, bool)> = mine
        .iter()
        .map(|t| (t.clone(), true))
        .chain(
            in_use
                .into_iter()
                .filter(|t| !mine.contains(t) && !annotation.tags().contains(t))
                .take(8)
                .map(|t| (t, false)),
        )
        .collect();
    if chips.is_empty() {
        return column.upcast();
    }
    let caption = gtk4::Label::new(Some("Tags — click to add or remove"));
    caption.add_css_class("dim-label");
    caption.add_css_class("caption");
    caption.set_xalign(0.0);
    caption.set_margin_top(4);
    column.append(&caption);
    let flow = gtk4::FlowBox::new();
    flow.set_selection_mode(gtk4::SelectionMode::None);
    flow.set_max_children_per_line(4);
    for (tag, on) in chips {
        let chip = gtk4::ToggleButton::with_label(&format!("#{tag}"));
        chip.add_css_class("flat");
        chip.add_css_class("caption");
        chip.set_active(on);
        let (reader, popover, id) = (reader.clone(), popover.clone(), annotation.id.clone());
        chip.connect_clicked(move |_| {
            popover.popdown();
            let store = reader.borrow().store.clone();
            let _ = store.update(&id, |a| {
                let mut tags = a.explicit_tags();
                if let Some(i) = tags.iter().position(|t| *t == tag) {
                    tags.remove(i);
                } else {
                    tags.push(tag.clone());
                }
                a.set_explicit_tags(&tags);
            });
        });
        flow.append(&chip);
    }
    column.append(&flow);
    column.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_whose_lines_cross_touch_and_distant_ones_do_not() {
        let line = |x0: f64, x1: f64, top: f64| [x0, top, x1, top, x0, top - 10.0, x1, top - 10.0];
        let a = [line(10.0, 100.0, 700.0)];
        assert!(quads_touch(&a, &[line(90.0, 150.0, 695.0)]));
        assert!(!quads_touch(&a, &[line(10.0, 100.0, 650.0)]));
        assert_eq!(bounds_of(&a), Some([10.0, 690.0, 100.0, 700.0]));
    }
}
