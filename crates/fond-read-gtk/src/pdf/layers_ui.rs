//! Sharing your notes as a layer, adding someone else's, showing and hiding it, and adopting its marks.

use super::*;
use crate::layer::{adopted_id, LayerFile};

fn page_labels_max(reader: &Rc<RefCell<ReaderState>>) -> u32 {
    reader.borrow().count as u32
}

/// Writes your notes on this document to a file to hand to someone.
pub(super) fn share(ui: &PdfUi) {
    let (hash, title, sidecar) = {
        let r = ui.reader.borrow();
        let hash = r
            .doc_ref
            .as_ref()
            .map(|d| d.hash.clone())
            .unwrap_or_default();
        let title = r
            .doc_ref
            .as_ref()
            .map(|d| d.title.clone())
            .unwrap_or_default();
        let sidecar = r.store.sidecar().clone();
        (hash, title, sidecar)
    };
    if sidecar.annotations.is_empty() {
        ui.host
            .notify("You have no notes on this document to share yet");
        return;
    }
    let name = match ui.host.person_name() {
        n if n.trim().is_empty() => "Me".to_string(),
        n => n,
    };
    let layer = LayerFile::from_sidecar(&name, &hash, &title, &sidecar);
    let dialog = gtk4::FileDialog::builder()
        .title("Share my notes on this document")
        .initial_name(format!("{title} — notes ({name}).pereplyot-layer.json"))
        .build();
    let host = ui.host.clone();
    dialog.save(
        Some(&ui.reader_window.get()),
        gtk4::gio::Cancellable::NONE,
        move |result| {
            let Some(path) = result.ok().and_then(|f| f.path()) else {
                return;
            };
            match layer.to_json().and_then(|j| {
                crate::fsutil::write_atomic(&path, j.as_bytes()).map_err(|e| e.to_string())
            }) {
                Ok(()) => host.notify(&format!(
                    "Shared {} notes as {}",
                    layer.annotations.len(),
                    path.display()
                )),
                Err(e) => host.notify(&format!("Couldn't write the file: {e}")),
            }
        },
    );
}

/// Adds someone's layer file. If it was made on a different copy of the document, its marks are found
/// again in this one by their words.
pub(super) fn add(ui: &PdfUi) {
    let dialog = gtk4::FileDialog::builder().title("Add a layer").build();
    let (reader, host) = (ui.reader.clone(), ui.host.clone());
    dialog.open(
        Some(&ui.reader_window.get()),
        gtk4::gio::Cancellable::NONE,
        move |result| {
            let Some(path) = result.ok().and_then(|f| f.path()) else {
                return;
            };
            let layer = match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| LayerFile::parse(&t))
            {
                Ok(l) => l,
                Err(e) => {
                    host.notify(&format!("Couldn't read that layer: {e}"));
                    return;
                }
            };
            let (hash, file) = {
                let r = reader.borrow();
                (
                    r.doc_ref
                        .as_ref()
                        .map(|d| d.hash.clone())
                        .unwrap_or_default(),
                    r.path.clone(),
                )
            };
            let (reader, host) = (reader.clone(), host.clone());
            glib::spawn_future_local(async move {
                let layer = if layer.hash == hash {
                    layer
                } else {
                    // Another copy or edition: place their marks by their words, and say so.
                    let moved = {
                        let layer = layer.clone();
                        let hash = hash.clone();
                        gtk4::gio::spawn_blocking(move || {
                            let mut old = fond_annot::AnnotationSidecar::new(&layer.hash);
                            old.annotations = layer.annotations.clone();
                            let pages = old
                                .annotations
                                .iter()
                                .filter_map(|a| a.page)
                                .max()
                                .unwrap_or(1);
                            crate::carry::carry(&old, pages, &file, &hash)
                        })
                        .await
                        .ok()
                        .flatten()
                    };
                    match moved {
                        Some((sidecar, summary)) => {
                            host.notify(&format!(
                                "Their copy differs from yours: {}",
                                summary.describe()
                            ));
                            LayerFile {
                                hash: hash.clone(),
                                annotations: sidecar.annotations,
                                ..layer
                            }
                        }
                        None => {
                            host.notify("Couldn't place that layer on this document");
                            return;
                        }
                    }
                };
                match host.add_layer(&layer) {
                    Ok(()) => {
                        {
                            let mut r = reader.borrow_mut();
                            r.layers.retain(|(l, _)| l.name != layer.name);
                            r.layers.push((layer.clone(), true));
                        }
                        mark_layer::redraw_all(&reader);
                        host.notify(&format!(
                            "Added {}'s notes ({} marks)",
                            layer.name,
                            layer.annotations.len()
                        ));
                    }
                    Err(e) => host.notify(&format!("Couldn't keep the layer: {e}")),
                }
            });
        },
    );
}

pub(super) fn toggle(ui: &PdfUi, name: &str) {
    {
        let mut r = ui.reader.borrow_mut();
        if let Some((_, on)) = r.layers.iter_mut().find(|(l, _)| l.name == name) {
            *on = !*on;
        }
    }
    mark_layer::redraw_all(&ui.reader);
}

pub(super) fn remove(ui: &PdfUi, name: &str) {
    ui.reader
        .borrow_mut()
        .layers
        .retain(|(l, _)| l.name != name);
    ui.host.remove_layer(name);
    mark_layer::redraw_all(&ui.reader);
}

/// Copies the layer's marks on the page in view into your own notes.
pub(super) fn adopt_page(ui: &PdfUi, name: &str) {
    let (page, marks) = {
        let r = ui.reader.borrow();
        let page = r.page as u32 + 1;
        let marks: Vec<fond_annot::Annotation> = r
            .layers
            .iter()
            .find(|(l, _)| l.name == name)
            .map(|(l, _)| {
                l.annotations
                    .iter()
                    .filter(|a| a.page == Some(page))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        (page, marks)
    };
    if marks.is_empty() {
        ui.host
            .notify(&format!("{name} has no marks on page {page}"));
        return;
    }
    let store = ui.reader.borrow().store.clone();
    let mut added = 0;
    store.group(|| {
        for mut a in marks {
            a.id = adopted_id(&a.id);
            a.extra.remove(crate::carry::STATE_KEY);
            if store.add(a).is_ok() {
                added += 1;
            }
        }
    });
    ui.host
        .notify(&format!("Took {added} of {name}'s marks into your notes"));
    let _ = page_labels_max;
}
