use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gio::prelude::*;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::about::show_about;
use crate::changelog::show_changelog;
use crate::config::Config;
use crate::reader_host::{self, LocalReaderHost};
use crate::recents::{DocKind, RecentEntry, Recents};
use crate::ui::{menu, toast, Widgets};

pub fn build(app: &adw::Application, config: Config) -> Rc<Widgets> {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Pereplyot")
        .default_width(720)
        .default_height(680)
        .build();

    app.style_manager().set_color_scheme(config.color_scheme());

    let header = adw::HeaderBar::new();
    let open_button = gtk4::Button::from_icon_name("document-open-symbolic");
    open_button.set_tooltip_text(Some("Open a PDF or EPUB"));
    open_button.set_action_name(Some("win.open"));
    header.pack_start(&open_button);

    let menu_button = gtk4::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_tooltip_text(Some("Main Menu"));
    menu_button.set_menu_model(Some(&menu::build()));
    header.pack_end(&menu_button);

    let recents_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);

    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_child(Some(&recents_box));
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_max_content_height(400);
    scroller.set_propagate_natural_height(true);

    let status = adw::StatusPage::builder()
        .icon_name("document-open-symbolic")
        .title("Open a PDF or EPUB")
        .description("Drag a file onto this window, or use Open.")
        .child(&scroller)
        .vexpand(true)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&status));
    toolbar.set_content(Some(&toasts));
    window.set_content(Some(&toolbar));

    let widgets = Rc::new(Widgets {
        window,
        toasts,
        recents_box,
        config: Rc::new(RefCell::new(config)),
        recents: RefCell::new(Recents::load()),
    });

    install_actions(app, &widgets);
    install_drop_target(&widgets);
    rebuild_recents(&widgets);

    widgets
}

fn install_actions(app: &adw::Application, widgets: &Rc<Widgets>) {
    let window = &widgets.window;

    let open_action = gio::SimpleAction::new("open", None);
    {
        let widgets = widgets.clone();
        open_action.connect_activate(move |_, _| show_open_dialog(&widgets));
    }
    window.add_action(&open_action);

    let theme_action = gio::SimpleAction::new_stateful(
        "theme",
        Some(glib::VariantTy::STRING),
        &"system".to_variant(),
    );
    theme_action.set_state(&widgets.config.borrow().theme.to_variant());
    {
        let widgets = widgets.clone();
        let app = app.clone();
        theme_action.connect_activate(move |action, parameter| {
            let Some(choice) = parameter.and_then(|v| v.get::<String>()) else {
                return;
            };
            action.set_state(&choice.to_variant());
            widgets.config.borrow_mut().theme = choice.clone();
            widgets.config.borrow().save();
            app.style_manager()
                .set_color_scheme(widgets.config.borrow().color_scheme());
        });
    }
    window.add_action(&theme_action);

    let changelog_action = gio::SimpleAction::new("changelog", None);
    {
        let widgets = widgets.clone();
        changelog_action.connect_activate(move |_, _| show_changelog(&widgets.window));
    }
    window.add_action(&changelog_action);

    let about_action = gio::SimpleAction::new("about", None);
    {
        let widgets = widgets.clone();
        about_action.connect_activate(move |_, _| show_about(&widgets.window));
    }
    window.add_action(&about_action);

    app.set_accels_for_action("win.open", &["<Control>o"]);
}

fn install_drop_target(widgets: &Rc<Widgets>) {
    let drop = gtk4::DropTarget::new(
        gtk4::gdk::FileList::static_type(),
        gtk4::gdk::DragAction::COPY,
    );
    let handler_widgets = widgets.clone();
    drop.connect_drop(move |_, value, _, _| {
        let Ok(files) = value.get::<gtk4::gdk::FileList>() else {
            toast(&handler_widgets, "Couldn't read the dropped file");
            return false;
        };
        let mut handled = false;
        for file in files.files() {
            if let Some(path) = file.path() {
                open_path(&handler_widgets, path);
                handled = true;
            }
        }
        if !handled {
            toast(&handler_widgets, "Only local files can be dropped");
        }
        handled
    });
    widgets.window.add_controller(drop);
}

