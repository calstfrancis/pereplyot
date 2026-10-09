use super::scan_thread::{self, Scan};
use super::*;

/// Start the background scan of every page's size and label, and apply it when it lands: real
/// page sizes replace the stand-in layout (rebuilding the scroll view only if some page really
/// differs), and printed page labels appear.
pub(super) fn install_scan(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        render,
        page_num_button,
        rebuild_notes,
        continuous_toggle,
        continuous_box,
        continuous_scroll,
        reader_window,
        ..
    } = ui.clone();
    let path = reader.borrow().path.clone();
    scan_thread::spawn(
        path,
        Rc::new(move |scan: Scan| {
            let (layout_changed, native_labels) = {
                let mut r = reader.borrow_mut();
                let assumed = r.display_size(r.layout_page);
                let mut changed = false;
                for (index, geom) in scan.geoms.iter().enumerate() {
                    if let Some(g) = geom {
                        changed |= g.display_size() != assumed;
                    }
                    r.page_geoms.borrow_mut().insert(index as u16, *geom);
                }
                r.scanned = true;
                let native = scan.labels.iter().any(|l| l.is_some());
                if native {
                    r.page_labels = scan.labels;
                }
                (changed, native)
            };
            crate::perf::mark_rss("document scan applied");
            if native_labels {
                page_num_button.set_sensitive(false);
                page_num_button
                    .set_tooltip_text(Some("This PDF already declares its own page numbers"));
            }
            render();
            if native_labels {
                rebuild_notes();
            }
            if layout_changed && continuous_toggle.is_active() {
                rebuild_continuous_view_for_zoom(
                    &host,
                    &reader,
                    &continuous_box,
                    &continuous_scroll,
                    &reader_window,
                );
                let page = reader.borrow().page;
                scroll_continuous_to_page(&reader, &continuous_scroll, page);
            }
        }),
    );
}
