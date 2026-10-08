use super::*;

/// Rotate an RGBA buffer clockwise by 0/90/180/270 degrees, returning the rotated buffer and
/// its (width, height) — swapped for 90/270. `degrees` outside that set is treated as 0.
pub(super) fn rotate_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
    degrees: u16,
) -> (Vec<u8>, u32, u32) {
    let w = width as usize;
    let h = height as usize;
    match degrees % 360 {
        90 => {
            let (nw, nh) = (h, w);
            let mut out = vec![0u8; rgba.len()];
            for y in 0..h {
                for x in 0..w {
                    let src = (y * w + x) * 4;
                    let dst = (x * nw + (h - 1 - y)) * 4;
                    out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                }
            }
            (out, nw as u32, nh as u32)
        }
        180 => {
            let mut out = vec![0u8; rgba.len()];
            for y in 0..h {
                for x in 0..w {
                    let src = (y * w + x) * 4;
                    let dst = ((h - 1 - y) * w + (w - 1 - x)) * 4;
                    out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                }
            }
            (out, width, height)
        }
        270 => {
            let (nw, nh) = (h, w);
            let mut out = vec![0u8; rgba.len()];
            for y in 0..h {
                for x in 0..w {
                    let src = (y * w + x) * 4;
                    let dst = ((w - 1 - x) * nw + y) * 4;
                    out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                }
            }
            (out, nw as u32, nh as u32)
        }
        _ => (rgba.to_vec(), width, height),
    }
}

/// Invert an RGBA buffer's colours in place, leaving alpha untouched — a night-reading mode
/// for scanned/white-background pages, which stay bright regardless of the app's own theme
/// since they're just pixels, not something CSS/`adw::StyleManager` can recolour.
pub(super) fn invert_rgba(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        px[0] = 255 - px[0];
        px[1] = 255 - px[1];
        px[2] = 255 - px[2];
    }
}

pub(super) fn render_open(
    r: &ReaderState,
    page: u16,
    width: u32,
) -> Option<fond_doc::RenderedPage> {
    let Some(doc) = &r.doc else {
        return fond_doc::render_page(r.pdfium, &r.bytes, page, width).ok();
    };
    let pdf_page = doc.pages().get(page).ok()?;
    let config = pdfium_render::prelude::PdfRenderConfig::new()
        .set_target_width(width.max(1) as pdfium_render::prelude::Pixels);
    let bitmap = pdf_page.render_with_config(&config).ok()?;
    Some(fond_doc::RenderedPage {
        width: bitmap.width() as u32,
        height: bitmap.height() as u32,
        rgba: bitmap.as_rgba_bytes(),
    })
}

/// Render `page` (0-based) to a ready-to-display texture, with this entry's saved
/// annotations — and, if `page` has the current search match, that match's highlight too —
/// blended in. Shared by both the page-by-page view and continuous-scroll mode so the two
/// can never visually disagree about what a page looks like. Returns the texture, its pixel
/// size, and the page's PDF-point size (the scale a drag-selected rectangle on that page
/// converts through).
pub(super) fn render_pdf_page_texture(
    r: &ReaderState,
    page: u16,
) -> Option<(gdk::Texture, u32, u32)> {
    let width = (READER_BASE_WIDTH * r.zoom) as u32;
    let geom = r.geom(page);
    let page_pts = geom.map(|g| g.display_size()).unwrap_or((0.0, 0.0));
    let to_display = |quads: &[[f64; 8]]| match geom {
        Some(g) => g.quads_to_display(quads),
        None => quads.to_vec(),
    };
    let mut rp = render_open(r, page, width)?;
    let current_page = page as u32 + 1;

    // A freestanding Note (no quadpoints — added via the "Note…" button on blank page) is
    // already excluded by the `!quadpoints.is_empty()` filter above; a Note created *from a
    // text selection* ("Create note from…", see `show_pdf_context_menu`) does carry real
    // quadpoints and is blended here like a highlight, so it stays visible on the page and
    // not just listed in the sidebar. Each annotation keeps its own colour (from the colour
    // picker at draw time, or the default amber for a Note, which has none), so this blends
    // per-annotation rather than batching every quad on the page into one shared-colour call.
    for a in r
        .store
        .sidecar()
        .annotations
        .iter()
        .filter(|a| a.page == Some(current_page) && !a.quadpoints.is_empty())
    {
        let kind = match a.kind {
            fond_annot::AnnotationKind::Highlight | fond_annot::AnnotationKind::Note => {
                fond_doc::MarkupKind::Highlight
            }
            fond_annot::AnnotationKind::Underline => fond_doc::MarkupKind::Underline,
            fond_annot::AnnotationKind::Strikeout => fond_doc::MarkupKind::Strikeout,
            #[allow(unreachable_patterns)]
            // AnnotationKind is non_exhaustive from fond-core's next rev
            _ => continue,
        };
        let items: Vec<(fond_doc::MarkupKind, [f64; 8])> = to_display(&a.quadpoints)
            .into_iter()
            .map(|q| (kind, q))
            .collect();
        fond_doc::blend_annotations(
            &mut rp,
            page_pts.0,
            page_pts.1,
            &items,
            annotation_rgba(a.color.as_deref()),
        );
    }

    // The current search match, if it's on this page — blended in its own colour, on top of
    // any saved highlights, so it reads as "found this" and not as another saved annotation.
    if let Some(current) = r.search_matches.get(r.search_current) {
        if current.page == page {
            fond_doc::blend_highlights(
                &mut rp,
                page_pts.0,
                page_pts.1,
                &to_display(&current.quads),
                SEARCH_MATCH_RGBA,
            );
        }
    }

    // A "Select text" drag's selection, if it's on this page — kept visible after the drag
    // ends (previously it vanished the instant you released the mouse, leaving only the
    // clipboard copy as any trace) until a new selection replaces it or it's consumed by
    // "Create note from…". Uses the live drag-preview's own colour so a selection looks the
    // same while dragging and once settled.
    if let Some((sel_page, _, quads)) = &r.last_selection {
        if *sel_page == page {
            fond_doc::blend_highlights(
                &mut rp,
                page_pts.0,
                page_pts.1,
                &to_display(quads),
                SELECTION_RGBA,
            );
        }
    }

    if r.invert_colors {
        invert_rgba(&mut rp.rgba);
    }
    let (data, out_w, out_h) = if r.rotation != 0 {
        let (rotated, w, h) = rotate_rgba(&rp.rgba, rp.width, rp.height, r.rotation);
        (glib::Bytes::from(&rotated), w, h)
    } else {
        (glib::Bytes::from(&rp.rgba), rp.width, rp.height)
    };
    let texture = gdk::MemoryTexture::new(
        out_w as i32,
        out_h as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &data,
        (out_w * 4) as usize,
    );
    Some((texture.upcast(), out_w, out_h))
}
