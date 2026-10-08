use super::*;

pub(super) fn install_export(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        reader_window,
        export_button,
        title,
        ..
    } = ui.clone();
    {
        let host = host.clone();
        let reader = reader.clone();
        let title = title.to_string();
        let dialog = reader_window.clone();
        export_button.connect_clicked(move |_| {
            export_notes(&host, &reader, &title, &dialog);
        });
    }
}
