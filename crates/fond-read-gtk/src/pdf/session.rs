use super::*;

pub(super) fn install_session(ui: &PdfUi, start_page: u16) {
    let PdfUi {
        host,
        reader,
        reader_tab,
        pdf_hash,
        ..
    } = ui.clone();
    let ui_for_position = ui.clone();
    // Save the current page back to the entry's Progress on close, so the next "Read" opens
    // where this session left off — the PDF-reader half of Tier 2a. `page`/`count` snapshot
    // out of `reader` up front since the RefCell isn't needed once we're just writing to the
    // library.
    let closed = Rc::new(Cell::new(false));
    {
        let host = host.clone();
        let reader = reader.clone();
        let closed = closed.clone();
        let last_saved = Cell::new(start_page);
        let last_fraction = Cell::new(0.0f32);
        let ui = ui_for_position.clone();
        glib::timeout_add_local(std::time::Duration::from_secs(5), move || {
            if closed.get() {
                return glib::ControlFlow::Break;
            }
            let (page, count) = {
                let r = reader.borrow();
                (r.page, r.count)
            };
            let fraction = position::current_fraction(&ui);
            if page != last_saved.get() || (fraction - last_fraction.get()).abs() > 0.03 {
                last_saved.set(page);
                last_fraction.set(fraction);
                host.save_position(page as u32 + 1, fraction);
                host.save_progress(fond_annot::Progress {
                    page: page as u32 + 1,
                    of: count as u32,
                    chapter_percent: None,
                });
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let host = host.clone();
        let reader = reader.clone();
        let pdf_hash = pdf_hash.to_string();
        let ui = ui_for_position.clone();
        crate::reader_host::on_tab_closed(&reader_tab, move || {
            closed.set(true);
            host.save_position(
                reader.borrow().page as u32 + 1,
                position::current_fraction(&ui),
            );
            let (page, count) = {
                let r = reader.borrow();
                (r.page as u32 + 1, r.count as u32)
            };
            host.save_progress(fond_annot::Progress {
                page,
                of: count,
                chapter_percent: None,
            });
            let hooks = std::mem::take(&mut reader.borrow_mut().close_hooks);
            for hook in hooks {
                hook();
            }
            reader.borrow().store.clear_listeners();
            {
                let mut r = reader.borrow_mut();
                r.worker = None;
                r.search = None;
            }
            crate::unregister_window(&pdf_hash);
        });
    }
}
