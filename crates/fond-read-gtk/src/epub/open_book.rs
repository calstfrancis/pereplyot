use super::*;

/// Open the EPUB and make sure its contents are extracted to the cache (content-addressed by
/// hash, so a repeat open reuses it); shows an error dialog and returns `None` on failure.
pub(super) fn open_book_cached(
    blob: &std::path::Path,
    hash: &str,
    window: &adw::ApplicationWindow,
) -> Option<(fond_doc::EpubBook, PathBuf)> {
    let book = match fond_doc::open_book(blob) {
        Ok(b) => b,
        Err(e) => {
            gtk4::AlertDialog::builder()
                .message("Could not open EPUB")
                .detail(e.to_string())
                .build()
                .show(Some(window));
            return None;
        }
    };

    let hex = hash.split_once(':').map(|(_, h)| h).unwrap_or(hash);
    let cache_dir = glib::user_cache_dir()
        .join("pereplyot")
        .join("epub")
        .join(hex);
    if !cache_dir.join(".complete").exists() {
        let partial = cache_dir.with_extension("partial");
        let _ = std::fs::remove_dir_all(&partial);
        let _ = std::fs::remove_dir_all(&cache_dir);
        let extracted = std::fs::create_dir_all(&partial)
            .map_err(|e| e.to_string())
            .and_then(|_| fond_doc::extract_epub(blob, &partial).map_err(|e| e.to_string()))
            .and_then(|_| std::fs::write(partial.join(".complete"), b"").map_err(|e| e.to_string()))
            .and_then(|_| std::fs::rename(&partial, &cache_dir).map_err(|e| e.to_string()));
        if let Err(e) = extracted {
            let _ = std::fs::remove_dir_all(&partial);
            gtk4::AlertDialog::builder()
                .message("Could not open EPUB")
                .detail(e)
                .build()
                .show(Some(window));
            return None;
        }
    }
    Some((book, cache_dir))
}
