use super::*;

/// Repaint the Reading view's marks from the reader's annotations.
pub(super) fn paint_text_marks(
    reader: &Rc<RefCell<ReaderState>>,
    view: &Rc<crate::reflow::view::ReadingView>,
) {
    use crate::reflow::view::{MarkStyle, TextMark};
    let marks: Vec<TextMark> = {
        let r = reader.borrow();
        let sidecar = r.store.sidecar();
        let collected: Vec<TextMark> = sidecar
            .annotations
            .iter()
            .filter_map(|a| {
                let page = a.page?.checked_sub(1)? as u16;
                let quote = a.snippet.clone()?;
                let rects = r
                    .geom(page)
                    .map(|geom| display_rects(geom, &a.quadpoints))
                    .unwrap_or_default();
                Some(TextMark {
                    page,
                    quote,
                    rects,
                    rgba: annotation_rgba(a.color.as_deref()),
                    style: match a.kind {
                        fond_annot::AnnotationKind::Underline => MarkStyle::Underline,
                        fond_annot::AnnotationKind::Strikeout => MarkStyle::Strikeout,
                        _ => MarkStyle::Highlight,
                    },
                })
            })
            .collect();
        collected
    };
    view.apply_marks(&marks);
}

/// Quadpoints (PDF user space) as rectangles in page points as displayed, origin top left.
pub(super) fn display_rects(geom: PageGeom, quads: &[[f64; 8]]) -> Vec<[f32; 4]> {
    let (_, height) = geom.display_size();
    geom.quads_to_display(quads)
        .iter()
        .map(|q| {
            let xs = [q[0], q[2], q[4], q[6]];
            let ys = [q[1], q[3], q[5], q[7]];
            let min = |v: &[f64; 4]| v.iter().cloned().fold(f64::INFINITY, f64::min) as f32;
            let max = |v: &[f64; 4]| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max) as f32;
            [min(&xs), height - max(&ys), max(&xs), height - min(&ys)]
        })
        .collect()
}

/// Rectangles on `page`, in page points as displayed with the origin top left, as quadpoints in
/// PDF user space — what an annotation stores.
pub(super) fn rects_to_quads(
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    rects: &[(u16, [f32; 4])],
) -> Vec<[f64; 8]> {
    let r = reader.borrow();
    let Some(geom) = r.geom(page) else {
        return Vec::new();
    };
    let (w, h) = geom.display_size();
    let (w, h) = (w as f64, h as f64);
    rects
        .iter()
        .filter(|(p, _)| *p == page)
        .map(|(_, b)| {
            let corner = |x: f32, y: f32| geom.px_to_pdf(x as f64, y as f64, w, h);
            let (tl, tr) = (corner(b[0], b[1]), corner(b[2], b[1]));
            let (bl, br) = (corner(b[0], b[3]), corner(b[2], b[3]));
            [tl.0, tl.1, tr.0, tr.1, bl.0, bl.1, br.0, br.1]
        })
        .collect()
}

/// The pointer shown over the page: an I-beam in "Select text" mode (this is a text
/// selection, not a highlight-drawing gesture), the default arrow otherwise. `None` means
/// "reset to default" — `Widget::set_cursor(None)` is how GTK4 clears a per-widget override.
pub(super) fn cursor_for_select_mode(select_mode: bool) -> Option<gdk::Cursor> {
    select_mode
        .then(|| gdk::Cursor::from_name("text", None))
        .flatten()
}

/// Convert a drag gesture's start/end (widget-local pixel coordinates on `page`'s own
/// render, at that page's own `render_w`×`render_h` scale and `PageGeom`) into
/// PDF-space quadpoints — hugging real text if the drag covers any, same as
/// `select_text_in_rect` documents — and save a new annotation for it. The shared save path
/// for both the page-by-page view's drag handler and continuous-scroll mode's per-page ones,
/// so the two don't duplicate the coordinate math and sidecar write. Returns whether an
/// annotation was actually saved, so the caller knows whether a redraw is needed.
/// The geometry a drag gesture needs converted to PDF-space quadpoints: the page's own
/// rendered pixel size and PDF-point size (its scale), and the drag's start/end in that
/// pixel space. Grouped into one struct rather than eight loose parameters on
/// `save_drag_annotation`.
pub(super) struct DragGeometry {
    pub(super) render_w: u32,
    pub(super) render_h: u32,
    pub(super) page: Option<PageGeom>,
    pub(super) start_x: f64,
    pub(super) start_y: f64,
    pub(super) end_x: f64,
    pub(super) end_y: f64,
}

