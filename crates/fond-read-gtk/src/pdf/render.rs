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

/// Recolour a page for reading. Dark inverts lightness and keeps hue, so photographs and
/// coloured figures stay recognisable, and stops short of pure black and white to ease glare;
/// Sepia multiplies the page with a warm paper colour.
pub(super) fn apply_tone(rgba: &mut [u8], tone: Tone) {
    match tone {
        Tone::Normal => {}
        Tone::Dark => {
            const FLOOR: f32 = 24.0;
            const CEIL: f32 = 224.0;
            for px in rgba.chunks_exact_mut(4) {
                let (r, g, b) = (
                    255.0 - px[0] as f32,
                    255.0 - px[1] as f32,
                    255.0 - px[2] as f32,
                );
                let out = [
                    -0.574 * r + 1.430 * g + 0.144 * b,
                    0.426 * r + 0.430 * g + 0.144 * b,
                    0.426 * r + 1.430 * g - 0.856 * b,
                ];
                for (dst, v) in px.iter_mut().zip(out) {
                    let v = v.clamp(0.0, 255.0);
                    *dst = (FLOOR + v * (CEIL - FLOOR) / 255.0).round() as u8;
                }
            }
        }
        Tone::Sepia => {
            const PAPER: [u32; 3] = [244, 232, 208];
            for px in rgba.chunks_exact_mut(4) {
                for (c, paper) in px.iter_mut().zip(PAPER) {
                    *c = (*c as u32 * paper / 255) as u8;
                }
            }
        }
    }
}

/// The marks drawn over a page — saved highlights, the current search match and the live
/// selection — as rectangles in the page's displayed point space.
pub(super) struct Mark {
    /// The saved annotation this is, if it is one (a search match or selection has no id).
    pub id: Option<String>,
    pub kind: fond_doc::MarkupKind,
    pub quads: Vec<[f64; 8]>,
    pub rgba: [u8; 4],
    pub shape: shapes::Shape,
    /// The annotation has a comment, which is shown as a small bubble beside the mark.
    pub has_note: bool,
}

pub(super) fn marks_for(r: &ReaderState, page: u16, geom: PageGeom) -> Vec<Mark> {
    let current_page = page as u32 + 1;
    let mut marks = Vec::new();
    // A Note made from a text selection carries quadpoints and is drawn like a highlight; a
    // freestanding one is an icon at its position; an Area is an outlined rectangle. Each mark
    // keeps its own colour.
    for a in r
        .store
        .sidecar()
        .annotations
        .iter()
        .filter(|a| a.page == Some(current_page))
    {
        let anchor = shapes::anchor_quads(a);
        if anchor.is_empty() {
            continue;
        }
        let kind = match a.kind {
            fond_annot::AnnotationKind::Highlight
            | fond_annot::AnnotationKind::Note
            | fond_annot::AnnotationKind::Area => fond_doc::MarkupKind::Highlight,
            fond_annot::AnnotationKind::Underline => fond_doc::MarkupKind::Underline,
            fond_annot::AnnotationKind::Strikeout => fond_doc::MarkupKind::Strikeout,
            _ => continue,
        };
        let quads = match &r.mark_edit.preview {
            Some((id, quads)) if *id == a.id => quads,
            _ => &anchor,
        };
        marks.push(Mark {
            id: Some(a.id.clone()),
            kind,
            quads: geom.quads_to_display(quads),
            rgba: annotation_rgba(a.color.as_deref()),
            shape: shapes::shape_of(a),
            has_note: a.note.as_deref().is_some_and(|n| !n.trim().is_empty()),
        });
    }
    // The current search match is drawn in its own colour on top of saved marks, and a "Select
    // text" selection stays visible after the drag ends until a new one replaces it.
    if let Some(current) = r.search_matches.get(r.search_current) {
        if current.page == page {
            marks.push(Mark {
                id: None,
                kind: fond_doc::MarkupKind::Highlight,
                quads: geom.quads_to_display(&current.quads),
                rgba: SEARCH_MATCH_RGBA,
                shape: shapes::Shape::Text,
                has_note: false,
            });
        }
    }
    if let Some(caret) = r.caret.as_ref().filter(|c| c.page == page) {
        marks.push(Mark {
            id: None,
            kind: fond_doc::MarkupKind::Highlight,
            quads: geom.quads_to_display(&[caret.quad()]),
            rgba: [20, 20, 20, 235],
            shape: shapes::Shape::Text,
            has_note: false,
        });
    }
    if let Some((sel_page, _, quads)) = &r.last_selection {
        if *sel_page == page {
            marks.push(Mark {
                id: None,
                kind: fond_doc::MarkupKind::Highlight,
                quads: geom.quads_to_display(quads),
                rgba: SELECTION_RGBA,
                shape: shapes::Shape::Text,
                has_note: false,
            });
        }
    }
    marks
}

/// The page's size on screen in logical pixels (unrotated, then swapped for a quarter turn).
pub(super) fn logical_page_size(r: &ReaderState, page: u16) -> (u32, u32) {
    let lw = (READER_BASE_WIDTH * r.zoom).max(1.0);
    let (pw, ph) = r.layout_size(page);
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
    let unrotated_w = (READER_BASE_WIDTH * r.zoom).max(1.0) as u32;
    let device_w = unrotated_w * scale;
    let tiled = tiles::tiling(device_w, r.rotation);
    let key = RenderKey {
        page,
        width: if tiled {
            tiles::BACKDROP_PX.min(device_w)
        } else {
            device_w
        },
        rotation: r.rotation,
        tone: r.tone,
        thumb: false,
        zoom_w: device_w,
        tile: None,
        crop: None,
    };
    mark_layer::redraw_beside(picture);
    if let Some(texture) = r.textures.get(&key) {
        picture.set_paintable(Some(&texture));
    } else if !r.defer_renders {
        let priority = (page as i32 - r.page as i32).unsigned_abs();
        if let Some(worker) = &r.worker {
            worker.submit(Job { key, priority });
        }
    }
    tiles::sync(&mut r, picture, page);
    if tiled {
        let reader = reader.clone();
        let picture = picture.clone();
        glib::idle_add_local_once(move || {
            tiles::sync(&mut reader.borrow_mut(), &picture, page);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_swaps_paper_and_ink_without_pure_black_or_white() {
        let mut px = [255, 255, 255, 255, 0, 0, 0, 255];
        apply_tone(&mut px, Tone::Dark);
        assert_eq!(&px[..4], &[24, 24, 24, 255]);
        assert_eq!(&px[4..], &[224, 224, 224, 255]);
    }

    #[test]
    fn dark_keeps_a_red_figure_red() {
        let mut px = [200, 30, 30, 255];
        apply_tone(&mut px, Tone::Dark);
        assert!(px[0] > px[1] && px[0] > px[2]);
    }

    #[test]
    fn sepia_turns_white_into_paper_and_leaves_black() {
        let mut px = [255, 255, 255, 255, 0, 0, 0, 255];
        apply_tone(&mut px, Tone::Sepia);
        assert_eq!(&px[..4], &[244, 232, 208, 255]);
        assert_eq!(&px[4..], &[0, 0, 0, 255]);
    }
}
