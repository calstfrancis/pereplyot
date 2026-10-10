pub mod highlight_labels;
pub mod markdown_notes;
pub mod menu;
pub mod notebooks_page;
pub mod notes_page;
pub mod ocr_languages;
pub mod resurface;
pub mod search_page;
pub mod styles;
pub mod welcome;
pub mod window;

use std::cell::RefCell;
use std::rc::Rc;

use libadwaita as adw;

use crate::config::Config;
use crate::library::Library;

pub struct Widgets {
    pub window: adw::ApplicationWindow,
    pub toasts: adw::ToastOverlay,
    /// Vertical box of History rows — cleared and repopulated by `window::rebuild_history`
    /// after every successful open and every Library add/remove (an "already in Library"
    /// row needs its add-button state refreshed too). Rebuilt from
    /// `fond_read_gtk::history::load()` each time, not a locally-held list — History is a
    /// shared, cross-app log now, so this app has no authoritative in-memory copy of it.
    pub history_box: gtk4::Box,
    /// Cards for intentionally-added documents — cleared and repopulated by
    /// `window::rebuild_library` whenever Library membership changes.
    pub library_flow: gtk4::FlowBox,
    /// The row of shelf tabs above the cards — rebuilt by `window::rebuild_library`.
    pub library_shelf_bar: gtk4::Box,
    /// Shown only when the Library is empty; toggled by `window::rebuild_library`.
    pub library_empty_hint: gtk4::Label,
    pub config: Rc<RefCell<Config>>,
    pub library: RefCell<Library>,
    /// The strip of resurfaced highlights above the Library cards.
    pub resurface_strip: gtk4::Box,
}

pub fn toast(widgets: &Rc<Widgets>, message: &str) {
    widgets.toasts.add_toast(adw::Toast::new(message));
}

/// Make a list row do `action` when it is clicked or activated from the keyboard. A row's own
/// `activate` signal is only raised by the keyboard, so a click needs a gesture of its own; the
/// pause stops a GTK that raises both from doing it twice.
pub fn on_open(row: &gtk4::ListBoxRow, action: impl Fn() + 'static) {
    use gtk4::prelude::*;
    let action = Rc::new(action);
    let last = Rc::new(std::cell::Cell::new(None::<std::time::Instant>));
    let run = {
        let action = action.clone();
        move || {
            let now = std::time::Instant::now();
            if last
                .get()
                .is_some_and(|t| now.duration_since(t) < std::time::Duration::from_millis(400))
            {
                return;
            }
            last.set(Some(now));
            action();
        }
    };
    let run = Rc::new(run);
    {
        let run = run.clone();
        row.connect_activate(move |_| run());
    }
    let click = gtk4::GestureClick::new();
    click.set_button(gtk4::gdk::BUTTON_PRIMARY);
    click.connect_released(move |gesture, _, _, _| {
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        run();
    });
    row.add_controller(click);
}
