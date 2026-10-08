use super::*;

pub(super) fn install_next_page(ui: &PdfUi) {
    let PdfUi {
        reader,
        render,
        next,
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
        next.connect_clicked(move |_| {
            if continuous_toggle.is_active() {
                let target = {
                    let r = reader.borrow();
                    (r.page + 1).min(r.count.saturating_sub(1))
                };
                scroll_continuous_to_page(&reader, &continuous_scroll, target);
                return;
            }
            let step = if two_page_toggle.is_active() { 2 } else { 1 };
            {
                let mut r = reader.borrow_mut();
                if r.page + step < r.count {
                    r.page += step;
                }
            }
            render();
        });
    }
}