/// The PDF-space points a drag's start/end cover, given its pixel geometry — the inverse of
/// the page's render scale. Order-preserving (unlike a sorted rectangle), since
/// `select_text_range` needs to know which endpoint is the reading-order start. `None` if
/// the geometry is degenerate (zero-sized render, or a page with no reported point size).
pub(super) fn drag_pdf_points(geom: &DragGeometry) -> Option<((f64, f64), (f64, f64))> {
    let &DragGeometry {
        render_w,
        render_h,
        page,
        start_x,
        start_y,
        end_x,
        end_y,
    } = geom;
    let page = page?;
    if render_w == 0 || render_h == 0 {
        return None;
    }
    let (w, h) = (render_w as f64, render_h as f64);
    Some((
        page.px_to_pdf(start_x, start_y, w, h),
        page.px_to_pdf(end_x, end_y, w, h),
    ))
}

/// The text under `quads` (one per line, user space), read back through PDFium's bounded-
/// text call. `fond_doc::select_text_range`'s own `text` is built from individual glyphs and
/// skips PDFium's generated spaces, so on the many PDFs that position words instead of
/// storing space characters (LaTeX and Typst output, for a start) it runs every word
/// together. Each line's box is shrunk to its middle band so neighbouring lines' glyph boxes,
/// which often overlap vertically, don't leak in.
pub(super) fn selection_text(r: &ReaderState, page: u16, quads: &[[f64; 8]]) -> Option<String> {
    let pdf_page = r.doc.as_ref()?.pages().get(page).ok()?;
    let text = pdf_page.text().ok()?;
    let lines: Vec<String> = quads
        .iter()
        .map(|q| {
            let (l, rt) = (
                q[0].min(q[2]).min(q[4]).min(q[6]),
                q[0].max(q[2]).max(q[4]).max(q[6]),
            );
            let (b, t) = (
                q[1].min(q[3]).min(q[5]).min(q[7]),
                q[1].max(q[3]).max(q[5]).max(q[7]),
            );
            // Shrink across the line a quarter each side, along it only a hair. A line is wide
            // on an upright page but a tall strip in user space on a quarter-turned one.
            let (dx, dy) = if rt - l >= t - b {
                (((rt - l) * 0.25).min(0.5), (t - b) * 0.25)
            } else {
                ((rt - l) * 0.25, ((t - b) * 0.25).min(0.5))
            };
            let rect = pdfium_render::prelude::PdfRect::new_from_values(
                (b + dy) as f32,
                (l + dx) as f32,
                (t - dy) as f32,
                (rt - dx) as f32,
            );
            text.inside_rect(rect)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join(" "))
}

pub(super) fn copy_to_clipboard(host: &Rc<dyn ReaderHost>, text: &str) {
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(text);
    }
    host.notify("Copied to clipboard");
}

/// Selects the text under a "Select text" mode drag, remembering it (page, text, quadpoints)
/// on `reader` so it stays visibly marked on the page and so the selection popover — and a
/// note added right after — can act on it; see `last_selection`. Returns whether anything was
/// selected, so the caller knows whether to re-render and show the popover.
pub(super) fn select_drag_text(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    geom: &DragGeometry,
) -> bool {
    let Some((start, end)) = drag_pdf_points(geom) else {
        return false;
    };
    let selection = {
        let r = reader.borrow();
        fond_doc::select_text_range(
            r.pdfium,
            r.bytes(),
            page,
            start.0 as f32,
            start.1 as f32,
            end.0 as f32,
            end.1 as f32,
        )
        .ok()
        .flatten()
    };
    match selection {
        Some(sel) if !sel.text.trim().is_empty() => {
            let text = selection_text(&reader.borrow(), page, &sel.quads).unwrap_or(sel.text);
            reader.borrow_mut().last_selection = Some((page, text, sel.quads));
            true
        }
        _ => {
            host.notify("No text found in selection");
            false
        }
    }
}

