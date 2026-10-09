use super::*;

/// Wraps `picture` in an `Overlay` with a semi-transparent `DrawingArea` on top that tracks a
/// live drag rectangle, so a highlight/underline/strikeout is visible *while* it's being
/// dragged instead of only appearing once the drag ends. Returns the overlay (to use in the
/// widget tree in place of `picture`), the preview `DrawingArea` itself (drag handlers call
/// `.queue_draw()` on it after updating the rect), and the shared cell those handlers write
/// into — `Some((x0, y0, x1, y1))` in the picture's own pixel space while dragging, `None`
/// otherwise. The preview doesn't try to match the final narrowed underline/strikeout band —
/// it's just the raw drag rectangle in the current draw colour, which is enough to show what's
/// about to be created.
pub(super) type PreviewRects = Option<Vec<(f64, f64, f64, f64)>>;
pub(super) type PreviewCache =
    Rc<RefCell<Option<((u16, i32, i32, i32, i32), std::time::Instant, PreviewRects)>>>;

/// Selection geometry is a full re-read of the PDF, so the live preview recomputes it at most
/// this often while the pointer moves.
pub(super) const PREVIEW_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(60);

pub(super) fn paint_preview_rects(
    cr: &gtk4::cairo::Context,
    rects: PreviewRects,
    drag: (f64, f64, f64, f64),
) {
    match rects {
        Some(rects) if !rects.is_empty() => {
            for (rx0, ry0, rx1, ry1) in rects {
                cr.rectangle(
                    rx0.min(rx1),
                    ry0.min(ry1),
                    (rx1 - rx0).abs(),
                    (ry1 - ry0).abs(),
                );
            }
        }
        _ => {
            let (x0, y0, x1, y1) = drag;
            cr.rectangle(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
        }
    }
    let _ = cr.fill();
}

/// A drag rectangle in a picture's own pixel space: `(x0, y0, x1, y1)`.
pub(super) type DragRectCell = Rc<Cell<Option<(f64, f64, f64, f64)>>>;
/// `page_of` resolves which page this preview belongs to at draw time — a fixed value in
/// continuous-scroll mode (one overlay per page), but the reader's *current* page in paged
/// mode (one recycled overlay, page changes as the user navigates).
pub(super) fn build_drag_preview_overlay(
    picture: &gtk4::Picture,
    reader: &Rc<RefCell<ReaderState>>,
    page_of: impl Fn() -> u16 + 'static,
) -> (gtk4::Overlay, gtk4::DrawingArea, DragRectCell) {
    let page_of: Rc<dyn Fn() -> u16> = Rc::new(page_of);
    let live_rect: DragRectCell = Rc::new(Cell::new(None));

    let preview = gtk4::DrawingArea::new();
    preview.set_can_target(false);
    preview.set_hexpand(true);
    preview.set_vexpand(true);
    {
        let live_rect = live_rect.clone();
        let reader = reader.clone();
        let preview_cache: PreviewCache = Rc::new(RefCell::new(None));
        let page_of = page_of.clone();
        preview.set_draw_func(move |_area, cr, w, h| {
            let Some((x0, y0, x1, y1)) = live_rect.get() else {
                return;
            };
            let select_mode = reader.borrow().draw_kind.is_none();
            let rgba = if select_mode {
                SELECTION_RGBA
            } else {
                annotation_rgba(Some(&reader.borrow().draw_color))
            };
            cr.set_source_rgba(
                rgba[0] as f64 / 255.0,
                rgba[1] as f64 / 255.0,
                rgba[2] as f64 / 255.0,
                (rgba[3] as f64 / 255.0 * 1.6).min(1.0),
            );

            // Line-aware preview: what a drag from (x0,y0) to (x1,y1) would actually select,
            // not just its own bounding rectangle — matches what `save_drag_annotation`/
            // `copy_drag_selection` compute at drag-end, so the preview doesn't lie about
            // what's about to happen. Falls back to the plain rectangle over blank space
            // (a figure, a margin) where there's no text to hug.
            let page = page_of();
            let key = (
                page,
                (x0 / 4.0) as i32,
                (y0 / 4.0) as i32,
                (x1 / 4.0) as i32,
                (y1 / 4.0) as i32,
            );
            let reuse = {
                let c = preview_cache.borrow();
                c.as_ref().is_some_and(|(k, at, _)| {
                    *k == key || (k.0 == key.0 && at.elapsed() < PREVIEW_MIN_INTERVAL)
                })
            };
            if reuse {
                let rects = preview_cache.borrow().as_ref().and_then(|c| c.2.clone());
                paint_preview_rects(cr, rects, (x0, y0, x1, y1));
                return;
            }
            let line_rects = (w > 0 && h > 0)
                .then(|| {
                    let r = reader.borrow();
                    let geom = r.geom(page)?;
                    let (w, h) = (w as f64, h as f64);
                    let (sx, sy) = geom.px_to_pdf(x0, y0, w, h);
                    let (ex, ey) = geom.px_to_pdf(x1, y1, w, h);
                    let sel = fond_doc::select_text_range(
                        r.pdfium,
                        r.bytes(),
                        page,
                        sx as f32,
                        sy as f32,
                        ex as f32,
                        ey as f32,
                    )
                    .ok()
                    .flatten()?;
                    Some(
                        sel.quads
                            .iter()
                            .map(|q| geom.quad_px_bounds(q, w, h))
                            .collect::<Vec<_>>(),
                    )
                })
                .flatten();

            *preview_cache.borrow_mut() =
                Some((key, std::time::Instant::now(), line_rects.clone()));
            paint_preview_rects(cr, line_rects, (x0, y0, x1, y1));
        });
    }

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(picture));
    overlay.add_overlay(&tiles::build_tile_layer());
    overlay.add_overlay(&mark_layer::build_mark_layer(reader, {
        let page_of = page_of.clone();
        move || page_of()
    }));
    overlay.add_overlay(&preview);
    mark_edit::install(&overlay, reader, page_of.clone());
    hover_preview::install(&overlay, reader, page_of.clone());

    (overlay, preview, live_rect)
}
