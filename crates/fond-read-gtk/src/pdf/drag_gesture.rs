use super::*;

pub(super) fn install_drag_gesture(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        reader_window,
        render,
        picture,
        drag_preview,
        drag_live_rect,
        ..
    } = ui.clone();
    // Click-drag on the page creates a highlight: the dragged rectangle (in the render's own
    // pixel grid — the Picture is size-requested to exactly that, so widget-local coordinates
    // from the gesture already are that grid) converts to PDF-space quadpoints via the current
    // page's point size, and is appended to the sidecar and written straight to disk.
    {
        let drag = gtk4::GestureDrag::new();
        let reader = reader.clone();
        let render = render.clone();
        let reader_window_for_drag = reader_window.clone();
        let picture_for_popover = picture.clone();
        let host = host.clone();
        {
            let live_rect = drag_live_rect.clone();
            let drag_preview = drag_preview.clone();
            drag.connect_drag_begin(move |_gesture, start_x, start_y| {
                live_rect.set(Some((start_x, start_y, start_x, start_y)));
                drag_preview.queue_draw();
            });
        }
        {
            let live_rect = drag_live_rect.clone();
            let drag_preview = drag_preview.clone();
            drag.connect_drag_update(move |gesture, offset_x, offset_y| {
                let Some((start_x, start_y)) = gesture.start_point() else {
                    return;
                };
                live_rect.set(Some((
                    start_x,
                    start_y,
                    start_x + offset_x,
                    start_y + offset_y,
                )));
                drag_preview.queue_draw();
            });
        }
        {
            let live_rect = drag_live_rect.clone();
            let drag_preview = drag_preview.clone();
            drag.connect_drag_end(move |gesture, offset_x, offset_y| {
                live_rect.set(None);
                drag_preview.queue_draw();
                if offset_x.abs() < MIN_DRAG_PX && offset_y.abs() < MIN_DRAG_PX {
                    return;
                }
                if reader.borrow().rotation != 0 {
                    // See `ReaderState::rotation`'s doc comment — the coordinate math below
                    // assumes an unrotated page, and the rotate control is meant to be
                    // disabled outside single-page mode anyway, so this should be
                    // unreachable via the UI; guarded regardless since a stray drag
                    // finishing mid-toggle is cheap to rule out.
                    host.notify("Rotate back to 0° to annotate or select text");
                    return;
                }
                let Some((start_x, start_y)) = gesture.start_point() else {
                    return;
                };
                let end_x = start_x + offset_x;
                let end_y = start_y + offset_y;

                let (page, render_w, render_h, page_geom) = {
                    let r = reader.borrow();
                    (r.page, r.render_px.0, r.render_px.1, r.geom(r.page))
                };
                let geom = DragGeometry {
                    render_w,
                    render_h,
                    page: page_geom,
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                };
                if reader.borrow().draw_kind.is_none() {
                    if select_drag_text(&host, &reader, page, &geom) {
                        render();
                        render_continuous_page(&reader, page);
                        let ctx = MarkCtx {
                            host: host.clone(),
                            reader: reader.clone(),
                            reader_window: reader_window_for_drag.clone(),
                        };
                        show_selection_popover(&ctx, &picture_for_popover, end_x, end_y, page);
                    }
                    return;
                }
                save_drag_annotation(&host, &reader, page, geom);
            });
        }
        picture.add_controller(drag);
    }
}
