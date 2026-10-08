use super::*;

pub(super) fn install_page_click(ui: &PdfUi) {
    let PdfUi {
        reader,
        prev,
        next,
        picture,
        ..
    } = ui.clone();
    // Click-to-turn zones: a plain (non-dragging) click in the leftmost/rightmost 20% of the
    // page turns to the previous/next page — the middle 60% is left alone so a normal
    // highlight drag or text click isn't at risk of being misread as page navigation.
    // Coexists with the drag-to-annotate `GestureDrag` above (both see the same primary-
    // button sequence, un-grouped, which is the standard GTK4 way for a click and a drag
    // gesture to share one widget) — a real drag past `MIN_DRAG_PX` already makes this
    // handler's own displacement check a no-op, the same threshold the drag handler itself
    // uses to decide whether anything was actually drawn.
    {
        let press_pos: Rc<Cell<Option<(f64, f64)>>> = Rc::new(Cell::new(None));
        let click_nav = gtk4::GestureClick::new();
        click_nav.set_button(gdk::BUTTON_PRIMARY);
        {
            let press_pos = press_pos.clone();
            click_nav.connect_pressed(move |_, _, x, y| press_pos.set(Some((x, y))));
        }
        {
            let picture_for_nav = picture.clone();
            let reader_for_nav = reader.clone();
            let prev = prev.clone();
            let next = next.clone();
            click_nav.connect_released(move |_, _, x, y| {
                let Some((sx, sy)) = press_pos.take() else {
                    return;
                };
                if (x - sx).abs() > MIN_DRAG_PX || (y - sy).abs() > MIN_DRAG_PX {
                    return;
                }
                let w = picture_for_nav.width().max(1) as f64;
                let h = picture_for_nav.height().max(1) as f64;
                let page = reader_for_nav.borrow().page;
                if follow_link(&reader_for_nav, page, x, y, w, h) {
                    return;
                }
                if x < w * 0.2 {
                    prev.emit_clicked();
                } else if x > w * 0.8 {
                    next.emit_clicked();
                }
            });
        }
        picture.add_controller(click_nav);
    }
}
