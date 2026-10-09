use super::*;

pub(super) const HANDLE_RADIUS: f64 = 5.5;
const HANDLE_HIT_PX: f64 = 11.0;
const RESIZE_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(60);

/// The saved mark under the pointer, the one selected for editing, and the new extent of the one
/// whose handle is being dragged.
#[derive(Default)]
pub(super) struct MarkEdit {
    pub hovered: Option<String>,
    pub selected: Option<String>,
    pub preview: Option<(String, Vec<[f64; 8]>)>,
}

/// Where the selected mark's two handles are on its page, and the points on the opposite ends
/// they pivot around.
pub(super) struct Handles {
    pub id: String,
    pub color: Option<String>,
    pub start: (f64, f64),
    pub end: (f64, f64),
    /// PDF-space point at the start of the mark's text, which stays put while the end moves.
    pub start_anchor: (f64, f64),
    /// PDF-space point at the end of the mark's text, which stays put while the start moves.
    pub end_anchor: (f64, f64),
}

pub(super) fn handles_of(r: &ReaderState, page: u16, w: f64, h: f64) -> Option<Handles> {
    if r.rotation != 0 {
        return None;
    }
    let id = r.mark_edit.selected.as_ref()?;
    let annotation = r.store.get(id)?;
    if annotation.page != Some(page as u32 + 1) {
        return None;
    }
    let quads = match &r.mark_edit.preview {
        Some((preview_id, quads)) if preview_id == id => quads.clone(),
        _ => annotation.quadpoints.clone(),
    };
    let geom = r.geom(page)?;
    let first = geom.quads_to_display(quads.get(..1)?).pop()?;
    let last = geom
        .quads_to_display(quads.get(quads.len().checked_sub(1)?..)?)
        .pop()?;
    let (fx0, fy0, _, fy1) = mark_layer::quad_rect(&first, geom, w, h);
    let (_, ly0, lx1, ly1) = mark_layer::quad_rect(&last, geom, w, h);
    Some(Handles {
        id: id.clone(),
        color: annotation.color.clone(),
        start: (fx0, fy1),
        end: (lx1, ly1),
        start_anchor: geom.px_to_pdf(fx0 + 1.0, (fy0 + fy1) / 2.0, w, h),
        end_anchor: geom.px_to_pdf(lx1 - 1.0, (ly0 + ly1) / 2.0, w, h),
    })
}

fn mark_at(r: &ReaderState, page: u16, x: f64, y: f64, w: f64, h: f64) -> Option<String> {
    if r.rotation != 0 || w < 1.0 || h < 1.0 {
        return None;
    }
    let geom = r.geom(page)?;
    let (px, py) = geom.px_to_pdf(x, y, w, h);
    annotation_at_pdf_point(&r.store.sidecar(), page, px as f32, py as f32)
}

fn set_hovered(reader: &Rc<RefCell<ReaderState>>, id: Option<String>) {
    if reader.borrow().mark_edit.hovered == id {
        return;
    }
    reader.borrow_mut().mark_edit.hovered = id;
    mark_layer::redraw_all(reader);
}

fn set_selected(reader: &Rc<RefCell<ReaderState>>, id: Option<String>) {
    if reader.borrow().mark_edit.selected == id {
        return;
    }
    reader.borrow_mut().mark_edit.selected = id;
    mark_layer::redraw_all(reader);
}

/// Delete removes the selected mark and Escape lets go of it. Returns whether the key was used.
pub(super) fn handle_key(reader: &Rc<RefCell<ReaderState>>, key: gdk::Key) -> bool {
    let Some(id) = reader.borrow().mark_edit.selected.clone() else {
        return false;
    };
    match key {
        gdk::Key::Delete | gdk::Key::KP_Delete => {
            let store = reader.borrow().store.clone();
            let _ = store.remove(&id);
            set_selected(reader, None);
            true
        }
        gdk::Key::Escape => {
            set_selected(reader, None);
            true
        }
        _ => false,
    }
}

struct Resizing {
    id: String,
    anchor: (f64, f64),
    last: Option<std::time::Instant>,
}

fn extent(
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    anchor: (f64, f64),
    to: (f64, f64),
) -> Option<fond_doc::TextSelection> {
    let r = reader.borrow();
    fond_doc::select_text_range(
        r.pdfium,
        r.bytes(),
        page,
        anchor.0 as f32,
        anchor.1 as f32,
        to.0 as f32,
        to.1 as f32,
    )
    .ok()
    .flatten()
}

