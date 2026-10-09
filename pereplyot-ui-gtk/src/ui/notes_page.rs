use std::rc::Rc;

use gtk4::prelude::*;

use fond_read_gtk::notebook;
use fond_read_gtk::notebook_ui;
use fond_read_gtk::palette::{self, HIGHLIGHT_COLORS};

use crate::notes_index::{self, NoteHit};
use crate::ui::window::{open_path_with_host, LaunchOptions};
use crate::ui::{toast, Widgets};

pub struct NotesPage {
    pub root: gtk4::Box,
    refresh: Rc<dyn Fn()>,
}

impl NotesPage {
    pub fn refresh(&self) {
        (self.refresh)();
    }
}

fn hit_row(widgets: &Rc<Widgets>, hit: NoteHit) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    outer.set_margin_top(8);
    outer.set_margin_bottom(8);
    outer.set_margin_start(10);
    outer.set_margin_end(10);

    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    if let Some(swatch) = fond_read_gtk::color_swatch(hit.color.as_deref()) {
        head.append(&swatch);
    }
    let title = gtk4::Label::new(Some(&format!("{} · {}", hit.doc_title, hit.location)));
    title.add_css_class("caption-heading");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    head.append(&title);
    let doc = notebook_ui::DocRef {
        hash: hit.hash.clone(),
        title: hit.doc_title.clone(),
    };
    let label = hit.page.map(|p| p.to_string()).unwrap_or_default();
    let quote = notebook::Quote {
        hash: hit.hash.clone(),
        id: hit.annotation_id.clone(),
        page: hit.page.unwrap_or(0),
        label: label.clone(),
        colour: hit.color.clone().unwrap_or_default(),
        title: hit.doc_title.clone(),
        key: notebook_ui::cite_key_for(&hit.hash).unwrap_or_default(),
        text: hit.snippet.clone().unwrap_or_default(),
        note: hit.note.clone().unwrap_or_default(),
        area: hit.area,
    };
    let to_notebook = gtk4::Button::from_icon_name("list-add-symbolic");
    to_notebook.add_css_class("flat");
    to_notebook.set_tooltip_text(Some("Add to the notebook (or drag it there)"));
    to_notebook.update_property(&[gtk4::accessible::Property::Label("Add to notebook")]);
    {
        let widgets = widgets.clone();
        let quote = quote.clone();
        to_notebook.connect_clicked(move |_| {
            notebook_ui::add_to_notebook(quote.clone(), &|m| toast(&widgets, m));
        });
    }
    head.append(&to_notebook);
    let connect = notebook_ui::connect::button(
        {
            let widgets = widgets.clone();
            Rc::new(move || Some(widgets.window.clone().upcast::<gtk4::Window>()))
        },
        fond_read_gtk::connections::End {
            hash: hit.hash.clone(),
            id: hit.annotation_id.clone(),
            page: hit.page.unwrap_or(0),
            label,
            colour: hit.color.clone().unwrap_or_default(),
            title: doc.title.clone(),
            text: hit.snippet.clone().unwrap_or_default(),
        },
        {
            let widgets = widgets.clone();
            Rc::new(move |m| toast(&widgets, m))
        },
    );
    {
        let quote = quote.clone();
        row.add_controller(notebook_ui::drag_source(Rc::new(move || {
            Some(quote.clone())
        })));
    }
    outer.append(&head);

    let connect_row = connect;
    if let Some(q) = hit.snippet.as_deref().filter(|q| !q.trim().is_empty()) {
        let label = gtk4::Label::new(Some(q));
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.set_lines(3);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.add_css_class("dim-label");
        outer.append(&label);
    }
    if let Some(n) = hit.note.as_deref().filter(|n| !n.trim().is_empty()) {
        let label = gtk4::Label::new(Some(n));
        label.set_wrap(true);
        label.set_xalign(0.0);
        outer.append(&label);
    }
    outer.append(&connect_row);
    row.set_child(Some(&outer));
    row.set_activatable(true);
    row.update_property(&[gtk4::accessible::Property::Label(&format!(
        "{}, {}: {}",
        hit.doc_title,
        hit.location,
        hit.snippet.as_deref().unwrap_or("")
    ))]);

    let widgets = widgets.clone();
    row.connect_activate(move |_| match &hit.path {
        Some(path) if path.is_file() => {
            open_path_with_host(
                &widgets,
                path.clone(),
                LaunchOptions {
                    start_page: hit.page,
                    start_annotation: hit.page.is_none().then(|| hit.annotation_id.clone()),
                    ..LaunchOptions::default()
                },
            );
        }
        _ => toast(
            &widgets,
            "That document is no longer at its recorded location",
        ),
    });
    row
}

pub fn build(widgets: &Rc<Widgets>) -> NotesPage {
    let search = gtk4::SearchEntry::new();
    search.set_placeholder_text(Some("Search every note and highlight"));
    search.set_hexpand(true);

    let mut options: Vec<String> = vec!["Any colour".to_string()];
    options.extend((0..HIGHLIGHT_COLORS.len()).map(palette::highlight_label));
    let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
    let colour = gtk4::DropDown::from_strings(&option_refs);
    colour.set_tooltip_text(Some("Show only one kind of highlight"));

    let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    bar.set_margin_top(8);
    bar.set_margin_start(12);
    bar.set_margin_end(12);
    bar.append(&search);
    bar.append(&colour);

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    list.set_margin_top(8);
    list.set_margin_bottom(12);
    list.set_margin_start(12);
    list.set_margin_end(12);

    let empty = gtk4::Label::new(None);
    empty.add_css_class("dim-label");
    empty.set_wrap(true);
    empty.set_margin_top(32);

    let count = gtk4::Label::new(None);
    count.add_css_class("dim-label");
    count.add_css_class("caption");
    count.set_xalign(0.0);
    count.set_margin_start(14);
    count.set_margin_top(4);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    body.append(&count);
    body.append(&list);
    body.append(&empty);
    scroll.set_child(Some(&body));

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&bar);
    root.append(&scroll);

    let refresh: Rc<dyn Fn()> = {
        let widgets = widgets.clone();
        let search = search.clone();
        let colour = colour.clone();
        let list = list.clone();
        let empty = empty.clone();
        let count = count.clone();
        Rc::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let hex = match colour.selected() {
                0 => None,
                i => HIGHLIGHT_COLORS.get(i as usize - 1).map(|c| c.hex),
            };
            let hits = notes_index::search(&widgets.library.borrow(), &search.text(), hex);
            let total = hits.len();
            for hit in hits {
                list.append(&hit_row(&widgets, hit));
            }
            list.set_visible(total > 0);
            count.set_visible(total > 0);
            empty.set_visible(total == 0);
            empty.set_text(if search.text().is_empty() && hex.is_none() {
                "No notes yet — highlight something in a document and it will appear here."
            } else {
                "Nothing matches."
            });
            count.set_text(&if total >= notes_index::MAX_HITS {
                format!("{total}+ results — narrow the search to see the rest")
            } else {
                format!("{total} result(s)")
            });
        })
    };

    {
        let refresh = refresh.clone();
        search.connect_search_changed(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        colour.connect_selected_notify(move |_| refresh());
    }

    {
        let refresh = refresh.clone();
        let root = root.clone();
        fond_read_gtk::connections::subscribe(Rc::new(move || {
            if root.is_mapped() {
                refresh();
            }
        }));
    }

    NotesPage { root, refresh }
}
