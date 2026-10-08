use super::*;

pub(super) type Action = Rc<dyn Fn()>;

/// The undo and redo actions, and the header buttons that trigger them.
pub(super) fn build_undo_redo(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<EpubReaderState>>,
    undo_button: &gtk4::Button,
    redo_button: &gtk4::Button,
) -> (Action, Action) {
    let epub_undo: Rc<dyn Fn()> = {
        let reader = reader.clone();
        let host = host.clone();
        Rc::new(move || {
            let store = reader.borrow().store.clone();
            match store.undo() {
                Ok(true) => host.notify("Undid last annotation change"),
                Ok(false) => host.notify("Nothing to undo"),
                Err(e) => host.notify(&format!("Could not undo: {e}")),
            }
        })
    };
    let epub_redo: Rc<dyn Fn()> = {
        let reader = reader.clone();
        let host = host.clone();
        Rc::new(move || {
            let store = reader.borrow().store.clone();
            match store.redo() {
                Ok(true) => host.notify("Redid annotation change"),
                Ok(false) => host.notify("Nothing to redo"),
                Err(e) => host.notify(&format!("Could not redo: {e}")),
            }
        })
    };
    {
        let epub_undo = epub_undo.clone();
        undo_button.connect_clicked(move |_| epub_undo());
    }
    {
        let epub_redo = epub_redo.clone();
        redo_button.connect_clicked(move |_| epub_redo());
    }
    (epub_undo, epub_redo)
}
