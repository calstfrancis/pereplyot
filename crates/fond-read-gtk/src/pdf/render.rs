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

/// What the page's overlay (saved marks, the current search match, the live selection) looks
/// like right now, in the page's displayed point space, plus a hash so it can key a cache.
fn overlay_for(r: &ReaderState, page: u16, geom: Option<PageGeom>) -> (Overlay, u64) {
    use std::hash::{Hash, Hasher};
    let page_pts = geom.map(|g| g.display_size()).unwrap_or((0.0, 0.0));
    let to_display = |quads: &[[f64; 8]]| match geom {
        Some(g) => g.quads_to_display(quads),
        None => quads.to_vec(),
    };
    let current_page = page as u32 + 1;
    let mut overlay = Overlay {
        page_pts,
        ..Overlay::default()
    };
    // A freestanding Note (no quadpoints) is skipped; a Note made from a text selection carries
    // real quadpoints and is drawn like a highlight. Each mark keeps its own colour.
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
            _ => continue,
        };
        overlay.marks.push((
            kind,
            to_display(&a.quadpoints),
            annotation_rgba(a.color.as_deref()),
        ));
    }
    // The current search match is drawn in its own colour on top of saved marks, and a "Select
    // text" selection stays visible after the drag ends until a new one replaces it.
    if let Some(current) = r.search_matches.get(r.search_current) {
        if current.page == page {
            overlay
                .highlights
                .push((to_display(&current.quads), SEARCH_MATCH_RGBA));
        }
    }
    if let Some((sel_page, _, quads)) = &r.last_selection {
        if *sel_page == page {
            overlay.highlights.push((to_display(quads), SELECTION_RGBA));
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let quads_hash = |h: &mut std::collections::hash_map::DefaultHasher, qs: &[[f64; 8]]| {
        for q in qs {
            for v in q {
                v.to_bits().hash(h);
            }
        }
    };
    for (kind, quads, rgba) in &overlay.marks {
        (*kind as u8).hash(&mut h);
        rgba.hash(&mut h);
        quads_hash(&mut h, quads);
    }
    for (quads, rgba) in &overlay.highlights {
        rgba.hash(&mut h);
        quads_hash(&mut h, quads);
    }
    (overlay, h.finish())
}

/// The page's size on screen in logical pixels (unrotated, then swapped for a quarter turn).
pub(super) fn logical_page_size(r: &ReaderState, page: u16) -> (u32, u32) {
    let lw = (READER_BASE_WIDTH * r.zoom).max(1.0);
    let (pw, ph) = r.display_size(page);
    let lh = (lw * ph as f64 / (pw as f64).max(1.0)).round().max(1.0);
    let (lw, lh) = (lw as u32, lh as u32);
    if r.rotation % 180 == 90 {
        (lh, lw)
    } else {
        (lw, lh)
    }
}

/// Show `page` in `picture`: from the texture cache if it is there, otherwise queue it on the
/// render thread and leave whatever the picture shows until the new pixels arrive. Returns the
/// page's logical size, which does not depend on the pixels, so layout never waits for them.
pub(super) fn paint_page(
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    picture: &gtk4::Picture,
) -> (u32, u32) {
    let scale = picture.scale_factor().max(1) as u32;
    let mut r = reader.borrow_mut();
    let size = logical_page_size(&r, page);
    let geom = r.geom(page);
    let (overlay, overlay_hash) = overlay_for(&r, page, geom);
    let unrotated_w = (READER_BASE_WIDTH * r.zoom).max(1.0) as u32;
    let key = RenderKey {
        page,
        width: unrotated_w * scale,
        rotation: r.rotation,
        invert: r.invert_colors,
        overlay: overlay_hash,
    };
    if let Some(texture) = r.textures.get(&key) {
        picture.set_paintable(Some(&texture));
        return size;
    }
    let priority = (page as i32 - r.page as i32).unsigned_abs();
    if let Some(worker) = &r.worker {
        worker.submit(Job {
            key,
            overlay,
            priority,
        });
    }
    size
}

/// Turn a finished render into a texture and keep it for reuse.
pub(super) fn accept_render(r: &mut ReaderState, done: Rendered) {
    let bytes = done.rgba.len();
    let stride = done.width as usize * 4;
    let texture = gdk::MemoryTexture::new(
        done.width as i32,
        done.height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(done.rgba),
        stride,
    );
    r.textures.insert(done.key, texture.upcast(), bytes);
}
