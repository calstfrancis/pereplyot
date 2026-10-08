use super::*;

pub(super) fn install_page_entry(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        render,
        prev,
        next,
        page_entry,
        page_of_label,
        bookmark_button,
        continuous_toggle,
        continuous_scroll,
        ..
    } = ui.clone();
    // Typing a page number (the document's own printed label, or a raw file page number —
    // see `find_page_by_label`) and pressing Enter jumps there, navigating by the printed
    // page number rather than always the raw file position.
    {
        let reader = reader.clone();
        let render = render.clone();
        let host = host.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        page_entry.clone().connect_activate(move |entry| {
            let text = entry.text();
            let target = {
                let r = reader.borrow();
                find_page_by_label(&r.page_labels, &text)
            };
            match target {
                Some(page) if page < reader.borrow().count => {
                    if continuous_toggle.is_active() {
                        scroll_continuous_to_page(&reader, &continuous_scroll, page);
                    } else {
                        reader.borrow_mut().page = page;
                        render();
                    }
                }
                _ => {
                    host.notify("No such page");
                    // Revert to the current page's actual label/number rather than leaving
                    // the entry showing whatever unresolvable text was typed.
                    let (page, count) = {
                        let r = reader.borrow();
                        (r.page, r.count)
                    };
                    let labels = reader.borrow().page_labels.clone();
                    let bookmarks = reader.borrow().bookmarks.clone();
                    update_page_display(
                        &page_entry,
                        &page_of_label,
                        &prev,
                        &next,
                        &bookmark_button,
                        page,
                        count,
                        &labels,
                        &bookmarks,
                    );
                }
            }
        });
    }
}
