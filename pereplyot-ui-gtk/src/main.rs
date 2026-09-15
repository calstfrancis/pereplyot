//! `pereplyot` — the standalone PDF/EPUB reader app, built around the shared
//! `fond-read-gtk` widget crate. See `crates/fond-read-gtk` for the reader itself and
//! `README.md` for how this app fits alongside Kartoteka and Sputnik, which embed the same
//! crate rather than duplicating it.

mod about;
mod changelog;
mod config;
mod library;
mod reader_host;
mod recents;
mod thumbnail;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;

use config::Config;

const APP_ID: &str = "io.github.calstfrancis.Pereplyot";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    // Built lazily, on first `activate`/`open` — and reused across a second `open` (e.g. a
    // file manager's "Open With" while Pereplyot is already running relays through GLib's
    // single-instance D-Bus activation into this same process rather than a new one).
    let widgets_slot: Rc<RefCell<Option<Rc<ui::Widgets>>>> = Rc::new(RefCell::new(None));

    fn ensure_window(
        app: &adw::Application,
        slot: &Rc<RefCell<Option<Rc<ui::Widgets>>>>,
    ) -> Rc<ui::Widgets> {
        if let Some(widgets) = slot.borrow().clone() {
            return widgets;
        }
        ui::styles::load_global_css();
        let widgets = ui::window::build(app, Config::load());
        *slot.borrow_mut() = Some(widgets.clone());
        widgets
    }

    {
        let widgets_slot = widgets_slot.clone();
        app.connect_activate(move |app| {
            let widgets = ensure_window(app, &widgets_slot);
            widgets.window.present();
        });
    }

    {
        let widgets_slot = widgets_slot.clone();
        app.connect_open(move |app, files, _hint| {
            let widgets = ensure_window(app, &widgets_slot);
            widgets.window.present();
            for file in files {
                if let Some(path) = file.path() {
                    ui::window::open_path(&widgets, path);
                }
            }
        });
    }

    app.run()
}
