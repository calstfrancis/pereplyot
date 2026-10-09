//! `pereplyot` — the standalone PDF/EPUB reader app, built around the shared
//! `fond-read-gtk` widget crate. See `crates/fond-read-gtk` for the reader itself and
//! `README.md` for how this app fits alongside Kartoteka and Sputnik, which launch this
//! same binary to open a document rather than duplicating its reader UI.

mod about;
mod changelog;
mod config;
mod library;
mod notes_index;
mod paths;
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
        options: ui::window::LaunchOptions,
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
    let mut title: Option<String> = None;
    let mut annotations_only = false;
    let mut start_annotation: Option<String> = None;
    let mut start_page: Option<u32> = None;
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
        } else if let Some(v) = s.strip_prefix("--title=") {
            title = Some(v.to_string());
        } else if s == "--annotations" {
            annotations_only = true;
        } else if let Some(v) = s.strip_prefix("--annotation=") {
            start_annotation = Some(v.to_string());
        } else if let Some(link) = fond_read_gtk::deeplink::parse(&s) {
            let entry = fond_read_gtk::history::load()
                .into_iter()
                .find(|e| e.hash == link.hash)
                .ok_or_else(|| {
                    "That link points at a document Pereplyot has not opened on this computer"
                        .to_string()
                })?;
            file = Some(entry.path);
            start_annotation = link.annotation;
            start_page = link.page;
        } else if s.starts_with("--") {
            // Ignored rather than fatal: Kartoteka/Sputnik may pass an option a
            // not-yet-updated Pereplyot doesn't know, and opening the file anyway beats
            // opening nothing.
            eprintln!("pereplyot: ignoring unknown option {s}");
        } else if file.is_none() {
            file = Some(
                fond_read_gtk::deeplink::file_uri_path(&s).unwrap_or_else(|| PathBuf::from(arg)),
            );
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
        options: ui::window::LaunchOptions {
            host_override,
            title,
            annotations_only,
            start_annotation,
            start_page,
        },
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
    fond_read_gtk::perf::init();
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
    // An `--annotations` launch's dialog keeps the process alive the same way a reader does.
    let dialogs_open: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    let close_if_unused = {
        let launcher_shown = launcher_shown.clone();
        let dialogs_open = dialogs_open.clone();
        Rc::new(move |widgets: &Rc<ui::Widgets>| {
            if !launcher_shown.get() && dialogs_open.get() == 0 && !fond_read_gtk::any_reader_open()
            {
                // `destroy`, not `close`: GTK4's `close` is a no-op on a window that was never
                // realized, which a never-shown launcher isn't — so the process used to
                // outlive its last reader.
                widgets.window.destroy();
            }
        })
    };

    let ensure_window: EnsureWindow = {
        let widgets_slot = widgets_slot.clone();
        let close_if_unused = close_if_unused.clone();
        Rc::new(move |app: &adw::Application| {
            if let Some(widgets) = widgets_slot.borrow().clone() {
                return widgets;
            }
            ui::styles::load_global_css();
            let widgets = ui::window::build(app, Config::load());
            *widgets_slot.borrow_mut() = Some(widgets.clone());

            let close_if_unused = close_if_unused.clone();
            let widgets_for_hook = widgets.clone();
            fond_read_gtk::on_all_readers_closed(move || close_if_unused(&widgets_for_hook));

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
                ParsedArgs::Open { file, options } => {
                    if launcher_shown.get() {
                        widgets.window.present();
                    }
                    if let Some(dialog) = ui::window::open_path_with_host(&widgets, file, options) {
                        dialogs_open.set(dialogs_open.get() + 1);
                        let dialogs_open = dialogs_open.clone();
                        let close_if_unused = close_if_unused.clone();
                        let widgets = widgets.clone();
                        dialog.connect_close_request(move |_| {
                            dialogs_open.set(dialogs_open.get().saturating_sub(1));
                            // After the dialog has actually gone — closing the launcher it's
                            // transient for, from inside its own close handler, would tear it
                            // down mid-signal.
                            let close_if_unused = close_if_unused.clone();
                            let widgets = widgets.clone();
                            glib::idle_add_local_once(move || close_if_unused(&widgets));
                            glib::Propagation::Proceed
                        });
                    }
                }
            }

            glib::ExitCode::SUCCESS
        });
    }

    app.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    fn temp_file() -> PathBuf {
        let p = std::env::temp_dir().join(format!("pereplyot-cli-{}.pdf", std::process::id()));
        std::fs::write(&p, b"x").unwrap();
        p
    }

    #[test]
    fn no_args_shows_launcher() {
        assert!(matches!(parse_args(&[]), Ok(ParsedArgs::Launcher)));
    }

    #[test]
    fn bare_file_opens_standalone() {
        match parse_args(&args(&["a.pdf"])) {
            Ok(ParsedArgs::Open { file, options }) => {
                assert_eq!(file, PathBuf::from("a.pdf"));
                assert!(options.host_override.is_none() && !options.annotations_only);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn vault_mode_with_title_and_annotations() {
        let f = temp_file();
        let f = f.to_string_lossy().to_string();
        match parse_args(&args(&[
            "--vault=/v",
            "--key=smith2020",
            "--title=T",
            "--annotations",
            &f,
        ])) {
            Ok(ParsedArgs::Open { options, .. }) => {
                assert!(options.annotations_only);
                assert_eq!(options.title.as_deref(), Some("T"));
                assert!(matches!(
                    options.host_override,
                    Some(HostOverride::Vault { ref key, .. }) if key == "smith2020"
                ));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn an_annotation_option_and_file_uris_are_understood() {
        match parse_args(&args(&["--annotation=hl-1", "file:///tmp/a%20b.pdf"])) {
            Ok(ParsedArgs::Open { file, options }) => {
                assert_eq!(file, PathBuf::from("/tmp/a b.pdf"));
                assert_eq!(options.start_annotation.as_deref(), Some("hl-1"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn a_link_to_a_document_never_opened_here_says_so() {
        let err = parse_args(&args(&["pereplyot://open?hash=not-a-real-hash-0000"]))
            .err()
            .expect("an error");
        assert!(err.contains("has not opened"), "{err}");
    }

    #[test]
    fn unknown_options_are_ignored() {
        assert!(matches!(
            parse_args(&args(&["--from-the-future=1", "a.pdf"])),
            Ok(ParsedArgs::Open { .. })
        ));
    }

    #[test]
    fn mixed_or_fileless_modes_are_rejected() {
        assert!(parse_args(&args(&["--vault=/v", "--key=k"])).is_err());
        assert!(parse_args(&args(&["--vault=/v", "--annotations-file=/a", "a.pdf"])).is_err());
        assert!(parse_args(&args(&["a.pdf", "b.pdf"])).is_err());
    }
}
