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
    crate::export::show_export_dialog(host, reader_window, title, items, bookmarks);
}

/// Replace the reading-theme/font stylesheet registered on `view`'s `UserContentManager`
/// with one built from the theme/font dropdowns' current selections — see the call site's
/// doc comment for why this goes through a `WebKitUserStyleSheet` rather than a JS injection
/// per chapter load. `theme`/`font` are the dropdowns' `selected()` indices, matching
/// `theme_labels`/`font_labels`'s declared order.
pub(super) fn apply_epub_style(view: &webkit6::WebView, theme: u32, font: u32) {
    let Some(ucm) = webkit6::prelude::WebViewExt::user_content_manager(view) else {
        return;
    };
    ucm.remove_all_style_sheets();

    let theme_css = match theme {
        1 => {
            "html, body { background: #f4ecd8 !important; color: #5b4636 !important; } \
              a, a:visited { color: #8a6d3b !important; }"
        }
        2 => {
            "html, body { background: #1e1e1e !important; color: #dddddd !important; } \
              a, a:visited { color: #8ab4f8 !important; }"
        }
        _ => "",
    };
    let font_css = match font {
        1 => {
            "body, p, div, span, li { font-family: Georgia, 'Times New Roman', serif !important; }"
        }
        2 => "body, p, div, span, li { font-family: -webkit-system-font, sans-serif !important; }",
        _ => "",
    };
    let css = format!("{theme_css}\n{font_css}");
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