fn show_open_dialog(widgets: &Rc<Widgets>) {
    let filter = gtk4::FileFilter::new();
    filter.set_name(Some("PDF and EPUB"));
    filter.add_pattern("*.pdf");
    filter.add_pattern("*.epub");
    let filters = gio::ListStore::new::<gtk4::FileFilter>();
    filters.append(&filter);

    let dialog = gtk4::FileDialog::builder()
        .title("Open")
        .filters(&filters)
        .build();

    let widgets = widgets.clone();
    dialog.open(
        Some(&widgets.window.clone()),
        gio::Cancellable::NONE,
        move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    open_path(&widgets, path);
                }
            }
        },
    );
}

/// Detect the file type, hash it, resolve a title, and hand it to the shared PDF/EPUB
/// reader with a fresh [`LocalReaderHost`] — the one path every entry point (the Open
/// button, drag-and-drop, and a CLI/"Open With" file argument) funnels through.
pub fn open_path(widgets: &Rc<Widgets>, path: PathBuf) {
    if !path.is_file() {
        toast(widgets, "Not a file");
        return;
    }

    let kind = if fond_doc::looks_like_pdf(&path) {
        DocKind::Pdf
    } else if fond_doc::looks_like_epub(&path) {
        DocKind::Epub
    } else {
        toast(widgets, "Only PDF and EPUB files are supported");
        return;
    };

    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            toast(widgets, &format!("Couldn't read file: {e}"));
            return;
        }
    };
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let title = sniff_title(kind, &path, &bytes)
        .unwrap_or_else(|| file_stem(&path).unwrap_or_else(|| "Untitled".to_string()));

    let host = LocalReaderHost::for_document(widgets, &hash);
    match kind {
        DocKind::Pdf => {
            let start_page = reader_host::saved_progress(&hash)
                .map(|p| p.page)
                .unwrap_or(1);
            fond_read_gtk::pdf::show_pdf_reader(
                &host,
                &widgets.window,
                &hash,
                &path,
                &title,
                start_page,
            );
        }
        DocKind::Epub => {
            let start_progress = reader_host::saved_progress(&hash);
            fond_read_gtk::epub::show_epub_reader(
                &host,
                &widgets.window,
                &hash,
                &path,
                &title,
                None,
                start_progress,
            );
        }
    }

    widgets.recents.borrow_mut().touch(RecentEntry {
        hash,
        path,
        title,
        kind,
        last_opened: chrono::Utc::now(),
    });
    rebuild_recents(widgets);
}

fn sniff_title(kind: DocKind, path: &Path, bytes: &[u8]) -> Option<String> {
    match kind {
        DocKind::Pdf => fond_doc::bind_pdfium()
            .and_then(|pdfium| fond_doc::extract_metadata(pdfium, bytes))
            .ok()
            .and_then(|meta| meta.title)
            .filter(|t| !t.is_empty()),
        DocKind::Epub => fond_doc::extract_epub_metadata(path)
            .ok()
            .and_then(|meta| meta.title)
            .filter(|t| !t.is_empty()),
    }
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem().map(|s| s.to_string_lossy().to_string())
}

fn rebuild_recents(widgets: &Rc<Widgets>) {
    while let Some(child) = widgets.recents_box.first_child() {
        widgets.recents_box.remove(&child);
    }

    let entries: Vec<_> = widgets.recents.borrow().entries().to_vec();
    if entries.is_empty() {
        return;
    }

    let heading = gtk4::Label::new(Some("Recent"));
    heading.add_css_class("heading");
    heading.set_halign(gtk4::Align::Start);
    heading.set_margin_bottom(4);
    widgets.recents_box.append(&heading);

    for entry in entries {
        let row = gtk4::Button::new();
        row.add_css_class("flat");
        row.add_css_class("recent-row");

        let inner = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let title = gtk4::Label::new(Some(&entry.title));
        title.add_css_class("title");
        title.set_halign(gtk4::Align::Start);
        title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let subtitle = gtk4::Label::new(Some(&entry.path.display().to_string()));
        subtitle.add_css_class("subtitle");
        subtitle.set_halign(gtk4::Align::Start);
        subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        inner.append(&title);
        inner.append(&subtitle);
        row.set_child(Some(&inner));

        let handler_widgets = widgets.clone();
        let path = entry.path.clone();
        row.connect_clicked(move |_| {
            if path.is_file() {
                open_path(&handler_widgets, path.clone());
            } else {
                toast(&handler_widgets, "File no longer exists at that location");
            }
        });

        widgets.recents_box.append(&row);
    }
}
