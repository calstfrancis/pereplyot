use super::*;

pub(super) fn install_popout(ui: &PdfUi) {
    let PdfUi {
        reader_tab,
        popout_button,
        pdf_hash,
        window,
        ..
    } = ui.clone();
    {
        let window = window.clone();
        let reader_tab = reader_tab.clone();
        let pdf_hash = pdf_hash.to_string();
        popout_button.connect_clicked(move |_| {
            let new_tab = reader_tab.pop_out(&window);
            crate::register_reader(&pdf_hash, &new_tab);
        });
    }
}
