use super::*;

const TICK_STRIP_WIDTH: i32 = 10;
const TICK_HEIGHT: f64 = 2.0;

/// A thin strip along the continuous view's scrollbar edge marking where the saved notes and
/// search matches are, so a whole book can be seen at a glance.
pub(super) fn build_tick_layer(reader: &Rc<RefCell<ReaderState>>) -> gtk4::DrawingArea {
    let layer = gtk4::DrawingArea::new();
    layer.set_can_target(false);
    layer.set_halign(gtk4::Align::End);
    layer.set_vexpand(true);
    layer.set_content_width(TICK_STRIP_WIDTH);
    let weak = reader.clone();
    layer.set_draw_func(move |_area, cr, w, h| {
        let r = weak.borrow();
        let Some(&last) = r.continuous_offsets.last() else {
            return;
        };
        let total = last + CONTINUOUS_PAGE_GAP;
        if total <= 0.0 || r.continuous_offsets.len() < 2 {
            return;
        }
        let (w, h) = (w as f64, h as f64);
        let y_of = |page: u16| {
            let top = r.continuous_offsets[(page as usize).min(r.continuous_offsets.len() - 2)];
            let next =
                r.continuous_offsets[(page as usize + 1).min(r.continuous_offsets.len() - 1)];
            (CONTINUOUS_PAGE_GAP + (top + next) / 2.0) / total * h
        };
        let paint = |page: u16, rgba: [u8; 4], width: f64| {
            cr.set_source_rgba(
                rgba[0] as f64 / 255.0,
                rgba[1] as f64 / 255.0,
                rgba[2] as f64 / 255.0,
                1.0,
            );
            cr.rectangle(
                w - width,
                y_of(page) - TICK_HEIGHT / 2.0,
                width,
                TICK_HEIGHT,
            );
            let _ = cr.fill();
        };
        for a in r.store.sidecar().annotations.iter() {
            if let Some(page) = a.page {
                paint(
                    (page.saturating_sub(1) as u16).min(r.count.saturating_sub(1)),
                    annotation_rgba(a.color.as_deref()),
                    6.0,
                );
            }
        }
        for m in &r.search_matches {
            paint(m.page, SEARCH_MATCH_RGBA, 4.0);
        }
        if let Some(current) = r.search_matches.get(r.search_current) {
            paint(current.page, SEARCH_MATCH_RGBA, w);
        }
    });
    reader.borrow_mut().tick_layer = Some(layer.clone());
    layer
}

pub(super) fn queue_ticks(reader: &Rc<RefCell<ReaderState>>) {
    if let Some(layer) = &reader.borrow().tick_layer {
        layer.queue_draw();
    }
}
