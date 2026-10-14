//! The Backup and Earlier versions windows, and the daily automatic backup.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::backup;
use crate::config::Config;
use crate::notes_versions;
use crate::ui::{toast, Widgets};

fn when(time: i64) -> String {
    chrono::DateTime::from_timestamp(time, 0)
        .unwrap_or_default()
        .with_timezone(&chrono::Local)
        .format("%-d %b %Y, %H:%M")
        .to_string()
}

fn page_box() -> gtk4::Box {
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    page.set_margin_top(12);
    page.set_margin_bottom(16);
    page.set_margin_start(12);
    page.set_margin_end(12);
    page
}

fn dialog_window(parent: &impl IsA<gtk4::Window>, title: &str, height: i32) -> adw::Window {
    adw::Window::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .default_width(500)
        .default_height(height)
        .build()
}

fn wrapped(text: &str) -> gtk4::Label {
    let l = gtk4::Label::new(Some(text));
    l.set_wrap(true);
    l.set_xalign(0.0);
    l
}

fn confirm(
    parent: &impl IsA<gtk4::Window>,
    heading: &str,
    body: &str,
    yes: &str,
    then: impl Fn() + 'static,
) {
    let dialog = adw::MessageDialog::new(Some(parent), Some(heading), Some(body));
    dialog.add_responses(&[("cancel", "Cancel"), ("yes", yes)]);
    dialog.set_response_appearance("yes", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, id| {
        if id == "yes" {
            then();
        }
    });
    dialog.present();
}

thread_local! {
    static RUNNING: Cell<bool> = const { Cell::new(false) };
    static WARNED: Cell<bool> = const { Cell::new(false) };
}

/// Back up to the chosen folder off the main thread; `done` gets the outcome, or `None` if a
/// backup was already running or none was due.
fn run_backup(
    dest: PathBuf,
    only_if_due: bool,
    done: impl FnOnce(Option<Result<backup::Backup, String>>) + 'static,
) {
    if RUNNING.with(|r| r.replace(true)) {
        done(None);
        return;
    }
    let data = backup::data_dir();
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || {
            let now = fond_read_gtk::versions::now();
            if only_if_due && !backup::due(&dest, now) {
                return None;
            }
            Some(backup::back_up(&data, &dest, now))
        })
        .await
        .unwrap_or_else(|_| Some(Err("Backup failed unexpectedly".into())));
        RUNNING.with(|r| r.set(false));
        done(result);
    });
}

/// Check now and every hour whether a daily backup is due.
pub fn start_automatic(widgets: &Rc<Widgets>) {
    let check = {
        let widgets = widgets.clone();
        move || {
            let folder = Config::load().backup_folder;
            if folder.is_empty() {
                return;
            }
            let widgets = widgets.clone();
            run_backup(PathBuf::from(&folder), true, move |outcome| {
                if let Some(Err(e)) = outcome {
                    if !WARNED.with(|w| w.replace(true)) {
                        toast(
                            &widgets,
                            &format!("Automatic backup to {folder} failed: {e}"),
                        );
                    }
                }
            });
        }
    };
    let first = check.clone();
    glib::timeout_add_seconds_local_once(20, first);
    glib::timeout_add_seconds_local(60 * 60, move || {
        check();
        glib::ControlFlow::Continue
    });
}

pub fn show_backup(widgets: &Rc<Widgets>) {
    let window = dialog_window(&widgets.window, "Backup", -1);
    let toasts = adw::ToastOverlay::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let page = page_box();
    toasts.set_child(Some(&page));
    toolbar.set_content(Some(&toasts));
    window.set_content(Some(&toolbar));
    fill_backup(widgets, &window, &toasts, &page);
    window.present();
}

