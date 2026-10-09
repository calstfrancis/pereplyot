use super::*;
use std::collections::BTreeMap;

/// Builds the widgets for one page of the continuous view: its picture, the marks layer and the
/// drag preview over it, and the gestures that act on that page.
pub(super) type PageFactory = Rc<dyn Fn(u16) -> (gtk4::Overlay, gtk4::Picture)>;

/// The live page widgets of the continuous view, by page, between two blank spacers that stand
/// in for every page above and below them.
pub(super) struct ContinuousPages {
    pub top: gtk4::Box,
    pub bottom: gtk4::Box,
    pub live: BTreeMap<u16, (gtk4::Overlay, gtk4::Picture)>,
    pub factory: PageFactory,
}

fn make_page(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    continuous_scroll: &gtk4::ScrolledWindow,
    reader_window: &adw::Window,
    page: u16,
) -> (gtk4::Overlay, gtk4::Picture) {
    let (count, select_mode) = {
        let r = reader.borrow();
        (r.count, r.draw_kind.is_none())
    };
    {
        let picture = gtk4::Picture::new();
        picture.set_halign(gtk4::Align::Center);
        picture.set_can_target(true);
        picture.set_focusable(true);
        picture.set_cursor(cursor_for_select_mode(select_mode).as_ref());

        // Plausible size from the page's own point dimensions — cheap metadata, not a
        // rasterization — so the layout is correct before this page's texture has rendered.
        let (w, h) = logical_page_size(&reader.borrow(), page);
        picture.set_size_request(w as i32, h as i32);
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

        (page_overlay, picture)
    }
}

/// Where each page starts in the continuous view, in pixels, plus the total height as a final
/// entry. Every page's size comes from the same function the pictures use, so the two agree.
pub(super) fn continuous_offsets_for(r: &ReaderState) -> Vec<f64> {
    let mut offsets = Vec::with_capacity(r.count as usize + 1);
    let mut y = 0.0f64;
    for page in 0..r.count {
        offsets.push(y);
        y += logical_page_size(r, page).1 as f64 + CONTINUOUS_PAGE_GAP;
    }
    offsets.push(y);
    offsets
}

/// Set up continuous-scroll mode if it is not already: the page positions, and two spacers that
/// hold the place of every page that has no widget. Only pages near the viewport get widgets (see
/// `refresh_continuous_window`), so opening a 600-page book builds a handful of them instead of
/// 600. A no-op if already built.
pub(super) fn build_continuous_view(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    continuous_box: &gtk4::Box,
    continuous_scroll: &gtk4::ScrolledWindow,
    reader_window: &adw::Window,
) {
    if !reader.borrow().continuous_offsets.is_empty() {
        return;
    }
    let _span = crate::perf::span(|| "build continuous view".to_string());
    let current_page = reader.borrow().page;
    let factory: PageFactory = {
        let host = host.clone();
        let reader = reader.clone();
        let continuous_scroll = continuous_scroll.clone();
        let reader_window = reader_window.clone();
        Rc::new(move |page| make_page(&host, &reader, &continuous_scroll, &reader_window, page))
    };
    let top = gtk4::Box::new(Orientation::Vertical, 0);
    let bottom = gtk4::Box::new(Orientation::Vertical, 0);
    top.set_visible(false);
    bottom.set_visible(false);
    continuous_box.append(&top);
    continuous_box.append(&bottom);
    {
        let mut r = reader.borrow_mut();
        r.continuous_offsets = continuous_offsets_for(&r);
        r.continuous = Some(ContinuousPages {
            top,
            bottom,
            live: BTreeMap::new(),
            factory,
        });
    }
    refresh_continuous_window(reader, continuous_scroll, Some(current_page));
}

/// Pages kept with widgets on each side of the viewport, in viewports; a page is only dropped
/// once it is `CONTINUOUS_DROP_VIEWPORTS` away, so scrolling back and forth over an edge does
/// not rebuild a page each time.
pub(super) const CONTINUOUS_KEEP_VIEWPORTS: f64 = 1.5;
const CONTINUOUS_DROP_VIEWPORTS: f64 = 2.5;

