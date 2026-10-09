use super::*;

/// The Notebook toggle: shows the shared notebook beside this document, taking it from any other
/// window that had it.
pub(super) fn install_notebook_toggle(ui: &PdfUi) {
    let PdfUi {
        notebook_toggle,
        notebook_paned,
        reader,
        ..
    } = ui.clone();
    let hiding = Rc::new(Cell::new(false));
    {
        let notebook_paned = notebook_paned.clone();
        let hiding = hiding.clone();
        let toggle = notebook_toggle.clone();
        notebook_toggle.connect_toggled(move |t| {
            if hiding.get() {
                return;
            }
            if t.is_active() {
                let toggle = toggle.clone();
                let hiding = hiding.clone();
                crate::notebook_ui::show_in_pane(
                    &notebook_paned,
                    Rc::new(move || {
                        hiding.set(true);
                        toggle.set_active(false);
                        hiding.set(false);
                    }),
                );
            } else {
                crate::notebook_ui::hide_in_pane(&notebook_paned);
            }
        });
    }
    reader.borrow_mut().close_hooks.push(Rc::new(move || {
        crate::notebook_ui::hide_in_pane(&notebook_paned);
    }));
}
