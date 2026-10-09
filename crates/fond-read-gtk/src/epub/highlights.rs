use super::*;

/// What the reader's selection-capture JS reports back: either nothing was meaningfully
/// selected, or the selected text plus up to 40 characters of surrounding context on each
/// side — the same prefix/suffix disambiguation scheme the PDF sidecar already uses.
#[derive(serde::Deserialize)]
pub(super) struct EpubSelectionCapture {
    pub(super) empty: bool,
    #[serde(default)]
    pub(super) text: Option<String>,
    #[serde(default)]
    pub(super) prefix: Option<String>,
    #[serde(default)]
    pub(super) suffix: Option<String>,
    /// The printed page the selection starts on, if the book has page numbers.
    #[serde(default)]
    pub(super) page: Option<String>,
    /// Where the selection starts in the chapter's text (UTF-16 units).
    #[serde(default)]
    pub(super) start: Option<usize>,
}

/// What `EPUB_APPLY_HIGHLIGHTS_FN` is given for each mark: where it is in the chapter's text
/// (UTF-16 offsets into `document.body.textContent`), found by `crate::anchor`, and how to draw it.
#[derive(serde::Serialize)]
pub(super) struct EpubHighlightPayload<'a> {
    pub(super) id: &'a str,
    pub(super) start: usize,
    pub(super) end: usize,
    /// `"highlight"`, `"underline"` or `"strikeout"` (the kind's own serialisation).
    pub(super) kind: fond_annot::AnnotationKind,
    /// Highlight colour (hex) — meaningless for underline/strikeout, which use the text colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) color: Option<&'a str>,
}

/// The chapter's text as the script that wraps marks sees it.
const EPUB_BODY_TEXT_JS: &str = "document.body ? document.body.textContent : ''";

/// A JS function *expression* (callers append `(args)`) that wraps each given range of the
/// chapter's text in a `<mark class="kartoteka-hl">`, clearing marks left by a previous call
/// first (so adding a highlight just re-runs this instead of reloading the page), and scrolls
/// the mark matching `scrollToId` into view.
pub(super) const EPUB_APPLY_HIGHLIGHTS_FN: &str = r#"(function(annotations, scrollToId) {
  function rangeAt(start, end) {
    var walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null);
    var node, pos = 0, startNode = null, startOffset = 0, endNode = null, endOffset = 0;
    while (node = walker.nextNode()) {
      var len = node.textContent.length;
      if (startNode === null && start >= pos && start < pos + len) {
        startNode = node; startOffset = start - pos;
      }
      if (end > pos && end <= pos + len) {
        endNode = node; endOffset = end - pos;
        break;
      }
      pos += len;
    }
    if (!startNode || !endNode) return null;
    var range = document.createRange();
    range.setStart(startNode, startOffset);
    range.setEnd(endNode, endOffset);
    return range;
  }

  document.querySelectorAll('mark.kartoteka-hl').forEach(function(m) {
    var parent = m.parentNode;
    if (!parent) return;
    while (m.firstChild) parent.insertBefore(m.firstChild, m);
    parent.removeChild(m);
    parent.normalize();
  });

  annotations.forEach(function(a) {
    var range = rangeAt(a.start, a.end);
    if (!range) return;
    var mark = document.createElement('mark');
    mark.className = 'kartoteka-hl';
    mark.dataset.annotationId = a.id;
    mark.dataset.kind = a.kind;
    if (a.kind === 'underline') {
      mark.style.background = 'transparent';
      mark.style.textDecoration = 'underline';
      mark.style.textDecorationColor = a.color || 'currentColor';
      mark.style.textDecorationThickness = '2px';
    } else if (a.kind === 'strikeout') {
      mark.style.background = 'transparent';
      mark.style.textDecoration = 'line-through';
      mark.style.textDecorationColor = a.color || 'currentColor';
    } else {
      mark.style.backgroundColor = a.color ? (a.color + '59') : 'rgba(246, 195, 68, 0.35)';
    }
    try {
      range.surroundContents(mark);
    } catch (e) {
      var contents = range.extractContents();
      mark.appendChild(contents);
      range.insertNode(mark);
    }
    if (scrollToId && a.id === scrollToId) {
      mark.scrollIntoView({ block: 'center' });
    }
  });
})"#;