/// Give widgets to the pages near the viewport and take them from the ones that have drifted
/// away, then resize the spacers to match. `focus_page` overrides the scroll position for the
/// very first build, before GTK has laid anything out.
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
    let (want, drop_bounds, factory, existing) = {
        let r = reader.borrow();
        let Some(pages) = &r.continuous else {
            return;
        };
        if r.continuous_offsets.len() < 2 {
            return;
        }
        let top = match focus_page {
            Some(p) => r.continuous_offsets.get(p as usize).copied().unwrap_or(0.0),
            None => adj.value(),
        };
        let at = |y: f64| continuous_page_at(&r.continuous_offsets, y.max(0.0));
        let keep = viewport * CONTINUOUS_KEEP_VIEWPORTS;
        let drop = viewport * CONTINUOUS_DROP_VIEWPORTS;
        (
            (at(top - keep), at(top + viewport + keep)),
            (at(top - drop), at(top + viewport + drop)),
            pages.factory.clone(),
            pages
                .live
                .keys()
                .next()
                .copied()
                .zip(pages.live.keys().next_back().copied()),
        )
    };
    let (lo, hi) = want;
    let (new_lo, new_hi) = match existing {
        Some((el, eh)) if el <= hi + 1 && eh + 1 >= lo => (
            if el < lo && el >= drop_bounds.0 {
                el
            } else {
                lo
            },
            if eh > hi && eh <= drop_bounds.1 {
                eh
            } else {
                hi
            },
        ),
        _ => (lo, hi),
    };

    let mut created: Vec<u16> = Vec::new();
    let mut removed: Vec<gtk4::Overlay> = Vec::new();
    {
        let mut r = reader.borrow_mut();
        let Some(pages) = r.continuous.as_mut() else {
            return;
        };
        let gone: Vec<u16> = pages
            .live
            .keys()
            .copied()
            .filter(|p| *p < new_lo || *p > new_hi)
            .collect();
        for p in gone {
            if let Some((overlay, _)) = pages.live.remove(&p) {
                removed.push(overlay);
            }
        }
    }
    let box_ = reader
        .borrow()
        .continuous
        .as_ref()
        .and_then(|p| p.top.parent())
        .and_then(|w| w.downcast::<gtk4::Box>().ok());
    let Some(box_) = box_ else {
        return;
    };
    for overlay in removed {
        box_.remove(&overlay);
    }
    // Build from the page nearest the viewport outwards would be nicer, but the order
    // widgets are inserted in is what fixes their order on screen, so go top to bottom.
    for page in new_lo..=new_hi {
        if reader
            .borrow()
            .continuous
            .as_ref()
            .is_some_and(|p| p.live.contains_key(&page))
        {
            continue;
        }
        let (overlay, picture) = factory(page);
        {
            let mut r = reader.borrow_mut();
            let Some(pages) = r.continuous.as_mut() else {
                return;
            };
            let after = pages
                .live
                .range(..page)
                .next_back()
                .map(|(_, (o, _))| o.clone().upcast::<gtk4::Widget>())
                .unwrap_or_else(|| pages.top.clone().upcast());
            box_.insert_child_after(&overlay, Some(&after));
            pages.live.insert(page, (overlay, picture));
        }
        created.push(page);
    }
    {
        let mut r = reader.borrow_mut();
        r.continuous_window = (new_lo, new_hi);
        let offsets = r.continuous_offsets.clone();
        let count = r.count as usize;
        if let Some(pages) = &r.continuous {
            let above = if new_lo > 0 {
                offsets[new_lo as usize] - CONTINUOUS_PAGE_GAP
            } else {
                0.0
            };
            let below = if (new_hi as usize) + 1 < count {
                offsets[count] - CONTINUOUS_PAGE_GAP - offsets[new_hi as usize + 1]
            } else {
                0.0
            };
            for (spacer, height) in [(&pages.top, above), (&pages.bottom, below)] {
                spacer.set_visible(height >= 1.0);
                spacer.set_size_request(-1, height.max(0.0) as i32);
            }
        }
    }
    for page in created {
        render_continuous_page(reader, page);
    }
}

