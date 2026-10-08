use super::*;

pub(super) fn install_sidebar_toggle(ui: &PdfUi) {
    let PdfUi {
        sidebar_toggle,
        sidebar_box,
        paned,
        ..
    } = ui.clone();
    {
        let paned = paned.clone();
        let sidebar_box = sidebar_box.clone();
        sidebar_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                paned.set_start_child(Some(&sidebar_box));
            } else {
                paned.set_start_child(gtk4::Widget::NONE);
            }
        });
    }
}
