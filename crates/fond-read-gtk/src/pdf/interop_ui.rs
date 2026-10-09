use super::*;

use gtk4::gio;

/// Two ways across to other PDF readers: marks made there come in once, offered when the PDF is
/// opened, and a copy of the PDF with the marks made here goes out.
pub(super) fn install(ui: &PdfUi) {
    {
        let ui = ui.clone();
        ui.clone()
            .copy_button
            .connect_clicked(move |_| save_copy(&ui));
    }
    if !ui.reader.borrow().is_pane {
        offer_import(ui);
    }
}

fn offer_import(ui: &PdfUi) {
    if ui.host.import_offered() {
        return;
    }
    let path = ui.reader.borrow().path.clone();
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let read = gio::spawn_blocking(move || {
            let pdfium = crate::pdfium::get().ok()?;
            let bytes = std::fs::read(&path).ok()?;
            fond_doc::interop::read_marks(pdfium, &bytes).ok()
        })
        .await
        .ok()
        .flatten();
        let Some(read) = read else {
            return;
        };
        let incoming = {
            let r = ui.reader.borrow();
            let sidecar = r.store.sidecar();
            #[allow(clippy::let_and_return)]
            let found = crate::interop::not_yet_present(
                &sidecar,
                crate::interop::annotations_from_marks(&read),
            );
            found
        };
        if incoming.is_empty() {
            return;
        }
        ui.host.set_import_offered();
        let n = incoming.len();
        let message = format!(
            "This PDF has {n} annotation{} made in another app.",
            if n == 1 { "" } else { "s" }
        );
        let reader = ui.reader.clone();
        let host = ui.host.clone();
        ui.host.notify_action(
            &message,
            "Bring them in",
            Rc::new(move || {
                let store = reader.borrow().store.clone();
                let mut added = 0;
                for a in incoming.clone() {
                    if store.add(a).is_ok() {
                        added += 1;
                    }
                }
                host.notify(&format!("Brought in {added} annotations"));
            }),
        );
    });
}

fn save_copy(ui: &PdfUi) {
    let (path, marks) = {
        let r = ui.reader.borrow();
        let sidecar = r.store.sidecar();
        let marks = crate::interop::marks_to_write(&sidecar);
        (r.path.clone(), marks)
    };
    if marks.is_empty() {
        ui.host.notify("Nothing to put in a copy yet");
        return;
    }
    let dialog = gtk4::FileDialog::builder()
        .title("Save a copy with annotations")
        .initial_name(format!("{} — annotated.pdf", ui.title))
        .build();
    let host = ui.host.clone();
    let window = ui.reader_window.clone();
    dialog.save(Some(&window), gio::Cancellable::NONE, move |result| {
        let Some(out) = result.ok().and_then(|f| f.path()) else {
            return;
        };
        let same_file = match (out.canonicalize(), path.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => out == path,
        };
        if same_file {
            host.notify(
                "That is the original — choose another name; the original is never changed",
            );
            return;
        }
        let source = path.clone();
        glib::spawn_future_local(async move {
            let written = gio::spawn_blocking(move || {
                let pdfium = crate::pdfium::get().map_err(|e| e.to_string())?;
                let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
                fond_doc::interop::write_marks(pdfium, &bytes, &marks).map_err(|e| e.to_string())
            })
            .await
            .map_err(|_| "the copy could not be made".to_string())
            .and_then(|r| r);
            match written.and_then(|bytes| {
                crate::fsutil::write_atomic(&out, &bytes).map_err(|e| e.to_string())
            }) {
                Ok(()) => host.notify(&format!("Saved a copy to {}", out.display())),
                Err(e) => host.notify(&format!("Couldn't save the copy: {e}")),
            }
        });
    });
}
