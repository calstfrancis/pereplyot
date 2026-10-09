use super::*;

/// The widget name of a second pane's root, which keyboard focus is checked against.
pub(super) const PANE_NAME: &str = "pereplyot-pane";

/// What is showing in the second pane, and how to take it down.
struct Active {
    view: adw::ToolbarView,
    close: Rc<dyn Fn()>,
    side_by_side: bool,
}

/// Split view: a second pane of the same document beside or below the first, to read chapter 3
/// with the endnotes or a figure open in the other. Each pane has its own page, zoom and search;
/// they share one set of annotations, so a mark made in either shows in both.
pub(super) fn install_split(ui: &PdfUi) {
    if ui.reader.borrow().is_pane {
        return;
    }
    let active: Rc<RefCell<Option<Active>>> = Rc::new(RefCell::new(None));
    let toggle: Rc<dyn Fn(bool)> = {
        let ui = ui.clone();
        let active = active.clone();
        Rc::new(move |side_by_side: bool| {
            let current = active.borrow_mut().take();
            if let Some(current) = current {
                let same = current.side_by_side == side_by_side;
                if same {
                    ui.split_paned.set_end_child(gtk4::Widget::NONE);
                    (current.close)();
                    return;
                }
                // The other orientation: keep the pane and turn the split.
                set_orientation(&ui.split_paned, side_by_side);
                *active.borrow_mut() = Some(Active {
                    side_by_side,
                    ..current
                });
                return;
            }
            // A pane beside the first is about half as wide, so open it at a zoom that fits.
            let zoom = if side_by_side {
                ((ui.split_paned.width() as f64 / 2.0 - 40.0) / READER_BASE_WIDTH).clamp(0.4, 1.0)
            } else {
                ui.reader.borrow().zoom
            };
            let Some((view, close)) = build_pane(&ui, zoom) else {
                return;
            };
            set_orientation(&ui.split_paned, side_by_side);
            ui.split_paned.set_end_child(Some(&view));
            let paned = ui.split_paned.clone();
            glib::idle_add_local_once(move || {
                let size = if paned.orientation() == Orientation::Horizontal {
                    paned.width()
                } else {
                    paned.height()
                };
                paned.set_position(size / 2);
            });
            *active.borrow_mut() = Some(Active {
                view,
                close,
                side_by_side,
            });
        })
    };
    {
        let toggle = toggle.clone();
        ui.split_side_button.connect_clicked(move |_| toggle(true));
    }
    {
        let toggle = toggle.clone();
        ui.split_stack_button
            .connect_clicked(move |_| toggle(false));
    }
    // Closing the document takes the second pane with it.
    let split_paned = ui.split_paned.clone();
    ui.reader.borrow_mut().close_hooks.push(Rc::new(move || {
        let current = active.borrow_mut().take();
        if let Some(current) = current {
            split_paned.set_end_child(gtk4::Widget::NONE);
            let _ = &current.view;
            (current.close)();
        }
    }));
}

fn set_orientation(paned: &gtk4::Paned, side_by_side: bool) {
    paned.set_orientation(if side_by_side {
        Orientation::Horizontal
    } else {
        Orientation::Vertical
    });
    let size = if side_by_side {
        paned.width()
    } else {
        paned.height()
    };
    if size > 0 {
        paned.set_position(size / 2);
    }
}
