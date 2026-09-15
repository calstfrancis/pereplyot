pub mod menu;
pub mod styles;
pub mod window;

use std::cell::RefCell;
use std::rc::Rc;

use libadwaita as adw;

use crate::config::Config;
use crate::library::Library;
use crate::recents::Recents;

pub struct Widgets {
    pub window: adw::ApplicationWindow,
    pub toasts: adw::ToastOverlay,
    /// Vertical box of History rows — cleared and repopulated by `window::rebuild_history`
    /// after every successful open and every Library add/remove (an "already in Library"
    /// row needs its add-button state refreshed too).
    pub history_box: gtk4::Box,
    /// Cards for intentionally-added documents — cleared and repopulated by
    /// `window::rebuild_library` whenever Library membership changes.
    pub library_flow: gtk4::FlowBox,
    /// Shown only when the Library is empty; toggled by `window::rebuild_library`.
    pub library_empty_hint: gtk4::Label,
    pub config: Rc<RefCell<Config>>,
    pub recents: RefCell<Recents>,
    pub library: RefCell<Library>,
}

pub fn toast(widgets: &Rc<Widgets>, message: &str) {
    widgets.toasts.add_toast(adw::Toast::new(message));
}
