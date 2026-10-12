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
        if let Err(e) = check_unpacked_size(blob) {
            gtk4::AlertDialog::builder()
                .message("Could not open EPUB")
                .detail(e)
                .build()
                .show(Some(window));
            return None;
        }
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

/// The most an EPUB may unpack to. Books with many large pictures reach a gigabyte or two; past
/// this it is far likelier to be built to fill the disk than to be read.
const MAX_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

fn check_unpacked_size(blob: &std::path::Path) -> Result<(), String> {
    check_unpacked_size_within(blob, MAX_UNPACKED_BYTES)
}

fn check_unpacked_size_within(blob: &std::path::Path, max: u64) -> Result<(), String> {
    let file = std::fs::File::open(blob).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("EPUB error: {e}"))?;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index_raw(i) else {
            continue;
        };
        total = total.saturating_add(entry.size());
        if total > max {
            return Err(
                "This EPUB would unpack to more than 4 GB, so it was not opened.".to_string(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    #[test]
    fn a_book_that_unpacks_to_a_normal_size_passes_and_a_bomb_does_not() {
        let dir = std::env::temp_dir().join(format!("pp-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str, mib: usize| {
            let path = dir.join(name);
            let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .large_file(true);
            z.start_file("a.bin", opts).unwrap();
            let chunk = vec![0u8; 1024 * 1024];
            for _ in 0..mib {
                z.write_all(&chunk).unwrap();
            }
            z.finish().unwrap();
            path
        };
        let limit = 8 * 1024 * 1024;
        assert!(super::check_unpacked_size_within(&make("ok.zip", 5), limit).is_ok());
        assert!(super::check_unpacked_size_within(&make("bomb.zip", 40), limit).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
