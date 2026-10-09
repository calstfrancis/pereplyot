use super::*;

/// How far down its page the top of the view is, 0 to 1.
pub(super) fn current_fraction(ui: &PdfUi) -> f32 {
    let r = ui.reader.borrow();
    if ui.text_toggle.is_active() {
        return 0.0;
    }
    if ui.continuous_toggle.is_active() && r.continuous_offsets.len() > 1 {
        let page = r.page as usize;
        let (Some(&top), Some(&next)) = (
            r.continuous_offsets.get(page),
            r.continuous_offsets.get(page + 1),
        ) else {
            return 0.0;
        };
        let span = (next - top - CONTINUOUS_PAGE_GAP).max(1.0);
        let y = ui.continuous_scroll.vadjustment().value() - CONTINUOUS_PAGE_GAP - top;
        return (y / span).clamp(0.0, 1.0) as f32;
    }
    let adj = ui.scroll.vadjustment();
    let range = (adj.upper() - adj.page_size()).max(1.0);
    (adj.value() / range).clamp(0.0, 1.0) as f32
}

/// Put the view back where the document was left, and say so: a hairline across the top of the
/// view with "Continue here", which fades after a few seconds or as soon as you scroll.
pub(super) fn restore(ui: &PdfUi, start_page: u16) {
    let Some((page, fraction)) = ui.host.position() else {
        return;
    };
    if page != start_page as u32 + 1 || fraction < 0.04 {
        return;
    }
    let ui = ui.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(450), move || {
        if ui.reader.borrow().page != start_page || ui.text_toggle.is_active() {
            return;
        }
        let moved = if ui.continuous_toggle.is_active() {
            // Read the target, let go of the reader, then scroll: moving the view makes the
            // scroll handlers borrow it.
            let target = {
                let r = ui.reader.borrow();
                match (
                    r.continuous_offsets.get(start_page as usize),
                    r.continuous_offsets.get(start_page as usize + 1),
                ) {
                    (Some(&top), Some(&next)) => {
                        let span = (next - top - CONTINUOUS_PAGE_GAP).max(1.0);
                        Some(top + CONTINUOUS_PAGE_GAP + fraction as f64 * span)
                    }
                    _ => None,
                }
            };
            match target {
                Some(value) => {
                    ui.continuous_scroll.vadjustment().set_value(value);
                    true
                }
                None => false,
            }
        } else {
            let adj = ui.scroll.vadjustment();
            adj.set_value(fraction as f64 * (adj.upper() - adj.page_size()).max(0.0));
            true
        };
        if moved {
            show_marker(&ui);
        }
    });
}

fn show_marker(ui: &PdfUi) {
    let Some(overlay) = ui.reader.borrow().pin.overlay.clone() else {
        return;
    };
    crate::style::ensure();
    let line = gtk4::Box::new(Orientation::Horizontal, 0);
    line.add_css_class("reading-position");
    line.set_height_request(2);
    line.set_valign(gtk4::Align::Start);
    line.set_margin_top(14);
    line.set_can_target(false);
    let pill = gtk4::Label::new(Some("Continue here"));
    pill.add_css_class("caption");
    pill.add_css_class("reading-position-label");
    pill.set_halign(gtk4::Align::Start);
    pill.set_valign(gtk4::Align::Start);
    pill.set_margin_top(16);
    pill.set_margin_start(12);
    pill.set_can_target(false);
    overlay.add_overlay(&line);
    overlay.add_overlay(&pill);
    let gone = Rc::new(Cell::new(false));
    let fade = {
        let line = line.clone();
        let pill = pill.clone();
        let overlay = overlay.clone();
        let gone = gone.clone();
        Rc::new(move || {
            if gone.replace(true) {
                return;
            }
            let (line, pill) = (line.clone(), pill.clone());
            let target = libadwaita::CallbackAnimationTarget::new({
                let (line, pill) = (line.clone(), pill.clone());
                move |v| {
                    line.set_opacity(v);
                    pill.set_opacity(v);
                }
            });
            let animation = libadwaita::TimedAnimation::new(&line, 1.0, 0.0, 700, target);
            let overlay = overlay.clone();
            animation.connect_done(move |_| {
                overlay.remove_overlay(&line);
                overlay.remove_overlay(&pill);
            });
            animation.play();
        })
    };
    {
        let fade = fade.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || fade());
    }
    // The first scrolling after the marker appears (the restore itself finishes within moments)
    // sends it away.
    let armed_at = std::time::Instant::now();
    for adj in [ui.continuous_scroll.vadjustment(), ui.scroll.vadjustment()] {
        let fade = fade.clone();
        adj.connect_value_changed(move |_| {
            if armed_at.elapsed() > std::time::Duration::from_millis(700) {
                fade();
            }
        });
    }
}
