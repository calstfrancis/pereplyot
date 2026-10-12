use super::*;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

const KEEP_DAYS: u64 = 1;

fn unlocked_dir() -> PathBuf {
    glib::user_cache_dir().join("pereplyot").join("unlocked")
}

fn unlocked_copy(hash: &str) -> PathBuf {
    let safe: String = hash.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    unlocked_dir().join(format!("{safe}.pdf"))
}

enum Attempt {
    Open,
    NeedsPassword,
    Unreadable,
}

fn is_password_error(debug: &str) -> bool {
    debug.contains("Password")
}

fn attempt(path: &std::path::Path, password: Option<&str>) -> Result<Attempt, String> {
    let pdfium = crate::pdfium::get().map_err(|e| e.to_string())?;
    match pdfium.load_pdf_from_file(path, password) {
        Ok(_) => Ok(Attempt::Open),
        Err(e) if is_password_error(&format!("{e:?}")) => Ok(Attempt::NeedsPassword),
        Err(_) => Ok(Attempt::Unreadable),
    }
}

fn save_unlocked(path: &std::path::Path, hash: &str, password: &str) -> Result<PathBuf, String> {
    let dir = unlocked_dir();
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|e| e.to_string())?;
    sweep(&dir);
    let out = unlocked_copy(hash);
    let pdfium = crate::pdfium::get().map_err(|e| e.to_string())?;
    let doc = pdfium
        .load_pdf_from_file(path, Some(password))
        .map_err(|e| format!("{e:?}"))?;
    let mut plain = pdfium.create_new_pdf().map_err(|e| format!("{e:?}"))?;
    plain
        .pages_mut()
        .append(&doc)
        .map_err(|e| format!("{e:?}"))?;
    plain.save_to_file(&out).map_err(|e| format!("{e:?}"))?;
    Ok(out)
}

fn sweep(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let limit = std::time::Duration::from_secs(KEEP_DAYS * 24 * 3600);
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Where to read the document from: the file itself, or — when it needs a password — an unlocked
/// copy kept in the cache for the day, so every other reader of the path works unchanged.
/// Asks for the password when needed; `open` gets `None` if the reader should not open at all.
pub(super) fn with_readable_path(
    window: &adw::ApplicationWindow,
    pdf_hash: &str,
    path: &std::path::Path,
    open: impl FnOnce(Option<PathBuf>) + 'static,
) {
    let cached = unlocked_copy(pdf_hash);
    if cached.is_file() {
        return open(Some(cached));
    }
    match attempt(path, None) {
        Ok(Attempt::NeedsPassword) => ask(
            window,
            pdf_hash.to_string(),
            path.to_path_buf(),
            None,
            Box::new(open),
        ),
        _ => open(Some(path.to_path_buf())),
    }
}

fn ask(
    window: &adw::ApplicationWindow,
    hash: String,
    path: PathBuf,
    wrong: Option<&'static str>,
    open: Box<dyn FnOnce(Option<PathBuf>)>,
) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dialog = adw::MessageDialog::new(
        Some(window),
        Some("Password needed"),
        Some(&format!(
            "\u{201c}{name}\u{201d} is protected. Pereplyot opens an unlocked copy kept in its cache \
             for a day; your file is not changed.{}",
            wrong.map(|w| format!("\n\n{w}")).unwrap_or_default()
        )),
    );
    let entry = gtk4::PasswordEntry::builder()
        .show_peek_icon(true)
        .activates_default(true)
        .build();
    entry.set_widget_name("pdf-password");
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "Cancel"), ("open", "Open")]);
    dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("open"));
    dialog.set_close_response("cancel");
    let window = window.clone();
    let open = std::cell::RefCell::new(Some(open));
    dialog.connect_response(None, {
        let entry = entry.clone();
        move |_, id| {
            let Some(open) = open.borrow_mut().take() else {
                return;
            };
            if id != "open" {
                return open(None);
            }
            let password = entry.text().to_string();
            let (window, hash, path) = (window.clone(), hash.clone(), path.clone());
            glib::spawn_future_local(async move {
                let job = {
                    let (path, hash) = (path.clone(), hash.clone());
                    gtk4::gio::spawn_blocking(move || match attempt(&path, Some(&password)) {
                        Ok(Attempt::Open) => save_unlocked(&path, &hash, &password).map(Some),
                        Ok(Attempt::NeedsPassword) => Ok(None),
                        Ok(Attempt::Unreadable) => Err("This PDF could not be read.".to_string()),
                        Err(e) => Err(e),
                    })
                    .await
                };
                match job {
                    Ok(Ok(Some(copy))) => open(Some(copy)),
                    Ok(Ok(None)) => ask(&window, hash, path, Some("That password was not right."), open),
                    Ok(Err(e)) => {
                        gtk4::AlertDialog::builder()
                            .message("Could not unlock PDF")
                            .detail(e)
                            .build()
                            .show(Some(&window));
                        open(None)
                    }
                    Err(_) => open(None),
                }
            });
        }
    });
    dialog.present();
    entry.grab_focus();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/locked.pdf")
    }

    #[test]
    fn a_protected_pdf_asks_for_a_password_and_the_right_one_opens_it() {
        assert!(matches!(attempt(&locked(), None), Ok(Attempt::NeedsPassword)));
        assert!(matches!(attempt(&locked(), Some("nope")), Ok(Attempt::NeedsPassword)));
        assert!(matches!(attempt(&locked(), Some("secret")), Ok(Attempt::Open)));

        let out = save_unlocked(&locked(), "unit-test-hash", "secret").unwrap();
        assert!(matches!(attempt(&out, None), Ok(Attempt::Open)));
        let pdfium = crate::pdfium::get().unwrap();
        let doc = pdfium.load_pdf_from_file(&out, None).unwrap();
        let original = pdfium.load_pdf_from_file(&locked(), Some("secret")).unwrap();
        assert_eq!(doc.pages().len(), original.pages().len());
        let text = doc.pages().get(0).unwrap().text().unwrap().all();
        assert!(!text.trim().is_empty());
        let _ = std::fs::remove_file(out);
    }
}
