use super::*;

pub(super) fn install_prev_next(ui: &PdfUi) {
    let PdfUi {
        reader,
        render,
        prev,
        continuous_toggle,
        two_page_toggle,
        continuous_scroll,
        ..
    } = ui.clone();
    {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let two_page_toggle = two_page_toggle.clone();
        prev.connect_clicked(move |_| {
            if continuous_toggle.is_active() {
                let target = reader.borrow().page.saturating_sub(1);
                scroll_continuous_to_page(&reader, &continuous_scroll, target);
                return;
            }
            // Two-page mode steps by a whole spread, not one page, so Prev/Next always land
            // back on a left-hand page.
            let step = if two_page_toggle.is_active() { 2 } else { 1 };
            {
                let mut r = reader.borrow_mut();
                r.page = r.page.saturating_sub(step);
            }
            render();
        });
    }
}
