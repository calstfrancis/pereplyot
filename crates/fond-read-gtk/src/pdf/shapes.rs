/// The side of a sticky note's icon on the page, in points.
pub(super) const STICKY_PT: f64 = 16.0;

/// What a saved mark is drawn as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Shape {
    /// Highlighted, underlined or struck-out text.
    Text,
    /// A clipped rectangle: outlined, with the page showing through.
    Area,
    /// A freestanding note's icon.
    Sticky,
}

pub(super) fn quad_of(x0: f64, y_bottom: f64, x1: f64, y_top: f64) -> [f64; 8] {
    [x0, y_top, x1, y_top, x0, y_bottom, x1, y_bottom]
}

/// What an annotation covers on its page, as quads in PDF user space: the quadpoints of marked
/// text, the rectangle of a clipped area, or the icon square of a sticky note.
pub(super) fn anchor_quads(a: &fond_annot::Annotation) -> Vec<[f64; 8]> {
    if !a.quadpoints.is_empty() {
        return a.quadpoints.clone();
    }
    if let Some([l, b, r, t]) = a.rect() {
        return vec![quad_of(l.min(r), b.min(t), l.max(r), b.max(t))];
    }
    if a.kind == fond_annot::AnnotationKind::Note {
        if let Some([x, y]) = a.position() {
            return vec![quad_of(x, y - STICKY_PT, x + STICKY_PT, y)];
        }
    }
    Vec::new()
}

pub(super) fn shape_of(a: &fond_annot::Annotation) -> Shape {
    match a.kind {
        fond_annot::AnnotationKind::Area => Shape::Area,
        fond_annot::AnnotationKind::Note if a.quadpoints.is_empty() => Shape::Sticky,
        _ => Shape::Text,
    }
}

/// A small speech bubble with its top-left corner at (x, y).
pub(super) fn draw_bubble(cr: &gtk4::cairo::Context, x: f64, y: f64, size: f64, rgba: [u8; 4]) {
    let [r, g, b, _] = rgba;
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let (w, h) = (size, size * 0.72);
    let radius = size * 0.2;
    cr.new_path();
    cr.arc(
        x + w - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        x + w - radius,
        y + h - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.line_to(x + w * 0.45, y + h);
    cr.line_to(x + w * 0.2, y + size);
    cr.line_to(x + w * 0.25, y + h);
    cr.arc(
        x + radius,
        y + h - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
    cr.set_source_rgba(r, g, b, 1.0);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
    cr.set_line_width(1.2);
    for i in 0..2 {
        let ly = y + h * (0.35 + 0.3 * i as f64);
        cr.move_to(x + w * 0.22, ly);
        cr.line_to(x + w * 0.78, ly);
        let _ = cr.stroke();
    }
}
