//! Mapping between a rendered PDF page's pixels and the page's own PDF user space.
//!
//! PDFium renders a page's *visible box* (media ∩ crop), rotated by the page's own `/Rotate`,
//! while text positions and every stored quadpoint (annotations, search matches, selections)
//! live in unrotated user space — whose origin is the box's lower-left corner, which is
//! frequently not (0, 0) in publisher and scanned PDFs. Assuming a (0, 0) origin and no
//! rotation (as this reader originally did) shifts every drag off the text under the pointer,
//! often onto blank margin where nothing gets selected at all.

use pdfium_render::prelude::PdfPageRenderRotation;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PageGeom {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
    /// The page's own `/Rotate`, clockwise: 0, 90, 180 or 270. Not the reader's view-only
    /// rotation, which is applied to the finished pixels separately.
    pub rotation: u16,
}

impl PageGeom {
    /// Read `index`'s geometry. `None` if the document or page can't be loaded.
    pub fn read_doc(
        document: &pdfium_render::prelude::PdfDocument<'_>,
        index: u16,
    ) -> Option<Self> {
        let page = document.pages().get(index).ok()?;
        let bounds = page.boundaries().bounding().ok()?.bounds;
        let rotation = match page.rotation().unwrap_or(PdfPageRenderRotation::None) {
            PdfPageRenderRotation::None => 0,
            PdfPageRenderRotation::Degrees90 => 90,
            PdfPageRenderRotation::Degrees180 => 180,
            PdfPageRenderRotation::Degrees270 => 270,
        };
        let geom = Self {
            left: bounds.left().value as f64,
            bottom: bounds.bottom().value as f64,
            right: bounds.right().value as f64,
            top: bounds.top().value as f64,
            rotation,
        };
        (geom.width() > 0.0 && geom.height() > 0.0).then_some(geom)
    }

    fn width(&self) -> f64 {
        self.right - self.left
    }

    fn height(&self) -> f64 {
        self.top - self.bottom
    }

    /// The page's on-screen size in points — width and height swapped for a quarter-turn.
    pub fn display_size(&self) -> (f32, f32) {
        if self.rotation % 180 == 90 {
            (self.height() as f32, self.width() as f32)
        } else {
            (self.width() as f32, self.height() as f32)
        }
    }

    /// A pixel position on a `render_w`×`render_h` render of this page (top-down) to PDF user
    /// space, clamped onto the page.
    pub fn px_to_pdf(&self, px: f64, py: f64, render_w: f64, render_h: f64) -> (f64, f64) {
        let u = (px / render_w.max(1.0)).clamp(0.0, 1.0);
        let v = (py / render_h.max(1.0)).clamp(0.0, 1.0);
        let (w, h) = (self.width(), self.height());
        match self.rotation {
            90 => (self.left + v * w, self.bottom + u * h),
            180 => (self.right - u * w, self.bottom + v * h),
            270 => (self.right - v * w, self.top - u * h),
            _ => (self.left + u * w, self.top - v * h),
        }
    }

    /// The inverse of [`Self::px_to_pdf`], unclamped.
    pub fn pdf_to_px(&self, x: f64, y: f64, render_w: f64, render_h: f64) -> (f64, f64) {
        let (w, h) = (self.width(), self.height());
        let (u, v) = match self.rotation {
            90 => ((y - self.bottom) / h, (x - self.left) / w),
            180 => ((self.right - x) / w, (y - self.bottom) / h),
            270 => ((self.top - y) / h, (self.right - x) / w),
            _ => ((x - self.left) / w, (self.top - y) / h),
        };
        (u * render_w, v * render_h)
    }

    /// Re-express user-space quadpoints in the rendered page's own point space — origin at
    /// its lower-left corner, already rotated — which is what `fond_doc`'s blend functions
    /// assume when scaling quads onto the render's pixels.
    pub fn quads_to_display(&self, quads: &[[f64; 8]]) -> Vec<[f64; 8]> {
        let (dw, dh) = self.display_size();
        let (dw, dh) = (dw as f64, dh as f64);
        quads
            .iter()
            .map(|q| {
                let mut out = [0.0; 8];
                for i in 0..4 {
                    let (px, py) = self.pdf_to_px(q[i * 2], q[i * 2 + 1], dw, dh);
                    out[i * 2] = px;
                    out[i * 2 + 1] = dh - py;
                }
                out
            })
            .collect()
    }

    /// A quad's axis-aligned bounding box in render pixels: `(x0, y0, x1, y1)`, top-down.
    pub fn quad_px_bounds(
        &self,
        q: &[f64; 8],
        render_w: f64,
        render_h: f64,
    ) -> (f64, f64, f64, f64) {
        let mut b = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for i in 0..4 {
            let (px, py) = self.pdf_to_px(q[i * 2], q[i * 2 + 1], render_w, render_h);
            b = (b.0.min(px), b.1.min(py), b.2.max(px), b.3.max(py));
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom(rotation: u16) -> PageGeom {
        PageGeom {
            left: 60.0,
            bottom: 40.0,
            right: 660.0,
            top: 840.0,
            rotation,
        }
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn unrotated_offset_origin() {
        let g = geom(0);
        assert_eq!(g.display_size(), (600.0, 800.0));
        // Top-left pixel is the box's top-left corner, not (0, height).
        assert!(close(g.px_to_pdf(0.0, 0.0, 300.0, 400.0), (60.0, 840.0)));
        assert!(close(
            g.px_to_pdf(300.0, 400.0, 300.0, 400.0),
            (660.0, 40.0)
        ));
        assert!(close(
            g.px_to_pdf(150.0, 100.0, 300.0, 400.0),
            (360.0, 640.0)
        ));
    }

    #[test]
    fn quarter_turn_swaps_display_size_and_corners() {
        let g = geom(90);
        assert_eq!(g.display_size(), (800.0, 600.0));
        // Clockwise: the page's bottom-left lands top-left, its top-left lands top-right.
        assert!(close(g.px_to_pdf(0.0, 0.0, 400.0, 300.0), (60.0, 40.0)));
        assert!(close(g.px_to_pdf(400.0, 0.0, 400.0, 300.0), (60.0, 840.0)));
        assert!(close(g.px_to_pdf(0.0, 300.0, 400.0, 300.0), (660.0, 40.0)));
    }

    #[test]
    fn round_trips_every_rotation() {
        for rotation in [0, 90, 180, 270] {
            let g = geom(rotation);
            let (w, h) = g.display_size();
            let (w, h) = (w as f64 / 2.0, h as f64 / 2.0);
            for (px, py) in [(0.0, 0.0), (w * 0.3, h * 0.7), (w, h), (w * 0.9, h * 0.1)] {
                let (x, y) = g.px_to_pdf(px, py, w, h);
                assert!(
                    close(g.pdf_to_px(x, y, w, h), (px, py)),
                    "rotation {rotation}"
                );
            }
        }
    }

    #[test]
    fn quads_to_display_moves_offset_origin_to_zero() {
        let g = geom(0);
        let q = [60.0, 840.0, 160.0, 840.0, 60.0, 820.0, 160.0, 820.0];
        let d = g.quads_to_display(&[q])[0];
        assert!(close((d[0], d[1]), (0.0, 800.0)));
        assert!(close((d[6], d[7]), (100.0, 780.0)));
    }
}
