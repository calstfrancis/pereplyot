use super::*;

pub(super) fn find_in_path(program: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(program))
            .find(|p| p.is_file())
    })
}

/// A scanned PDF has no text for highlights to snap to and nothing for search to find. Warn
/// once on open, and offer to run `ocrmypdf` (when installed) to make a searchable copy.
pub(super) fn warn_if_no_text_layer(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    blob: &std::path::Path,
) {
    let has_text = {
        let r = reader.borrow();
        let Some(doc) = &r.doc else { return };
        let probe = r.count.min(5);
        (0..probe).any(|i| {
            let Ok(page) = doc.pages().get(i) else {
                return false;
            };
            crate::page_text(&page).chars().any(|c| !c.is_whitespace())
        })
    };
    if has_text {
        return;
    }
    let Some(ocr) = find_in_path("ocrmypdf") else {
        host.notify(
            "This PDF has no text layer, so highlights can't follow text and search finds nothing. \
             Install ocrmypdf to add one.",
        );
        return;
    };
    let out = blob.with_file_name(format!(
        "{}-ocr.pdf",
        blob.file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    ));
    let input = blob.to_path_buf();
    let host_for_action = host.clone();
    host.notify_action(
        "This PDF has no text layer, so highlights can't follow text and search finds nothing.",
        "Run OCR",
        Rc::new(move || {
            let host = host_for_action.clone();
            let (ocr, input, out) = (ocr.clone(), input.clone(), out.clone());
            host.notify("Running OCR — this can take a while; the original stays untouched");
            glib::spawn_future_local(async move {
                let job = {
                    let (ocr, input, out) = (ocr.clone(), input.clone(), out.clone());
                    gtk4::gio::spawn_blocking(move || {
                        std::process::Command::new(ocr)
                            .arg("--skip-text")
                            .arg(&input)
                            .arg(&out)
                            .output()
                    })
                };
                match job.await {
                    Ok(Ok(o)) if o.status.success() => {
                        let host_open = host.clone();
                        let out_open = out.clone();
                        host.notify_action(
                            &format!(
                                "OCR finished: {}",
                                out.file_name()
                                    .map(|n| n.to_string_lossy())
                                    .unwrap_or_default()
                            ),
                            "Open",
                            Rc::new(move || host_open.open_document(&out_open)),
                        );
                    }
                    Ok(Ok(o)) => {
                        let err = String::from_utf8_lossy(&o.stderr);
                        host.notify(&format!(
                            "OCR failed: {}",
                            err.lines().last().unwrap_or("ocrmypdf reported an error")
                        ));
                    }
                    _ => host.notify("OCR could not be started"),
                }
            });
        }),
    );
}
