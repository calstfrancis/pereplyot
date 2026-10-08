use super::*;

/// Build continuous-scroll mode's per-page `Picture` widgets, if not already built. A no-op
/// if `reader.continuous_pictures` is already populated (from an earlier toggle-on this
/// session).
///
/// Widget layout (sizes and offsets) is computed eagerly from each page's cheap PDF-point
/// metadata alone — every page gets one *permanent* widget up front, so its drag-to-annotate
/// gesture can capture that page's index directly with no risk of a recycled widget later
/// belonging to a different page (the failure mode a `ListView`-based virtualized version
/// would have to guard against), and so scrolling to any page works immediately. Actually
/// rasterizing each page's texture is the expensive part (PDFium render), so that's deferred
/// to `schedule_continuous_render`, spread one page per idle tick starting from the reader's
/// current page — this used to run inline here, which blocked the whole UI thread for the
/// entire document on every open once continuous mode became the default (previously it only
/// cost anything on an explicit toggle-on, rare enough not to notice).
pub(super) fn build_continuous_view(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    continuous_box: &gtk4::Box,
    continuous_scroll: &gtk4::ScrolledWindow,
    reader_window: &adw::Window,
) {
    if !reader.borrow().continuous_pictures.is_empty() {
        return;
    }
    let (count, zoom, current_page) = {
        let r = reader.borrow();
        (r.count, r.zoom, r.page)
    };

    let mut pictures = Vec::with_capacity(count as usize);
    let mut offsets = Vec::with_capacity(count as usize + 1);
    let mut y = 0.0f64;

    let select_mode = reader.borrow().draw_kind.is_none();
    for page in 0..count {
        let picture = gtk4::Picture::new();
        picture.set_halign(gtk4::Align::Center);
        picture.set_can_target(true);
        picture.set_focusable(true);
        picture.set_cursor(cursor_for_select_mode(select_mode).as_ref());

        // Plausible size from the page's own point dimensions — cheap metadata, not a
        // rasterization — so the layout is correct before this page's texture has rendered.
        let pts = reader.borrow().display_size(page);
        let w = (READER_BASE_WIDTH * zoom) as u32;
        let h = if pts.0 > 0.0 {
            (w as f32 * pts.1 / pts.0) as u32
        } else {
            (w as f32 * 792.0 / 612.0) as u32
        };
        picture.set_size_request(w as i32, h as i32);
        offsets.push(y);
        y += h as f64 + CONTINUOUS_PAGE_GAP;

        let (page_overlay, drag_preview, drag_live_rect) =
            build_drag_preview_overlay(&picture, reader, move || page);
        page_overlay.set_halign(gtk4::Align::Center);

        // Drag-to-annotate on this page's own permanent Picture — `page` is captured by
        // value, so (unlike a recycled `ListView` row) it can never go stale.
        {
            let drag = gtk4::GestureDrag::new();
            let host = host.clone();
            let reader = reader.clone();
            let reader_window = reader_window.clone();
            let this_picture = picture.clone();
            {
                let live_rect = drag_live_rect.clone();
                let drag_preview = drag_preview.clone();
                drag.connect_drag_begin(move |_gesture, start_x, start_y| {
                    live_rect.set(Some((start_x, start_y, start_x, start_y)));
                    drag_preview.queue_draw();
                });
            }
            {
                let live_rect = drag_live_rect.clone();
                let drag_preview = drag_preview.clone();
                drag.connect_drag_update(move |gesture, offset_x, offset_y| {
                    let Some((start_x, start_y)) = gesture.start_point() else {
                        return;
                    };
                    live_rect.set(Some((
                        start_x,
                        start_y,
                        start_x + offset_x,
                        start_y + offset_y,
                    )));
                    drag_preview.queue_draw();
                });
            }
            {
                let live_rect = drag_live_rect.clone();
                let drag_preview = drag_preview.clone();
                drag.connect_drag_end(move |gesture, offset_x, offset_y| {
                    live_rect.set(None);
                    drag_preview.queue_draw();
                    if offset_x.abs() < MIN_DRAG_PX && offset_y.abs() < MIN_DRAG_PX {
                        return;
                    }
                    let Some((start_x, start_y)) = gesture.start_point() else {
                        return;
                    };
                    let end_x = start_x + offset_x;
                    let end_y = start_y + offset_y;
                    let render_w = this_picture.width().max(0) as u32;
                    let render_h = this_picture.height().max(0) as u32;
                    let geom = DragGeometry {
                        render_w,
                        render_h,
                        page: reader.borrow().geom(page),
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                    };
                    if reader.borrow().draw_kind.is_none() {
                        if select_drag_text(&host, &reader, page, &geom) {
                            render_continuous_page(&reader, page);
                            let ctx = MarkCtx {
                                host: host.clone(),
                                reader: reader.clone(),
                                reader_window: reader_window.clone(),
                            };
                            show_selection_popover(&ctx, &this_picture, end_x, end_y, page);
                        }
                        return;
                    }
                    save_drag_annotation(&host, &reader, page, geom);
                });
            }
            picture.add_controller(drag);
        }

        // Right-click: edit/delete the annotation under the cursor, or add a note — same
        // context menu as the paged view, hit-testing against this page's own picture size
        // (each page can differ slightly after per-page render fallbacks).
        {
            let click = gtk4::GestureClick::new();
            click.set_button(gdk::BUTTON_SECONDARY);
            let host = host.clone();
            let reader = reader.clone();
            let this_picture = picture.clone();
            let reader_window = reader_window.clone();
            click.connect_pressed(move |_gesture, _n, x, y| {
                let render_w = this_picture.width().max(0) as u32;
                let render_h = this_picture.height().max(0) as u32;
                let page_geom = reader.borrow().geom(page);
                show_pdf_context_menu(
                    &host,
                    &reader,
                    &this_picture,
                    page,
                    ClickGeometry {
                        render_w,
                        render_h,
                        page: page_geom,
                        click_x: x,
                        click_y: y,
                    },
                    &reader_window,
                );
            });
            picture.add_controller(click);
        }

        // Click-to-turn zones — same 20/60/20 split and same-widget coexistence-with-drag
        // reasoning as the paged view's own (see that block's comment); "previous"/"next"
        // here means scrolling to the adjacent page, since there are no prev/next buttons
        // to reuse inside this per-page loop.
        {
            let press_pos: Rc<Cell<Option<(f64, f64)>>> = Rc::new(Cell::new(None));
            let click_nav = gtk4::GestureClick::new();
            click_nav.set_button(gdk::BUTTON_PRIMARY);
            {
                let press_pos = press_pos.clone();
                click_nav.connect_pressed(move |_, _, x, y| press_pos.set(Some((x, y))));
            }
            {
                let this_picture = picture.clone();
                let reader = reader.clone();
                let continuous_scroll = continuous_scroll.clone();
                click_nav.connect_released(move |_, _, x, y| {
                    let Some((sx, sy)) = press_pos.take() else {
                        return;
                    };
                    if (x - sx).abs() > MIN_DRAG_PX || (y - sy).abs() > MIN_DRAG_PX {
                        return;
                    }
                    let w = this_picture.width().max(1) as f64;
                    let h = this_picture.height().max(1) as f64;
                    if follow_link(&reader, page, x, y, w, h) {
                        return;
                    }
                    if x < w * 0.2 {
                        let target = page.saturating_sub(1);
                        scroll_continuous_to_page(&reader, &continuous_scroll, target);
                    } else if x > w * 0.8 {
                        let target = (page + 1).min(count.saturating_sub(1));
                        scroll_continuous_to_page(&reader, &continuous_scroll, target);
                    }
                });
            }
            picture.add_controller(click_nav);
        }

        continuous_box.append(&page_overlay);
        pictures.push(picture);
    }
    offsets.push(y); // sentinel: total content height

    {
        let mut r = reader.borrow_mut();
        r.continuous_pictures = pictures;
        r.continuous_offsets = offsets;
        r.continuous_rendered = vec![false; count as usize];
    }

    refresh_continuous_window(reader, continuous_scroll, Some(current_page));
}

