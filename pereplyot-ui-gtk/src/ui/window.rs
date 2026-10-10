use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gio::prelude::*;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use fond_read_gtk::history::{self, DocKind, HistoryEntry};

use crate::about::show_about;
use crate::changelog::show_changelog;
use crate::config::{Config, LIBRARY_SIZE_MAX, LIBRARY_SIZE_MIN};
use crate::library::{Library, LibraryEntry, Sort};
use crate::reader_host;
use crate::thumbnail;
use crate::ui::{menu, notebooks_page, notes_page, search_page, toast, Widgets};

pub fn build(app: &adw::Application, config: Config) -> Rc<Widgets> {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Pereplyot")
        .default_width(720)
        .default_height(680)
        .build();

    app.style_manager().set_color_scheme(config.color_scheme());
    fond_read_gtk::palette::set_highlight_labels(&config.highlight_labels);
    fond_read_gtk::palette::set_patterns(config.patterns);

    // The shared reader host window (`fond-read-gtk`, also embedded in Kartoteka/Sputnik)
    // has no version of its own to show — only Pereplyot's makes sense here, so this app
    // opts in with its own button rather than the crate building one itself.
    fond_read_gtk::reader_host::set_host_footer(|| {
        let button = gtk4::Button::builder()
            .label(concat!("v", env!("CARGO_PKG_VERSION")))
            .tooltip_text("View changelog")
            .build();
        button.add_css_class("flat");
        button.add_css_class("caption");
        button.connect_clicked(|btn| {
            if let Some(window) = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
                show_changelog(&window);
            }
        });
        button.upcast()
    });

    let header = adw::HeaderBar::new();
    let open_button = gtk4::Button::from_icon_name("document-open-symbolic");
    open_button.set_tooltip_text(Some("Open a PDF or EPUB"));
    open_button.set_action_name(Some("win.open"));
    header.pack_start(&open_button);

    let menu_button = gtk4::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_tooltip_text(Some("Main Menu"));
    header.pack_end(&menu_button);

    let maximize_button = gtk4::Button::from_icon_name("window-maximize-symbolic");
    maximize_button.set_tooltip_text(Some("Maximize window"));
    {
        let window = window.clone();
        maximize_button.connect_clicked(move |_| window.maximize());
    }
    header.pack_end(&maximize_button);

    let fullscreen_button = gtk4::Button::from_icon_name("view-fullscreen-symbolic");
    fullscreen_button.set_tooltip_text(Some("Fullscreen (F11)"));
    {
        let window = window.clone();
        let button = fullscreen_button.clone();
        fullscreen_button.connect_clicked(move |_| toggle_fullscreen(&window, &button));
    }
    header.pack_end(&fullscreen_button);

    {
        let window_for_key = window.clone();
        let button = fullscreen_button.clone();
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, keyval, _keycode, _modifiers| {
            if keyval == gtk4::gdk::Key::F11 {
                toggle_fullscreen(&window_for_key, &button);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(key_controller);
    }

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

    let library_shelf_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let shelf_scroll = gtk4::ScrolledWindow::new();
    shelf_scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Never);
    shelf_scroll.set_hexpand(true);
    shelf_scroll.set_child(Some(&library_shelf_bar));

    let sort_drop = gtk4::DropDown::from_strings(&["Recently added", "Title"]);
    sort_drop.set_selected(u32::from(config.library_sort == "title"));
    sort_drop.set_tooltip_text(Some("Sort the Library"));

    let size_scale = gtk4::Scale::with_range(
        gtk4::Orientation::Horizontal,
        LIBRARY_SIZE_MIN as f64,
        LIBRARY_SIZE_MAX as f64,
        10.0,
    );
    size_scale.set_value(config.library_card_size as f64);
    size_scale.set_draw_value(false);
    size_scale.set_width_request(130);
    size_scale.set_tooltip_text(Some("Card size"));
    size_scale.update_property(&[gtk4::accessible::Property::Label("Library card size")]);

    let library_controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    library_controls.set_halign(gtk4::Align::End);
    library_controls.append(&sort_drop);
    library_controls.append(&gtk4::Image::from_icon_name("view-grid-symbolic"));
    library_controls.append(&size_scale);

    let library_tools = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    library_tools.set_margin_top(8);
    library_tools.set_margin_start(12);
    library_tools.set_margin_end(12);
    library_tools.append(&shelf_scroll);
    library_tools.append(&library_controls);

    let library_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    let resurface_strip = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    resurface_strip.set_margin_top(12);
    resurface_strip.set_margin_start(12);
    resurface_strip.set_margin_end(12);
    resurface_strip.set_visible(false);
    library_page.append(&resurface_strip);
    library_page.append(&library_tools);
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

    let notes_page_slot: Rc<RefCell<Option<notes_page::NotesPage>>> = Rc::new(RefCell::new(None));
    let notebooks_page_slot: Rc<RefCell<Option<notebooks_page::NotebooksPage>>> =
        Rc::new(RefCell::new(None));

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
        library_shelf_bar,
        library_empty_hint,
        config: Rc::new(RefCell::new(config)),
        library: RefCell::new(Library::load()),
        resurface_strip,
    });

    {
        let widgets = widgets.clone();
        version_button.connect_clicked(move |_| show_changelog(&widgets.window));
    }
    menu_button.set_popover(Some(&menu::build(&widgets)));
    {
        let widgets = widgets.clone();
        fond_read_gtk::reader_host::set_host_menu(move || {
            let button = gtk4::MenuButton::new();
            button.set_icon_name("open-menu-symbolic");
            button.set_tooltip_text(Some("Main Menu"));
            button.add_css_class("flat");
            button.set_popover(Some(&menu::build(&widgets)));
            button.upcast()
        });
    }

    {
        let widgets = widgets.clone();
        sort_drop.connect_selected_notify(move |d| {
            {
                let mut c = widgets.config.borrow_mut();
                c.library_sort = if d.selected() == 1 { "title" } else { "added" }.to_string();
                c.save();
            }
            rebuild_library_cards(&widgets);
        });
    }
    {
        let widgets = widgets.clone();
        let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        size_scale.connect_value_changed(move |sc| {
            widgets.config.borrow_mut().library_card_size = sc.value().round() as u32;
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let widgets = widgets.clone();
            let slot = pending.clone();
            let id =
                glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
                    slot.borrow_mut().take();
                    widgets.config.borrow().save();
                    rebuild_library_cards(&widgets);
                });
            *pending.borrow_mut() = Some(id);
        });
    }

    {
        let page = notes_page::build(&widgets);
        view_stack.add_titled_with_icon(
            &page.root,
            Some("notes"),
            "Notes",
            "document-edit-symbolic",
        );
        *notes_page_slot.borrow_mut() = Some(page);
        let slot = notes_page_slot.clone();
        view_stack.connect_visible_child_name_notify(move |stack| {
            if stack.visible_child_name().as_deref() == Some("notes") {
                if let Some(page) = slot.borrow().as_ref() {
                    page.refresh();
                }
            }
        });
    }

    {
        let page = notebooks_page::build(&widgets);
        view_stack.add_titled_with_icon(
            &page.root,
            Some("notebooks"),
            "Notebooks",
            "accessories-text-editor-symbolic",
        );
        *notebooks_page_slot.borrow_mut() = Some(page);
        let slot = notebooks_page_slot.clone();
        view_stack.connect_visible_child_name_notify(move |stack| {
            if stack.visible_child_name().as_deref() == Some("notebooks") {
                if let Some(page) = slot.borrow().as_ref() {
                    page.refresh();
                }
            }
        });
    }

    {
        let page = search_page::build(&widgets);
        view_stack.add_titled_with_icon(
            &page.root,
            Some("search"),
            "Search",
            "system-search-symbolic",
        );
        let page = Rc::new(page);
        {
            let page = page.clone();
            view_stack.connect_visible_child_name_notify(move |stack| {
                if stack.visible_child_name().as_deref() == Some("search") {
                    page.shown();
                    page.entry.grab_focus();
                }
            });
        }
        {
            let page = page.clone();
            let stack = view_stack.clone();
            let palette_widgets = widgets.clone();
            let key = gtk4::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _, modifiers| {
                if keyval == gtk4::gdk::Key::k
                    && modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK)
                {
                    show_palette(&palette_widgets, &stack);
                    return glib::Propagation::Stop;
                }
                if keyval == gtk4::gdk::Key::f
                    && modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK)
                {
                    stack.set_visible_child_name("search");
                    page.entry.grab_focus();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            widgets.window.add_controller(key);
        }
        // Index in the background soon after start, so the first search has something to read.
        let page = page.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || page.shown());
    }

    crate::ui::resurface::refresh(&widgets, &widgets.resurface_strip);
    install_notebook_hooks(&widgets);
    install_actions(app, &widgets);
    install_drop_target(&widgets);
    rebuild_history(&widgets);
    rebuild_library(&widgets);

    widgets
}

