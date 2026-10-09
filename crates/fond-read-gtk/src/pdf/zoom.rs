use super::*;

pub(super) fn install_zoom(ui: &PdfUi) {
    let PdfUi {
        reader,
        picture,
        render,
        zoom_out,
        zoom_in,
        zoom_fit_width,
        zoom_fit_page,
        continuous_toggle,
        scroll,
        continuous_box,
        continuous_scroll,
        ..
    } = ui.clone();
    // Every zoom-changing control (in/out, fit-width, fit-page, wheel, pinch) funnels through
    // one `request_zoom`. The view follows at once, at most once a frame: pages are resized in
    // place and keep showing their last picture, stretched, while the render thread draws them
    // at the new size. Page renders are held back until the zoom has been still for 150 ms, so a
    // pinch does not queue a render for every size it passes through.
    tiles::watch_paged_scroll(&reader, &scroll, &picture);
    let pending_zoom: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let last_applied: Rc<Cell<Option<std::time::Instant>>> = Rc::new(Cell::new(None));
    let apply_scheduled = Rc::new(Cell::new(false));
    let settle: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let zoom_baseline = {
        let reader = reader.clone();
        let pending_zoom = pending_zoom.clone();
        move || pending_zoom.get().unwrap_or_else(|| reader.borrow().zoom)
    };
    let apply: Rc<dyn Fn()> = {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let scroll = scroll.clone();
        let pending_zoom = pending_zoom.clone();
        let last_applied = last_applied.clone();
        Rc::new(move || {
            let Some(target) = pending_zoom.get() else {
                return;
            };
            last_applied.set(Some(std::time::Instant::now()));
            let old = reader.borrow().zoom;
            if (target - old).abs() < 1e-9 {
                return;
            }
            if continuous_toggle.is_active() && !reader.borrow().continuous_offsets.is_empty() {
                zoom_continuous_in_place(&reader, &continuous_scroll, target);
            } else {
                let h = scroll.hadjustment();
                let v = scroll.vadjustment();
                let centre = |adj: &gtk4::Adjustment| {
                    (adj.value() + adj.page_size() / 2.0) / adj.upper().max(1.0)
                };
                let (fx, fy) = (centre(&h), centre(&v));
                {
                    let mut r = reader.borrow_mut();
                    r.zoom = target;
                    r.defer_renders = true;
                }
                render();
                let ratio = target / old.max(0.01);
                for (adj, fraction) in [(&h, fx), (&v, fy)] {
                    let upper = adj.upper() * ratio;
                    let value = (fraction * upper - adj.page_size() / 2.0)
                        .clamp(0.0, (upper - adj.page_size()).max(0.0));
                    adj.configure(
                        value,
                        adj.lower(),
                        upper,
                        adj.step_increment(),
                        adj.page_increment(),
                        adj.page_size(),
                    );
                }
            }
        })
    };
    let request_zoom: Rc<dyn Fn(f64)> = {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_box = continuous_box.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let pending_zoom = pending_zoom.clone();
        let last_applied = last_applied.clone();
        let apply_scheduled = apply_scheduled.clone();
        let settle = settle.clone();
        let apply = apply.clone();
        Rc::new(move |target: f64| {
            let text_zoom = reader.borrow().text_zoom.clone();
            if let Some(text_zoom) = text_zoom {
                let base = reader.borrow().zoom.max(0.01);
                text_zoom(target / base);
                return;
            }
            pending_zoom.set(Some(target.clamp(0.35, 4.0)));
            let frame = std::time::Duration::from_millis(16);
            match last_applied.get() {
                Some(at) if at.elapsed() < frame => {
                    if !apply_scheduled.replace(true) {
                        let apply = apply.clone();
                        let apply_scheduled = apply_scheduled.clone();
                        glib::timeout_add_local_once(frame, move || {
                            apply_scheduled.set(false);
                            apply();
                        });
                    }
                }
                _ => apply(),
            }
            if let Some(id) = settle.borrow_mut().take() {
                id.remove();
            }
            let reader = reader.clone();
            let render = render.clone();
            let continuous_box = continuous_box.clone();
            let continuous_toggle = continuous_toggle.clone();
            let continuous_scroll = continuous_scroll.clone();
            let pending_zoom = pending_zoom.clone();
            let slot = settle.clone();
            let apply = apply.clone();
            let id =
                glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                    slot.borrow_mut().take();
                    apply();
                    pending_zoom.set(None);
                    reader.borrow_mut().defer_renders = false;
                    render();
                    if continuous_toggle.is_active() {
                        rerender_loaded_continuous_pages(&reader);
                    } else if !reader.borrow().continuous_offsets.is_empty() {
                        clear_continuous_view(&reader, &continuous_box);
                    }
                    let _ = &continuous_scroll;
                });
            *settle.borrow_mut() = Some(id);
        })
    };
    for target in [scroll.clone(), continuous_scroll.clone()] {
        let wheel = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        wheel.set_propagation_phase(gtk4::PropagationPhase::Capture);
        {
            let request_zoom = request_zoom.clone();
            let zoom_baseline = zoom_baseline.clone();
            wheel.connect_scroll(move |ctl, _dx, dy| {
                if !ctl
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                {
                    return glib::Propagation::Proceed;
                }
                request_zoom(zoom_baseline() * (1.0 - dy * 0.1));
                glib::Propagation::Stop
            });
        }
        target.add_controller(wheel);

        let pinch = gtk4::GestureZoom::new();
        pinch.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let start_zoom = Rc::new(Cell::new(1.0f64));
        {
            let start_zoom = start_zoom.clone();
            let zoom_baseline = zoom_baseline.clone();
            pinch.connect_begin(move |_, _| start_zoom.set(zoom_baseline()));
        }
        {
            let request_zoom = request_zoom.clone();
            pinch.connect_scale_changed(move |_, scale| request_zoom(start_zoom.get() * scale));
        }
        target.add_controller(pinch);
    }
    {
        let request_zoom = request_zoom.clone();
        let zoom_baseline = zoom_baseline.clone();
        zoom_in.connect_clicked(move |_| request_zoom(zoom_baseline() * 1.25));
    }
    {
        let request_zoom = request_zoom.clone();
        let zoom_baseline = zoom_baseline.clone();
        zoom_out.connect_clicked(move |_| request_zoom(zoom_baseline() / 1.25));
    }
    {
        let request_zoom = request_zoom.clone();
        let scroll = scroll.clone();
        zoom_fit_width.connect_clicked(move |_| {
            let viewport_width = scroll.width().max(1) as f64;
            request_zoom(viewport_width / READER_BASE_WIDTH);
        });
    }
    {
        let request_zoom = request_zoom.clone();
        let reader = reader.clone();
        let scroll = scroll.clone();
        zoom_fit_page.connect_clicked(move |_| {
            let viewport_width = scroll.width().max(1) as f64;
            let viewport_height = scroll.height().max(1) as f64;
            let page_pts = {
                let r = reader.borrow();
                r.display_size(r.page)
            };
            let fit_width_zoom = viewport_width / READER_BASE_WIDTH;
            let aspect = if page_pts.0 > 0.0 {
                page_pts.1 as f64 / page_pts.0 as f64
            } else {
                792.0 / 612.0
            };
            let fit_height_zoom = viewport_height / (READER_BASE_WIDTH * aspect);
            request_zoom(fit_width_zoom.min(fit_height_zoom));
        });
    }
}
