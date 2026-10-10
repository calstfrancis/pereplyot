use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn install_notes_sidebar(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    bookmark_button: &gtk4::Button,
    notes_rows: &gtk4::Box,
    notes_scroll: &gtk4::ScrolledWindow,
    page_entry: &gtk4::Entry,
    doc: crate::notebook_ui::DocRef,
    window: &gtk4::Window,
) -> (Rc<dyn Fn()>, Rc<Cell<bool>>) {
    let rebuild_notes_cell: RebuildNotesCell = Rc::new(RefCell::new(None));
    let quiet_notes = Rc::new(Cell::new(false));
    let filter: Rc<RefCell<NotesFilter>> = Rc::new(RefCell::new(NotesFilter::default()));
    let cards: Rc<RefCell<std::collections::HashMap<String, gtk4::Widget>>> =
        Rc::new(RefCell::new(std::collections::HashMap::new()));
    let active_card: Rc<RefCell<Option<gtk4::Widget>>> = Rc::new(RefCell::new(None));
    {
        let notes_rows = notes_rows.clone();
        let host = host.clone();
        let reader = reader.clone();
        let bookmark_button = bookmark_button.clone();
        let rebuild_notes_cell_inner = rebuild_notes_cell.clone();
        let quiet_notes = quiet_notes.clone();
        let filter = filter.clone();
        let cards = cards.clone();
        let active_card = active_card.clone();
        let doc = doc.clone();
        let window = window.clone();
        let builder = move || {
            while let Some(child) = notes_rows.first_child() {
                notes_rows.remove(&child);
            }
            cards.borrow_mut().clear();
            *active_card.borrow_mut() = None;
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
            let any_annotations = !all.is_empty();
            let current_page = reader.borrow().page as u32 + 1;
            {
                let f = filter.borrow();
                all.retain(|a| {
                    (!f.page_only || a.page == Some(current_page))
                        && f.tag.as_ref().map_or(true, |t| a.tags().contains(t))
                });
            }
            if any_annotations {
                notes_rows.append(&filter_bar(
                    &filter,
                    rebuild_notes_cell_inner.clone(),
                    &all_tags(&reader.borrow().store.sidecar()),
                ));
            }
            if all.is_empty() && any_annotations {
                let label = gtk4::Label::new(Some("Nothing matches this filter"));
                label.add_css_class("dim-label");
                label.set_margin_top(6);
                label.set_margin_bottom(6);
                notes_rows.append(&label);
            }
            if all.is_empty() && bookmarks.is_empty() && !any_annotations {
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
                outer.set_hexpand(true);

                let header_box = gtk4::Box::new(Orientation::Horizontal, 6);
                let header_label = gtk4::Label::new(Some(&format!(
                    "p.{page_label} — {}",
                    kind_name(&annotation)
                )));
                header_label.set_xalign(0.0);
                header_label.set_hexpand(true);
                header_label.add_css_class("dim-label");
                header_label.add_css_class("caption-heading");
                header_box.append(&header_label);
                let quote_now: Rc<dyn Fn() -> Option<crate::notebook::Quote>> = {
                    let doc = doc.clone();
                    let label = page_label.clone();
                    let a = annotation.clone();
                    Rc::new(move || Some(crate::notebook_ui::quote_of(&doc, &label, &a)))
                };
                let to_notebook = gtk4::Button::from_icon_name("list-add-symbolic");
                to_notebook.add_css_class("flat");
                to_notebook.set_tooltip_text(Some("Add to the notebook (or drag it there)"));
                to_notebook
                    .update_property(&[gtk4::accessible::Property::Label("Add to notebook")]);
                {
                    let host = host.clone();
                    let quote_now = quote_now.clone();
                    to_notebook.connect_clicked(move |_| {
                        if let Some(q) = quote_now() {
                            crate::notebook_ui::add_to_notebook(q, &|m| host.notify(m));
                        }
                    });
                }
                header_box.append(&to_notebook);
                let connect_button = {
                    let window = window.clone();
                    let host = host.clone();
                    crate::notebook_ui::connect::button(
                        Rc::new(move || Some(window.clone().upcast::<gtk4::Window>())),
                        crate::notebook_ui::end_of(&doc, &page_label, &annotation),
                        Rc::new(move |m| host.notify(m)),
                    )
                };
                header_box.add_controller(crate::notebook_ui::drag_source(quote_now.clone()));
                let delete_button = gtk4::Button::from_icon_name("user-trash-symbolic");
                delete_button.add_css_class("flat");
                delete_button.set_tooltip_text(Some("Delete this annotation"));
                header_box.append(&delete_button);
                outer.append(&header_box);

                {
                    let reader = reader.clone();
                    let id = annotation.id.clone();
                    crate::make_jump(&header_label, move || {
                        let target = (page_num.saturating_sub(1))
                            .min(reader.borrow().count.saturating_sub(1) as u32)
                            as u16;
                        jump(&reader, target, JumpKind::Annotation);
                        mark_edit::flash(&reader, &id);
                    });
                }

                if annotation.kind == fond_annot::AnnotationKind::Area {
                    if let Some(rect) = annotation.rect() {
                        let picture = gtk4::Picture::new();
                        picture.set_can_shrink(true);
                        picture.set_content_fit(gtk4::ContentFit::Contain);
                        picture.set_size_request(-1, 96);
                        picture.set_halign(gtk4::Align::Start);
                        picture.set_tooltip_text(Some("The clipped area"));
                        picture.update_property(&[gtk4::accessible::Property::Label(
                            "Picture of the clipped area",
                        )]);
                        outer.append(&picture);
                        area_thumbnail(
                            &reader.borrow().path.clone(),
                            annotation.page.unwrap_or(1),
                            rect,
                            picture,
                        );
                    }
                }
                if let Some(snippet) = &annotation.snippet {
                    let snippet_label = gtk4::Label::new(Some(snippet));
                    snippet_label.set_xalign(0.0);
                    snippet_label.set_wrap(true);
                    snippet_label.set_lines(4);
                    snippet_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                    snippet_label.add_css_class("dim-label");
                    snippet_label.add_css_class("caption");
                    {
                        let reader = reader.clone();
                        let id = annotation.id.clone();
                        crate::make_jump(&snippet_label, move || {
                            let target = (page_num.saturating_sub(1))
                                .min(reader.borrow().count.saturating_sub(1) as u32)
                                as u16;
                            jump(&reader, target, JumpKind::Annotation);
                            mark_edit::flash(&reader, &id);
                        });
                    }
                    snippet_label
                        .add_controller(crate::notebook_ui::drag_source(quote_now.clone()));
                    outer.append(&snippet_label);
                }
                outer.append(&crate::notebook_ui::connect::rows(
                    &doc.hash,
                    &annotation.id,
                ));
                outer.append(&connect_button);
                let tags = annotation.tags();
                if !tags.is_empty() {
                    let row = gtk4::Box::new(Orientation::Horizontal, 4);
                    for tag in tags {
                        let chip = gtk4::Button::with_label(&format!("#{tag}"));
                        chip.add_css_class("flat");
                        chip.add_css_class("caption");
                        chip.set_tooltip_text(Some("Show only notes with this tag"));
                        let filter = filter.clone();
                        let rebuild = rebuild_notes_cell_inner.clone();
                        chip.connect_clicked(move |_| {
                            filter.borrow_mut().tag = Some(tag.clone());
                            let f = rebuild.borrow().clone();
                            if let Some(f) = f {
                                f();
                            }
                        });
                        row.append(&chip);
                    }
                    outer.append(&row);
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

                let card = gtk4::Box::new(Orientation::Horizontal, 8);
                card.add_css_class("note-card");
                if let Some(stripe) = color_swatch(annotation.color.as_deref()) {
                    if let Some(area) = stripe.downcast_ref::<gtk4::DrawingArea>() {
                        area.set_content_width(4);
                        area.set_content_height(0);
                    }
                    stripe.set_valign(gtk4::Align::Fill);
                    card.append(&stripe);
                }
                card.append(&outer);
                cards
                    .borrow_mut()
                    .insert(annotation.id.clone(), card.clone().upcast());
                notes_rows.append(&card);
                if i != last {
                    notes_rows.append(&popover_separator());
                }
            }
        };
        *rebuild_notes_cell.borrow_mut() = Some(Rc::new(builder));
    }
    // A mark selected on the page lights up its card and scrolls the list to it.
    {
        let cards = cards.clone();
        let active_card = active_card.clone();
        let notes_scroll = notes_scroll.clone();
        reader.borrow_mut().notes_focus = Some(Rc::new(move |id: &str| {
            if let Some(old) = active_card.borrow_mut().take() {
                old.remove_css_class("note-card-active");
            }
            let Some(card) = cards.borrow().get(id).cloned() else {
                return;
            };
            card.add_css_class("note-card-active");
            if let Some(content) = notes_scroll.child() {
                if let Some(b) = card.compute_bounds(&content) {
                    let adj = notes_scroll.vadjustment();
                    let (top, bottom) = (b.y() as f64, (b.y() + b.height()) as f64);
                    if top < adj.value() || bottom > adj.value() + adj.page_size() {
                        adj.set_value((top - 12.0).max(0.0));
                    }
                }
            }
            *active_card.borrow_mut() = Some(card);
        }));
    }
    // With "This page" on, the list follows the page being read.
    {
        let cell = rebuild_notes_cell.clone();
        let filter = filter.clone();
        let pending = Rc::new(Cell::new(false));
        page_entry.connect_changed(move |_| {
            if !filter.borrow().page_only || pending.replace(true) {
                return;
            }
            let cell = cell.clone();
            let pending = pending.clone();
            glib::idle_add_local_once(move || {
                pending.set(false);
                let f = cell.borrow().clone();
                if let Some(f) = f {
                    f();
                }
            });
        });
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
    {
        let rebuild = rebuild_notes.clone();
        let id = crate::connections::subscribe(Rc::new(move || rebuild()));
        reader
            .borrow_mut()
            .close_hooks
            .push(Rc::new(move || crate::connections::unsubscribe(id)));
    }
    (rebuild_notes, quiet_notes)
}

/// What the Notes list is narrowed to.
#[derive(Default, Clone)]
struct NotesFilter {
    page_only: bool,
    tag: Option<String>,
}

fn kind_name(a: &fond_annot::Annotation) -> &'static str {
    use fond_annot::AnnotationKind::*;
    match a.kind {
        Highlight => "Highlight",
        Underline => "Underline",
        Strikeout => "Strikeout",
        Area => "Area",
        Note if a.quadpoints.is_empty() => "Note",
        Note => "Noted text",
        _ => "Annotation",
    }
}

/// Every tag in use, most-used first.
pub(super) fn all_tags(sidecar: &fond_annot::AnnotationSidecar) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for a in &sidecar.annotations {
        for t in a.tags() {
            match counts.iter_mut().find(|(n, _)| *n == t) {
                Some((_, c)) => *c += 1,
                None => counts.push((t, 1)),
            }
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    counts.into_iter().map(|(t, _)| t).collect()
}

/// The strip above the cards: "This page", and the tag the list is narrowed to (or a menu of the
/// tags there are).
fn filter_bar(
    filter: &Rc<RefCell<NotesFilter>>,
    rebuild: RebuildNotesCell,
    tags: &[String],
) -> gtk4::Widget {
    let bar = gtk4::Box::new(Orientation::Horizontal, 6);
    bar.set_margin_bottom(6);
    let refresh = {
        let rebuild = rebuild.clone();
        move || {
            let f = rebuild.borrow().clone();
            if let Some(f) = f {
                f();
            }
        }
    };
    let page = gtk4::ToggleButton::with_label("This page");
    page.add_css_class("flat");
    page.set_active(filter.borrow().page_only);
    page.set_tooltip_text(Some("Show only the notes on the page you are reading"));
    {
        let filter = filter.clone();
        let refresh = refresh.clone();
        page.connect_toggled(move |b| {
            if filter.borrow().page_only != b.is_active() {
                filter.borrow_mut().page_only = b.is_active();
                refresh();
            }
        });
    }
    bar.append(&page);
    let current_tag = filter.borrow().tag.clone();
    if let Some(tag) = current_tag {
        let clear = gtk4::Button::with_label(&format!("#{tag}  ✕"));
        clear.add_css_class("flat");
        clear.set_tooltip_text(Some("Show all tags again"));
        let filter = filter.clone();
        clear.connect_clicked(move |_| {
            filter.borrow_mut().tag = None;
            refresh();
        });
        bar.append(&clear);
    } else if !tags.is_empty() {
        let menu = gtk4::MenuButton::new();
        menu.set_label("Tags");
        menu.add_css_class("flat");
        let popover = gtk4::Popover::new();
        let rows = gtk4::Box::new(Orientation::Vertical, 2);
        rows.set_margin_top(6);
        rows.set_margin_bottom(6);
        rows.set_margin_start(6);
        rows.set_margin_end(6);
        for tag in tags.iter().take(20) {
            let row = popover_button(&format!("#{tag}"), false);
            let filter = filter.clone();
            let refresh = refresh.clone();
            let popover = popover.clone();
            let tag = tag.clone();
            row.connect_clicked(move |_| {
                popover.popdown();
                filter.borrow_mut().tag = Some(tag.clone());
                refresh();
            });
            rows.append(&row);
        }
        popover.set_child(Some(&rows));
        menu.set_popover(Some(&popover));
        bar.append(&menu);
    }
    bar.upcast()
}

thread_local! {
    static THUMBS: RefCell<std::collections::HashMap<String, gdk::Texture>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Draw `rect` of `page` small, off the main thread, into `picture` (kept for the next time the
/// Notes list is rebuilt).
fn area_thumbnail(path: &std::path::Path, page: u32, rect: [f64; 4], picture: gtk4::Picture) {
    let key = format!("{}:{page}:{rect:?}", path.display());
    if let Some(texture) = THUMBS.with(|t| t.borrow().get(&key).cloned()) {
        picture.set_paintable(Some(&texture));
        return;
    }
    let path = path.to_path_buf();
    glib::spawn_future_local(async move {
        let clip =
            gtk4::gio::spawn_blocking(move || crate::clip::render_at(&path, page, rect, 1.6))
                .await
                .ok()
                .flatten();
        if let Some(clip) = clip {
            let texture = crate::clip::texture(&clip);
            THUMBS.with(|t| t.borrow_mut().insert(key, texture.clone()));
            picture.set_paintable(Some(&texture));
        }
    });
}
