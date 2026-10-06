//! Library-card thumbnails: a page-1 PDFium render, cached as a PNG under the user cache
//! directory keyed by content hash (so the cache never goes stale).

use std::path::Path;

use gtk4::prelude::*;
use gtk4::{gdk, glib};

use fond_read_gtk::history::DocKind;

/// A rendered thumbnail for `path`, or `None` if one couldn't be produced — an unreadable
/// file, or (for now) an EPUB: `fond_doc::EpubBook` exposes no manifest/resource access, so
/// real cover extraction needs a new `fond-doc` function, which needs a Kartoteka release
/// to reach Pereplyot's pinned tag. Not this task's call to make unilaterally (root
/// `CLAUDE.md`'s release policy) — EPUBs get a placeholder icon in the card instead.
/// Render width for a card `card_px` wide: twice that for hi-dpi, snapped to a few buckets so
/// dragging the size slider doesn't fill the cache with one file per pixel.
pub fn bucket_width(card_px: u32) -> u32 {
    [160, 240, 360, 560]
        .into_iter()
        .find(|b| *b >= card_px * 2)
        .unwrap_or(560)
}

pub fn render_thumbnail(
    kind: DocKind,
    path: &Path,
    hash: &str,
    card_px: u32,
) -> Option<gdk::Texture> {
    let width = bucket_width(card_px);
    let cached = glib::user_cache_dir()
        .join("pereplyot")
        .join("thumbs")
        .join(format!("{hash}-{width}.png"));
    if let Ok(texture) = gdk::Texture::from_filename(&cached) {
        return Some(texture);
    }
    let texture = render_uncached(kind, path, width)?;
    if let Some(dir) = cached.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = texture.save_to_png(&cached);
    Some(texture)
}

fn render_uncached(kind: DocKind, path: &Path, width: u32) -> Option<gdk::Texture> {
    match kind {
        DocKind::Pdf => {
            let bytes = std::fs::read(path).ok()?;
            let pdfium = fond_doc::bind_pdfium().ok()?;
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
        DocKind::Epub => None,
    }
}