fn fill_backup(
    widgets: &Rc<Widgets>,
    window: &adw::Window,
    toasts: &adw::ToastOverlay,
    page: &gtk4::Box,
) {
    while let Some(c) = page.first_child() {
        page.remove(&c);
    }
    let folder = Config::load().backup_folder;
    page.append(&wrapped(&format!(
        "Pereplyot can copy your notes, highlights, bookmarks, reading places, notebooks and Library \
         into a folder you choose, once a day, keeping the last {}. Pick a folder that a sync \
         service or backup drive looks after, and the copies leave this computer too. They are \
         plain files you can open yourself.",
        backup::KEEP
    )));
    let aside = wrapped(
        "Notes on documents opened from Kartoteka or Sputnik are kept in those apps' own folders, \
         so they are backed up with those, not here.",
    );
    aside.add_css_class("dim-label");
    page.append(&aside);

    let status = wrapped("");
    status.add_css_class("dim-label");
    page.append(&status);
    if folder.is_empty() {
        status.set_text("Off");
    } else {
        status.set_text(&format!("Backing up to {folder}"));
        let status = status.clone();
        let dest = PathBuf::from(&folder);
        let folder = folder.clone();
        glib::spawn_future_local(async move {
            let newest = gio::spawn_blocking(move || backup::list(&dest).into_iter().next())
                .await
                .ok()
                .flatten();
            let last = match newest {
                Some(b) => format!(
                    "last backup {} ({} document(s) with notes)",
                    when(b.created),
                    b.documents
                ),
                None => "no backup there yet".to_string(),
            };
            status.set_text(&format!("Backing up to {folder} — {last}"));
        });
    }

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let choose = gtk4::Button::with_label(if folder.is_empty() {
        "Choose a folder…"
    } else {
        "Change folder…"
    });
    {
        let (widgets, window, toasts, page) = (
            widgets.clone(),
            window.clone(),
            toasts.clone(),
            page.clone(),
        );
        choose.connect_clicked(move |_| {
            let dialog = gtk4::FileDialog::builder()
                .title("Folder for backups")
                .build();
            let (widgets, window2, toasts, page) = (
                widgets.clone(),
                window.clone(),
                toasts.clone(),
                page.clone(),
            );
            dialog.select_folder(Some(&window), gio::Cancellable::NONE, move |result| {
                let Some(path) = result.ok().and_then(|f| f.path()) else {
                    return;
                };
                let mut config = Config::load();
                config.backup_folder = path.to_string_lossy().into_owned();
                config.save();
                back_up_now(&widgets, &window2, &toasts, &page);
            });
        });
    }
    buttons.append(&choose);
    if !folder.is_empty() {
        let now = gtk4::Button::with_label("Back up now");
        let (w, window2, toasts2, page2) = (
            widgets.clone(),
            window.clone(),
            toasts.clone(),
            page.clone(),
        );
        now.connect_clicked(move |b| {
            b.set_sensitive(false);
            back_up_now(&w, &window2, &toasts2, &page2);
        });
        buttons.append(&now);
        let off = gtk4::Button::with_label("Turn off");
        off.add_css_class("flat");
        let (w, window2, toasts2, page2) = (
            widgets.clone(),
            window.clone(),
            toasts.clone(),
            page.clone(),
        );
        off.connect_clicked(move |_| {
            let mut config = Config::load();
            config.backup_folder.clear();
            config.save();
            fill_backup(&w, &window2, &toasts2, &page2);
        });
        buttons.append(&off);
    }
    page.append(&buttons);

    let restore = gtk4::Button::with_label("Restore from a backup…");
    restore.set_halign(gtk4::Align::Start);
    restore.set_tooltip_text(Some(
        "Bring back notes, notebooks and Library documents that are missing here",
    ));
    {
        let (widgets, window, toasts) = (widgets.clone(), window.clone(), toasts.clone());
        restore.connect_clicked(move |_| pick_restore(&widgets, &window, &toasts));
    }
    page.append(&restore);
}

fn back_up_now(
    widgets: &Rc<Widgets>,
    window: &adw::Window,
    toasts: &adw::ToastOverlay,
    page: &gtk4::Box,
) {
    let folder = Config::load().backup_folder;
    let (widgets, window, toasts, page) = (
        widgets.clone(),
        window.clone(),
        toasts.clone(),
        page.clone(),
    );
    run_backup(PathBuf::from(folder), false, move |outcome| {
        let message = match outcome {
            Some(Ok(b)) => format!("Backed up {} document(s) with notes", b.documents),
            Some(Err(e)) => e,
            None => "A backup is already running".to_string(),
        };
        toasts.add_toast(adw::Toast::new(&message));
        fill_backup(&widgets, &window, &toasts, &page);
    });
}

