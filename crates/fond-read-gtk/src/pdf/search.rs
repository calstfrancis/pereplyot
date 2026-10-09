use super::search_thread::SearchEvent;
use super::*;

pub(super) fn install_search(ui: &PdfUi) {
    let PdfUi {
        reader,
        render,
        continuous_toggle,
        search_entry,
        search_prev,
        search_next,
        search_count,
        continuous_scroll,
        ..
    } = ui.clone();
    // Search: run on Enter (not per-keystroke — PDFium re-searches every page each time, not
    // worth doing on every character typed), jumping straight to the first match's page.
    // Prev/Next cycle `search_current` with wraparound; the count label and match highlight
    // (blended in `render()`, a distinct colour from saved highlights) follow along.
    // Jump to `page` after a search match changes: in continuous mode, scroll there and
    // re-render both it and the previous match's page (to clear that page's now-stale match
    // tint — cheap no-op if continuous mode was never built); in paged mode, the shared
    // `render()` already re-blends the match into whichever page it lands on.
    let goto_search_match = {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        Rc::new(move |page: u16, previous_page: Option<u16>| {
            if continuous_toggle.is_active() {
                scroll_continuous_to_page(&reader, &continuous_scroll, page);
                render_continuous_page(&reader, page);
                if let Some(prev_page) = previous_page {
                    if prev_page != page {
                        render_continuous_page(&reader, prev_page);
                    }
                }
            } else {
                render();
            }
        })
    };
    let run_search = {
        let reader = reader.clone();
        let render = render.clone();
        let goto_search_match = goto_search_match.clone();
        let search_prev = search_prev.clone();
        let search_next = search_next.clone();
        let search_count = search_count.clone();
        Rc::new(move |query: &str| {
            // Starting a search cancels the one in flight, and drops the previous results (and
            // the tint of the match that was showing) straight away.
            let cleared_page = {
                let mut r = reader.borrow_mut();
                r.search = None;
                let page = r.search_matches.get(r.search_current).map(|m| m.page);
                r.search_matches.clear();
                r.search_current = 0;
                page
            };
            search_prev.set_sensitive(false);
            search_next.set_sensitive(false);
            if query.trim().is_empty() {
                search_count.set_text("");
            } else {
                search_count.set_text("Searching…");
            }
            scrollbar_ticks::queue_ticks(&reader);
            if let Some(page) = cleared_page {
                render();
                render_continuous_page(&reader, page);
            }
            if query.trim().is_empty() {
                return;
            }
            let sink: Rc<dyn Fn(SearchEvent)> = {
                let reader = reader.clone();
                let goto_search_match = goto_search_match.clone();
                let search_prev = search_prev.clone();
                let search_next = search_next.clone();
                let search_count = search_count.clone();
                let scanning = Rc::new(std::cell::Cell::new(true));
                Rc::new(move |event| {
                    let label = |r: &ReaderState, scanning: bool| {
                        let n = r.search_matches.len();
                        match (n, scanning) {
                            (0, true) => "Searching…".to_string(),
                            (0, false) => "No matches".to_string(),
                            (n, true) => format!("{} of {n}+", r.search_current + 1),
                            (n, false) => format!("{} of {n}", r.search_current + 1),
                        }
                    };
                    match event {
                        SearchEvent::Matches(batch, _scanned) => {
                            let first_page = {
                                let mut r = reader.borrow_mut();
                                let was_empty = r.search_matches.is_empty();
                                r.search_matches.extend(batch);
                                let first = was_empty
                                    .then(|| r.search_matches.first().map(|m| m.page))
                                    .flatten();
                                if let Some(page) = first {
                                    r.search_current = 0;
                                    r.page = page;
                                }
                                search_count.set_text(&label(&r, scanning.get()));
                                first
                            };
                            search_prev.set_sensitive(true);
                            search_next.set_sensitive(true);
                            scrollbar_ticks::queue_ticks(&reader);
                            if let Some(page) = first_page {
                                crate::perf::mark("search first match shown");
                                goto_search_match(page, None);
                            }
                        }
                        SearchEvent::Done => {
                            scanning.set(false);
                            let r = reader.borrow();
                            search_count.set_text(&label(&r, false));
                        }
                    }
                })
            };
            crate::perf::mark("search started");
            let path = reader.borrow().path.clone();
            let handle = search_thread::spawn(path, query.to_string(), sink);
            reader.borrow_mut().search = Some(handle);
        })
    };
    {
        let run_search = run_search.clone();
        search_entry.connect_activate(move |entry| run_search(&entry.text()));
    }
    {
        // Clear stale results (and the match highlight) as soon as the box is emptied,
        // rather than leaving them stuck until another search is run.
        let reader = reader.clone();
        let render = render.clone();
        let search_prev = search_prev.clone();
        let search_next = search_next.clone();
        let search_count = search_count.clone();
        search_entry.connect_search_changed(move |entry| {
            if entry.text().is_empty() {
                let cleared_page = {
                    let mut r = reader.borrow_mut();
                    r.search = None;
                    let page = r.search_matches.get(r.search_current).map(|m| m.page);
                    r.search_matches.clear();
                    page
                };
                search_prev.set_sensitive(false);
                search_next.set_sensitive(false);
                search_count.set_text("");
                scrollbar_ticks::queue_ticks(&reader);
                render();
                if let Some(page) = cleared_page {
                    render_continuous_page(&reader, page);
                }
            }
        });
    }
    {
        let reader = reader.clone();
        let goto_search_match = goto_search_match.clone();
        let search_count = search_count.clone();
        search_prev.connect_clicked(move |_| {
            let (previous_page, page) = {
                let mut r = reader.borrow_mut();
                if r.search_matches.is_empty() {
                    return;
                }
                let previous_page = r.search_matches[r.search_current].page;
                r.search_current = if r.search_current == 0 {
                    r.search_matches.len() - 1
                } else {
                    r.search_current - 1
                };
                let page = r.search_matches[r.search_current].page;
                r.page = page;
                search_count.set_text(&format!(
                    "{} of {}",
                    r.search_current + 1,
                    r.search_matches.len()
                ));
                (previous_page, page)
            };
            scrollbar_ticks::queue_ticks(&reader);
            goto_search_match(page, Some(previous_page));
        });
    }
    {
        let reader = reader.clone();
        let goto_search_match = goto_search_match.clone();
        let search_count = search_count.clone();
        search_next.connect_clicked(move |_| {
            let (previous_page, page) = {
                let mut r = reader.borrow_mut();
                if r.search_matches.is_empty() {
                    return;
                }
                let previous_page = r.search_matches[r.search_current].page;
                r.search_current = (r.search_current + 1) % r.search_matches.len();
                let page = r.search_matches[r.search_current].page;
                r.page = page;
                search_count.set_text(&format!(
                    "{} of {}",
                    r.search_current + 1,
                    r.search_matches.len()
                ));
                (previous_page, page)
            };
            scrollbar_ticks::queue_ticks(&reader);
            goto_search_match(page, Some(previous_page));
        });
    }
}
