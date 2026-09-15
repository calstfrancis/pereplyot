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
use crate::library::{Library, LibraryEntry};
use crate::reader_host::{self, LocalReaderHost};
use crate::recents::{DocKind, RecentEntry, Recents};
use crate::thumbnail;
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
    header.pack_end(&menu_button);

    // Library: intentionally-added documents, cards in a plain FlowBox. Nothing lands here
    // except via History's "Add to Library" action.
    let library_flow = gtk4::FlowBox::new();
    library_flow.set_valign(gtk4::Align::Start);
    library_flow.set_selection_mode(gtk4::SelectionMode::None);
    library_flow.set_homogeneous(true);
    library_flow.set_row_spacing(12);
    library_flow.set_column_spacing(12);
    library_flow.set_margin_top(12);
    library_flow.set_margin_bottom(12);
    library_flow.set_margin_start(12);
    library_flow.set_margin_end(12);

    let library_empty_hint = gtk4::Label::new(Some(
        "Nothing in your library yet — open a document and add it here from History.",
    ));
    library_empty_hint.set_wrap(true);
    library_empty_hint.set_justify(gtk4::Justification::Center);
    library_empty_hint.add_css_class("dim-label");
    library_empty_hint.set_margin_top(48);
    library_empty_hint.set_margin_start(24);
    library_empty_hint.set_margin_end(24);

    let library_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    library_page.append(&library_empty_hint);
    library_page.append(&library_flow);
    let library_scroll = gtk4::ScrolledWindow::new();
    library_scroll.set_child(Some(&library_page));
    library_scroll.set_hexpand(true);
    library_scroll.set_vexpand(true);

    // History: every document ever opened, auto-populated — unchanged in role from the
    // original "Recent" list, just renamed now that Library exists as the opt-in shelf.
    let history_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    history_box.set_margin_top(12);
    history_box.set_margin_bottom(12);
    history_box.set_margin_start(12);
    history_box.set_margin_end(12);
    let history_scroll = gtk4::ScrolledWindow::new();
    history_scroll.set_child(Some(&history_box));
    history_scroll.set_hexpand(true);
    history_scroll.set_vexpand(true);

    let view_stack = adw::ViewStack::new();
    view_stack.add_titled_with_icon(
        &library_scroll,
        Some("library"),
        "Library",
        "view-grid-symbolic",
    );
    view_stack.add_titled_with_icon(
        &history_scroll,
        Some("history"),
        "History",
        "document-open-recent-symbolic",
    );

    let switcher = adw::ViewSwitcher::new();
    switcher.set_stack(Some(&view_stack));
    header.set_title_widget(Some(&switcher));

    // Status bar (house style): blank on the left — Pereplyot has no per-window status
    // message the way Kartoteka's "No library open"/entry count does — a version →
    // changelog button on the right.
    let statusbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    statusbar.add_css_class("toolbar");
    let statusbar_spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    statusbar_spacer.set_hexpand(true);
    let version_button = gtk4::Button::builder()
        .label(concat!("v", env!("CARGO_PKG_VERSION")))
        .tooltip_text("View changelog")
        .build();
    version_button.add_css_class("flat");
    version_button.add_css_class("caption");
    statusbar.append(&statusbar_spacer);
    statusbar.append(&version_button);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_bottom_bar(&statusbar);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view_stack));
    toolbar.set_content(Some(&toasts));
    window.set_content(Some(&toolbar));

    let widgets = Rc::new(Widgets {
        window,
        toasts,
        history_box,
        library_flow,
        library_empty_hint,
        config: Rc::new(RefCell::new(config)),
        recents: RefCell::new(Recents::load()),
        library: RefCell::new(Library::load()),
    });

    {
        let widgets = widgets.clone();
        version_button.connect_clicked(move |_| show_changelog(&widgets.window));
    }
    menu_button.set_popover(Some(&menu::build(&widgets)));

    install_actions(app, &widgets);
    install_drop_target(&widgets);
    rebuild_history(&widgets);
    rebuild_library(&widgets);

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
    rebuild_history(widgets);
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