/// JS that reports the `WebView`'s current text selection (if any) as JSON: `{empty: true}`
/// if nothing is meaningfully selected, else `{empty: false, text, prefix, suffix}` — the
/// selected text plus up to 40 characters of context on each side, computed by walking
/// `document.body`'s text nodes to find the selection's absolute character offset (the same
/// approach `EPUB_APPLY_HIGHLIGHTS_FN` uses in reverse, to re-locate a snippet later).
pub(super) const EPUB_CAPTURE_SELECTION_JS: &str = r#"(function() {
  var sel = window.getSelection();
  if (!sel || sel.rangeCount === 0 || !sel.toString().trim()) {
    return JSON.stringify({ empty: true });
  }
  var text = sel.toString();
  var range = sel.getRangeAt(0);
  function textOffsetOf(node, offset) {
    var walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null);
    var total = 0, n;
    while (n = walker.nextNode()) {
      if (n === node) return total + offset;
      total += n.textContent.length;
    }
    return total;
  }
  var page = null;
  (window.__pp || []).forEach(function(p) {
    var el = document.getElementById(p.id);
    if (el && (el === range.startContainer ||
        (el.compareDocumentPosition(range.startContainer) & Node.DOCUMENT_POSITION_FOLLOWING))) {
      page = p.label;
    }
  });
  var fullText = document.body.textContent;
  var startIdx = textOffsetOf(range.startContainer, range.startOffset);
  var prefix = fullText.slice(Math.max(0, startIdx - 40), startIdx);
  var suffix = fullText.slice(startIdx + text.length, startIdx + text.length + 40);
  sel.removeAllRanges();
  return JSON.stringify({ empty: false, text: text, prefix: prefix, suffix: suffix, page: page, start: startIdx });
})()"#;

/// An annotation's id, where it now starts and ends in the chapter's text, and whether the
/// words matched exactly.
pub(super) type Placed = (String, usize, usize, bool);

/// Where each annotation of `chapter` is in `text` (the chapter's text content), and which ones
/// could not be found. Offsets are UTF-16 units, as the page's script counts.
pub(super) fn locate_annotations(
    sidecar: &fond_annot::AnnotationSidecar,
    chapter: &str,
    text: &str,
) -> (Vec<Placed>, Vec<String>) {
    let mut found = Vec::new();
    let mut lost = Vec::new();
    for a in sidecar
        .annotations
        .iter()
        .filter(|a| a.chapter.as_deref() == Some(chapter))
    {
        let Some(snippet) = a.snippet.as_deref() else {
            continue;
        };
        let hint = crate::text_position_of(a);
        match crate::anchor::locate(
            text,
            snippet,
            a.snippet_prefix.as_deref(),
            a.snippet_suffix.as_deref(),
            hint,
        ) {
            Some(l) => {
                let units = crate::anchor::to_utf16(text, &[l.start, l.end]);
                found.push((a.id.clone(), units[0], units[1], l.exact));
            }
            None => lost.push(a.id.clone()),
        }
    }
    (found, lost)
}

/// Mark every saved annotation of the chapter on show: read the chapter's text, find each
/// passage in it (approximately if the book has changed since it was marked), then wrap them.
/// Passages that cannot be found are remembered, so the Notes list can say so. Called after
/// every chapter load and right after a mark is added or removed. Scrolls `scroll_to_id`'s mark
/// into view if given.
pub(super) fn epub_apply_highlights(
    view: &webkit6::WebView,
    state: &Rc<RefCell<EpubReaderState>>,
    scroll_to_id: Option<&str>,
) {
    let scroll_json = match scroll_to_id {
        Some(id) => serde_json::to_string(id).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    };
    let state = state.clone();
    let view_for_apply = view.clone();
    view.evaluate_javascript(
        EPUB_BODY_TEXT_JS,
        None,
        None,
        gio::Cancellable::NONE,
        move |result| {
            let Ok(text) = result.map(|v| v.to_str().to_string()) else {
                return;
            };
            let (payload, lost_changed) = {
                let mut r = state.borrow_mut();
                let Some(chapter) = r.spine.get(r.index).cloned() else {
                    return;
                };
                let store = r.store.clone();
                let sidecar = store.sidecar();
                let (found, lost) = locate_annotations(&sidecar, &chapter, &text);
                let ids_here: Vec<String> = sidecar
                    .annotations
                    .iter()
                    .filter(|a| a.chapter.as_deref() == Some(chapter.as_str()))
                    .map(|a| a.id.clone())
                    .collect();
                let before = r.lost.clone();
                r.lost.retain(|id| !ids_here.contains(id));
                r.lost.extend(lost);
                let changed = r.lost != before;
                let items: Vec<EpubHighlightPayload> = found
                    .iter()
                    .filter_map(|(id, start, end, _)| {
                        let a = sidecar.annotations.iter().find(|a| a.id == *id)?;
                        Some(EpubHighlightPayload {
                            id: &a.id,
                            start: *start,
                            end: *end,
                            kind: a.kind,
                            color: a.color.as_deref(),
                        })
                    })
                    .collect();
                (
                    serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string()),
                    changed.then(|| r.on_lost_changed.clone()).flatten(),
                )
            };
            let script = format!("{EPUB_APPLY_HIGHLIGHTS_FN}({payload}, {scroll_json})");
            view_for_apply.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});
            if let Some(f) = lost_changed {
                f();
            }
        },
    );
}

