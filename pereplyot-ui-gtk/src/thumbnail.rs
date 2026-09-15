//! Library-card thumbnails, generated on demand rather than cached to disk — a single
//! page-1 PDFium render at a small width is cheap enough that regenerating it whenever the
//! Library page is (re)built avoids needing an image-encoding dependency or a cache-
//! invalidation story for a feature this size. Worth revisiting (a disk cache, or moving
//! generation off the main thread) if a real library ever makes this noticeably slow.

use std::path::Path;

use gtk4::prelude::*;
use gtk4::{gdk, glib};

use crate::recents::DocKind;

const THUMBNAIL_WIDTH: u32 = 160;

/// A rendered thumbnail for `path`, or `None` if one couldn't be produced — an unreadable
/// file, or (for now) an EPUB: `fond_doc::EpubBook` exposes no manifest/resource access, so
/// real cover extraction needs a new `fond-doc` function, which needs a Kartoteka release
/// to reach Pereplyot's pinned tag. Not this task's call to make unilaterally (root
/// `CLAUDE.md`'s release policy) — EPUBs get a placeholder icon in the card instead.
pub fn render_thumbnail(kind: DocKind, path: &Path) -> Option<gdk::Texture> {
    match kind {
        DocKind::Pdf => {
            let bytes = std::fs::read(path).ok()?;
            let pdfium = fond_doc::bind_pdfium().ok()?;
            let rp = fond_doc::render_page(pdfium, &bytes, 0, THUMBNAIL_WIDTH).ok()?;
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
