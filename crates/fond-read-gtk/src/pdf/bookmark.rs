use super::*;

pub(super) fn install_bookmark(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        rebuild_notes,
        bookmark_button,
        ..
    } = ui.clone();
    {
        let host = host.clone();
        let reader = reader.clone();
        let rebuild_notes = rebuild_notes.clone();
        bookmark_button.connect_clicked(move |btn| {
            let page_num = reader.borrow().page as u32 + 1;
            let now_bookmarked = {
                let mut r = reader.borrow_mut();
                if let Some(pos) = r.bookmarks.iter().position(|&p| p == page_num) {
                    r.bookmarks.remove(pos);
                    false
                } else {
                    r.bookmarks.push(page_num);
                    r.bookmarks.sort_unstable();
                    true
                }
            };
            host.save_bookmarks(&reader.borrow().bookmarks);
            update_bookmark_button(btn, now_bookmarked);
            rebuild_notes();
        });
    }
}