fn pick_restore(widgets: &Rc<Widgets>, window: &adw::Window, toasts: &adw::ToastOverlay) {
    let dialog = gtk4::FileDialog::builder()
        .title("Choose a backup folder (named “Pereplyot backup …”)")
        .build();
    let folder = Config::load().backup_folder;
    if !folder.is_empty() {
        dialog.set_initial_folder(Some(&gio::File::for_path(&folder)));
    }
    let (widgets, window2, toasts) = (widgets.clone(), window.clone(), toasts.clone());
    dialog.select_folder(Some(window), gio::Cancellable::NONE, move |result| {
        let Some(path) = result.ok().and_then(|f| f.path()) else {
            return;
        };
        let (widgets, toasts) = (widgets.clone(), toasts.clone());
        confirm(
            &window2,
            "Restore from this backup?",
            "Notes, notebooks and Library documents missing here are brought back, and documents \
             here gain any highlights they lack, including ones deleted since the backup. Nothing \
             here is removed or overwritten, and each document's notes are kept as an earlier \
             version before they change. Documents open in a reader are left alone.",
            "Restore",
            move || restore_from(&widgets, &toasts, path.clone()),
        );
    });
}

fn restore_from(widgets: &Rc<Widgets>, toasts: &adw::ToastOverlay, path: PathBuf) {
    let open: Vec<String> = fond_read_gtk::history::load()
        .into_iter()
        .map(|e| e.hash)
        .chain(
            widgets
                .library
                .borrow()
                .entries()
                .iter()
                .map(|e| e.hash.clone()),
        )
        .filter(|h| fond_read_gtk::existing_reader(h).is_some())
        .collect();
    let (widgets, toasts) = (widgets.clone(), toasts.clone());
    let data = backup::data_dir();
    glib::spawn_future_local(async move {
        let library_file = path.join("library.json");
        let result = gio::spawn_blocking(move || {
            backup::restore(
                &path,
                &data,
                &|h| open.iter().any(|o| o == h),
                fond_read_gtk::versions::now(),
            )
        })
        .await
        .unwrap_or_else(|_| Err("Restore failed unexpectedly".into()));
        let message = match result {
            Ok(r) => {
                let library = crate::library::Library::read(&library_file)
                    .map(|l| widgets.library.borrow_mut().merge_from(&l))
                    .unwrap_or(0);
                crate::ui::window::rebuild_library(&widgets);
                let mut parts = Vec::new();
                for (n, what) in [
                    (r.documents, "documents' notes"),
                    (r.annotations, "highlights"),
                    (r.notebooks, "notebooks"),
                    (library, "Library documents"),
                ] {
                    if n > 0 {
                        parts.push(format!("{n} {what}"));
                    }
                }
                let mut m = if parts.is_empty() {
                    "Nothing was missing; everything in the backup is already here".to_string()
                } else {
                    format!("Brought back {}", parts.join(", "))
                };
                if r.skipped_open > 0 {
                    m.push_str(&format!(
                        "; {} open document(s) left alone — close them and restore again",
                        r.skipped_open
                    ));
                }
                if r.unreadable > 0 {
                    m.push_str(&format!(
                        "; {} file(s) in the backup couldn't be read",
                        r.unreadable
                    ));
                }
                m
            }
            Err(e) => e,
        };
        let t = adw::Toast::new(&message);
        t.set_timeout(0);
        toasts.add_toast(t);
    });
}

