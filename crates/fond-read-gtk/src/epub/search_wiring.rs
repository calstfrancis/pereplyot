use super::*;

pub(super) fn install_search(ui: &EpubUi) {
    let EpubUi {
        reader,
        web_view,
        prev,
        next,
        chapter_label,
        bookmark_button,
        pending_search,
        results_list,
        results_revealer,
        search_bar_entry,
        search_count,
        search_next,
        search_prev,
        search_revealer,
        search_toggle,
        whole_book_toggle,
        ..
    } = ui;
    // Search wiring: toggling `search_toggle` reveals the bar and focuses the entry; turning
    // it off clears the query, WebKit's highlight state (`search_finish`), the whole-book
    // results list, and drops out of whole-book mode — so reopening search always starts
    // from the same clean state rather than leaving stale results/highlights visible.
    {
        let search_revealer = search_revealer.clone();
        let search_bar_entry = search_bar_entry.clone();
        let search_count = search_count.clone();
        let results_list = results_list.clone();
        let results_revealer = results_revealer.clone();
        let whole_book_toggle = whole_book_toggle.clone();
        let view = web_view.clone();
        search_toggle.connect_toggled(move |btn| {
            let on = btn.is_active();
            search_revealer.set_reveal_child(on);
            if on {
                search_bar_entry.grab_focus();
            } else {
                search_bar_entry.set_text("");
                search_count.set_text("");
                whole_book_toggle.set_active(false);
                results_revealer.set_reveal_child(false);
                while let Some(child) = results_list.first_child() {
                    results_list.remove(&child);
                }
                if let Some(fc) = webkit6::prelude::WebViewExt::find_controller(&view) {
                    fc.search_finish();
                }
            }
        });
    }
    {
        let search_bar_entry = search_bar_entry.clone();
        let search_count = search_count.clone();
        let results_list = results_list.clone();
        let results_revealer = results_revealer.clone();
        let view = web_view.clone();
        whole_book_toggle.connect_toggled(move |btn| {
            search_bar_entry.set_placeholder_text(Some(if btn.is_active() {
                "Search the whole book"
            } else {
                "Search this chapter"
            }));
            search_bar_entry.set_text("");
            search_count.set_text("");
            results_revealer.set_reveal_child(false);
            while let Some(child) = results_list.first_child() {
                results_list.remove(&child);
            }
            if let Some(fc) = webkit6::prelude::WebViewExt::find_controller(&view) {
                fc.search_finish();
            }
            search_bar_entry.grab_focus();
        });
    }
    {
        let view = web_view.clone();
        let search_count = search_count.clone();
        let whole_book_toggle = whole_book_toggle.clone();
        let results_list = results_list.clone();
        let results_revealer = results_revealer.clone();
        let reader = reader.clone();
        let prev = prev.clone();
        let next = next.clone();
        let chapter_label = chapter_label.clone();
        let bookmark_button = bookmark_button.clone();
        let pending_search = pending_search.clone();
        search_bar_entry.connect_search_changed(move |entry| {
            let text = entry.text();

            if whole_book_toggle.is_active() {
                while let Some(child) = results_list.first_child() {
                    results_list.remove(&child);
                }
                if text.is_empty() {
                    search_count.set_text("");
                    results_revealer.set_reveal_child(false);
                    return;
                }
                let matches = epub_search_whole_book(&reader, &text);
                if matches.is_empty() {
                    search_count.set_text("No matches");
                    results_revealer.set_reveal_child(false);
                    return;
                }
                search_count.set_text(&if matches.len() >= EPUB_WHOLE_BOOK_MATCH_LIMIT {
                    format!("{EPUB_WHOLE_BOOK_MATCH_LIMIT}+ found")
                } else {
                    format!("{} found", matches.len())
                });
                let spine_len = reader.borrow().spine.len();
                for m in &matches {
                    let before = glib::markup_escape_text(&m.snippet[..m.match_start]);
                    let hit = glib::markup_escape_text(&m.snippet[m.match_start..m.match_end]);
                    let after = glib::markup_escape_text(&m.snippet[m.match_end..]);
                    let row = gtk4::ListBoxRow::new();
                    let box_ = gtk4::Box::new(Orientation::Vertical, 2);
                    box_.set_margin_top(6);
                    box_.set_margin_bottom(6);
                    box_.set_margin_start(6);
                    box_.set_margin_end(6);
                    let heading = gtk4::Label::new(Some(&format!(
                        "Chapter {} of {}",
                        m.chapter + 1,
                        spine_len
                    )));
                    heading.set_xalign(0.0);
                    heading.add_css_class("dim-label");
                    heading.add_css_class("caption-heading");
                    box_.append(&heading);
                    let excerpt = gtk4::Label::new(None);
                    excerpt.set_markup(&format!("…{before}<b>{hit}</b>{after}…"));
                    excerpt.set_xalign(0.0);
                    excerpt.set_wrap(true);
                    excerpt.set_ellipsize(gtk4::pango::EllipsizeMode::None);
                    box_.append(&excerpt);
                    row.set_child(Some(&box_));

                    let click = gtk4::GestureClick::new();
                    let reader = reader.clone();
                    let view = view.clone();
                    let prev = prev.clone();
                    let next = next.clone();
                    let chapter_label = chapter_label.clone();
                    let bookmark_button = bookmark_button.clone();
                    let pending_search = pending_search.clone();
                    let query = text.to_string();
                    let chapter_idx = m.chapter;
                    let go: Rc<dyn Fn()> = Rc::new(move || {
                        let target = {
                            let r = reader.borrow();
                            r.spine.get(chapter_idx).cloned()
                        };
                        let Some(target) = target else { return };
                        *pending_search.borrow_mut() = Some(query.clone());
                        epub_go_to(
                            &reader,
                            &view,
                            &prev,
                            &next,
                            &chapter_label,
                            &bookmark_button,
                            &target,
                        );
                    });
                    {
                        let go = go.clone();
                        click.connect_released(move |_, _, _, _| go());
                    }
                    row.connect_activate(move |_| go());
                    row.set_activatable(true);
                    row.update_property(&[gtk4::accessible::Property::Label(&format!(
                        "Chapter {}: {}",
                        m.chapter + 1,
                        m.snippet
                    ))]);
                    row.add_controller(click);
                    results_list.append(&row);
                }
                results_revealer.set_reveal_child(true);
                return;
            }

            results_revealer.set_reveal_child(false);
            let Some(fc) = webkit6::prelude::WebViewExt::find_controller(&view) else {
                return;
            };
            if text.is_empty() {
                fc.search_finish();
                search_count.set_text("");
                return;
            }
            let options =
                (webkit6::FindOptions::CASE_INSENSITIVE | webkit6::FindOptions::WRAP_AROUND).bits();
            fc.search(&text, options, 1000);
        });
    }
    {
        let view = web_view.clone();
        let whole_book_toggle = whole_book_toggle.clone();
        search_prev.connect_clicked(move |_| {
            if whole_book_toggle.is_active() {
                return;
            }
            if let Some(fc) = webkit6::prelude::WebViewExt::find_controller(&view) {
                fc.search_previous();
            }
        });
    }
    {
        let view = web_view.clone();
        let whole_book_toggle = whole_book_toggle.clone();
        search_next.connect_clicked(move |_| {
            if whole_book_toggle.is_active() {
                return;
            }
            if let Some(fc) = webkit6::prelude::WebViewExt::find_controller(&view) {
                fc.search_next();
            }
        });
    }
    if let Some(fc) = webkit6::prelude::WebViewExt::find_controller(web_view) {
        let count_label = search_count.clone();
        let toggle = whole_book_toggle.clone();
        fc.connect_found_text(move |_, count| {
            if !toggle.is_active() {
                count_label.set_text(&format!("{count} found"));
            }
        });
        let count_label = search_count.clone();
        let toggle = whole_book_toggle.clone();
        fc.connect_failed_to_find_text(move |_| {
            if !toggle.is_active() {
                count_label.set_text("No matches");
            }
        });
    }
}
