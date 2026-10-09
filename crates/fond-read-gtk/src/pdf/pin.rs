use super::*;

const MAX_CARDS: usize = 4;
const MIN_REGION_PTS: f32 = 18.0;
const CARD_MIN_WIDTH: f64 = 180.0;
const CARD_MAX_WIDTH: f64 = 480.0;

struct Pending {
    key: RenderKey,
    caption: String,
}

/// Pinning a region of a page as a small floating card that stays put while you scroll on: for the
/// figure or table the next few pages keep referring to.
#[derive(Default)]
pub(super) struct PinState {
    active: bool,
    overlay: Option<gtk4::Overlay>,
    hint: Option<gtk4::Label>,
    picture: Option<gtk4::Picture>,
    saved_hint: String,
    cards: Vec<gtk4::Widget>,
    pending: Vec<Pending>,
}

pub(super) fn install(
    reader: &Rc<RefCell<ReaderState>>,
    overlay: &gtk4::Overlay,
    hint: &gtk4::Label,
    picture: &gtk4::Picture,
) {
    crate::style::ensure();
    let mut r = reader.borrow_mut();
    r.pin.overlay = Some(overlay.clone());
    r.pin.hint = Some(hint.clone());
    r.pin.picture = Some(picture.clone());
}

pub(super) fn is_active(reader: &Rc<RefCell<ReaderState>>) -> bool {
    reader.borrow().pin.active
}

fn set_cursor(reader: &Rc<RefCell<ReaderState>>, name: Option<&str>) {
    let r = reader.borrow();
    let cursor = match name {
        Some(n) => gdk::Cursor::from_name(n, None),
        None => cursor_for_select_mode(r.draw_kind.is_none()),
    };
    if let Some(p) = &r.pin.picture {
        p.set_cursor(cursor.as_ref());
    }
    if let Some(pages) = &r.continuous {
        for (_, picture) in pages.live.values() {
            picture.set_cursor(cursor.as_ref());
        }
    }
}

/// The next drag on a page draws the box to pin.
pub(super) fn begin(reader: &Rc<RefCell<ReaderState>>) {
    {
        let mut r = reader.borrow_mut();
        if r.pin.active {
            return;
        }
        r.pin.active = true;
        if let Some(hint) = r.pin.hint.clone() {
            r.pin.saved_hint = hint.text().to_string();
            hint.set_text("Drag a box around the figure or table to pin it — Esc cancels");
        }
    }
    set_cursor(reader, Some("crosshair"));
}

/// Leave pin mode; true if it was on.
pub(super) fn cancel(reader: &Rc<RefCell<ReaderState>>) -> bool {
    {
        let mut r = reader.borrow_mut();
        if !r.pin.active {
            return false;
        }
        r.pin.active = false;
        if let Some(hint) = &r.pin.hint {
            hint.set_text(&r.pin.saved_hint);
        }
    }
    set_cursor(reader, None);
    true
}

/// The dragged box `(x0, y0, x1, y1)` on a `w`×`h` picture of `page` is what to pin.
pub(super) fn finish(
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    drag: (f64, f64, f64, f64),
    w: f64,
    h: f64,
) {
    cancel(reader);
    let (key, caption) = {
        let r = reader.borrow();
        if r.rotation != 0 || w < 1.0 || h < 1.0 {
            return;
        }
        let Some(geom) = r.geom(page) else {
            return;
        };
        let (dw, dh) = geom.display_size();
        let (sx, sy) = (dw as f64 / w, dh as f64 / h);
        let x0 = (drag.0.min(drag.2) * sx).clamp(0.0, dw as f64) as f32;
        let x1 = (drag.0.max(drag.2) * sx).clamp(0.0, dw as f64) as f32;
        let y0 = (drag.1.min(drag.3) * sy).clamp(0.0, dh as f64) as f32;
        let y1 = (drag.1.max(drag.3) * sy).clamp(0.0, dh as f64) as f32;
        if x1 - x0 < MIN_REGION_PTS || y1 - y0 < MIN_REGION_PTS {
            return;
        }
        let scale = r
            .pin
            .picture
            .as_ref()
            .map_or(1, |p| p.scale_factor().max(1)) as f64;
        let card_w = ((x1 - x0) as f64 * 1.3).clamp(CARD_MIN_WIDTH, CARD_MAX_WIDTH);
        let per_pt = card_w * scale / (x1 - x0) as f64;
        let zoom_w = (dw as f64 * per_pt).round() as u32;
        let label = r
            .page_labels
            .get(page as usize)
            .and_then(|l| l.clone())
            .unwrap_or_else(|| (page + 1).to_string());
        (
            RenderKey {
                page,
                width: zoom_w,
                rotation: 0,
                tone: r.tone,
                thumb: false,
                zoom_w,
                tile: None,
                crop: Some([
                    (x0 as f64 * per_pt).round() as i32,
                    (y0 as f64 * per_pt).round() as i32,
                    ((x1 - x0) as f64 * per_pt).round() as i32,
                    ((y1 - y0) as f64 * per_pt).round() as i32,
                ]),
            },
            format!("p. {label}"),
        )
    };
    let mut r = reader.borrow_mut();
    if let Some(worker) = &r.worker {
        worker.submit(Job {
            key: key.clone(),
            priority: 0,
        });
        r.pin.pending.push(Pending { key, caption });
    }
}