/// Pages kept rendered on each side of the viewport, in viewports.
pub(super) const CONTINUOUS_KEEP_VIEWPORTS: f64 = 1.5;

/// Decide which pages should be rendered (viewport plus a margin), unload the rest, and
/// render the missing ones one per idle tick, nearest first. `focus_page` overrides the
/// scroll position for the very first build, before GTK has laid anything out.
pub(super) fn refresh_continuous_window(
    reader: &Rc<RefCell<ReaderState>>,
    scroll: &gtk4::ScrolledWindow,
    focus_page: Option<u16>,
) {
    let adj = scroll.vadjustment();
    let viewport = if adj.page_size() > 1.0 {
        adj.page_size()
    } else {
        1000.0
    };
    let (lo, hi, center) = {
        let r = reader.borrow();
        if r.continuous_offsets.len() < 2 {
            return;
        }
        let (top, center) = match focus_page {
            Some(p) => {
                let t = r.continuous_offsets.get(p as usize).copied().unwrap_or(0.0);
                (t, t + viewport / 2.0)
            }
            None => (adj.value(), adj.value() + viewport / 2.0),
        };
        let keep = viewport * CONTINUOUS_KEEP_VIEWPORTS;
        let lo = continuous_page_at(&r.continuous_offsets, (top - keep).max(0.0));
        let hi = continuous_page_at(&r.continuous_offsets, top + viewport + keep);
        (lo, hi, center)
    };
    let mut missing: Vec<u16> = Vec::new();
    {
        let mut r = reader.borrow_mut();
        r.continuous_window = (lo, hi);
        for page in 0..r.continuous_pictures.len() as u16 {
            let in_window = page >= lo && page <= hi;
            let rendered = r
                .continuous_rendered
                .get(page as usize)
                .copied()
                .unwrap_or(false);
            if in_window && !rendered {
                missing.push(page);
            } else if !in_window && rendered {
                r.continuous_pictures[page as usize].set_paintable(gdk::Paintable::NONE);
                r.continuous_rendered[page as usize] = false;
            }
        }
        let offsets = r.continuous_offsets.clone();
        missing.sort_by(|a, b| {
            let da = (offsets[*a as usize] - center).abs();
            let db = (offsets[*b as usize] - center).abs();
            da.total_cmp(&db)
        });
    }
    if !missing.is_empty() {
        schedule_continuous_render(reader.clone(), missing, 0);
    }
}

