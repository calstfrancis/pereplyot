use super::*;

pub(super) fn install_zoom(ui: &PdfUi) {
    let PdfUi {
        host,
        reader,
        reader_window,
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
    // Every zoom-changing control (in/out, fit-width, fit-page) funnels through one
    // debounced `request_zoom`, factored out of what used to be two near-identical
    // zoom_in/zoom_out handlers. Two things this buys beyond de-duplication:
    //
    // - Debounce: rapid clicking coalesces into one render+continuous-rebuild ~150ms after
    //   the last click, instead of one full cycle per click.
    // - Deferred continuous rebuild: `rebuild_continuous_view_for_zoom` re-renders every
    //   page in the document (spread across idle ticks — see `build_continuous_view`'s own
    //   doc comment). Paying that cost on every zoom change even while continuous mode
    //   isn't the visible view was pure wasted background work; now it only rebuilds
    //   immediately when continuous mode is actually on-screen, and otherwise just clears
    //   the stale state so the *next* toggle-to-continuous rebuilds fresh at the new zoom.
    let pending_zoom: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let zoom_debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let zoom_baseline = {
        let reader = reader.clone();
        let pending_zoom = pending_zoom.clone();
        move || pending_zoom.get().unwrap_or_else(|| reader.borrow().zoom)
    };
    let request_zoom: Rc<dyn Fn(f64)> = {
        let host = host.clone();
        let reader = reader.clone();
        let render = render.clone();
        let continuous_box = continuous_box.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        let dialog = reader_window.clone();
        let pending_zoom = pending_zoom.clone();
        let zoom_debounce = zoom_debounce.clone();
        Rc::new(move |target: f64| {
            let text_zoom = reader.borrow().text_zoom.clone();
            if let Some(text_zoom) = text_zoom {
                let base = reader.borrow().zoom.max(0.01);
                text_zoom(target / base);
                return;
            }
            pending_zoom.set(Some(target.clamp(0.35, 4.0)));
            if let Some(id) = zoom_debounce.borrow_mut().take() {
                id.remove();
            }
            let host = host.clone();
            let reader = reader.clone();
            let render = render.clone();
            let continuous_box = continuous_box.clone();
            let continuous_toggle = continuous_toggle.clone();
            let continuous_scroll = continuous_scroll.clone();
            let dialog = dialog.clone();
            let pending_zoom = pending_zoom.clone();
            let zoom_debounce_slot = zoom_debounce.clone();
            let id = glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
                zoom_debounce_slot.borrow_mut().take();
                let Some(new_zoom) = pending_zoom.take() else {
                    return glib::ControlFlow::Break;
                };
                reader.borrow_mut().zoom = new_zoom;
                render();
                if continuous_toggle.is_active() {
                    rebuild_continuous_view_for_zoom(
                        &host,
                        &reader,
                        &continuous_box,
                        &continuous_scroll,
                        &dialog,
                    );
                    let page = reader.borrow().page;
                    scroll_continuous_to_page(&reader, &continuous_scroll, page);
                } else if !reader.borrow().continuous_offsets.is_empty() {
                    clear_continuous_view(&reader, &continuous_box);
                }
                glib::ControlFlow::Break
            });
            *zoom_debounce.borrow_mut() = Some(id);
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