/// The launcher's Ctrl+K palette: tabs, actions, and every recent document and notebook.
fn show_palette(widgets: &Rc<Widgets>, stack: &adw::ViewStack) {
    use fond_read_gtk::commands::{self, Command};
    let mut list: Vec<Command> = Vec::new();
    for (label, name) in [
        ("Library", "library"),
        ("History", "history"),
        ("Notes", "notes"),
        ("Notebooks", "notebooks"),
        ("Search", "search"),
    ] {
        let stack = stack.clone();
        list.push(Command::new(format!("Show {label}"), "Tab", move || {
            stack.set_visible_child_name(name)
        }));
    }
    let action = |title: &str, hint: &str, name: &'static str| {
        let window = widgets.window.clone();
        Command::new(title.to_string(), hint.to_string(), move || {
            gio::prelude::ActionGroupExt::activate_action(&window, name, None)
        })
    };
    list.push(action("Open a file…", "Ctrl+O", "open"));
    list.push(action("Highlight labels…", "", "highlight-labels"));
    list.push(action("Resurface highlights (on/off)", "", "resurface"));
    list.push(action("Textures on highlights (on/off)", "", "patterns"));
    list.push(action("OCR languages…", "", "ocr-languages"));
    list.push(action("Markdown notes folder…", "", "markdown-notes"));
    list.push(action("What's new…", "", "whats-new"));
    list.push(action("About Pereplyot", "", "about"));
    for (label, name) in [("System", "system"), ("Light", "light"), ("Dark", "dark")] {
        let window = widgets.window.clone();
        list.push(Command::new(
            format!("Theme: {label}"),
            "Appearance",
            move || {
                gio::prelude::ActionGroupExt::activate_action(
                    &window,
                    "theme",
                    Some(&name.to_variant()),
                )
            },
        ));
    }
    {
        let widgets = widgets.clone();
        list.push(Command::new("New notebook", "Notebooks", move || {
            let shelves = widgets.library.borrow().shelves().to_vec();
            fond_read_gtk::notebook_ui::prompt_new(Some(widgets.window.upcast_ref()), shelves, {
                let widgets = widgets.clone();
                move |title, shelf| {
                    if let Some(stem) = fond_read_gtk::notebook_ui::create_notebook(&title, shelf) {
                        fond_read_gtk::notebook_ui::open_window(
                            Some(widgets.window.upcast_ref()),
                            Some(&stem),
                        );
                    }
                }
            });
        }));
    }
    for s in fond_read_gtk::notebook_ui::summaries() {
        let widgets = widgets.clone();
        list.push(Command::new(
            format!("Notebook: {}", s.title),
            s.shelf.clone().unwrap_or_else(|| "Stand-alone".into()),
            move || {
                fond_read_gtk::notebook_ui::open_window(
                    Some(widgets.window.upcast_ref()),
                    Some(&s.stem),
                )
            },
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut docs: Vec<(String, PathBuf, &str)> = Vec::new();
    for e in widgets.library.borrow().entries() {
        if seen.insert(e.hash.clone()) {
            docs.push((e.title.clone(), e.path.clone(), "Library"));
        }
    }
    for e in history::load() {
        if seen.insert(e.hash.clone()) {
            docs.push((e.title, e.path, "History"));
        }
    }
    for (title, path, hint) in docs {
        let widgets = widgets.clone();
        list.push(Command::new(format!("Open: {title}"), hint, move || {
            open_path(&widgets, path.clone());
        }));
    }
    commands::show(&widgets.window, list, None);
}

fn install_notebook_hooks(widgets: &Rc<Widgets>) {
    let open_widgets = widgets.clone();
    let shelf_widgets = widgets.clone();
    fond_read_gtk::notebook_ui::set_hooks(fond_read_gtk::notebook_ui::Hooks {
        open_source: Some(Rc::new(move |hash, id, page| {
            let path = history::load()
                .into_iter()
                .find(|e| e.hash == hash)
                .map(|e| e.path)
                .or_else(|| {
                    open_widgets
                        .library
                        .borrow()
                        .entries()
                        .iter()
                        .find(|e| e.hash == hash)
                        .map(|e| e.path.clone())
                });
            match path {
                Some(path) if path.is_file() => {
                    open_path_with_host(
                        &open_widgets,
                        path,
                        LaunchOptions {
                            start_page: (page > 0).then_some(page),
                            start_annotation: Some(id.to_string()),
                            ..LaunchOptions::default()
                        },
                    );
                }
                _ => toast(
                    &open_widgets,
                    "That document is no longer at its recorded location",
                ),
            }
        })),
        cite_key: Some(Rc::new(reader_host::citation_key_of)),
        shelves: Some(Rc::new(move || {
            shelf_widgets.library.borrow().shelves().to_vec()
        })),
    });
}

/// Flip the window between fullscreen and normal, swapping the header button's icon and
/// tooltip to match — `adw::ApplicationWindow` tracks fullscreen state itself via
/// `is_fullscreen`, so this just reads it back rather than keeping a separate bool.
fn toggle_fullscreen(window: &adw::ApplicationWindow, button: &gtk4::Button) {
    if window.is_fullscreen() {
        window.unfullscreen();
        button.set_icon_name("view-fullscreen-symbolic");
        button.set_tooltip_text(Some("Fullscreen (F11)"));
    } else {
        window.fullscreen();
        button.set_icon_name("view-restore-symbolic");
        button.set_tooltip_text(Some("Leave fullscreen (F11)"));
    }
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

    let labels_action = gio::SimpleAction::new("highlight-labels", None);
    {
        let widgets = widgets.clone();
        labels_action.connect_activate(move |_, _| {
            crate::ui::highlight_labels::show(&widgets.window, widgets.config.clone())
        });
    }
    window.add_action(&labels_action);

    let ocr_action = gio::SimpleAction::new("ocr-languages", None);
    {
        let widgets = widgets.clone();
        ocr_action.connect_activate(move |_, _| crate::ui::ocr_languages::show(&widgets.window));
    }
    window.add_action(&ocr_action);

    let markdown_action = gio::SimpleAction::new("markdown-notes", None);
    {
        let widgets = widgets.clone();
        markdown_action
            .connect_activate(move |_, _| crate::ui::markdown_notes::show(&widgets.window));
    }
    window.add_action(&markdown_action);

    let whats_new_action = gio::SimpleAction::new("whats-new", None);
    {
        let widgets = widgets.clone();
        whats_new_action
            .connect_activate(move |_, _| crate::ui::welcome::show(&widgets.window, false));
    }
    window.add_action(&whats_new_action);

    let about_action = gio::SimpleAction::new("about", None);
    {
        let widgets = widgets.clone();
        about_action.connect_activate(move |_, _| show_about(&widgets.window));
    }
    window.add_action(&about_action);

    let resurface_action = gio::SimpleAction::new("resurface", None);
    {
        let widgets = widgets.clone();
        resurface_action.connect_activate(move |_, _| {
            {
                let mut c = widgets.config.borrow_mut();
                c.resurface = !c.resurface;
                c.save();
            }
            let on = widgets.config.borrow().resurface;
            toast(
                &widgets,
                if on {
                    "Highlights from past reading will show on the Library page"
                } else {
                    "Resurfacing is off"
                },
            );
            crate::ui::resurface::refresh(&widgets, &widgets.resurface_strip);
        });
    }
    window.add_action(&resurface_action);

    let patterns_action = gio::SimpleAction::new("patterns", None);
    {
        let widgets = widgets.clone();
        patterns_action.connect_activate(move |_, _| {
            let on = {
                let mut c = widgets.config.borrow_mut();
                c.patterns = !c.patterns;
                c.save();
                c.patterns
            };
            fond_read_gtk::palette::set_patterns(on);
            toast(
                &widgets,
                if on {
                    "Highlights now carry a texture as well as a colour (shown as each page is drawn again)"
                } else {
                    "Highlight textures are off"
                },
            );
        });
    }
    window.add_action(&patterns_action);

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
/// button, drag-and-drop, and a plain CLI/"Open With" file argument) funnels through.
pub fn open_path(widgets: &Rc<Widgets>, path: PathBuf) {
    open_path_with_host(widgets, path, LaunchOptions::default());
}

/// What a launch on Kartoteka's/Sputnik's behalf adds to a plain open (see `main.rs`).
#[derive(Default)]
pub struct LaunchOptions {
    pub host_override: Option<reader_host::HostOverride>,
    /// The calling app's own title for the document (e.g. Kartoteka's bibliographic title),
    /// preferred over whatever the file's metadata says.
    pub title: Option<String>,
    /// Open the document's Annotations dialog instead of the reader.
    pub annotations_only: bool,
    /// Open a PDF at this page instead of where it was last left.
    pub start_page: Option<u32>,
    /// Open an EPUB on this annotation's chapter.
    pub start_annotation: Option<String>,
    /// Mark this text in the document on arrival (the words a search hit was found by).
    pub search: Option<String>,
}

/// [`open_path`], but for a document Pereplyot was launched to open on another app's
/// behalf — Kartoteka, or Sputnik — with `override_` set to route its annotations/progress
/// into that app's own vault/material storage instead of Pereplyot's local one. See
/// `reader_host::HostOverride`. Used by `main.rs`'s `--vault=`/`--annotations-file=`
/// command-line handling; `open_path` above is just this with `override_: None`.
/// Returns the Annotations dialog, when `options.annotations_only` actually showed one.
pub fn open_path_with_host(
    widgets: &Rc<Widgets>,
    path: PathBuf,
    options: LaunchOptions,
) -> Option<adw::Window> {
    let LaunchOptions {
        host_override: override_,
        title: title_override,
        annotations_only,
        start_page,
        start_annotation,
        search,
    } = options;
    if !path.is_file() {
        toast(widgets, "Not a file");
        return None;
    }

    let kind = if fond_doc::looks_like_pdf(&path) {
        DocKind::Pdf
    } else if fond_doc::looks_like_epub(&path) {
        DocKind::Epub
    } else {
        toast(widgets, "Only PDF and EPUB files are supported");
        return None;
    };

    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            toast(widgets, &format!("Couldn't read file: {e}"));
            return None;
        }
    };
    let hash = blake3::hash(&bytes).to_hex().to_string();

    // The same path now has different contents than when it was last opened (re-saved, OCR'd,
    // annotated in another program): offer to bring its notes along instead of letting them
    // silently appear to have vanished.
    if override_.is_none() {
        let previous = crate::paths::earlier_hashes(&path, &hash)
            .into_iter()
            .map(|h| (reader_host::local_annotation_count(&h), h))
            .find(|(n, _)| *n > 0);
        if let Some((carry, old)) = previous {
            if reader_host::local_annotation_count(&hash) == 0 {
                crate::paths::record(&path, &hash);
                let dialog = adw::MessageDialog::new(
                    Some(&widgets.window),
                    Some("This file has changed"),
                    Some(&format!(
                        "It looks like a newer version of a document you annotated (it may have been \
                         re-saved or OCR'd). Bring over its {carry} annotation(s), bookmarks and \
                         reading position?"
                    )),
                );
                dialog.add_responses(&[("fresh", "Start fresh"), ("carry", "Bring them over")]);
                dialog.set_response_appearance("carry", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("carry"));
                dialog.set_close_response("fresh");
                let widgets = widgets.clone();
                dialog.connect_response(None, move |_, id| {
                    if id == "carry" {
                        if let Err(e) = reader_host::reattach_local(&old, &hash) {
                            toast(&widgets, &format!("Couldn't bring annotations over: {e}"));
                        }
                    }
                    open_path_with_host(
                        &widgets,
                        path.clone(),
                        LaunchOptions {
                            title: title_override.clone(),
                            annotations_only,
                            ..LaunchOptions::default()
                        },
                    );
                });
                dialog.present();
                return None;
            }
        }
        crate::paths::record(&path, &hash);
    }
    let title = title_override
        .filter(|t| !t.trim().is_empty())
        .or_else(|| sniff_title(kind, &path, &bytes))
        .unwrap_or_else(|| file_stem(&path).unwrap_or_else(|| "Untitled".to_string()));

    // Resolve the saved reading position before the host exists — same reason
    // `LocalReaderHost`'s own case reads `reader_host::saved_progress` up front rather than
    // through the `ReaderHost` trait, which has no "load progress" method (only `save_`):
    // `show_pdf_reader`/`show_epub_reader` need a starting page *before* they can call
    // anything on the host at all.
    let saved_progress = match &override_ {
        None => reader_host::saved_progress(&hash),
        Some(o) => reader_host::saved_progress_for_override(o),
    };

    let host = match reader_host::build_host(widgets, &hash, override_) {
        Ok(host) => host,
        Err(e) => {
            toast(widgets, &format!("Couldn't open: {e}"));
            return None;
        }
    };

    // A reader already open on this document holds the live copy of its annotations; a
    // second editor beside it would have its changes overwritten by the reader's next save.
    if annotations_only && fond_read_gtk::present_existing(&hash) {
        return None;
    }

    // A document with no annotations yet has nothing to list, so it opens in the reader
    // instead of leaving the launch with no visible window at all.
    if annotations_only && !host.load_annotations().annotations.is_empty() {
        let attachment = Some((hash.clone(), path.clone()));
        let (pdf, epub) = match kind {
            DocKind::Pdf => (attachment, None),
            DocKind::Epub => (None, attachment),
        };
        return fond_read_gtk::annotations::show_annotations_dialog(
            &host,
            &widgets.window,
            pdf,
            epub,
            &title,
        );
    }
    if let Some(query) = search.as_deref() {
        if !fond_read_gtk::search_in_open_reader(&hash, query) {
            fond_read_gtk::request_search(&hash, query);
        }
    }
    match kind {
        DocKind::Pdf => {
            let annotation_page = start_annotation.as_deref().and_then(|id| {
                host.load_annotations()
                    .annotations
                    .iter()
                    .find(|a| a.id == id)
                    .and_then(|a| a.page)
            });
            let start_page = start_page
                .or(annotation_page)
                .or(saved_progress.map(|p| p.page))
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
            fond_read_gtk::epub::show_epub_reader(
                &host,
                &widgets.window,
                &hash,
                &path,
                &title,
                start_annotation.as_deref(),
                start_page
                    .map(|chapter| fond_annot::Progress {
                        page: chapter,
                        of: 0,
                        chapter_percent: None,
                    })
                    .or(saved_progress),
            );
        }
    }

    // Kartoteka and Sputnik open documents by launching Pereplyot, so recording here covers
    // what was read from those apps too.
    history::record_open(kind, &hash, &path, &title);
    rebuild_history(widgets);
    None
}

fn sniff_title(kind: DocKind, path: &Path, bytes: &[u8]) -> Option<String> {
    match kind {
        DocKind::Pdf => fond_read_gtk::pdfium::get()
            .ok()
            .and_then(|pdfium| fond_doc::extract_metadata(pdfium, bytes).ok())
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

    let entries = history::load();
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

fn add_to_library(widgets: &Rc<Widgets>, entry: &HistoryEntry) {
    let shelf = current_shelf(widgets);
    widgets.library.borrow_mut().add(LibraryEntry {
        hash: entry.hash.clone(),
        path: entry.path.clone(),
        title: entry.title.clone(),
        kind: entry.kind,
        added_at: chrono::Utc::now(),
        shelf,
    });
    toast(
        widgets,
        &format!("Added \u{201c}{}\u{201d} to Library", entry.title),
    );
    rebuild_library(widgets);
    rebuild_history(widgets);
}

/// The shelf currently shown, if it still exists; `None` means "All".
fn current_shelf(widgets: &Rc<Widgets>) -> Option<String> {
    let want = widgets.config.borrow().library_shelf.clone();
    widgets
        .library
        .borrow()
        .shelves()
        .iter()
        .find(|s| **s == want)
        .cloned()
}

fn rebuild_library(widgets: &Rc<Widgets>) {
    rebuild_shelf_bar(widgets);
    rebuild_library_cards(widgets);
}

fn prompt_name(
    parent: &adw::ApplicationWindow,
    heading: &str,
    initial: &str,
    ok_label: &str,
    on_ok: impl Fn(String) + 'static,
) {
    let dialog = adw::MessageDialog::new(Some(parent), Some(heading), None);
    let entry = gtk4::Entry::new();
    entry.set_text(initial);
    entry.set_activates_default(true);
    entry.update_property(&[gtk4::accessible::Property::Label(heading)]);
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "Cancel"), ("ok", ok_label)]);
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, id| {
        if id == "ok" {
            on_ok(entry.text().to_string());
        }
    });
    dialog.present();
}

