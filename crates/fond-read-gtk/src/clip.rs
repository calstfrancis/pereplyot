//! Pictures of clipped areas, for exports: the part of the page drawn at print sharpness. The
//! drawing is done off the main thread; saving the PNG needs GDK and is done on it.

use std::path::Path;

use gtk4::{gdk, glib, prelude::*};
use pdfium_render::prelude::{PdfPoints, PdfRenderConfig, Pixels};

use crate::page_geom::PageGeom;

/// Pixels per point of the clipped picture: 300 dpi.
const SCALE: f64 = 300.0 / 72.0;
const MAX_SIDE_PX: f64 = 4000.0;

pub struct Clip {
    pub(crate) rgba: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// A clipped area as it is kept: the 1-based page and the rectangle `[left, bottom, right,
/// top]` in PDF user space.
#[derive(Clone, Debug)]
pub struct AreaClip {
    pub id: String,
    pub page: u32,
    pub rect: [f64; 4],
    /// For a picture that is already a file (an image clipped from an EPUB): copy this instead of
    /// drawing a part of a page.
    pub copy_from: Option<std::path::PathBuf>,
}

impl AreaClip {
    /// The name the picture is saved under in the export's figures folder.
    pub fn file_name(&self) -> String {
        let ext = self
            .copy_from
            .as_ref()
            .and_then(|p| p.extension())
            .map_or("png".to_string(), |e| {
                e.to_string_lossy().to_ascii_lowercase()
            });
        format!("{}.{ext}", self.id)
    }
}

/// Draw `rect` of page `page` (1-based) of the PDF at `path`. Blocking: call it off the main
/// thread.
pub fn render(path: &Path, page: u32, rect: [f64; 4]) -> Option<Clip> {
    let pdfium = crate::pdfium::get().ok()?;
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    let index = page.checked_sub(1)? as u16;
    let geom = PageGeom::read_doc(&doc, index)?;
    let (dw, dh) = geom.display_size();
    let (dw, dh) = (dw as f64, dh as f64);
    let (ax, ay) = geom.pdf_to_px(rect[0], rect[1], dw, dh);
    let (bx, by) = geom.pdf_to_px(rect[2], rect[3], dw, dh);
    let (x0, x1) = (ax.min(bx), ax.max(bx));
    let (y0, y1) = (ay.min(by), ay.max(by));
    let scale = SCALE.min(MAX_SIDE_PX / (x1 - x0).max(1.0).max(y1 - y0));
    let (w, h) = (
        ((x1 - x0) * scale).round().max(1.0) as u32,
        ((y1 - y0) * scale).round().max(1.0) as u32,
    );
    let pdf_page = doc.pages().get(index).ok()?;
    let per_pt = scale as f32;
    let config = PdfRenderConfig::new()
        .set_fixed_width(w as Pixels)
        .set_fixed_height(h as Pixels)
        .scale_page_by_factor(per_pt)
        .translate(PdfPoints::new(-x0 as f32), PdfPoints::new(-y0 as f32))
        .ok()?;
    let bitmap = pdf_page.render_with_config(&config).ok()?;
    Some(Clip {
        rgba: bitmap.as_rgba_bytes(),
        width: bitmap.width() as u32,
        height: bitmap.height() as u32,
    })
}

/// Write the clip as a PNG. On the main thread.
pub fn save_png(clip: &Clip, out: &Path) -> Result<(), String> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let texture = gdk::MemoryTexture::new(
        clip.width as i32,
        clip.height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from(&clip.rgba[..]),
        clip.width as usize * 4,
    );
    texture.save_to_png(out).map_err(|e| e.to_string())
}
