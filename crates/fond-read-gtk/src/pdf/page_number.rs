use super::*;

pub(super) fn install_page_number(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        reader_window,
        prev,
        next,
        page_entry,
        page_of_label,
        bookmark_button,
        page_num_button,
        ..
    } = ui.clone();
    {
        let host = host.clone();
        let reader = reader.clone();
        let page_entry = page_entry.clone();
        let page_of_label = page_of_label.clone();
        let prev = prev.clone();
        let next = next.clone();
        let bookmark_button = bookmark_button.clone();
        let dialog = reader_window.clone();
        page_num_button.connect_clicked(move |_| {
            show_page_number_dialog(
                &host,
                &reader,
                &page_entry,
                &page_of_label,
                &prev,
                &next,
                &bookmark_button,
                &dialog,
            );
        });
    }
}