/// Change the zoom of the continuous view without rebuilding it: positions are recomputed, the
/// pages that have widgets are resized, and the scroll position is moved so the same point of the
/// document stays in the middle of the viewport.
pub(super) fn zoom_continuous_in_place(
    reader: &Rc<RefCell<ReaderState>>,
    scroll: &gtk4::ScrolledWindow,
    zoom: f64,
) {
    let adj = scroll.vadjustment();
    let hadj = scroll.hadjustment();
    let h_fraction = (hadj.value() + hadj.page_size() / 2.0) / hadj.upper().max(1.0);
    let anchor = {
        let r = reader.borrow();
        let y = adj.value() + adj.page_size() / 2.0 - CONTINUOUS_PAGE_GAP;
        let page = continuous_page_at(&r.continuous_offsets, y);
        let span = r.continuous_offsets[page as usize + 1]
            - r.continuous_offsets[page as usize]
            - CONTINUOUS_PAGE_GAP;
        let fraction = (y - r.continuous_offsets[page as usize]) / span.max(1.0);
        (page, fraction.clamp(0.0, 1.0))
    };
    {
        let mut r = reader.borrow_mut();
        r.zoom = zoom;
        r.defer_renders = true;
        r.continuous_offsets = continuous_offsets_for(&r);
    }
    rerender_loaded_continuous_pages(reader);
    let (value, upper) = {
        let r = reader.borrow();
        let (page, fraction) = anchor;
        let span = r.continuous_offsets[page as usize + 1]
            - r.continuous_offsets[page as usize]
            - CONTINUOUS_PAGE_GAP;
        let y = CONTINUOUS_PAGE_GAP + r.continuous_offsets[page as usize] + fraction * span
            - adj.page_size() / 2.0;
        let upper = r.continuous_offsets.last().copied().unwrap_or(0.0) + CONTINUOUS_PAGE_GAP;
        (y, upper)
    };
    adj.configure(
        value.clamp(0.0, (upper - adj.page_size()).max(0.0)),
        adj.lower(),
        upper,
        adj.step_increment(),
        adj.page_increment(),
        adj.page_size(),
    );
    let width = (READER_BASE_WIDTH * zoom).max(hadj.page_size());
    hadj.configure(
        (h_fraction * width - hadj.page_size() / 2.0)
            .clamp(0.0, (width - hadj.page_size()).max(0.0)),
        hadj.lower(),
        width,
        hadj.step_increment(),
        hadj.page_increment(),
        hadj.page_size(),
    );
    refresh_continuous_window(reader, scroll, None);
}

/// Throw away every continuous-view widget and position; the next `build_continuous_view`
/// starts from scratch.
pub(super) fn clear_continuous_view(reader: &Rc<RefCell<ReaderState>>, continuous_box: &gtk4::Box) {
    while let Some(child) = continuous_box.first_child() {
        continuous_box.remove(&child);
    }
    let mut r = reader.borrow_mut();
    r.continuous = None;
    r.continuous_offsets.clear();
}

/// Tear down and rebuild continuous-scroll mode's widgets after a layout change — page sizes
/// all changed, so every offset is stale too. A no-op if continuous mode was never built (the
/// next toggle-on will build fresh already). Cheap now that only the pages near the viewport
/// have widgets.
pub(super) fn rebuild_continuous_view_for_zoom(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    continuous_box: &gtk4::Box,
    continuous_scroll: &gtk4::ScrolledWindow,
    reader_window: &adw::Window,
) {
    if reader.borrow().continuous_offsets.is_empty() {
        return;
    }
    clear_continuous_view(reader, continuous_box);
    build_continuous_view(
        host,
        reader,
        continuous_box,
        continuous_scroll,
        reader_window,
    );
}

/// Re-render one page's `Picture` in continuous-scroll mode in place (after an annotation on
/// it changed) — its position doesn't move, only its content. Does nothing for a page that has
/// no widget at the moment.
pub(super) fn render_continuous_page(reader: &Rc<RefCell<ReaderState>>, page: u16) {
    let picture = {
        let r = reader.borrow();
        r.continuous
            .as_ref()
            .and_then(|p| p.live.get(&page))
            .map(|(_, picture)| picture.clone())
    };
    let Some(picture) = picture else {
        return;
    };
    let (w, h) = paint_page(reader, page, &picture);
    picture.set_size_request(w as i32, h as i32);
}

/// Re-render every continuous page that has a widget; the rest pick up the new state whenever
/// they next scroll into the window.
pub(super) fn rerender_loaded_continuous_pages(reader: &Rc<RefCell<ReaderState>>) {
    let loaded: Vec<u16> = {
        let r = reader.borrow();
        r.continuous
            .as_ref()
            .map(|p| p.live.keys().copied().collect())
            .unwrap_or_default()
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
