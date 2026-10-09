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
    hash: &str,
    reader_window: &adw::Window,
) {
    let (items, bookmarks, clips) = {
        let r = reader.borrow();
        let items = epub_items(&r.store.sidecar(), &r.spine, hash, &r.cache_dir);
        let clips: Vec<crate::export::AreaClip> = r
            .store
            .sidecar()
            .annotations
            .iter()
            .filter_map(|a| epub_clip_of(a, &r.cache_dir))
            .collect();
        let bookmarks = r
            .bookmarks
            .iter()
            .map(|&c| format!("ch. {}", c + 1))
            .collect();
        (items, bookmarks, clips)
    };
    crate::export::show_export_dialog(host, reader_window, title, items, bookmarks, None, clips);
}

/// Export items for an EPUB's annotations in reading order, each cited by its printed page when
/// the book has page numbers and by chapter when it doesn't, with a link back to the passage.
pub(super) fn epub_items(
    sidecar: &fond_annot::AnnotationSidecar,
    spine: &[String],
    hash: &str,
    cache_dir: &std::path::Path,
) -> Vec<crate::export::Item> {
    let chapter_number = |c: &str| spine.iter().position(|p| p == c).map(|i| i + 1);
    let mut keyed: Vec<((usize, Option<String>), crate::export::Item)> = Vec::new();
    let mut one = sidecar.clone();
    for a in &sidecar.annotations {
        one.annotations = vec![a.clone()];
        let link = |id: &str| {
            Some(crate::deeplink::build(&crate::deeplink::DeepLink {
                hash: hash.to_string(),
                annotation: Some(id.to_string()),
                page: None,
            }))
        };
        let Some(mut item) =
            fond_annot::export::items_with_links(&one, &[], &chapter_number, &|_| None, &link)
                .into_iter()
                .next()
        else {
            continue;
        };
        if let Some(label) = crate::page_label_of(a) {
            item.locator = label;
            item.is_chapter = false;
        }
        if let Some(clip) = epub_clip_of(a, cache_dir) {
            item.image = Some(format!(
                "{}{}",
                crate::export::FIGURES_DIR,
                clip.file_name()
            ));
        }
        let at = a
            .chapter
            .as_deref()
            .and_then(chapter_number)
            .unwrap_or(usize::MAX);
        keyed.push(((at, a.created.clone()), item));
    }
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.into_iter().map(|(_, i)| i).collect()
}

/// The picture file a clipped image annotation stands for, as an export figure to copy.
fn epub_clip_of(
    a: &fond_annot::Annotation,
    cache_dir: &std::path::Path,
) -> Option<crate::export::AreaClip> {
    let image = crate::image_of(a)?;
    let source = cache_dir.join(&image);
    source.is_file().then(|| crate::export::AreaClip {
        id: a.id.clone(),
        page: 0,
        rect: [0.0; 4],
        copy_from: Some(source),
    })
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
    let css = typography.epub_css_for(super::paged::is_paged(view));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn annotation(chapter: &str, snippet: &str, label: Option<&str>) -> fond_annot::Annotation {
        let mut a = fond_annot::Annotation::drawn_epub(
            fond_annot::AnnotationKind::Highlight,
            chapter.to_string(),
            snippet.to_string(),
            None,
            None,
            None,
        );
        crate::set_page_label(&mut a, label);
        a
    }

    #[test]
    fn exports_cite_the_printed_page_when_there_is_one_and_the_chapter_when_not() {
        let spine = vec!["a.xhtml".to_string(), "b.xhtml".to_string()];
        let mut sidecar = fond_annot::AnnotationSidecar::new("k");
        sidecar.annotations = vec![
            annotation("b.xhtml", "later", None),
            annotation("a.xhtml", "first", Some("xii")),
        ];
        let items = epub_items(
            &sidecar,
            &spine,
            "hash",
            std::path::Path::new("/nonexistent"),
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].quote.as_deref(), Some("first"));
        assert_eq!(
            (items[0].locator.as_str(), items[0].is_chapter),
            ("xii", false)
        );
        assert_eq!(
            (items[1].locator.as_str(), items[1].is_chapter),
            ("2", true)
        );
        assert!(items[0]
            .link
            .as_deref()
            .unwrap()
            .starts_with("pereplyot://open?hash=hash&annotation="));
    }
}
