use super::*;

pub(super) fn install_link_nav(ui: &PdfUi) {
    let PdfUi {
        reader,
        render,
        continuous_toggle,
        link_back,
        continuous_scroll,
        ..
    } = ui.clone();
    {
        let reader_for_goto = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let link_back = link_back.clone();
        let jump_to = {
            let reader = reader.clone();
            Rc::new(move |page: u16| {
                if continuous_toggle.is_active() {
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                } else {
                    reader.borrow_mut().page = page;
                    render();
                }
            })
        };
        {
            let jump_to = jump_to.clone();
            let link_back = link_back.clone();
            reader.borrow_mut().link_goto = Some(Rc::new(move |page| {
                let from = {
                    let mut r = reader_for_goto.borrow_mut();
                    let from = r.page;
                    r.nav_back.push(from);
                    from
                };
                link_back.set_label(&format!("← Back to p. {}", from + 1));
                link_back.set_visible(true);
                jump_to(page);
            }));
        }
        let reader = reader.clone();
        let link_back_inner = link_back.clone();
        link_back.connect_clicked(move |_| {
            let dest = reader.borrow_mut().nav_back.pop();
            if let Some(page) = dest {
                jump_to(page);
            }
            match reader.borrow().nav_back.last() {
                Some(p) => link_back_inner.set_label(&format!("← Back to p. {}", p + 1)),
                None => link_back_inner.set_visible(false),
            }
        });
    }
}