/// Hover outlines, click to select, and drag a selected mark's handles to change how much text
/// it covers. The gestures run before the page's own, and take a press only when it is a click
/// on a mark or a drag from a handle, so selecting text and drawing new marks work as before.
pub(super) fn install(
    overlay: &gtk4::Overlay,
    reader: &Rc<RefCell<ReaderState>>,
    page_of: Rc<dyn Fn() -> u16>,
) {
    let size_of = {
        let overlay = overlay.clone();
        move || (overlay.width() as f64, overlay.height() as f64)
    };

    let motion = gtk4::EventControllerMotion::new();
    motion.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let reader = reader.clone();
        let page_of = page_of.clone();
        let size_of = size_of.clone();
        motion.connect_motion(move |_, x, y| {
            if reader.borrow().mark_edit.preview.is_some() {
                return;
            }
            let (w, h) = size_of();
            let hit = mark_at(&reader.borrow(), page_of(), x, y, w, h);
            set_hovered(&reader, hit);
        });
    }
    {
        let reader = reader.clone();
        motion.connect_leave(move |_| set_hovered(&reader, None));
    }
    overlay.add_controller(motion);

    let click = gtk4::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);
    click.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let reader = reader.clone();
        let page_of = page_of.clone();
        let size_of = size_of.clone();
        click.connect_released(move |gesture, _, x, y| {
            let (w, h) = size_of();
            let hit = mark_at(&reader.borrow(), page_of(), x, y, w, h);
            match hit {
                Some(id) => {
                    set_selected(&reader, Some(id));
                    gesture.set_state(gtk4::EventSequenceState::Claimed);
                }
                None => set_selected(&reader, None),
            }
        });
    }
    overlay.add_controller(click);

    let drag = gtk4::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    drag.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let resizing: Rc<RefCell<Option<Resizing>>> = Rc::new(RefCell::new(None));
    {
        let reader = reader.clone();
        let page_of = page_of.clone();
        let size_of = size_of.clone();
        let resizing = resizing.clone();
        drag.connect_drag_begin(move |gesture, x, y| {
            let (w, h) = size_of();
            let grabbed = handles_of(&reader.borrow(), page_of(), w, h).and_then(|hs| {
                let near = |p: (f64, f64)| (p.0 - x).hypot(p.1 - y) <= HANDLE_HIT_PX;
                if near(hs.end) {
                    Some((hs.id, hs.start_anchor))
                } else if near(hs.start) {
                    Some((hs.id, hs.end_anchor))
                } else {
                    None
                }
            });
            match grabbed {
                Some((id, anchor)) => {
                    gesture.set_state(gtk4::EventSequenceState::Claimed);
                    *resizing.borrow_mut() = Some(Resizing {
                        id,
                        anchor,
                        last: None,
                    });
                }
                None => {
                    gesture.set_state(gtk4::EventSequenceState::Denied);
                }
            }
        });
    }
    {
        let reader = reader.clone();
        let page_of = page_of.clone();
        let size_of = size_of.clone();
        let resizing = resizing.clone();
        drag.connect_drag_update(move |gesture, dx, dy| {
            let Some((sx, sy)) = gesture.start_point() else {
                return;
            };
            let mut slot = resizing.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            if state
                .last
                .is_some_and(|at| at.elapsed() < RESIZE_MIN_INTERVAL)
            {
                return;
            }
            state.last = Some(std::time::Instant::now());
            let page = page_of();
            let (w, h) = size_of();
            let Some(geom) = reader.borrow().geom(page) else {
                return;
            };
            let to = geom.px_to_pdf(sx + dx, sy + dy, w, h);
            if let Some(sel) = extent(&reader, page, state.anchor, to) {
                reader.borrow_mut().mark_edit.preview = Some((state.id.clone(), sel.quads));
                mark_layer::redraw_all(&reader);
            }
        });
    }
    {
        let reader = reader.clone();
        let page_of = page_of.clone();
        let size_of = size_of.clone();
        drag.connect_drag_end(move |gesture, dx, dy| {
            let Some(state) = resizing.borrow_mut().take() else {
                return;
            };
            let page = page_of();
            let (w, h) = size_of();
            let target = gesture.start_point().and_then(|(sx, sy)| {
                let geom = reader.borrow().geom(page)?;
                Some(geom.px_to_pdf(sx + dx, sy + dy, w, h))
            });
            reader.borrow_mut().mark_edit.preview = None;
            if let Some(to) = target {
                if let Some(sel) = extent(&reader, page, state.anchor, to) {
                    let text = selection_text(&reader.borrow(), page, &sel.quads)
                        .unwrap_or_else(|| sel.text.clone());
                    let store = reader.borrow().store.clone();
                    let _ = store.update(&state.id, |a| {
                        a.quadpoints = sel.quads.clone();
                        a.snippet = Some(text);
                    });
                }
            }
            mark_layer::redraw_all(&reader);
        });
    }
    overlay.add_controller(drag);
}
