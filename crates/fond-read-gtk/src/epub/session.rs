use super::*;

pub(super) fn install_progress_saving(
    host: &Rc<dyn ReaderHost>,
    hash: &str,
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
    reader_tab: &crate::reader_host::ReaderTab,
    start_percent: Option<u8>,
) {
    // Save the current chapter+scroll-percent back to the entry's Progress on close, so the
    // next "Read" resumes where this session left off — the EPUB half of the same resume
    // Tier 2a already gives the PDF reader (see its own `connect_close_request` above).
    // Reading `document.documentElement.scrollTop`/`scrollHeight`/`clientHeight` is async
    // (`evaluate_javascript`), so this returns `Propagation::Proceed` immediately and writes
    // the note in the callback — nothing after that write depends on the dialog still being
    // open, it just needs `state`/`key`, both cheap `Rc`/`String` clones.
    const SCROLL_PERCENT_JS: &str = "(function() {\n  var el = document.documentElement;\n  var range = el.scrollHeight - el.clientHeight;\n  return range > 0 ? Math.round((el.scrollTop / range) * 100) : 0;\n})()";
    let last_percent: Rc<Cell<u8>> = Rc::new(Cell::new(start_percent.unwrap_or(0)));
    let closed = Rc::new(Cell::new(false));
    {
        let host = host.clone();
        let reader = reader.clone();
        let web_view = web_view.clone();
        let last_percent = last_percent.clone();
        let closed = closed.clone();
        glib::timeout_add_local(std::time::Duration::from_secs(5), move || {
            if closed.get() {
                return glib::ControlFlow::Break;
            }
            let host = host.clone();
            let reader = reader.clone();
            let last_percent = last_percent.clone();
            web_view.evaluate_javascript(
                SCROLL_PERCENT_JS,
                None,
                None,
                gio::Cancellable::NONE,
                move |result| {
                    if let Ok(v) = result {
                        last_percent.set(v.to_int32().clamp(0, 100) as u8);
                        let r = reader.borrow();
                        host.save_progress(fond_annot::Progress {
                            page: r.index as u32 + 1,
                            of: r.spine.len() as u32,
                            chapter_percent: Some(last_percent.get()),
                        });
                    }
                },
            );
            glib::ControlFlow::Continue
        });
    }
    // Saved synchronously from the last known scroll position first, then refreshed from the
    // WebView and only then unregistered: unregistering the last reader can quit the app, and
    // the WebView's answer arrives asynchronously.
    {
        let host = host.clone();
        let hash = hash.to_string();
        let reader = reader.clone();
        let web_view = web_view.clone();
        crate::reader_host::on_tab_closed(reader_tab, move || {
            closed.set(true);
            let (chapter_num, chapter_count) = {
                let r = reader.borrow();
                (r.index as u32 + 1, r.spine.len() as u32)
            };
            host.save_progress(fond_annot::Progress {
                page: chapter_num,
                of: chapter_count,
                chapter_percent: Some(last_percent.get()),
            });
            let unregistered = Rc::new(Cell::new(false));
            let finish = {
                let hash = hash.clone();
                let unregistered = unregistered.clone();
                let store = reader.borrow().store.clone();
                Rc::new(move || {
                    if !unregistered.replace(true) {
                        store.clear_listeners();
                        crate::unregister_window(&hash);
                    }
                })
            };
            {
                let finish = finish.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
                    finish()
                });
            }
            let host = host.clone();
            web_view.evaluate_javascript(
                SCROLL_PERCENT_JS,
                None,
                None,
                gio::Cancellable::NONE,
                move |result| {
                    if let Ok(v) = result {
                        host.save_progress(fond_annot::Progress {
                            page: chapter_num,
                            of: chapter_count,
                            chapter_percent: Some(v.to_int32().clamp(0, 100) as u8),
                        });
                    }
                    finish();
                },
            );
        });
    }
}