pub fn show_versions(widgets: &Rc<Widgets>) {
    let window = dialog_window(&widgets.window, "Earlier versions", 560);
    let toasts = adw::ToastOverlay::new();
    let nav = adw::NavigationView::new();
    toasts.set_child(Some(&nav));
    window.set_content(Some(&toasts));

    let titles = crate::notes_index::known_documents(&widgets.library.borrow());
    let title_of = move |hash: &str| -> String {
        titles
            .get(hash)
            .map(|(t, _, _)| t.clone())
            .unwrap_or_else(|| format!("Unknown document ({})", &hash[..hash.len().min(8)]))
    };

    let page = page_box();
    page.append(&wrapped(
        "Pereplyot keeps a copy of a document's notes when you start changing them, then at most \
         once an hour, and before every bulk change or restore. The last 20 are kept, plus one a \
         day for 90 days.",
    ));
    let docs = notes_versions::documents();
    if docs.is_empty() {
        let empty = wrapped("No earlier versions yet.");
        empty.add_css_class("dim-label");
        page.append(&empty);
    }
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    for doc in docs {
        let title = title_of(&doc.hash);
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&title).as_str())
            .subtitle(format!(
                "{} earlier version(s), newest {} · {} note(s) now",
                doc.versions.len(),
                when(doc.versions[0].time),
                notes_versions::current_count(&doc.hash)
            ))
            .activatable(true)
            .build();
        row.add_suffix(&gtk4::Image::from_icon_name("go-next-symbolic"));
        let (nav, toasts) = (nav.clone(), toasts.clone());
        let hash = doc.hash.clone();
        row.connect_activated(move |_| {
            nav.push(&versions_page(&toasts, &hash, &title));
        });
        list.append(&row);
    }
    page.append(&list);
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&page));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroll));
    nav.add(&adw::NavigationPage::new(&toolbar, "Earlier versions"));
    window.present();
}

fn versions_page(toasts: &adw::ToastOverlay, hash: &str, title: &str) -> adw::NavigationPage {
    let page = page_box();
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&page));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroll));
    fill_versions(&page, toasts, hash, title);
    adw::NavigationPage::new(&toolbar, title)
}

fn fill_versions(page: &gtk4::Box, toasts: &adw::ToastOverlay, hash: &str, title: &str) {
    while let Some(c) = page.first_child() {
        page.remove(&c);
    }
    let now = notes_versions::current_count(hash);
    page.append(&wrapped(&format!("{now} note(s) now. Restoring a version keeps the current notes as a version too, so it can be undone here.")));
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    let dir = crate::reader_host::versions_dir(hash);
    for v in fond_read_gtk::versions::list(&dir) {
        let count = notes_versions::count(&v.path);
        let subtitle = match count {
            None => "Can't be read".to_string(),
            Some(n) if n == now => format!("{n} note(s), as many as now"),
            Some(n) if n > now => format!("{n} note(s), {} more than now", n - now),
            Some(n) => format!("{n} note(s), {} fewer than now", now - n),
        };
        let row = adw::ActionRow::builder()
            .title(when(v.time))
            .subtitle(subtitle)
            .build();
        let restore = gtk4::Button::with_label("Restore");
        restore.set_valign(gtk4::Align::Center);
        restore.set_sensitive(count.is_some());
        let (page, toasts, hash, title) = (
            page.clone(),
            toasts.clone(),
            hash.to_string(),
            title.to_string(),
        );
        restore.connect_clicked(move |b| {
            if fond_read_gtk::existing_reader(&hash).is_some() {
                toasts.add_toast(adw::Toast::new("Close the document first, then restore"));
                return;
            }
            let Some(parent) = b.root().and_downcast::<gtk4::Window>() else {
                return;
            };
            let (page, toasts, hash, title, path) =
                (page.clone(), toasts.clone(), hash.clone(), title.clone(), v.path.clone());
            confirm(
                &parent,
                "Restore this version?",
                &format!("The notes for “{title}” go back to how they were then. The current notes are kept as an earlier version."),
                "Restore",
                move || {
                    let message = match notes_versions::restore(&hash, &path) {
                        Ok(()) => "Restored".to_string(),
                        Err(e) => format!("Couldn't restore: {e}"),
                    };
                    toasts.add_toast(adw::Toast::new(&message));
                    fill_versions(&page, &toasts, &hash, &title);
                },
            );
        });
        row.add_suffix(&restore);
        list.append(&row);
    }
    page.append(&list);
}