/// Everything the selection popover and the 1–4 quick-mark keys need to save a mark.
#[derive(Clone)]
pub(super) struct MarkCtx {
    pub(super) host: Rc<dyn ReaderHost>,
    pub(super) reader: Rc<RefCell<ReaderState>>,
    pub(super) reader_window: adw::Window,
}

/// Turn the current selection into a mark of `kind`, in `color` (hex). Consumes the selection.
pub(super) fn apply_selection_mark(
    ctx: &MarkCtx,
    kind: fond_annot::AnnotationKind,
    color: &str,
) -> bool {
    let Some((page, text, mut quads)) = ctx.reader.borrow_mut().last_selection.take() else {
        return false;
    };
    if quads.is_empty() {
        // A selection made in the Text view has no page geometry yet; find the quote on the
        // page so the mark also shows on the page image.
        let r = ctx.reader.borrow();
        quads = fond_doc::search_document(r.pdfium, r.bytes(), &text)
            .unwrap_or_default()
            .into_iter()
            .find(|m| m.page == page)
            .map(|m| m.quads)
            .unwrap_or_default();
    }
    let annotation = fond_annot::Annotation::drawn(
        kind,
        page as u32 + 1,
        quads,
        Some(text),
        None,
        Some(color.to_string()),
    );
    let store = ctx.reader.borrow().store.clone();
    match store.add(annotation) {
        Ok(()) => {
            ctx.host.notify(match kind {
                fond_annot::AnnotationKind::Underline => "Underline added",
                fond_annot::AnnotationKind::Strikeout => "Strikeout added",
                _ => "Highlight added",
            });
            true
        }
        Err(e) => {
            ctx.host.notify(&e);
            false
        }
    }
}

/// Copy the current selection, optionally as a quotation with its citation in the format last
/// used for export (Typst by default).
pub(super) fn copy_selection(ctx: &MarkCtx, with_citation: bool) {
    let (page, text) = {
        let r = ctx.reader.borrow();
        match &r.last_selection {
            Some((p, t, _)) => (*p, t.clone()),
            None => return,
        }
    };
    if !with_citation {
        copy_to_clipboard(&ctx.host, &text);
        return;
    }
    let locator = {
        let r = ctx.reader.borrow();
        r.page_labels
            .get(page as usize)
            .and_then(|l| l.clone())
            .unwrap_or_else(|| (page + 1).to_string())
    };
    let snippet = crate::export::cite_snippet(
        crate::export::preferred_format(),
        &text,
        &locator,
        false,
        ctx.host.citation_key().as_deref(),
    );
    copy_to_clipboard(&ctx.host, &snippet);
}

/// The popover offered when a text selection ends: the four colours, underline, strikeout, a
/// note, and copy / copy-with-citation.
pub(super) fn show_selection_popover(
    ctx: &MarkCtx,
    parent: &impl IsA<gtk4::Widget>,
    x: f64,
    y: f64,
    page: u16,
) {
    let parent: gtk4::Widget = parent.as_ref().clone();
    let popover = gtk4::Popover::new();
    popover.set_parent(&parent);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
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
        let ctx = ctx.clone();
        let popover = popover.clone();
        let hex = color.hex;
        button.connect_clicked(move |_| {
            popover.popdown();
            apply_selection_mark(&ctx, fond_annot::AnnotationKind::Highlight, hex);
        });
        swatches.push(button.clone());
        colours.append(&button);
    }
    crate::bind_number_keys(&popover, &swatches);
    rows.append(&colours);

    let action = |label: &str, run: Rc<dyn Fn()>| {
        let button = popover_button(label, false);
        let popover = popover.clone();
        button.connect_clicked(move |_| {
            popover.popdown();
            run();
        });
        button
    };
    let default_hex = ctx.reader.borrow().draw_color.clone();
    for (label, kind) in [
        ("Underline", fond_annot::AnnotationKind::Underline),
        ("Strike out", fond_annot::AnnotationKind::Strikeout),
    ] {
        let ctx = ctx.clone();
        let hex = default_hex.clone();
        rows.append(&action(
            label,
            Rc::new(move || {
                apply_selection_mark(&ctx, kind, &hex);
            }),
        ));
    }
    {
        let ctx = ctx.clone();
        rows.append(&action(
            "Add note…",
            Rc::new(move || {
                show_pdf_note_dialog(&ctx.host, &ctx.reader, &ctx.reader_window);
            }),
        ));
    }
    rows.append(&popover_separator());
    {
        let ctx = ctx.clone();
        rows.append(&action(
            "Copy",
            Rc::new(move || copy_selection(&ctx, false)),
        ));
    }
    {
        let ctx = ctx.clone();
        rows.append(&action(
            "Copy with citation",
            Rc::new(move || copy_selection(&ctx, true)),
        ));
    }
    let _ = page;

    popover.set_child(Some(&rows));
    popover.connect_closed(move |p| {
        p.unparent();
        crate::grab_focus_keeping_scroll(&parent);
    });
    popover.popup();
}

