use super::*;

/// Export every bookmark and annotation — format, grouping and citation key chosen in a
/// dialog (see [`crate::export`]).
pub(super) fn export_notes(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    title: &str,
    reader_window: &gtk4::Window,
    hash: &str,
) {
    let (items, bookmarks, clips, path) = {
        let r = reader.borrow();
        let items =
            crate::export::items_with_figures(&r.store.sidecar(), &r.page_labels, Some(hash));
        let clips = crate::export::clips_of(&r.store.sidecar());
        let bookmarks = r
            .bookmarks
            .iter()
            .map(|&p| {
                let label = r
                    .page_labels
                    .get((p as usize).saturating_sub(1))
                    .and_then(|l| l.clone())
                    .unwrap_or_else(|| p.to_string());
                format!("p. {label}")
            })
            .collect();
        (items, bookmarks, clips, r.path.clone())
    };
    crate::export::show_export_dialog(
        host,
        reader_window,
        title,
        items,
        bookmarks,
        Some(path),
        clips,
    );
}

/// A small modal that anchors the reader's *current* physical page to its own printed page
/// number — the manual counterpart to the automatic `/PageLabels` read in `show_pdf_reader`,
/// for a PDF that declares no page labels of its own (see `fond_annot::PageLabelOverride`).
/// Leaving the entry blank and confirming clears any existing override, reverting to raw file
/// page numbers. Writes straight to `notes/<key>.md` (same `load_note`/`write_note` pattern
/// as `Progress`) and updates the live reader in place, so the change is visible immediately
/// without reopening the document.
#[allow(clippy::too_many_arguments)]
pub(super) fn show_page_number_dialog(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    page_entry: &gtk4::Entry,
    page_of_label: &gtk4::Label,
    prev: &gtk4::Button,
    next: &gtk4::Button,
    bookmark_button: &gtk4::Button,
    reader_window: &gtk4::Window,
) {
    let (page, count) = {
        let r = reader.borrow();
        (r.page, r.count)
    };

    let dialog = adw::Window::new();
    dialog.set_title(Some("Set page numbering"));
    dialog.set_modal(true);
    // Modal against the reader window, not the main library window — see the same note on
    // `show_pdf_note_dialog`.
    dialog.set_transient_for(Some(reader_window));
    dialog.set_default_size(380, -1);

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.add_css_class("fond-chrome");
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    let set_button = gtk4::Button::with_label("Set");
    set_button.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&set_button);
    view.add_top_bar(&header);

    let content = gtk4::Box::new(Orientation::Vertical, 8);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    let hint = gtk4::Label::new(Some(&format!(
        "This is file page {} of {count}. What number is printed on it? Later pages count up \
         from here; earlier ones are left unlabeled.",
        page + 1
    )));
    hint.add_css_class("dim-label");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    let entry = gtk4::Entry::builder()
        .placeholder_text("e.g. 1 — leave blank to clear")
        .activates_default(true)
        .build();
    content.append(&entry);
    content.append(&hint);
    view.set_content(Some(&content));
    dialog.set_content(Some(&view));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let host = host.clone();
        let dialog = dialog.clone();
        let reader = reader.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        set_button.connect_clicked(move |_| {
            let text = entry.text().trim().to_string();
            let override_value = if text.is_empty() {
                None
            } else {
                match text.parse::<i64>() {
                    Ok(n) => Some(fond_annot::PageLabelOverride {
                        start_page: page as u32 + 1,
                        start_label: n,
                    }),
                    Err(_) => {
                        host.notify("Enter a whole number, or leave blank to clear");
                        return;
                    }
                }
            };

            host.set_page_label_override(override_value);

            let new_labels = override_value.map(|ov| ov.apply(count)).unwrap_or_default();
            reader.borrow_mut().page_labels = new_labels.clone();
            let bookmarks = reader.borrow().bookmarks.clone();
            update_page_display(
                &page_entry,
                &page_of_label,
                &prev,
                &next,
                &bookmark_button,
                page,
                count,
                &new_labels,
                &bookmarks,
            );
            host.notify(if override_value.is_some() {
                "Page numbering set"
            } else {
                "Page numbering cleared"
            });
            dialog.close();
        });
    }

    dialog.present();
}