/// `(kind, colour hex, note)` -> mark the current browser selection.
pub(super) type EpubMarkFn = Rc<dyn Fn(fond_annot::AnnotationKind, Option<String>, Option<String>)>;

/// The selection-capture script; `clear` drops the selection once read (used when marking).
pub(super) fn epub_selection_js(clear: bool) -> String {
    if clear {
        EPUB_CAPTURE_SELECTION_JS.to_string()
    } else {
        EPUB_CAPTURE_SELECTION_JS.replace("sel.removeAllRanges();", "")
    }
}

pub(super) struct EpubSelection {
    pub(super) text: String,
    pub(super) chapter_number: usize,
    /// The printed page, when the book has page numbers.
    pub(super) page: Option<String>,
}

pub(super) fn show_epub_selection_popover(
    view: &webkit6::WebView,
    at: (f64, f64),
    selection: EpubSelection,
    host: &Rc<dyn ReaderHost>,
    apply_mark: &EpubMarkFn,
    reader_window: &adw::Window,
) {
    let popover = gtk4::Popover::new();
    popover.set_parent(view);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        at.0.round() as i32,
        at.1.round() as i32,
        1,
        1,
    )));
    let rows = gtk4::Box::new(Orientation::Vertical, 4);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);

    let colours = gtk4::Box::new(Orientation::Horizontal, 4);
    colours.set_halign(gtk4::Align::Center);
    let mut swatches: Vec<gtk4::Button> = Vec::new();
    for (i, color) in crate::palette::HIGHLIGHT_COLORS.iter().enumerate() {
        let button = gtk4::Button::new();
        button.add_css_class("flat");
        button.set_child(Some(&crate::palette::numbered_swatch(color.hex, i + 1)));
        button.set_tooltip_text(Some(&format!(
            "Highlight: {} ({})",
            crate::palette::highlight_label(i),
            i + 1
        )));
        button.update_property(&[gtk4::accessible::Property::Label(&format!(
            "Highlight as {}",
            crate::palette::highlight_label(i)
        ))]);
        let apply_mark = apply_mark.clone();
        let popover = popover.clone();
        let hex = color.hex.to_string();
        button.connect_clicked(move |_| {
            popover.popdown();
            apply_mark(
                fond_annot::AnnotationKind::Highlight,
                Some(hex.clone()),
                None,
            );
        });
        swatches.push(button.clone());
        colours.append(&button);
    }
    crate::bind_number_keys(&popover, &swatches);
    rows.append(&colours);

    let row = |label: &str, run: Rc<dyn Fn()>| {
        let button = popover_button(label, false);
        let popover = popover.clone();
        button.connect_clicked(move |_| {
            popover.popdown();
            run();
        });
        rows.append(&button);
    };
    let first = crate::palette::HIGHLIGHT_COLORS[0].hex.to_string();
    for (label, kind) in [
        ("Underline", fond_annot::AnnotationKind::Underline),
        ("Strike out", fond_annot::AnnotationKind::Strikeout),
    ] {
        let apply_mark = apply_mark.clone();
        let hex = first.clone();
        row(
            label,
            Rc::new(move || apply_mark(kind, Some(hex.clone()), None)),
        );
    }
    {
        let apply_mark = apply_mark.clone();
        let reader_window = reader_window.clone();
        let quote = selection.text.clone();
        let hex = first.clone();
        row(
            "Add note…",
            Rc::new(move || {
                let apply_mark = apply_mark.clone();
                let hex = hex.clone();
                crate::note_dialog(&reader_window, Some(&quote), move |note| {
                    apply_mark(
                        fond_annot::AnnotationKind::Highlight,
                        Some(hex.clone()),
                        Some(note),
                    );
                });
            }),
        );
    }
    rows.append(&popover_separator());
    for (label, with_citation) in [("Copy", false), ("Copy with citation", true)] {
        let host = host.clone();
        let text = selection.text.clone();
        let chapter = selection.chapter_number;
        let page = selection.page.clone();
        row(
            label,
            Rc::new(move || {
                let out = if with_citation {
                    crate::export::cite_snippet(
                        crate::export::preferred_format(),
                        &text,
                        &page.clone().unwrap_or_else(|| chapter.to_string()),
                        page.is_none(),
                        host.citation_key().as_deref(),
                    )
                } else {
                    text.clone()
                };
                if let Some(display) = gdk::Display::default() {
                    display.clipboard().set_text(&out);
                }
                host.notify("Copied to clipboard");
            }),
        );
    }

    popover.set_child(Some(&rows));
    let view = view.clone();
    popover.connect_closed(move |p| {
        p.unparent();
        view.grab_focus();
    });
    popover.popup();
}