pub(super) fn save_drag_annotation(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    geom: DragGeometry,
) -> bool {
    let Some((start, end)) = drag_pdf_points(&geom) else {
        return false;
    };

    let (draw_kind, draw_color) = {
        let r = reader.borrow();
        (r.draw_kind, r.draw_color.clone())
    };
    // "Select text" mode has no `AnnotationKind` to save under — the caller routes that mode
    // to `copy_drag_selection` instead and never reaches here, but guard anyway.
    let Some(draw_kind) = draw_kind else {
        return false;
    };

    // Prefer the actual text under the drag, line-aware (a straight vertical drag through a
    // paragraph selects each in-between line in full, not just the narrow column under the
    // pointer) — over the drag rectangle's own bounding box. Falls back to the plain
    // rectangle when the drag covers no text (a figure, a blank margin).
    let (quads, snippet) = {
        let r = reader.borrow();
        fond_doc::select_text_range(
            r.pdfium,
            r.bytes(),
            page,
            start.0 as f32,
            start.1 as f32,
            end.0 as f32,
            end.1 as f32,
        )
        .ok()
        .flatten()
    }
    .map(|sel| {
        let text = selection_text(&reader.borrow(), page, &sel.quads).unwrap_or(sel.text);
        (sel.quads, Some(text))
    })
    .unwrap_or_else(|| {
        let x0 = start.0.min(end.0);
        let x1 = start.0.max(end.0);
        let y_top = start.1.max(end.1);
        let y_bottom = start.1.min(end.1);
        (
            vec![[x0, y_top, x1, y_top, x0, y_bottom, x1, y_bottom]],
            None,
        )
    });

    let annotation = fond_annot::Annotation::drawn(
        draw_kind,
        page as u32 + 1,
        quads,
        snippet,
        None,
        Some(draw_color),
    );

    let store = reader.borrow().store.clone();
    match store.add(annotation) {
        Ok(()) => {
            let label = match draw_kind {
                fond_annot::AnnotationKind::Highlight => "Highlight added",
                fond_annot::AnnotationKind::Underline => "Underline added",
                fond_annot::AnnotationKind::Strikeout => "Strikeout added",
                fond_annot::AnnotationKind::Note => "Annotation added",
                // AnnotationKind is non_exhaustive from fond-core's next rev
                _ => "Annotation added",
            };
            host.notify(label);
            true
        }
        Err(e) => {
            host.notify(&e);
            false
        }
    }
}

/// Which annotation (if any) on `page` contains the PDF-space point `(x_pt, y_pt)` — the
/// topmost (most recently added) one whose quad bounding box covers the point, so a
/// right-click over overlapping highlights lands on the one the user most likely means.
/// Returns the annotation's id.
pub(super) fn annotation_at_pdf_point(
    annotations: &fond_annot::AnnotationSidecar,
    page: u16,
    x_pt: f32,
    y_pt: f32,
) -> Option<String> {
    let page_num = page as u32 + 1;
    annotations
        .annotations
        .iter()
        .rev()
        .find(|a| {
            a.page == Some(page_num)
                && a.quadpoints.iter().any(|q| {
                    let min_x = q[0].min(q[2]).min(q[4]).min(q[6]) as f32;
                    let max_x = q[0].max(q[2]).max(q[4]).max(q[6]) as f32;
                    let min_y = q[1].min(q[3]).min(q[5]).min(q[7]) as f32;
                    let max_y = q[1].max(q[3]).max(q[5]).max(q[7]) as f32;
                    x_pt >= min_x && x_pt <= max_x && y_pt >= min_y && y_pt <= max_y
                })
        })
        .map(|a| a.id.clone())
}
