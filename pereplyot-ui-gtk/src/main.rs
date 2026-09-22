//! `pereplyot` — the standalone PDF/EPUB reader app, built around the shared
//! `fond-read-gtk` widget crate. See `crates/fond-read-gtk` for the reader itself and
//! `README.md` for how this app fits alongside Kartoteka and Sputnik, which launch this
//! same binary to open a document rather than duplicating its reader UI.

mod about;
mod changelog;
mod config;
mod library;
mod reader_host;
mod thumbnail;
mod ui;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;

use config::Config;
use reader_host::HostOverride;

const APP_ID: &str = "io.github.calstfrancis.Pereplyot";

type EnsureWindow = Rc<dyn Fn(&adw::Application) -> Rc<ui::Widgets>>;

/// What a command-line invocation asked for — either just "show the launcher" (no
/// arguments, e.g. an icon click) or "open this file", optionally with a
/// [`HostOverride`] telling this process to route the document's annotations into
/// Kartoteka's or Sputnik's own storage instead of Pereplyot's local one.
enum ParsedArgs {
    Launcher,
    Open {
        file: PathBuf,
        host_override: Option<HostOverride>,
    },
}

/// Parse the arguments a `command-line` invocation carries (already stripped of argv[0] by
/// the caller) — a bare file path for the ordinary standalone/"Open With" case, or
/// `--vault=<root> --key=<key> <file>` / `--annotations-file=<path>
/// [--progress-file=<path>] <file>` for a launch on Kartoteka's/Sputnik's behalf. See
/// `reader_host::HostOverride`'s doc comment for what each mode means; this function only
/// parses, it doesn't validate the paths exist.
fn parse_args(args: &[std::ffi::OsString]) -> Result<ParsedArgs, String> {
    let mut vault: Option<PathBuf> = None;
    let mut key: Option<String> = None;
    let mut annotations_file: Option<PathBuf> = None;
    let mut progress_file: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;

    for arg in args {
        let s = arg.to_string_lossy();
        if let Some(v) = s.strip_prefix("--vault=") {
            vault = Some(PathBuf::from(v));
        } else if let Some(v) = s.strip_prefix("--key=") {
            key = Some(v.to_string());
        } else if let Some(v) = s.strip_prefix("--annotations-file=") {
            annotations_file = Some(PathBuf::from(v));
        } else if let Some(v) = s.strip_prefix("--progress-file=") {
            progress_file = Some(PathBuf::from(v));
        } else if s.starts_with("--") {
            return Err(format!("Unknown option: {s}"));
        } else if file.is_none() {
            file = Some(PathBuf::from(arg));
        } else {
            return Err(format!("Unexpected extra argument: {s}"));
        }
    }

    let Some(file) = file else {
        if vault.is_some() || key.is_some() || annotations_file.is_some() || progress_file.is_some()
        {
            return Err(
                "A file argument is required alongside --vault/--key/--annotations-file"
                    .to_string(),
            );
        }
        return Ok(ParsedArgs::Launcher);
    };

    let host_override = match (vault, key, annotations_file, progress_file) {
        (None, None, None, None) => None,
        (Some(root), Some(key), None, None) => {
            let hash = content_hash(&file)?;
            Some(HostOverride::Vault { root, key, hash })
        }
        (None, None, Some(annotations), progress) => {
            let hash = content_hash(&file)?;
            Some(HostOverride::ExternalPaths {
                annotations,
                progress,
                hash,
            })
        }
        _ => {
            return Err(
                "Pass either --vault with --key, or --annotations-file (optionally with \
                 --progress-file), not a mix"
                    .to_string(),
            )
        }
    };

    Ok(ParsedArgs::Open {
        file,
        host_override,
    })
}

/// Bookmarks (the one thing every `HostOverride` still keys by content hash rather than
/// vault/key — see that type's doc comment) need the hash before `open_path_with_host`
/// would otherwise compute it, so a launch-with-override reads the file a moment earlier
/// than a plain open does.
fn content_hash(path: &std::path::Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {path:?}: {e}"))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    // Built lazily, on first invocation — and reused across a second one (e.g. a file
    // manager's "Open With" while Pereplyot is already running relays through GLib's
    // single-instance D-Bus activation into this same process rather than a new one).
    let widgets_slot: Rc<RefCell<Option<Rc<ui::Widgets>>>> = Rc::new(RefCell::new(None));

    // Whether the launcher has actually been shown to the user this session — a bare
    // invocation (icon launch, or a second one while already running) always counts; the
    // *first* file-opening invocation on a fresh process does not. Without this, opening
    // Pereplyot purely to view one file (the common case when Kartoteka/Sputnik hand it a
    // file, or a file manager's "Open With") presented the launcher window right alongside
    // the reader — invisible to the user most of the time (behind the reader, or never
    // focused), but still the one `ApplicationWindow` GTK tracks for its own
    // window-count-based auto-quit. Closing the reader the user actually came for then did
    // nothing to the process: it looked like Pereplyot "wouldn't close" (worse,
    // unpredictably — only obviously so when the launcher happened to end up focused/on
    // top), because the app was still alive on account of a window nobody meant to open.
    // Fixed by not presenting the launcher on that first open, and using
    // `fond_read_gtk::on_all_readers_closed` (the reader's own tab/window closing is
    // otherwise invisible to this crate — it's a plain `adw::Window`, not one the
    // `Application` tracks at all) to close the launcher once nothing is left to read, if
    // it was never genuinely shown.
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
        app.connect_command_line(move |app, cmdline| {
            let args = cmdline.arguments();
            // `args[0]` is the invoked binary's own path — GLib includes it, unlike
            // `std::env::args().skip(1)`'s usual convention, so it's stripped here rather
            // than expected of every caller.
            let parsed = match parse_args(args.get(1..).unwrap_or_default()) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("pereplyot: {e}");
                    return glib::ExitCode::FAILURE;
                }
            };

            let widgets = ensure_window(app);

            match parsed {
                ParsedArgs::Launcher => {
                    launcher_shown.set(true);
                    widgets.window.present();
                }
                ParsedArgs::Open {
                    file,
                    host_override,
                } => {
                    if launcher_shown.get() {
                        widgets.window.present();
                    }
                    ui::window::open_path_with_host(&widgets, file, host_override);
                }
            }

            glib::ExitCode::SUCCESS
        });
    }

    app.run()
}
