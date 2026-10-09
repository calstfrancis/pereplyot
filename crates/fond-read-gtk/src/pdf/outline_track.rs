use super::*;

/// The outline entries leading to the section `page` (1-based) is in: the last entry that starts
/// on or before it, and the chain of entries above that one.
pub(super) fn section_path(entries: &[fond_doc::PdfOutlineEntry], page: u32) -> Vec<usize> {
    let Some(current) = entries
        .iter()
        .rposition(|e| e.page.is_some_and(|p| p as u32 <= page))
    else {
        return Vec::new();
    };
    let mut path = vec![current];
    let mut depth = entries[current].depth;
    for i in (0..current).rev() {
        if depth == 0 {
            break;
        }
        if entries[i].depth < depth {
            path.push(i);
            depth = entries[i].depth;
        }
    }
    path.reverse();
    path
}

/// Keep the Contents list and the status bar's breadcrumb on the section being read.
pub(super) fn install_outline_tracking(
    reader: &Rc<RefCell<ReaderState>>,
    entries: Rc<RefCell<Vec<fond_doc::PdfOutlineEntry>>>,
    rows: Rc<RefCell<Vec<gtk4::Button>>>,
    scroll: gtk4::ScrolledWindow,
    breadcrumb: &gtk4::Label,
    page_entry: &gtk4::Entry,
) -> Rc<dyn Fn()> {
    crate::style::ensure();
    breadcrumb.set_visible(false);
    let current: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    let current_reset = current.clone();
    let update: Rc<dyn Fn()> = {
        let reader = reader.clone();
        let breadcrumb = breadcrumb.clone();
        Rc::new(move || {
            let page = reader.borrow().page as u32 + 1;
            let entries = entries.borrow();
            let rows = rows.borrow();
            let path = section_path(&entries, page);
            let leaf = path.last().copied();
            if let Some(i) = current.get() {
                if Some(i) != leaf {
                    if let Some(row) = rows.get(i) {
                        row.remove_css_class("outline-current");
                    }
                }
            }
            let text = path
                .iter()
                .map(|&i| entries[i].title.trim())
                .collect::<Vec<_>>()
                .join(" › ");
            breadcrumb.set_text(&text);
            breadcrumb.set_tooltip_text((!text.is_empty()).then_some(text.as_str()));
            breadcrumb.set_visible(!text.is_empty());
            if let Some(i) = leaf {
                if current.get() != leaf {
                    current.set(leaf);
                    if let Some(row) = rows.get(i) {
                        row.add_css_class("outline-current");
                        scroll_into_view(&scroll, row);
                    }
                }
            } else {
                current.set(None);
            }
        })
    };
    // The page counter is rewritten whenever the page changes, so its changes are the signal;
    // the work waits for the next idle moment, after whoever changed it has let go of the reader.
    let pending = Rc::new(Cell::new(false));
    {
        let update = update.clone();
        page_entry.connect_changed(move |_| {
            if pending.replace(true) {
                return;
            }
            let update = update.clone();
            let pending = pending.clone();
            glib::idle_add_local_once(move || {
                pending.set(false);
                update();
            });
        });
    }
    let initial = update.clone();
    glib::idle_add_local_once(move || initial());
    // After the entries are replaced: the old rows are gone, so forget which one was current.
    Rc::new(move || {
        current_reset.set(None);
        update();
    })
}

/// Scroll `scroll` just far enough that `row` is in view, with a little context above it.
fn scroll_into_view(scroll: &gtk4::ScrolledWindow, row: &gtk4::Button) {
    let Some(content) = scroll.child() else {
        return;
    };
    let Some(bounds) = row.compute_bounds(&content) else {
        return;
    };
    let adj = scroll.vadjustment();
    let (top, bottom) = (bounds.y() as f64, (bounds.y() + bounds.height()) as f64);
    if top < adj.value() + 24.0 {
        adj.set_value((top - 24.0).max(0.0));
    } else if bottom > adj.value() + adj.page_size() - 12.0 {
        adj.set_value(bottom - adj.page_size() + 12.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, depth: u32, page: Option<u16>) -> fond_doc::PdfOutlineEntry {
        fond_doc::PdfOutlineEntry {
            title: title.to_string(),
            depth,
            page,
        }
    }

    fn outline() -> Vec<fond_doc::PdfOutlineEntry> {
        vec![
            entry("Preface", 0, Some(3)),
            entry("Part I", 0, Some(10)),
            entry("Ch. 1", 1, Some(11)),
            entry("Origins", 2, Some(14)),
            entry("Ch. 2", 1, Some(30)),
            entry("Part II", 0, Some(60)),
        ]
    }

    #[test]
    fn the_path_runs_from_the_part_down_to_the_section() {
        let titles: Vec<String> = section_path(&outline(), 20)
            .iter()
            .map(|&i| outline()[i].title.clone())
            .collect();
        assert_eq!(titles, ["Part I", "Ch. 1", "Origins"]);
    }

    #[test]
    fn a_page_in_a_later_chapter_drops_the_deeper_section() {
        let titles: Vec<String> = section_path(&outline(), 31)
            .iter()
            .map(|&i| outline()[i].title.clone())
            .collect();
        assert_eq!(titles, ["Part I", "Ch. 2"]);
    }

    #[test]
    fn before_the_first_entry_there_is_no_section() {
        assert!(section_path(&outline(), 1).is_empty());
        assert!(section_path(&[], 5).is_empty());
    }

    #[test]
    fn entries_with_no_page_are_skipped() {
        let entries = vec![entry("A", 0, Some(1)), entry("Dangling", 0, None)];
        assert_eq!(section_path(&entries, 9), vec![0]);
    }
}