fn rebuild_history(widgets: &Rc<Widgets>) {
    while let Some(child) = widgets.history_box.first_child() {
        widgets.history_box.remove(&child);
    }

    let entries: Vec<_> = widgets.recents.borrow().entries().to_vec();
    if entries.is_empty() {
        return;
    }

    for entry in entries {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        row.add_css_class("recent-row");

        let open_button = gtk4::Button::new();
        open_button.add_css_class("flat");
        open_button.set_hexpand(true);

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
        open_button.set_child(Some(&inner));

        let handler_widgets = widgets.clone();
        let path = entry.path.clone();
        open_button.connect_clicked(move |_| {
            if path.is_file() {
                open_path(&handler_widgets, path.clone());
            } else {
                toast(&handler_widgets, "File no longer exists at that location");
            }
        });
        row.append(&open_button);

        let already_in_library = widgets.library.borrow().contains(&entry.hash);
        let add_button = gtk4::Button::from_icon_name(if already_in_library {
            "object-select-symbolic"
        } else {
            "list-add-symbolic"
        });
        add_button.add_css_class("flat");
        add_button.set_valign(gtk4::Align::Center);
        if already_in_library {
            add_button.set_tooltip_text(Some("Already in Library"));
            add_button.set_sensitive(false);
        } else {
            add_button.set_tooltip_text(Some("Add to Library"));
            let handler_widgets = widgets.clone();
            let entry = entry.clone();
            add_button.connect_clicked(move |_| {
                add_to_library(&handler_widgets, &entry);
            });
        }
        row.append(&add_button);

        widgets.history_box.append(&row);
    }
}

fn add_to_library(widgets: &Rc<Widgets>, entry: &RecentEntry) {
    widgets.library.borrow_mut().add(LibraryEntry {
        hash: entry.hash.clone(),
        path: entry.path.clone(),
        title: entry.title.clone(),
        kind: entry.kind,
        added_at: chrono::Utc::now(),
    });
    toast(
        widgets,
        &format!("Added \u{201c}{}\u{201d} to Library", entry.title),
    );
    rebuild_library(widgets);
    rebuild_history(widgets);
}

fn rebuild_library(widgets: &Rc<Widgets>) {
    while let Some(child) = widgets.library_flow.first_child() {
        widgets.library_flow.remove(&child);
    }

    let entries: Vec<_> = widgets.library.borrow().entries().to_vec();
    widgets.library_empty_hint.set_visible(entries.is_empty());

    for entry in entries {
        widgets
            .library_flow
            .insert(&build_library_card(widgets, &entry), -1);
    }
}

fn build_library_card(widgets: &Rc<Widgets>, entry: &LibraryEntry) -> gtk4::Widget {
    let card = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    card.set_width_request(120);

    let cover_slot = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    cover_slot.add_css_class("library-cover-slot");
    cover_slot.set_size_request(120, 160);
    cover_slot.set_halign(gtk4::Align::Center);
    cover_slot.set_valign(gtk4::Align::Center);

    match thumbnail::render_thumbnail(entry.kind, &entry.path) {
        Some(texture) => {
            let picture = gtk4::Picture::for_paintable(&texture);
            picture.set_content_fit(gtk4::ContentFit::Cover);
            cover_slot.append(&picture);
        }
        None => {
            let icon_name = match entry.kind {
                DocKind::Pdf => "x-office-document-symbolic",
                DocKind::Epub => "accessories-dictionary-symbolic",
            };
            let icon = gtk4::Image::from_icon_name(icon_name);
            icon.set_pixel_size(48);
            icon.add_css_class("dim-label");
            cover_slot.append(&icon);
        }
    }
    card.append(&cover_slot);

    let title = gtk4::Label::new(Some(&entry.title));
    title.set_wrap(true);
    title.set_justify(gtk4::Justification::Center);
    title.set_lines(2);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(16);
    card.append(&title);

    let button = gtk4::Button::new();
    button.add_css_class("flat");
    button.set_child(Some(&card));
    button.set_tooltip_text(Some("Right-click to remove from Library"));

    let handler_widgets = widgets.clone();
    let path = entry.path.clone();
    button.connect_clicked(move |_| {
        if path.is_file() {
            open_path(&handler_widgets, path.clone());
        } else {
            toast(&handler_widgets, "File no longer exists at that location");
        }
    });

    let click = gtk4::GestureClick::new();
    click.set_button(gtk4::gdk::BUTTON_SECONDARY);
    let handler_widgets = widgets.clone();
    let hash = entry.hash.clone();
    let parent_button = button.clone();
    click.connect_pressed(move |_, _, _, _| {
        let popover = gtk4::Popover::new();
        let remove_button = gtk4::Button::with_label("Remove from Library");
        remove_button.add_css_class("flat");
        remove_button.add_css_class("destructive-action");
        let handler_widgets = handler_widgets.clone();
        let hash = hash.clone();
        let popover_to_close = popover.clone();
        remove_button.connect_clicked(move |_| {
            handler_widgets.library.borrow_mut().remove(&hash);
            rebuild_library(&handler_widgets);
            rebuild_history(&handler_widgets);
            popover_to_close.popdown();
        });
        popover.set_child(Some(&remove_button));
        popover.set_parent(&parent_button);
        popover.popup();
    });
    button.add_controller(click);

    button.upcast()
}
