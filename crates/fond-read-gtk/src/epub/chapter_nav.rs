use super::*;

pub(super) fn install_chapter_nav(
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
    prev: &gtk4::Button,
    next: &gtk4::Button,
    chapter_label: &gtk4::Label,
    bookmark_button: &gtk4::Button,
    start_index: usize,
) {
    // Load the first chapter up front (the TOC/prev/next/notes-jump handlers all reuse
    // this same navigation path for consistency, but chapter 0 has to start somewhere).
    let first_chapter = reader.borrow().spine.get(start_index).cloned();
    if let Some(first) = first_chapter {
        epub_go_to(
            reader,
            web_view,
            prev,
            next,
            chapter_label,
            bookmark_button,
            &first,
        );
    }

    {
        let reader = reader.clone();
        let view = web_view.clone();
        let prev_for_handler = prev.clone();
        let next = next.clone();
        let chapter_label = chapter_label.clone();
        let bookmark_button = bookmark_button.clone();
        prev.connect_clicked(move |_| {
            let target = {
                let r = reader.borrow();
                (r.index > 0).then(|| r.spine[r.index - 1].clone())
            };
            if let Some(target) = target {
                epub_go_to(
                    &reader,
                    &view,
                    &prev_for_handler,
                    &next,
                    &chapter_label,
                    &bookmark_button,
                    &target,
                );
            }
        });
    }
    {
        let reader = reader.clone();
        let view = web_view.clone();
        let prev = prev.clone();
        let next_for_handler = next.clone();
        let chapter_label = chapter_label.clone();
        let bookmark_button = bookmark_button.clone();
        next.connect_clicked(move |_| {
            let target = {
                let r = reader.borrow();
                (r.index + 1 < r.spine.len()).then(|| r.spine[r.index + 1].clone())
            };
            if let Some(target) = target {
                epub_go_to(
                    &reader,
                    &view,
                    &prev,
                    &next_for_handler,
                    &chapter_label,
                    &bookmark_button,
                    &target,
                );
            }
        });
    }
}