/// A finished crop: if it is one asked for here, turn it into a card.
pub(super) fn deliver(reader: &Rc<RefCell<ReaderState>>, done: &Rendered) -> bool {
    let (caption, overlay) = {
        let mut r = reader.borrow_mut();
        let Some(i) = r.pin.pending.iter().position(|p| p.key == done.key) else {
            return false;
        };
        let pending = r.pin.pending.remove(i);
        let Some(overlay) = r.pin.overlay.clone() else {
            return true;
        };
        (pending.caption, overlay)
    };
    let scale = overlay.scale_factor().max(1);
    let stride = done.width as usize * 4;
    let texture = gdk::MemoryTexture::new(
        done.width as i32,
        done.height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from(&done.rgba[..]),
        stride,
    );
    let (card_w, card_h) = (done.width as i32 / scale, done.height as i32 / scale);
    let picture = gtk4::Picture::for_paintable(&texture);
    picture.set_size_request(card_w, card_h);
    picture.set_can_shrink(false);

    let title = gtk4::Label::new(Some(&caption));
    title.add_css_class("caption");
    title.add_css_class("dim-label");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_margin_start(8);
    let close = gtk4::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("flat");
    close.add_css_class("circular");
    close.set_tooltip_text(Some("Unpin"));
    let grip = gtk4::Box::new(Orientation::Horizontal, 0);
    grip.add_css_class("figure-grip");
    grip.set_cursor_from_name(Some("grab"));
    grip.append(&title);
    grip.append(&close);

    let card = gtk4::Box::new(Orientation::Vertical, 0);
    card.add_css_class("pinned-figure");
    card.set_halign(gtk4::Align::Start);
    card.set_valign(gtk4::Align::Start);
    card.append(&grip);
    card.append(&picture);

    let n = reader.borrow().pin.cards.len() as i32;
    let (ow, oh) = (overlay.width().max(card_w + 60), overlay.height());
    card.set_margin_start((ow - card_w - 24 - 26 * n).max(8));
    card.set_margin_top((16 + 26 * n).min((oh - 80).max(8)));

    let drag = gtk4::GestureDrag::new();
    let origin = Rc::new(Cell::new((0, 0)));
    {
        let card = card.clone();
        let origin = origin.clone();
        drag.connect_drag_begin(move |_, _, _| {
            origin.set((card.margin_start(), card.margin_top()));
        });
    }
    {
        let card = card.clone();
        let overlay = overlay.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            let (sx, sy) = origin.get();
            let max_x = (overlay.width() - card.width()).max(0);
            let max_y = (overlay.height() - card.height()).max(0);
            card.set_margin_start((sx + dx as i32).clamp(0, max_x));
            card.set_margin_top((sy + dy as i32).clamp(0, max_y));
        });
    }
    grip.add_controller(drag);

    {
        let reader = reader.clone();
        let overlay = overlay.clone();
        let card = card.clone();
        close.connect_clicked(move |_| {
            overlay.remove_overlay(&card);
            reader
                .borrow_mut()
                .pin
                .cards
                .retain(|c| c != card.upcast_ref::<gtk4::Widget>());
        });
    }
    overlay.add_overlay(&card);
    let evicted = {
        let mut r = reader.borrow_mut();
        r.pin.cards.push(card.upcast());
        (r.pin.cards.len() > MAX_CARDS).then(|| r.pin.cards.remove(0))
    };
    if let Some(old) = evicted {
        if let Ok(old) = old.downcast::<gtk4::Box>() {
            overlay.remove_overlay(&old);
        }
    }
    true
}
