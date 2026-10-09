use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use fond_read_gtk::notebook::Summary;
use fond_read_gtk::notebook_ui;

use crate::ui::{toast, Widgets};

type Refresh = Rc<dyn Fn()>;

pub struct NotebooksPage {
    pub root: gtk4::Box,
    refresh: Rc<dyn Fn()>,
}

impl NotebooksPage {
    pub fn refresh(&self) {
        (self.refresh)();
    }
}

fn ago(t: std::time::SystemTime) -> String {
    let secs = t.elapsed().map_or(0, |d| d.as_secs());
    match secs {
        0..=89 => "just now".to_string(),
        90..=5399 => format!("{} min ago", secs / 60),
        5400..=129_599 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

fn notebook_row(widgets: &Rc<Widgets>, s: &Summary, refresh: Rc<dyn Fn()>) -> gtk4::ListBoxRow {
    let row = adw::ActionRow::new();
    row.set_title(&glib::markup_escape_text(&s.title));
    row.set_subtitle(&format!(
        "{} quote{} · changed {}",
        s.quotes,
        if s.quotes == 1 { "" } else { "s" },
        ago(s.modified)
    ));
    row.set_activatable(true);
    let delete = gtk4::Button::from_icon_name("user-trash-symbolic");
    delete.add_css_class("flat");
    delete.set_valign(gtk4::Align::Center);
    delete.set_tooltip_text(Some("Delete this notebook"));
    delete.update_property(&[gtk4::accessible::Property::Label("Delete notebook")]);
    row.add_suffix(&delete);
    {
        let widgets = widgets.clone();
        let stem = s.stem.clone();
        row.connect_activated(move |_| {
            notebook_ui::open_window(Some(widgets.window.upcast_ref()), Some(&stem));
        });
    }
    {
        let widgets = widgets.clone();
        let stem = s.stem.clone();
        let title = s.title.clone();
        delete.connect_clicked(move |_| {
            let dialog = adw::MessageDialog::new(
                Some(&widgets.window),
                Some("Delete this notebook?"),
                Some(&format!(
                    "“{title}” will be deleted. The annotations in it stay where they are."
                )),
            );
            dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_close_response("cancel");
            let stem = stem.clone();
            let refresh = refresh.clone();
            dialog.connect_response(None, move |_, id| {
                if id == "delete" {
                    notebook_ui::delete_notebook(&stem);
                    refresh();
                }
            });
            dialog.present();
        });
    }
    row.upcast()
}

pub fn build(widgets: &Rc<Widgets>) -> NotebooksPage {
    let new = gtk4::Button::with_label("New notebook");
    new.add_css_class("suggested-action");
    let hint = gtk4::Label::new(Some(
        "A notebook is a Typst page you write in, with highlights from any document dragged in \
         and cited. Keep one per paper or chapter.",
    ));
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.set_hexpand(true);
    hint.add_css_class("dim-label");
    let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    bar.set_margin_top(12);
    bar.set_margin_start(12);
    bar.set_margin_end(12);
    bar.append(&hint);
    bar.append(&new);

    let groups = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    groups.set_margin_top(12);
    groups.set_margin_bottom(12);
    groups.set_margin_start(12);
    groups.set_margin_end(12);
    let empty = gtk4::Label::new(Some("No notebooks yet."));
    empty.add_css_class("dim-label");
    empty.set_margin_top(32);
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    body.append(&groups);
    body.append(&empty);
    scroll.set_child(Some(&body));
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&bar);
    root.append(&scroll);

    let refresh_cell: Rc<std::cell::RefCell<Option<Refresh>>> =
        Rc::new(std::cell::RefCell::new(None));
    let refresh: Rc<dyn Fn()> = {
        let widgets = widgets.clone();
        let groups = groups.clone();
        let empty = empty.clone();
        let cell = refresh_cell.clone();
        Rc::new(move || {
            notebook_ui::flush();
            while let Some(child) = groups.first_child() {
                groups.remove(&child);
            }
            let all = notebook_ui::summaries();
            empty.set_visible(all.is_empty());
            let mut shelves: Vec<Option<String>> = Vec::new();
            for s in &all {
                if !shelves.contains(&s.shelf) {
                    shelves.push(s.shelf.clone());
                }
            }
            shelves.sort_by_key(|s| (s.is_none(), s.as_ref().map(|s| s.to_lowercase())));
            let again: Rc<dyn Fn()> = {
                let cell = cell.clone();
                Rc::new(move || {
                    let f = cell.borrow().clone();
                    if let Some(f) = f {
                        f();
                    }
                })
            };
            for shelf in shelves {
                let heading = gtk4::Label::new(Some(shelf.as_deref().unwrap_or("Stand-alone")));
                heading.add_css_class("heading");
                heading.set_xalign(0.0);
                let list = gtk4::ListBox::new();
                list.set_selection_mode(gtk4::SelectionMode::None);
                list.add_css_class("boxed-list");
                for s in all.iter().filter(|s| s.shelf == shelf) {
                    list.append(&notebook_row(&widgets, s, again.clone()));
                }
                let section = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
                section.append(&heading);
                section.append(&list);
                groups.append(&section);
            }
        })
    };
    *refresh_cell.borrow_mut() = Some(refresh.clone());

    {
        let widgets = widgets.clone();
        let refresh = refresh.clone();
        new.connect_clicked(move |_| {
            let window = widgets.window.clone();
            let shelves = widgets.library.borrow().shelves().to_vec();
            let widgets = widgets.clone();
            let refresh = refresh.clone();
            notebook_ui::prompt_new(Some(window.upcast_ref()), shelves, move |title, shelf| {
                match notebook_ui::create_notebook(&title, shelf) {
                    Some(stem) => {
                        refresh();
                        notebook_ui::open_window(Some(widgets.window.upcast_ref()), Some(&stem));
                    }
                    None => toast(&widgets, "Couldn't create the notebook"),
                }
            });
        });
    }

    NotebooksPage { root, refresh }
}
