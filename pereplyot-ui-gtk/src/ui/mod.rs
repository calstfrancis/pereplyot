pub mod menu;
pub mod styles;
pub mod window;

use std::cell::RefCell;
use std::rc::Rc;

use libadwaita as adw;

use crate::config::Config;
use crate::recents::Recents;

pub struct Widgets {
    pub window: adw::ApplicationWindow,
    pub toasts: adw::ToastOverlay,
    /// Vertical box of recent-file rows inside the empty-state page — cleared and
    /// repopulated by `window::rebuild_recents` after every successful open.
    pub recents_box: gtk4::Box,
    pub config: Rc<RefCell<Config>>,
    pub recents: RefCell<Recents>,
}

pub fn toast(widgets: &Rc<Widgets>, message: &str) {
    widgets.toasts.add_toast(adw::Toast::new(message));
}