/// Rasterize one page of `order`, then yield to the main loop before the next, so filling
/// the window never blocks the UI for more than one page's render. Pages that have since
/// left the window (the user scrolled on) or were already rendered are skipped.
pub(super) fn schedule_continuous_render(
    reader: Rc<RefCell<ReaderState>>,
    order: Vec<u16>,
    idx: usize,
) {
    let Some(&page) = order.get(idx) else {
        return;
    };
    let wanted = {
        let r = reader.borrow();
        let (lo, hi) = r.continuous_window;
        r.continuous_pictures.get(page as usize).is_some()
            && page >= lo
            && page <= hi
            && !r
                .continuous_rendered
                .get(page as usize)
                .copied()
                .unwrap_or(true)
    };
    if wanted {
        render_continuous_page(&reader, page);
    }
    glib::idle_add_local_once(move || {
        schedule_continuous_render(reader, order, idx + 1);
    });
}

/// Tear down and rebuild continuous-scroll mode's widgets after a zoom change — page pixel
/// sizes all changed, so every offset is stale too. A no-op if continuous mode was never
/// built (the next toggle-on will build fresh at the new zoom already). Simpler than
/// resizing everything in place: zoom changes are infrequent, so paying a full rebuild is a
/// reasonable trade for not having two code paths (initial build vs. resize-in-place) to
/// keep in sync.
pub(super) fn rebuild_continuous_view_for_zoom(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    continuous_box: &gtk4::Box,
    continuous_scroll: &gtk4::ScrolledWindow,
    reader_window: &adw::Window,
) {
    if reader.borrow().continuous_pictures.is_empty() {
        return;
    }
    while let Some(child) = continuous_box.first_child() {
        continuous_box.remove(&child);
    }
    {
        let mut r = reader.borrow_mut();
        r.continuous_pictures.clear();
        r.continuous_offsets.clear();
        r.continuous_rendered.clear();
    }
    build_continuous_view(
        host,
        reader,
        continuous_box,
        continuous_scroll,
        reader_window,
    );
}

/// Re-render one page's `Picture` in continuous-scroll mode in place (after an annotation on
/// it changed) — its position doesn't move, only its content, so this doesn't touch
/// `continuous_offsets`.
pub(super) fn render_continuous_page(reader: &Rc<RefCell<ReaderState>>, page: u16) {
    let picture = {
        let r = reader.borrow();
        r.continuous_pictures.get(page as usize).cloned()
    };
    let Some(picture) = picture else {
        return;
    };
    let mut r = reader.borrow_mut();
    if let Some((texture, w, h)) = render_pdf_page_texture(&r, page) {
        picture.set_paintable(Some(&texture));
        picture.set_size_request(w as i32, h as i32);
        if let Some(flag) = r.continuous_rendered.get_mut(page as usize) {
            *flag = true;
        }
    }
}

/// Re-render only the continuous pages that currently hold a texture; the rest pick up the
/// new state whenever they next scroll into the window.
pub(super) fn rerender_loaded_continuous_pages(reader: &Rc<RefCell<ReaderState>>) {
    let loaded: Vec<u16> = {
        let r = reader.borrow();
        r.continuous_rendered
            .iter()
            .enumerate()
            .filter(|(_, rendered)| **rendered)
            .map(|(i, _)| i as u16)
            .collect()
    };
    for page in loaded {
        render_continuous_page(reader, page);
    }
}

/// Scroll continuous-scroll mode's `ScrolledWindow` so `page` is at the top of the viewport.
pub(super) fn scroll_continuous_to_page(
    reader: &Rc<RefCell<ReaderState>>,
    scroll: &gtk4::ScrolledWindow,
    page: u16,
) {
    let text_goto = reader.borrow().text_goto.clone();
    if let Some(goto) = text_goto {
        goto(page);
        return;
    }
    let offset = {
        let r = reader.borrow();
        r.continuous_offsets
            .get(page as usize)
            .copied()
            .unwrap_or(0.0)
    };
    scroll.vadjustment().set_value(offset);
}

/// Which page's span contains vertical position `y` (both in continuous-scroll pixel space)
/// — the last page whose own top offset is at or above `y`. `offsets` is
/// `ReaderState.continuous_offsets`: `count` real page-top offsets plus one trailing
/// sentinel (the total content height), ascending.
pub(super) fn continuous_page_at(offsets: &[f64], y: f64) -> u16 {
    if offsets.len() < 2 {
        return 0;
    }
    let count = offsets.len() - 1;
    let i = offsets[..count].partition_point(|&o| o <= y);
    i.saturating_sub(1).min(count.saturating_sub(1)) as u16
}
