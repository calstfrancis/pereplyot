use super::*;

/// Navigate the EPUB reader's `WebView` to `target` (a zip-internal path, optionally with a
/// `#fragment` for an in-chapter anchor — the same shape `fond_doc::EpubBook::spine`/`toc`
/// entries use). Updates `state.index` when `target`'s path (fragment stripped) matches a
/// spine entry, so the chapter label and prev/next sensitivity stay correct whether the jump
/// came from a TOC entry, the prev/next buttons, or the initial chapter-0 load — all three
/// funnel through here rather than duplicating the URI-building and label/button refresh.
#[allow(clippy::too_many_arguments)]
/// Export every bookmark and annotation — format, grouping and citation key chosen in a
/// dialog (see [`crate::export`]), chapters located by spine position.
pub(super) fn export_notes(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<EpubReaderState>>,
    title: &str,
    reader_window: &adw::Window,
) {
    let (items, bookmarks) = {
        let r = reader.borrow();
        let spine = &r.spine;
        let items = crate::export::items_from_sidecar(&r.store.sidecar(), &[], &|c| {
            spine.iter().position(|p| p == c).map(|i| i + 1)
        });
        let bookmarks = r
            .bookmarks
            .iter()
            .map(|&c| format!("ch. {}", c + 1))
            .collect();
        (items, bookmarks)
    };
    crate::export::show_export_dialog(
        host,
        reader_window,
        title,
        items,
        bookmarks,
        None,
        Vec::new(),
    );
}

/// Replace the stylesheet registered on `view`'s `UserContentManager` with one built from the
/// shared reading typography — see the call site's doc comment for why this goes through a
/// `WebKitUserStyleSheet` rather than a JS injection per chapter load.
pub(super) fn apply_epub_style(
    view: &webkit6::WebView,
    typography: &crate::typography::Typography,
) {
    let Some(ucm) = webkit6::prelude::WebViewExt::user_content_manager(view) else {
        return;
    };
    ucm.remove_all_style_sheets();
    let css = typography.epub_css();
    if css.trim().is_empty() {
        return;
    }
    let sheet = webkit6::UserStyleSheet::new(
        &css,
        webkit6::UserContentInjectedFrames::TopFrame,
        webkit6::UserStyleLevel::User,
        &[],
        &[],
    );
    ucm.add_style_sheet(&sheet);
}

pub(super) fn epub_go_to(
    state: &Rc<RefCell<EpubReaderState>>,
    view: &webkit6::WebView,
    prev: &gtk4::Button,
    next: &gtk4::Button,
    chapter_label: &gtk4::Label,
    bookmark_button: &gtk4::Button,
    target: &str,
) {
    let (path, fragment) = target
        .split_once('#')
        .map_or((target, None), |(p, f)| (p, Some(f)));

    let cache_dir = {
        let mut r = state.borrow_mut();
        if let Some(idx) = r.spine.iter().position(|p| p == path) {
            r.index = idx;
        }
        r.cache_dir.clone()
    };

    let mut uri = gio::File::for_path(cache_dir.join(path)).uri().to_string();
    if let Some(fragment) = fragment {
        uri.push('#');
        uri.push_str(fragment);
    }
    view.load_uri(&uri);

    let r = state.borrow();
    let percent = if !r.spine.is_empty() {
        ((r.index + 1) * 100 / r.spine.len()).min(100)
    } else {
        0
    };
    chapter_label.set_text(&format!(
        "Chapter {} of {} · {percent}%",
        r.index + 1,
        r.spine.len()
    ));
    prev.set_sensitive(r.index > 0);
    next.set_sensitive(r.index + 1 < r.spine.len());
    update_bookmark_button(bookmark_button, r.bookmarks.contains(&r.index));
}

pub(super) fn build_contents_sidebar(
    toc: &[fond_doc::TocEntry],
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
    prev: &gtk4::Button,
    next: &gtk4::Button,
    chapter_label: &gtk4::Label,
    bookmark_button: &gtk4::Button,
) -> gtk4::ScrolledWindow {
    let rows = gtk4::Box::new(Orientation::Vertical, 2);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);
    let last = toc.len().saturating_sub(1);
    for (i, entry) in toc.iter().enumerate() {
        let row = popover_button(&entry.label, false);
        if let Some(lbl) = row.child().and_then(|w| w.downcast::<gtk4::Label>().ok()) {
            lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        }
        {
            let reader = reader.clone();
            let view = web_view.clone();
            let prev = prev.clone();
            let next = next.clone();
            let chapter_label = chapter_label.clone();
            let bookmark_button = bookmark_button.clone();
            let target = entry.target.clone();
            row.connect_clicked(move |_| {
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
        rows.append(&row);
        if i != last {
            rows.append(&popover_separator());
        }
    }
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_child(Some(&rows));
    scroll
}
