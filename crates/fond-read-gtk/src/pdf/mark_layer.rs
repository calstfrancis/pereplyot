use super::*;

const MARK_LAYER_NAME: &str = "mark-layer";

/// A transparent layer over one page that draws its saved marks, search match and selection as
/// vectors. The page picture underneath never changes when a mark does, so editing a highlight
/// costs a redraw of rectangles rather than a new render of the page.
pub(super) fn build_mark_layer(
    reader: &Rc<RefCell<ReaderState>>,
    page_of: impl Fn() -> u16 + 'static,
) -> gtk4::DrawingArea {
    let layer = gtk4::DrawingArea::new();
    layer.set_widget_name(MARK_LAYER_NAME);
    layer.set_can_target(false);
    layer.set_hexpand(true);
    layer.set_vexpand(true);
    {
        let mut r = reader.borrow_mut();
        if r.mark_layers.len() > 64 {
            r.mark_layers.retain(|l| l.upgrade().is_some());
        }
        r.mark_layers.push(layer.downgrade());
    }
    let reader = reader.clone();
    layer.set_draw_func(move |_area, cr, w, h| {
        if w < 1 || h < 1 {
            return;
        }
        let page = page_of();
        let r = reader.borrow();
        let Some(geom) = r.geom(page) else {
            return;
        };
        let rotation = r.rotation;
        let (w, h) = (w as f64, h as f64);
        let (uw, uh) = if rotation % 180 == 90 { (h, w) } else { (w, h) };
        for mark in marks_for(&r, page, geom) {
            let [red, green, blue, alpha] = mark.rgba;
            cr.set_source_rgba(
                red as f64 / 255.0,
                green as f64 / 255.0,
                blue as f64 / 255.0,
                alpha as f64 / 255.0,
            );
            for quad in &mark.quads {
                let rect = band(mark.kind, quad_rect(quad, geom, uw, uh));
                let (x0, y0, x1, y1) = rotate_rect(rect, rotation, uw, uh);
                cr.rectangle(x0, y0, x1 - x0, y1 - y0);
            }
            let _ = cr.fill();
        }
        if rotation == 0 {
            draw_edit_chrome(cr, &r, page, geom, &marks_for(&r, page, geom), w, h);
        }
    });
    layer
}

/// An outline round the mark under the pointer, and round the selected one with a handle at each
/// end to drag.
fn draw_edit_chrome(
    cr: &gtk4::cairo::Context,
    r: &ReaderState,
    page: u16,
    geom: PageGeom,
    marks: &[Mark],
    w: f64,
    h: f64,
) {
    let selected = r.mark_edit.selected.as_deref();
    let hovered = r.mark_edit.hovered.as_deref();
    for mark in marks {
        let Some(id) = mark.id.as_deref() else {
            continue;
        };
        let is_selected = Some(id) == selected;
        if !is_selected && Some(id) != hovered {
            continue;
        }
        let [red, green, blue, _] = mark.rgba;
        cr.set_source_rgba(
            red as f64 / 255.0,
            green as f64 / 255.0,
            blue as f64 / 255.0,
            1.0,
        );
        cr.set_line_width(if is_selected { 2.0 } else { 1.5 });
        for quad in &mark.quads {
            let (x0, y0, x1, y1) = quad_rect(quad, geom, w, h);
            cr.rectangle(x0, y0, x1 - x0, y1 - y0);
        }
        let _ = cr.stroke();
    }
    if let Some(handles) = mark_edit::handles_of(r, page, w, h) {
        let [red, green, blue, _] = annotation_rgba(handles.color.as_deref());
        for (x, y) in [handles.start, handles.end] {
            cr.arc(x, y, mark_edit::HANDLE_RADIUS, 0.0, std::f64::consts::TAU);
            cr.set_source_rgba(
                red as f64 / 255.0,
                green as f64 / 255.0,
                blue as f64 / 255.0,
                1.0,
            );
            let _ = cr.fill_preserve();
            cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            cr.set_line_width(1.5);
            let _ = cr.stroke();
        }
    }
}

/// Repaint every page's mark layer, after the hovered or selected mark changed.
pub(super) fn redraw_all(reader: &Rc<RefCell<ReaderState>>) {
    let layers: Vec<gtk4::DrawingArea> = reader
        .borrow()
        .mark_layers
        .iter()
        .filter_map(|l| l.upgrade())
        .collect();
    for layer in layers {
        layer.queue_draw();
    }
}

/// Ask the mark layer drawn over `picture`, if it has one, to repaint.
pub(super) fn redraw_beside(picture: &gtk4::Picture) {
    let Some(overlay) = picture.parent() else {
        return;
    };
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        if widget.widget_name() == MARK_LAYER_NAME {
            widget.queue_draw();
        }
        child = widget.next_sibling();
    }
}

pub(super) fn quad_rect(quad: &[f64; 8], geom: PageGeom, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let (dw, dh) = geom.display_size();
    let (sx, sy) = (w / (dw as f64).max(1.0), h / (dh as f64).max(1.0));
    let xs = [quad[0], quad[2], quad[4], quad[6]];
    let ys = [quad[1], quad[3], quad[5], quad[7]];
    let min = |v: &[f64; 4]| v.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = |v: &[f64; 4]| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    // Display-space y runs up from the bottom; the layer's runs down from the top.
    (
        min(&xs) * sx,
        (dh as f64 - max(&ys)) * sy,
        max(&xs) * sx,
        (dh as f64 - min(&ys)) * sy,
    )
}

/// Underline and strikeout use a thin band of the text line, a highlight the whole of it.
fn band(
    kind: fond_doc::MarkupKind,
    (x0, y0, x1, y1): (f64, f64, f64, f64),
) -> (f64, f64, f64, f64) {
    let height = y1 - y0;
    match kind {
        fond_doc::MarkupKind::Highlight => (x0, y0, x1, y1),
        fond_doc::MarkupKind::Underline => (x0, y1 - height * 0.18, x1, y1),
        fond_doc::MarkupKind::Strikeout => (x0, y1 - height * 0.58, x1, y1 - height * 0.42),
    }
}

/// Turn a rectangle on the unrotated `w`×`h` page the way the page picture is turned.
fn rotate_rect(
    (x0, y0, x1, y1): (f64, f64, f64, f64),
    degrees: u16,
    w: f64,
    h: f64,
) -> (f64, f64, f64, f64) {
    let point = |x: f64, y: f64| match degrees % 360 {
        90 => (h - y, x),
        180 => (w - x, h - y),
        270 => (y, w - x),
        _ => (x, y),
    };
    let (ax, ay) = point(x0, y0);
    let (bx, by) = point(x1, y1);
    (ax.min(bx), ay.min(by), ax.max(bx), ay.max(by))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turn_moves_the_top_left_corner_to_the_top_right() {
        let (x0, y0, x1, y1) = rotate_rect((0.0, 0.0, 10.0, 5.0), 90, 100.0, 200.0);
        assert_eq!((x0, y0, x1, y1), (195.0, 0.0, 200.0, 10.0));
    }

    #[test]
    fn half_turn_mirrors_both_axes() {
        let (x0, y0, x1, y1) = rotate_rect((10.0, 20.0, 30.0, 40.0), 180, 100.0, 200.0);
        assert_eq!((x0, y0, x1, y1), (70.0, 160.0, 90.0, 180.0));
    }

    #[test]
    fn underline_sits_at_the_bottom_of_the_line() {
        let (_, y0, _, y1) = band(fond_doc::MarkupKind::Underline, (0.0, 0.0, 10.0, 100.0));
        assert_eq!((y0, y1), (82.0, 100.0));
    }
}
