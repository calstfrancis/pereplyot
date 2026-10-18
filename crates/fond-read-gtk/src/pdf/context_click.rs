use super::*;

pub(super) fn install_context_click(ui: &PdfUi) {
    install_page_context_click(ui, &ui.picture, 0);
    install_page_context_click(ui, &ui.right_picture, 1);
}

fn install_page_context_click(ui: &PdfUi, picture: &gtk4::Picture, page_offset: u16) {
    let PdfUi {
        host,
        reader,
        reader_window,
        ..
    } = ui.clone();
    let picture = picture.clone();
    // Right-click: edit or delete the annotation under the cursor, or add a marginal note
    // if the click landed on blank page — replaces the old "This page" dropdown, which
    // listed the same actions in a fixed menu instead of at the annotation itself.
    {
        let click = gtk4::GestureClick::new();
        click.set_button(gdk::BUTTON_SECONDARY);
        let host = host.clone();
        let reader = reader.clone();
        let picture_for_menu = picture.clone();
        let dialog_for_menu = reader_window.clone();
        click.connect_pressed(move |_gesture, _n, x, y| {
            if reader.borrow().rotation != 0 {
                host.notify("Rotate back to 0° to edit annotations");
                return;
            }
            let (page, render_w, render_h, page_geom) = {
                let r = reader.borrow();
                let page = r.page + page_offset;
                let (w, h) = picture_for_menu.size_request();
                (page, w.max(0) as u32, h.max(0) as u32, r.geom(page))
            };
            show_pdf_context_menu(
                &host,
                &reader,
                &picture_for_menu,
                page,
                ClickGeometry {
                    render_w,
                    render_h,
                    page: page_geom,
                    click_x: x,
                    click_y: y,
                },
                &dialog_for_menu,
            );
        });
        picture.add_controller(click);
    }
}
