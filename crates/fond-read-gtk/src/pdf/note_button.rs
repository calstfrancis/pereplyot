use super::*;

pub(super) fn install_note_button(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        reader_window,
        note_button,
        ..
    } = ui.clone();
    {
        let host = host.clone();
        let reader = reader.clone();
        let dialog = reader_window.clone();
        note_button.connect_clicked(move |_| {
            show_pdf_note_dialog(&host, &reader, &dialog, None);
        });
    }
}
