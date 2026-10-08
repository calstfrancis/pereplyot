use super::*;

pub(super) fn install_chapter_load(
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
    pending_scroll: &Rc<RefCell<Option<String>>>,
    pending_scroll_percent: &Rc<RefCell<Option<u8>>>,
    pending_search: &Rc<RefCell<Option<String>>>,
) {
    // Re-apply saved highlights after every chapter load (initial load, TOC jump,
    // prev/next, a notes-sidebar jump — all funnel through `epub_go_to`'s `load_uri`, so
    // one handler here covers all of them), consuming `pending_scroll` if the navigation
    // that triggered this load set one. Also consumes `pending_scroll_percent` (reading-
    // position resume, first load only) and `pending_search` (a whole-book search result's
    // chapter jump, handed to WebKit's own `FindController` once the page is actually there).
    {
        let reader = reader.clone();
        let pending_scroll = pending_scroll.clone();
        let pending_scroll_percent = pending_scroll_percent.clone();
        let pending_search = pending_search.clone();
        web_view.connect_load_changed(move |view, event| {
            if event == webkit6::LoadEvent::Finished {
                let scroll_to = pending_scroll.borrow_mut().take();
                epub_apply_highlights(view, &reader, scroll_to.as_deref());
                if let Some(percent) = pending_scroll_percent.borrow_mut().take() {
                    let script = format!(
                        "window.scrollTo(0, Math.round((document.documentElement.scrollHeight - document.documentElement.clientHeight) * {}));",
                        (percent.min(100) as f64) / 100.0
                    );
                    view.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});
                }
                if let Some(query) = pending_search.borrow_mut().take() {
                    if let Some(fc) = webkit6::prelude::WebViewExt::find_controller(view) {
                        let options = (webkit6::FindOptions::CASE_INSENSITIVE
                            | webkit6::FindOptions::WRAP_AROUND)
                            .bits();
                        fc.search(&query, options, 1000);
                    }
                }
            }
        });
    }
}