fn new_shelf_prompt(widgets: &Rc<Widgets>, then_file: Option<String>) {
    let w = widgets.clone();
    prompt_name(&widgets.window, "New shelf", "", "Create", move |name| {
        let name = name.trim().to_string();
        if !w.library.borrow_mut().add_shelf(&name) {
            toast(&w, "That name is empty or already used");
            return;
        }
        if let Some(hash) = &then_file {
            w.library.borrow_mut().set_shelf(hash, Some(&name));
        }
        w.config.borrow_mut().library_shelf = name;
        w.config.borrow().save();
        rebuild_library(&w);
        rebuild_history(&w);
    });
}

fn rebuild_shelf_bar(widgets: &Rc<Widgets>) {
    let bar = &widgets.library_shelf_bar;
    while let Some(child) = bar.first_child() {
        bar.remove(&child);
    }
    let selected = current_shelf(widgets);
    let (total, shelves): (usize, Vec<(String, usize)>) = {
        let lib = widgets.library.borrow();
        (
            lib.entries().len(),
            lib.shelves()
                .iter()
                .map(|s| (s.clone(), lib.count_on(s)))
                .collect(),
        )
    };

    let all = gtk4::ToggleButton::with_label(&format!("All ({total})"));
    all.add_css_class("flat");
    all.set_active(selected.is_none());
    bar.append(&all);
    {
        let w = widgets.clone();
        all.connect_toggled(move |b| {
            if b.is_active() {
                w.config.borrow_mut().library_shelf = String::new();
                w.config.borrow().save();
                rebuild_library_cards(&w);
            }
        });
    }

    for (name, count) in shelves {
        let chip = gtk4::ToggleButton::with_label(&format!("{name} ({count})"));
        chip.add_css_class("flat");
        chip.set_group(Some(&all));
        chip.set_tooltip_text(Some("Right-click to rename or delete this shelf"));
        chip.set_active(selected.as_deref() == Some(name.as_str()));
        bar.append(&chip);
        {
            let w = widgets.clone();
            let name = name.clone();
            chip.connect_toggled(move |b| {
                if b.is_active() {
                    w.config.borrow_mut().library_shelf = name.clone();
                    w.config.borrow().save();
                    rebuild_library_cards(&w);
                }
            });
        }
        let show_menu = {
            let w = widgets.clone();
            let name = name.clone();
            let chip = chip.clone();
            Rc::new(move || {
                let popover = gtk4::Popover::new();
                let rows = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
                let rename = gtk4::Button::with_label("Rename…");
                rename.add_css_class("flat");
                let delete = gtk4::Button::with_label("Delete shelf (keeps documents)");
                delete.add_css_class("flat");
                delete.add_css_class("destructive-action");
                rows.append(&rename);
                rows.append(&delete);
                popover.set_child(Some(&rows));
                {
                    let w = w.clone();
                    let name = name.clone();
                    let popover = popover.clone();
                    rename.connect_clicked(move |_| {
                        popover.popdown();
                        let w2 = w.clone();
                        let old = name.clone();
                        prompt_name(&w.window, "Rename shelf", &name, "Rename", move |new| {
                            if w2.library.borrow_mut().rename_shelf(&old, &new) {
                                fond_read_gtk::notebook_ui::shelf_changed(&old, Some(new.trim()));
                                w2.config.borrow_mut().library_shelf = new.trim().to_string();
                                w2.config.borrow().save();
                                rebuild_library(&w2);
                            } else {
                                toast(&w2, "That name is empty or already used");
                            }
                        });
                    });
                }
                {
                    let w = w.clone();
                    let name = name.clone();
                    let popover = popover.clone();
                    delete.connect_clicked(move |_| {
                        popover.popdown();
                        w.library.borrow_mut().delete_shelf(&name);
                        fond_read_gtk::notebook_ui::shelf_changed(&name, None);
                        w.config.borrow_mut().library_shelf = String::new();
                        w.config.borrow().save();
                        rebuild_library(&w);
                    });
                }
                popover.set_parent(&chip);
                popover.connect_closed(|p| p.unparent());
                popover.popup();
            })
        };
        let click = gtk4::GestureClick::new();
        click.set_button(gtk4::gdk::BUTTON_SECONDARY);
        {
            let show_menu = show_menu.clone();
            click.connect_pressed(move |_, _, _, _| show_menu());
        }
        chip.add_controller(click);
        let keys = gtk4::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, mods| {
            if key == gtk4::gdk::Key::Menu
                || (key == gtk4::gdk::Key::F10
                    && mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK))
            {
                show_menu();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        chip.add_controller(keys);
    }

    let add = gtk4::Button::from_icon_name("list-add-symbolic");
    add.add_css_class("flat");
    add.set_tooltip_text(Some("New shelf"));
    add.update_property(&[gtk4::accessible::Property::Label("New shelf")]);
    {
        let w = widgets.clone();
        add.connect_clicked(move |_| new_shelf_prompt(&w, None));
    }
    bar.append(&add);
}

fn rebuild_library_cards(widgets: &Rc<Widgets>) {
    while let Some(child) = widgets.library_flow.first_child() {
        widgets.library_flow.remove(&child);
    }
    let shelf = current_shelf(widgets);
    let sort = if widgets.config.borrow().library_sort == "title" {
        Sort::Title
    } else {
        Sort::Added
    };
    let entries = widgets.library.borrow().view(shelf.as_deref(), sort);
    let nothing_at_all = widgets.library.borrow().entries().is_empty();
    widgets.library_empty_hint.set_visible(entries.is_empty());
    widgets.library_empty_hint.set_text(if nothing_at_all {
        "Nothing in your library yet — open a document and add it here from History."
    } else {
        "This shelf is empty — right-click a document in the Library to move it here."
    });

    for entry in entries {
        widgets
            .library_flow
            .insert(&build_library_card(widgets, &entry), -1);
    }
}

fn show_card_menu(widgets: &Rc<Widgets>, hash: &str, anchor: &gtk4::Button) {
    let popover = gtk4::Popover::new();
    let rows = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);

    let heading = gtk4::Label::new(Some("Move to shelf"));
    heading.add_css_class("dim-label");
    heading.add_css_class("caption-heading");
    heading.set_xalign(0.0);
    rows.append(&heading);

    let (current, shelves) = {
        let lib = widgets.library.borrow();
        (
            lib.entries()
                .iter()
                .find(|e| e.hash == hash)
                .and_then(|e| e.shelf.clone()),
            lib.shelves().to_vec(),
        )
    };
    let mut choices: Vec<(String, Option<String>)> = vec![("No shelf".into(), None)];
    choices.extend(shelves.into_iter().map(|s| (s.clone(), Some(s))));
    for (label, target) in choices {
        let marker = if target == current { "✓ " } else { "" };
        let button = gtk4::Button::with_label(&format!("{marker}{label}"));
        button.add_css_class("flat");
        let w = widgets.clone();
        let hash = hash.to_string();
        let popover = popover.clone();
        button.connect_clicked(move |_| {
            popover.popdown();
            w.library.borrow_mut().set_shelf(&hash, target.as_deref());
            rebuild_library(&w);
        });
        rows.append(&button);
    }
    let new_shelf = gtk4::Button::with_label("New shelf…");
    new_shelf.add_css_class("flat");
    {
        let w = widgets.clone();
        let hash = hash.to_string();
        let popover = popover.clone();
        new_shelf.connect_clicked(move |_| {
            popover.popdown();
            new_shelf_prompt(&w, Some(hash.clone()));
        });
    }
    rows.append(&new_shelf);

    let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    rows.append(&sep);
    let remove = gtk4::Button::with_label("Remove from Library");
    remove.add_css_class("flat");
    remove.add_css_class("destructive-action");
    {
        let w = widgets.clone();
        let hash = hash.to_string();
        let popover = popover.clone();
        remove.connect_clicked(move |_| {
            popover.popdown();
            w.library.borrow_mut().remove(&hash);
            rebuild_library(&w);
            rebuild_history(&w);
        });
    }
    rows.append(&remove);

    popover.set_child(Some(&rows));
    popover.set_parent(anchor);
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

fn build_library_card(widgets: &Rc<Widgets>, entry: &LibraryEntry) -> gtk4::Widget {
    let size = widgets
        .config
        .borrow()
        .library_card_size
        .clamp(LIBRARY_SIZE_MIN, LIBRARY_SIZE_MAX);
    let cover_h = size * 4 / 3;

    let card = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    card.set_width_request(size as i32);

    let cover_slot = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    cover_slot.add_css_class("library-cover-slot");
    cover_slot.set_size_request(size as i32, cover_h as i32);
    cover_slot.set_halign(gtk4::Align::Center);
    cover_slot.set_valign(gtk4::Align::Start);
    card.set_valign(gtk4::Align::Start);

    match thumbnail::render_thumbnail(entry.kind, &entry.path, &entry.hash, size) {
        Some(texture) => {
            let picture = gtk4::Picture::for_paintable(&texture);
            picture.set_content_fit(gtk4::ContentFit::Cover);
            picture.set_can_shrink(true);
            // A bare Picture's natural size is the texture's, which ignores the card size;
            // a non-propagating scroller pins it (and passes clicks/wheel through).
            let pin = gtk4::ScrolledWindow::new();
            pin.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Never);
            pin.set_min_content_width(size as i32);
            pin.set_min_content_height(cover_h as i32);
            pin.set_propagate_natural_width(false);
            pin.set_propagate_natural_height(false);
            pin.set_can_target(false);
            pin.set_focusable(false);
            picture.set_hexpand(true);
            picture.set_vexpand(true);
            pin.set_child(Some(&picture));
            cover_slot.append(&pin);
        }
        None => {
            let icon_name = match entry.kind {
                DocKind::Pdf => "x-office-document-symbolic",
                DocKind::Epub => "accessories-dictionary-symbolic",
            };
            let icon = gtk4::Image::from_icon_name(icon_name);
            icon.set_pixel_size((size / 2) as i32);
            icon.add_css_class("dim-label");
            icon.set_vexpand(true);
            cover_slot.append(&icon);
        }
    }
    card.append(&cover_slot);

    let title = gtk4::Label::new(Some(&entry.title));
    title.set_wrap(true);
    title.set_justify(gtk4::Justification::Center);
    title.set_lines(2);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars((size / 8).max(8) as i32);
    card.append(&title);

    let progress_text = match reader_host::saved_progress(&entry.hash) {
        Some(p) if p.of > 0 => {
            let unit = match entry.kind {
                DocKind::Pdf => "p.",
                DocKind::Epub => "ch.",
            };
            format!(
                "{unit} {} of {} · {}%",
                p.page,
                p.of,
                (p.page * 100 / p.of).min(100)
            )
        }
        _ => "Not started".to_string(),
    };
    let progress = gtk4::Label::new(Some(&progress_text));
    progress.add_css_class("dim-label");
    progress.add_css_class("caption");
    card.append(&progress);

    let button = gtk4::Button::new();
    button.add_css_class("flat");
    button.set_child(Some(&card));
    button.set_tooltip_text(Some(&format!(
        "{}\nRight-click (or Menu key) to organise",
        entry.title
    )));
    button.update_property(&[gtk4::accessible::Property::Label(&entry.title)]);

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
    {
        let w = widgets.clone();
        let hash = entry.hash.clone();
        let anchor = button.clone();
        click.connect_pressed(move |_, _, _, _| show_card_menu(&w, &hash, &anchor));
    }
    button.add_controller(click);

    let keys = gtk4::EventControllerKey::new();
    {
        let w = widgets.clone();
        let hash = entry.hash.clone();
        let anchor = button.clone();
        keys.connect_key_pressed(move |_, key, _, mods| {
            if key == gtk4::gdk::Key::Menu
                || (key == gtk4::gdk::Key::F10
                    && mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK))
            {
                show_card_menu(&w, &hash, &anchor);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    button.add_controller(keys);

    button.upcast()
}
