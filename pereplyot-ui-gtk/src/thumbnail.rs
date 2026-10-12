//! Library-card thumbnails: a page-1 PDFium render (or an EPUB's cover), cached as a PNG under the user cache
//! directory keyed by content hash (so the cache never goes stale).

use std::path::Path;

use gtk4::prelude::*;
use gtk4::{gdk, glib};

use fond_read_gtk::history::DocKind;

/// Render width for a card `card_px` wide: twice that for hi-dpi, snapped to a few buckets so
/// dragging the size slider doesn't fill the cache with one file per pixel.
pub fn bucket_width(card_px: u32) -> u32 {
    [160, 240, 360, 560]
        .into_iter()
        .find(|b| *b >= card_px * 2)
        .unwrap_or(560)
}

fn cache_path(hash: &str, width: u32) -> std::path::PathBuf {
    glib::user_cache_dir()
        .join("pereplyot")
        .join("thumbs")
        .join(format!("{hash}-{width}.png"))
}

/// The thumbnail already cached for this document, if there is one — a small PNG to decode,
/// cheap enough for the GTK thread.
pub fn cached_texture(hash: &str, card_px: u32) -> Option<gdk::Texture> {
    gdk::Texture::from_filename(cache_path(hash, bucket_width(card_px))).ok()
}

/// One thumbnail is made at a time: each means reading a whole file, and a library added all at
/// once would otherwise read dozens of them into memory together.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Make sure a thumbnail is cached for `path`: a render of a PDF's first page, or an EPUB's cover
/// picture. Blocking — run it off the GTK thread. False if none could be made (an unreadable
/// file, or a book with no cover); a document that read fine but has nothing to show is
/// remembered, so it is not worked through again each time the library is drawn.
pub fn ensure_cached(kind: DocKind, path: &Path, hash: &str, card_px: u32) -> bool {
    let width = bucket_width(card_px);
    let cached = cache_path(hash, width);
    if cached.is_file() {
        return true;
    }
    let none = cached.with_extension("none");
    if none.is_file() {
        return false;
    }
    let _gate = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    if cached.is_file() {
        return true;
    }
    if let Some(texture) = render_uncached(kind, path, width) {
        if let Some(dir) = cached.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        return texture.save_to_png(&cached).is_ok();
    }
    if path.is_file() {
        if let Some(dir) = none.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&none, b"");
    }
    false
}

fn render_uncached(kind: DocKind, path: &Path, width: u32) -> Option<gdk::Texture> {
    match kind {
        DocKind::Pdf => {
            let bytes = std::fs::read(path).ok()?;
            let pdfium = fond_read_gtk::pdfium::get().ok()?;
            let rp = fond_doc::render_page(pdfium, &bytes, 0, width).ok()?;
            let data = glib::Bytes::from(&rp.rgba);
            let texture = gdk::MemoryTexture::new(
                rp.width as i32,
                rp.height as i32,
                gdk::MemoryFormat::R8g8b8a8,
                &data,
                (rp.width * 4) as usize,
            );
            Some(texture.upcast())
        }
        DocKind::Epub => {
            let bytes = fond_read_gtk::epub_cover::cover_bytes(path)?;
            gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok()
        }
    }
}
