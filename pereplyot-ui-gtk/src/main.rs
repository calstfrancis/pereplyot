//! `pereplyot` — the standalone PDF/EPUB reader app, built around the shared
//! `fond-read-gtk` widget crate. See `crates/fond-read-gtk` for the reader itself and
//! `README.md` for how this app fits alongside Kartoteka and Sputnik, which embed the same
//! crate rather than duplicating it.

mod about;
mod changelog;
mod config;
mod library;
mod reader_host;
mod thumbnail;
mod ui;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;

use config::Config;

const APP_ID: &str = "io.github.calstfrancis.Pereplyot";

type EnsureWindow = Rc<dyn Fn(&adw::Application) -> Rc<ui::Widgets>>;

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    // Built lazily, on first `activate`/`open` — and reused across a second `open` (e.g. a
    // file manager's "Open With" while Pereplyot is already running relays through GLib's
    // single-instance D-Bus activation into this same process rather than a new one).
    let widgets_slot: Rc<RefCell<Option<Rc<ui::Widgets>>>> = Rc::new(RefCell::new(None));

    // Whether the launcher has actually been shown to the user this session — `activate`
    // (icon launch, or a second `open` while already running) always counts; the *first*
    // `open` on a fresh process does not. Without this, opening Pereplyot purely to view
    // one file (the common case when Kartoteka/Sputnik hand it a file, or a file manager's
    // "Open With") presented the launcher window right alongside the reader — invisible to
    // the user most of the time (behind the reader, or never focused), but still the one
    // `ApplicationWindow` GTK tracks for its own window-count-based auto-quit. Closing the
    // reader the user actually came for then did nothing to the process: it looked like
    // Pereplyot "wouldn't close" (worse, unpredictably — only obviously so when the launcher
    // happened to end up focused/on top), because the app was still alive on account of a
    // window nobody meant to open. Fixed by not presenting the launcher on that first `open`,
    // and using `fond_read_gtk::on_all_readers_closed` (the reader's own tab/window closing
    // is otherwise invisible to this crate — it's a plain `adw::Window`, not one the
    // `Application` tracks at all) to close the launcher once nothing is left to read, if it
    // was never genuinely shown.
    let launcher_shown: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    let ensure_window: EnsureWindow = {
        let widgets_slot = widgets_slot.clone();
        let launcher_shown = launcher_shown.clone();
        Rc::new(move |app: &adw::Application| {
            if let Some(widgets) = widgets_slot.borrow().clone() {
                return widgets;
            }
            ui::styles::load_global_css();
            let widgets = ui::window::build(app, Config::load());
            *widgets_slot.borrow_mut() = Some(widgets.clone());

            let launcher_shown = launcher_shown.clone();
            let widgets_for_hook = widgets.clone();
            fond_read_gtk::on_all_readers_closed(move || {
                if !launcher_shown.get() {
                    widgets_for_hook.window.close();
                }
            });

            widgets
        })
    };

    {
        let ensure_window = ensure_window.clone();
        let launcher_shown = launcher_shown.clone();
        app.connect_activate(move |app| {
            let widgets = ensure_window(app);
            launcher_shown.set(true);
            widgets.window.present();
        });
    }

    {
        let ensure_window = ensure_window.clone();
        app.connect_open(move |app, files, _hint| {
            let widgets = ensure_window(app);
            if launcher_shown.get() {
                widgets.window.present();
            }
            for file in files {
                if let Some(path) = file.path() {
                    ui::window::open_path(&widgets, path);
                }
            }
        });
    }

    app.run()
}