/// Marks the current selection. Shared by the header's Apply button and the popover that
/// appears when a selection ends; the browser selection is cleared once it's marked.
pub(super) fn build_apply_mark(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
) -> EpubMarkFn {
    let reader = reader.clone();
    let view = web_view.clone();
    let host = host.clone();
    Rc::new(move |kind, color, note| {
        let reader = reader.clone();
        let host = host.clone();
        view.evaluate_javascript(
            &epub_selection_js(true),
            None,
            None,
            gio::Cancellable::NONE,
            move |result| {
                let raw = match result {
                    Ok(v) => v.to_str().to_string(),
                    Err(e) => {
                        host.notify(&format!("Could not read selection: {e}"));
                        return;
                    }
                };
                let Ok(capture) = serde_json::from_str::<EpubSelectionCapture>(&raw) else {
                    host.notify("Could not read selection");
                    return;
                };
                let capture_page = capture.page.clone();
                let capture_start = capture.start;
                let snippet = capture.text.filter(|t| !t.trim().is_empty());
                let Some(snippet) = (!capture.empty).then_some(snippet).flatten() else {
                    host.notify("Select some text first");
                    return;
                };
                let chapter = {
                    let r = reader.borrow();
                    r.spine.get(r.index).cloned()
                };
                let Some(chapter) = chapter else {
                    return;
                };
                let chapter_for_label = chapter.clone();
                let mut annotation = fond_annot::Annotation::drawn_epub(
                    kind,
                    chapter,
                    snippet,
                    capture.prefix,
                    capture.suffix,
                    note,
                );
                annotation.color = color;
                if let Some(start) = capture_start {
                    let len = annotation
                        .snippet
                        .as_deref()
                        .map_or(0, |t| t.encode_utf16().count());
                    crate::set_text_position(&mut annotation, Some((start, start + len)));
                }
                {
                    let r = reader.borrow();
                    let label = page_label::label_for(&r, &chapter_for_label, capture_page);
                    crate::set_page_label(&mut annotation, label.as_deref());
                }
                let store = reader.borrow().store.clone();
                match store.add(annotation) {
                    Ok(()) => host.notify("Added"),
                    Err(e) => host.notify(&e),
                }
            },
        );
    })
}

/// Select first, then mark: when the pointer is released over a selection, offer the same
/// actions as the PDF reader's selection popover.
pub(super) fn install_selection_popover(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
    apply_mark: &EpubMarkFn,
    reader_window: &adw::Window,
) {
    {
        let click = gtk4::GestureDrag::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        click.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let view = web_view.clone();
        let host = host.clone();
        let reader = reader.clone();
        let apply_mark = apply_mark.clone();
        let reader_window = reader_window.clone();
        let pointer: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((0.0, 0.0)));
        {
            let motion = gtk4::EventControllerMotion::new();
            motion.set_propagation_phase(gtk4::PropagationPhase::Capture);
            let pointer = pointer.clone();
            motion.connect_motion(move |_, x, y| pointer.set((x, y)));
            web_view.add_controller(motion);
        }
        click.connect_drag_end(move |_, _, _| {
            let (x, y) = pointer.get();
            let view = view.clone();
            let host = host.clone();
            let reader = reader.clone();
            let apply_mark = apply_mark.clone();
            let reader_window = reader_window.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(80), move || {
                let view_for_popover = view.clone();
                view.evaluate_javascript(
                    &epub_selection_js(false),
                    None,
                    None,
                    gio::Cancellable::NONE,
                    move |result| {
                        let Ok(v) = result else { return };
                        let Ok(capture) = serde_json::from_str::<EpubSelectionCapture>(&v.to_str())
                        else {
                            return;
                        };
                        let Some(text) = capture
                            .text
                            .filter(|t| !capture.empty && !t.trim().is_empty())
                        else {
                            return;
                        };
                        let (chapter_number, page) = {
                            let r = reader.borrow();
                            let chapter = r.spine.get(r.index).cloned().unwrap_or_default();
                            (
                                r.index + 1,
                                page_label::label_for(&r, &chapter, capture.page.clone()),
                            )
                        };
                        show_epub_selection_popover(
                            &view_for_popover,
                            (x, y),
                            EpubSelection {
                                text,
                                chapter_number,
                                page,
                            },
                            &host,
                            &apply_mark,
                            &reader_window,
                        );
                    },
                );
            });
        });
        web_view.add_controller(click);
    }
}
