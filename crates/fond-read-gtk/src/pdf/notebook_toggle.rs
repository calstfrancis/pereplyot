use super::*;

/// The Notebook toggle: shows the shared notebook beside this document, taking it from any other
/// window that had it.
pub(super) fn install_notebook_toggle(ui: &PdfUi) {
    crate::notebook_ui::bind_toggle(&ui.notebook_toggle, &ui.notebook_paned);
    let notebook_paned = ui.notebook_paned.clone();
    ui.reader.borrow_mut().close_hooks.push(Rc::new(move || {
        crate::notebook_ui::hide_in_pane(&notebook_paned);
    }));
}
