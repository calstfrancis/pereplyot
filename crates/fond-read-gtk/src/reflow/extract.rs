//! Reading a page's words, with their positions and type, out of PDFium.

use pdfium_render::prelude::{
    PdfDocument, PdfPageObjectCommon, PdfPageObjectType, PdfPageObjectsCommon,
};

use super::model::{RawPage, Word};
use crate::page_geom::PageGeom;

fn expand_ligature(c: char) -> Option<&'static str> {
    Some(match c {
        'ﬀ' => "ff",
        'ﬁ' => "fi",
        'ﬂ' => "fl",
        'ﬃ' => "ffi",
        'ﬄ' => "ffl",
        'ﬅ' | 'ﬆ' => "st",
        _ => return None,
    })
}

struct Building {
    text: String,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    size: f32,
    bold: bool,
    italic: bool,
}

impl Building {
    fn finish(self) -> Option<Word> {
        let text = self.text.trim().to_string();
        (!text.is_empty()).then_some(Word {
            text,
            x0: self.x0,
            y0: self.y0,
            x1: self.x1,
            y1: self.y1,
            size: self.size,
            bold: self.bold,
            italic: self.italic,
        })
    }
}

/// Every word of page `index`, positioned as the page is displayed. `None` if the page cannot
/// be read or has no text layer.
pub fn extract_page(doc: &PdfDocument<'_>, index: u16) -> Option<RawPage> {
    let geom = PageGeom::read_doc(doc, index)?;
    let (dw, dh) = geom.display_size();
    let (dw, dh) = (dw as f64, dh as f64);
    let page = doc.pages().get(index).ok()?;
    let text = page.text().ok()?;
    let mut words: Vec<Word> = Vec::new();
    let mut current: Option<Building> = None;
    for ch in text.chars().iter() {
        let Some(c) = ch.unicode_char() else {
            continue;
        };
        if c.is_whitespace() || c.is_control() {
            words.extend(current.take().and_then(Building::finish));
            continue;
        }
        let Ok(bounds) = ch.loose_bounds() else {
            continue;
        };
        let (a, b) = geom.pdf_to_px(
            bounds.left().value as f64,
            bounds.bottom().value as f64,
            dw,
            dh,
        );
        let (c2, d) = geom.pdf_to_px(
            bounds.right().value as f64,
            bounds.top().value as f64,
            dw,
            dh,
        );
        let (x0, x1) = (a.min(c2) as f32, a.max(c2) as f32);
        let (y0, y1) = (b.min(d) as f32, b.max(d) as f32);
        let size = ch.scaled_font_size().value;
        let name = ch.font_name().to_ascii_lowercase();
        let bold = ch.font_weight().is_some_and(|w| {
            use pdfium_render::prelude::PdfFontWeight::*;
            matches!(w, Weight600 | Weight700Bold | Weight800 | Weight900)
                || matches!(w, Custom(n) if n >= 600)
        }) || name.contains("bold")
            || name.contains("black");
        let italic = ch.font_is_italic() || name.contains("italic") || name.contains("oblique");
        let piece = expand_ligature(c)
            .map(str::to_string)
            .unwrap_or_else(|| c.to_string());
        let starts_new = match &current {
            None => true,
            Some(w) => {
                let centre = (y0 + y1) / 2.0;
                let w_centre = (w.y0 + w.y1) / 2.0;
                x0 - w.x1 > 0.3 * size.max(1.0)
                    || (centre - w_centre).abs() > 0.5 * size.max(w.size)
                    || (size / w.size.max(0.1) > 1.3)
                    || (w.size / size.max(0.1) > 1.3)
                    || w.bold != bold
                    || w.italic != italic
                    || x1 < w.x0
            }
        };
        if starts_new {
            words.extend(current.take().and_then(Building::finish));
            current = Some(Building {
                text: piece,
                x0,
                y0,
                x1,
                y1,
                size,
                bold,
                italic,
            });
        } else if let Some(w) = current.as_mut() {
            w.text.push_str(&piece);
            w.x0 = w.x0.min(x0);
            w.y0 = w.y0.min(y0);
            w.x1 = w.x1.max(x1);
            w.y1 = w.y1.max(y1);
        }
    }
    words.extend(current.take().and_then(Building::finish));
    let mut rects: Vec<[f32; 4]> = Vec::new();
    for object in page.objects().iter() {
        if !matches!(
            object.object_type(),
            PdfPageObjectType::Path
                | PdfPageObjectType::Image
                | PdfPageObjectType::Shading
                | PdfPageObjectType::XObjectForm
        ) {
            continue;
        }
        let Ok(bounds) = object.bounds() else {
            continue;
        };
        let r = bounds.to_rect();
        let (a, b) = geom.pdf_to_px(r.left().value as f64, r.bottom().value as f64, dw, dh);
        let (c, d) = geom.pdf_to_px(r.right().value as f64, r.top().value as f64, dw, dh);
        let rect = [
            a.min(c) as f32,
            b.min(d) as f32,
            a.max(c) as f32,
            b.max(d) as f32,
        ];
        let area = (rect[2] - rect[0]) * (rect[3] - rect[1]);
        // A page-sized backdrop or scan is not a figure on the page.
        if area < 0.8 * dw as f32 * dh as f32 {
            rects.push(rect);
        }
    }
    Some(RawPage {
        page: index,
        width: dw as f32,
        height: dh as f32,
        words,
        graphics: super::figures::join_touching(&rects, dw as f32, dh as f32),
    })
}