/// A small modal for adding a freestanding marginal note (`AnnotationKind::Note`) to the
/// PDF reader's *current* page — unlike Highlight/Underline/Strikeout, a note isn't tied to
/// a drawn region, so there's no drag gesture for it, just this prompt. Saves straight into
/// `reader`'s in-memory sidecar and to disk, the same `annots/<key>.json` the drag gesture
/// writes. When opened right after a "Select text" drag on this page, the note carries that
/// selection's real quadpoints (see `last_selection`), so — unlike a plain marginal note on
/// blank page — it does need a re-render (`refresh`) afterward to show up on the page.
pub(super) fn show_pdf_note_dialog(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    reader_window: &gtk4::Window,
    at: Option<[f64; 2]>,
) {
    let current_page = reader.borrow().page as u32 + 1;

    let dialog = adw::Window::new();
    dialog.set_title(Some(&format!("Note on page {current_page}")));
    dialog.set_modal(true);
    // Modal against the reader window itself, not the main library window — otherwise
    // opening this from within the reader leaves the reader interactive but blocks the
    // library behind it, which is backwards and is what "the reader blocks the library"
    // reports were actually seeing (the reader's own top-level window was never modal).
    dialog.set_transient_for(Some(reader_window));
    dialog.set_default_size(420, 260);

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.add_css_class("fond-chrome");
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    let save = gtk4::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&save);
    view.add_top_bar(&header);

    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_margin_top(8);
    text_view.set_margin_bottom(8);
    text_view.set_margin_start(8);
    text_view.set_margin_end(8);

    // Pre-fill with the last "Select text" copy, quoted with its page number, if it was made
    // on this same page — consumed either way so a stale selection from another page doesn't
    // linger into some later, unrelated note. Its quadpoints (if any) carry over onto the
    // created annotation too, so the note anchors to — and stays visibly marked at — the
    // actual selected text on the page, rather than being a page-only marginal note.
    let (selection_quads, selection_quote) = {
        let mut r = reader.borrow_mut();
        match r.last_selection.take() {
            Some((sel_page, text, quads)) if sel_page == r.page => (quads, Some(text)),
            _ => (Vec::new(), None),
        }
    };

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&text_view));
    let body = gtk4::Box::new(Orientation::Vertical, 0);
    if let Some(quote) = &selection_quote {
        let quote_label = gtk4::Label::new(Some(quote));
        quote_label.set_wrap(true);
        quote_label.set_xalign(0.0);
        quote_label.set_selectable(true);
        quote_label.add_css_class("dim-label");
        quote_label.set_margin_top(8);
        quote_label.set_margin_start(12);
        quote_label.set_margin_end(12);
        body.append(&quote_label);
    }
    body.append(&scrolled);
    view.set_content(Some(&body));
    text_view.grab_focus();
    dialog.set_content(Some(&view));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let host = host.clone();
        let host = host.clone();
        let reader = reader.clone();
        let text_view = text_view.clone();
        save.connect_clicked(move |_| {
            let buffer = text_view.buffer();
            let text = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .trim()
                .to_string();
            if text.is_empty() {
                host.notify("Note is empty");
                return;
            }

            let mut annotation = fond_annot::Annotation::drawn(
                fond_annot::AnnotationKind::Note,
                current_page,
                selection_quads.clone(),
                selection_quote.clone(),
                Some(text),
                None,
            );
            if selection_quads.is_empty() {
                annotation.set_position(Some(
                    at.unwrap_or_else(|| sticky_default_position(&reader.borrow(), current_page)),
                ));
            }
            let store = reader.borrow().store.clone();
            match store.add(annotation) {
                Ok(()) => {
                    host.notify("Note added");
                    dialog.close();
                }
                Err(e) => host.notify(&e),
            }
        });
    }

    dialog.present();
}

/// Where a sticky note goes when nobody chose a spot: down the page's top-right corner, below
/// the ones already there.
fn sticky_default_position(r: &ReaderState, page: u32) -> [f64; 2] {
    let stacked = r
        .store
        .sidecar()
        .annotations
        .iter()
        .filter(|a| a.page == Some(page) && shapes::shape_of(a) == shapes::Shape::Sticky)
        .count() as f64;
    let Some(geom) = r.geom((page - 1) as u16) else {
        return [20.0, 780.0];
    };
    let (dw, dh) = geom.display_size();
    let (x, y) = geom.px_to_pdf(
        dw as f64 - shapes::STICKY_PT - 12.0,
        12.0 + stacked * (shapes::STICKY_PT + 6.0),
        dw as f64,
        dh as f64,
    );
    [x, y]
}
