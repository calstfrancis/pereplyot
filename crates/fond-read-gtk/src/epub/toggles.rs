use super::*;

pub(super) fn install_sidebar_toggles(ui: &EpubUi) {
    let EpubUi {
        contents_scroll,
        notes_scroll,
        notes_toggle,
        notes_paned,
        paned,
        sidebar_toggle,
        rebuild_notes,
        ..
    } = ui;
    {
        let paned = paned.clone();
        let contents_scroll = contents_scroll.clone();
        sidebar_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                paned.set_start_child(Some(&contents_scroll));
            } else {
                paned.set_start_child(gtk4::Widget::NONE);
            }
        });
    }
    {
        let notes_paned = notes_paned.clone();
        let notes_scroll = notes_scroll.clone();
        let rebuild_notes = rebuild_notes.clone();
        const NOTES_SIDEBAR_WIDTH: i32 = 300;
        notes_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                rebuild_notes();
                let available = notes_paned.width();
                let total = if available > 0 { available } else { 900 };
                notes_paned.set_position((total - NOTES_SIDEBAR_WIDTH).max(200));
                notes_paned.set_end_child(Some(&notes_scroll));
            } else {
                notes_paned.set_end_child(gtk4::Widget::NONE);
            }
        });
    }
}
