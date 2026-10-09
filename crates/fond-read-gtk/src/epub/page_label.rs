use super::*;

/// The printed page in force at the top of the view: the index of the last page break in the
/// chapter that is at or above the top edge (or, paginated, before the right edge), or -1.
pub(super) const CURRENT_PAGE_JS: &str = r#"(function() {
  var pp = window.__pp || [];
  var cur = -1;
  var paged = !!window.__paged;
  for (var i = 0; i < pp.length; i++) {
    var el = document.getElementById(pp[i].id);
    if (!el) continue;
    var r = el.getBoundingClientRect();
    var before = paged ? r.left < window.innerWidth - 4 : r.top <= 24;
    if (before) cur = i; else break;
  }
  return cur;
})()"#;

/// Tell the page this chapter's page breaks, for [`CURRENT_PAGE_JS`] and the selection capture.
pub(super) fn chapter_pages_js(pages: &[pages::PageBreak], chapter: &str) -> String {
    #[derive(serde::Serialize)]
    struct P<'a> {
        id: &'a str,
        label: &'a str,
    }
    let here: Vec<P> = pages::in_chapter(pages, chapter)
        .into_iter()
        .map(|p| P {
            id: &p.id,
            label: &p.label,
        })
        .collect();
    format!(
        "window.__pp = {};",
        serde_json::to_string(&here).unwrap_or_else(|_| "[]".to_string())
    )
}

/// The printed page now at the top of the chapter in view, kept up to date in `label`.
pub(super) fn install_page_label(
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
    label: &gtk4::Label,
) {
    if reader.borrow().pages.is_empty() {
        label.set_visible(false);
        return;
    }
    let reader = Rc::downgrade(reader);
    let view = web_view.downgrade();
    let label = label.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        let (Some(reader), Some(view)) = (reader.upgrade(), view.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        let label = label.clone();
        view.evaluate_javascript(
            CURRENT_PAGE_JS,
            None,
            None,
            gio::Cancellable::NONE,
            move |result| {
                let Ok(v) = result else { return };
                let r = reader.borrow();
                let Some(chapter) = r.spine.get(r.index) else {
                    return;
                };
                let in_chapter = pages::in_chapter(&r.pages, chapter);
                let shown = usize::try_from(v.to_int32())
                    .ok()
                    .and_then(|i| in_chapter.get(i).map(|p| p.label.clone()))
                    .or_else(|| pages::label_entering(&r.pages, &r.spine, chapter))
                    .or_else(|| in_chapter.first().map(|p| p.label.clone()));
                match shown {
                    Some(l) => {
                        label.set_text(&format!("p. {l}"));
                        label.set_visible(true);
                    }
                    None => label.set_visible(false),
                }
            },
        );
        glib::ControlFlow::Continue
    });
}

/// The printed page that `captured` (from the selection script) or, failing that, the start of
/// the chapter falls on.
pub(super) fn label_for(
    reader: &EpubReaderState,
    chapter: &str,
    captured: Option<String>,
) -> Option<String> {
    captured
        .filter(|l| !l.is_empty())
        .or_else(|| pages::label_entering(&reader.pages, &reader.spine, chapter))
}
