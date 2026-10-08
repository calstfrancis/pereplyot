use super::*;

/// One annotation as sent to the reader's highlight-apply JS: just enough to find it in the
/// rendered DOM (`snippet`, plus `prefix`/`suffix` context to disambiguate a snippet that
/// appears more than once in the chapter) and mark it (`id`, for the CSS class and as the
/// optional scroll target).
#[derive(serde::Serialize)]
pub(super) struct EpubHighlightPayload<'a> {
    pub(super) id: &'a str,
    pub(super) snippet: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) prefix: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) suffix: Option<&'a str>,
    /// Serializes lowercase (`"highlight"`/`"underline"`/`"strikeout"`) via
    /// `AnnotationKind`'s own `Serialize` impl — `EPUB_APPLY_HIGHLIGHTS_FN` switches on
    /// this to decide which CSS treatment to apply.
    pub(super) kind: fond_annot::AnnotationKind,
    /// Highlight colour (hex, e.g. `#f6c344`) — meaningless for underline/strikeout,
    /// which always use the current text colour so they read correctly in dark mode too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) color: Option<&'a str>,
}

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
}

/// A JS function *expression* (no trailing call — callers append `(args)`) that finds each
/// given annotation's snippet text in the current document and wraps it in a
/// `<mark class="kartoteka-hl">`, clearing any marks left by a previous call first
/// (idempotent re-apply, so adding a highlight can just re-run this instead of reloading the
/// page). Scrolls the mark matching `scrollToId` into view, if given. Mirrors
/// `select_text_in_rect`'s multi-node-aware approach in `fond-doc/src/pdf.rs` — walk every
/// text node, find the target substring, map it back onto node+offset pairs — just over DOM
/// text nodes instead of PDF characters, since there's no PDFium text layer here.
pub(super) const EPUB_APPLY_HIGHLIGHTS_FN: &str = r#"(function(annotations, scrollToId) {
  function findTextRange(root, snippet, prefix, suffix) {
    if (!snippet) return null;
    var walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, null);
    var nodes = [];
    var fullText = '';
    var node;
    while (node = walker.nextNode()) {
      nodes.push({ node: node, start: fullText.length });
      fullText += node.textContent;
    }
    var combined = (prefix || '') + snippet + (suffix || '');
    var idx, endIdx;
    var combinedIdx = combined.length > snippet.length ? fullText.indexOf(combined) : -1;
    if (combinedIdx !== -1) {
      idx = combinedIdx + (prefix || '').length;
      endIdx = idx + snippet.length;
    } else {
      var plainIdx = fullText.indexOf(snippet);
      if (plainIdx === -1) return null;
      idx = plainIdx;
      endIdx = idx + snippet.length;
    }
    var startNode = null, startOffset = 0, endNode = null, endOffset = 0;
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      var nEnd = n.start + n.node.textContent.length;
      if (startNode === null && idx >= n.start && idx < nEnd) {
        startNode = n.node; startOffset = idx - n.start;
      }
      if (endIdx > n.start && endIdx <= nEnd) {
        endNode = n.node; endOffset = endIdx - n.start;
      }
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
    var range = findTextRange(document.body, a.snippet, a.prefix, a.suffix);
    if (!range) return;
    var mark = document.createElement('mark');
    mark.className = 'kartoteka-hl';
    mark.dataset.annotationId = a.id;
    mark.dataset.kind = a.kind;
    // No background/foreground colour is hardcoded beyond the highlight tint itself
    // (which is the whole point of a highlight) — underline/strikeout use `currentColor`
    // so they read correctly against the page's own text colour in light or dark mode.
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
  var fullText = document.body.textContent;
  var startIdx = textOffsetOf(range.startContainer, range.startOffset);
  var prefix = fullText.slice(Math.max(0, startIdx - 40), startIdx);
  var suffix = fullText.slice(startIdx + text.length, startIdx + text.length + 40);
  sel.removeAllRanges();
  return JSON.stringify({ empty: false, text: text, prefix: prefix, suffix: suffix });
})()"#;

/// Serialize the annotations anchored to `chapter` into the JSON array
/// `EPUB_APPLY_HIGHLIGHTS_FN` expects. An annotation with no `snippet` (shouldn't happen for
/// an EPUB one — `drawn_epub` always sets it — but the field is `Option` since the type is
/// shared with PDF annotations) is skipped rather than sent as an unfindable empty search.
pub(super) fn epub_highlight_payload_json(
    sidecar: &fond_annot::AnnotationSidecar,
    chapter: &str,
) -> String {
    let items: Vec<EpubHighlightPayload> = sidecar
        .annotations
        .iter()
        .filter(|a| a.chapter.as_deref() == Some(chapter))
        .filter_map(|a| {
            a.snippet.as_deref().map(|s| EpubHighlightPayload {
                id: &a.id,
                snippet: s,
                prefix: a.snippet_prefix.as_deref(),
                suffix: a.snippet_suffix.as_deref(),
                kind: a.kind,
                color: a.color.as_deref(),
            })
        })
        .collect();
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
}

/// Run `EPUB_APPLY_HIGHLIGHTS_FN` against the currently-loaded chapter, scrolling
/// `scroll_to_id`'s mark into view if given. Called after every chapter load (so navigating
/// away and back keeps showing highlights) and right after adding a new highlight (so it
/// appears immediately, no reload needed).
pub(super) fn epub_apply_highlights(
    view: &webkit6::WebView,
    state: &Rc<RefCell<EpubReaderState>>,
    scroll_to_id: Option<&str>,
) {
    let payload_json = {
        let r = state.borrow();
        match r.spine.get(r.index) {
            Some(chapter) => epub_highlight_payload_json(&r.store.sidecar(), chapter),
            None => return,
        }
    };
    let scroll_json = match scroll_to_id {
        Some(id) => serde_json::to_string(id).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    };
    let script = format!("{EPUB_APPLY_HIGHLIGHTS_FN}({payload_json}, {scroll_json})");
    view.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});
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
        row(
            label,
            Rc::new(move || {
                let out = if with_citation {
                    crate::export::cite_snippet(
                        crate::export::preferred_format(),
                        &text,
                        &chapter.to_string(),
                        true,
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
                let mut annotation = fond_annot::Annotation::drawn_epub(
                    kind,
                    chapter,
                    snippet,
                    capture.prefix,
                    capture.suffix,
                    note,
                );
                annotation.color = color;
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
                        let chapter_number = reader.borrow().index + 1;
                        show_epub_selection_popover(
                            &view_for_popover,
                            (x, y),
                            EpubSelection {
                                text,
                                chapter_number,
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
