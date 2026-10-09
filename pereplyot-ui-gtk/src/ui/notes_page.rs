use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use fond_read_gtk::notebook;
use fond_read_gtk::notebook_ui;
use fond_read_gtk::palette::{self, HIGHLIGHT_COLORS};

use crate::notes_edit;
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

pub fn hit_row(widgets: &Rc<Widgets>, hit: NoteHit) -> gtk4::ListBoxRow {
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
    let label = match (&hit.label, hit.page) {
        (Some(l), _) => l.clone(),
        (None, Some(p)) => p.to_string(),
        (None, None) => hit.location.clone(),
    };
    let chapter = hit.page.is_none() && hit.label.is_none();
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
        chapter,
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
            chapter,
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
    if !hit.tags.is_empty() {
        let tags = gtk4::Label::new(Some(
            &hit.tags
                .iter()
                .map(|t| format!("#{t}"))
                .collect::<Vec<_>>()
                .join("  "),
        ));
        tags.set_xalign(0.0);
        tags.add_css_class("caption");
        tags.add_css_class("accent");
        outer.append(&tags);
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

/// A row with a check button in front, for choosing annotations to change together.
fn selectable_row(
    widgets: &Rc<Widgets>,
    hit: NoteHit,
    chosen: Rc<RefCell<BTreeSet<(String, String)>>>,
    changed: Rc<dyn Fn()>,
) -> gtk4::ListBoxRow {
    let key = (hit.hash.clone(), hit.annotation_id.clone());
    let row = hit_row(widgets, hit);
    let check = gtk4::CheckButton::new();
    check.set_tooltip_text(Some(
        "Select, to recolour, tag, export or delete several at once",
    ));
    check.update_property(&[gtk4::accessible::Property::Label("Select this note")]);
    check.set_active(chosen.borrow().contains(&key));
    check.connect_toggled(move |c| {
        if c.is_active() {
            chosen.borrow_mut().insert(key.clone());
        } else {
            chosen.borrow_mut().remove(&key);
        }
        changed();
    });
    if let Some(head) = row
        .child()
        .and_then(|c| c.downcast::<gtk4::Box>().ok())
        .and_then(|outer| outer.first_child())
        .and_then(|h| h.downcast::<gtk4::Box>().ok())
    {
        head.prepend(&check);
    }
    row
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Grouping {
    Document,
    Tag,
    None,
}

fn group_of(hits: Vec<NoteHit>, by: Grouping) -> Vec<(String, Vec<NoteHit>)> {
    let mut groups: Vec<(String, Vec<NoteHit>)> = Vec::new();
    let mut put = |name: String, hit: NoteHit| match groups.iter_mut().find(|(n, _)| *n == name) {
        Some((_, v)) => v.push(hit),
        None => groups.push((name, vec![hit])),
    };
    for h in hits {
        match by {
            Grouping::None => put(String::new(), h),
            Grouping::Document => put(h.doc_title.clone(), h),
            Grouping::Tag => {
                if h.tags.is_empty() {
                    put("No tag".to_string(), h);
                } else {
                    for t in h.tags.clone() {
                        put(format!("#{t}"), h.clone());
                    }
                }
            }
        }
    }
    groups
}

pub fn build(widgets: &Rc<Widgets>) -> NotesPage {
    let search = gtk4::SearchEntry::new();
    search.set_placeholder_text(Some(
        "Search every note and highlight — #tag filters by tag",
    ));
    search.set_hexpand(true);

    let mut options: Vec<String> = vec!["Any colour".to_string()];
    options.extend((0..HIGHLIGHT_COLORS.len()).map(palette::highlight_label));
    let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
    let colour = gtk4::DropDown::from_strings(&option_refs);
    colour.set_tooltip_text(Some("Show only one kind of highlight"));
    let group = gtk4::DropDown::from_strings(&["By document", "By tag", "No grouping"]);
    group.set_tooltip_text(Some("How the list is grouped"));

    let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    bar.set_margin_top(8);
    bar.set_margin_start(12);
    bar.set_margin_end(12);
    bar.append(&search);
    bar.append(&colour);
    bar.append(&group);

    // Shown while anything is selected.
    let chosen: Rc<RefCell<BTreeSet<(String, String)>>> = Rc::new(RefCell::new(BTreeSet::new()));
    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    actions.set_margin_top(6);
    actions.set_margin_start(12);
    actions.set_margin_end(12);
    let selected_label = gtk4::Label::new(None);
    selected_label.set_hexpand(true);
    selected_label.set_xalign(0.0);
    selected_label.add_css_class("heading");
    actions.append(&selected_label);
    let recolour = gtk4::MenuButton::new();
    recolour.set_label("Colour");
    recolour.add_css_class("flat");
    let add_tag = gtk4::Button::with_label("Add tag…");
    let remove_tag = gtk4::Button::with_label("Remove tag…");
    let export = gtk4::Button::with_label("Export…");
    let delete = gtk4::Button::with_label("Delete");
    delete.add_css_class("destructive-action");
    let clear = gtk4::Button::with_label("Clear");
    for b in [&add_tag, &remove_tag, &export, &clear] {
        b.add_css_class("flat");
    }
    actions.append(&recolour);
    actions.append(&add_tag);
    actions.append(&remove_tag);
    actions.append(&export);
    actions.append(&delete);
    actions.append(&clear);
    actions.set_visible(false);

    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    body.set_margin_top(8);
    body.set_margin_bottom(12);
    body.set_margin_start(12);
    body.set_margin_end(12);
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
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    page.append(&count);
    page.append(&body);
    page.append(&empty);
    scroll.set_child(Some(&page));

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&bar);
    root.append(&actions);
    root.append(&scroll);

    let update_actions: Rc<dyn Fn()> = {
        let chosen = chosen.clone();
        let actions = actions.clone();
        let selected_label = selected_label.clone();
        Rc::new(move || {
            let n = chosen.borrow().len();
            actions.set_visible(n > 0);
            selected_label.set_text(&format!("{n} selected"));
        })
    };
    let refresh: Rc<dyn Fn()> = {
        let widgets = widgets.clone();
        let search = search.clone();
        let colour = colour.clone();
        let group = group.clone();
        let body = body.clone();
        let empty = empty.clone();
        let count = count.clone();
        let chosen = chosen.clone();
        let update_actions = update_actions.clone();
        Rc::new(move || {
            while let Some(child) = body.first_child() {
                body.remove(&child);
            }
            let hex = match colour.selected() {
                0 => None,
                i => HIGHLIGHT_COLORS.get(i as usize - 1).map(|c| c.hex),
            };
            let parsed = fond_read_gtk::index::Query::parse(&search.text());
            let text = parsed
                .words
                .iter()
                .chain(parsed.phrases.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            let hits = notes_index::search_with(
                &widgets.library.borrow(),
                &notes_index::Filter {
                    text,
                    colour: hex.map(str::to_string),
                    tags: parsed.tags.clone(),
                    ..notes_index::Filter::default()
                },
            );
            let total = hits.len();
            let by = match group.selected() {
                1 => Grouping::Tag,
                2 => Grouping::None,
                _ => Grouping::Document,
            };
            for (name, items) in group_of(hits, by) {
                let list = gtk4::ListBox::new();
                list.set_selection_mode(gtk4::SelectionMode::None);
                list.add_css_class("boxed-list");
                for hit in items {
                    list.append(&selectable_row(
                        &widgets,
                        hit,
                        chosen.clone(),
                        update_actions.clone(),
                    ));
                }
                if by == Grouping::None {
                    body.append(&list);
                } else {
                    let heading = gtk4::Label::new(Some(&name));
                    heading.add_css_class("heading");
                    heading.set_xalign(0.0);
                    let section = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
                    section.append(&heading);
                    section.append(&list);
                    body.append(&section);
                }
            }
            body.set_visible(total > 0);
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
            update_actions();
        })
    };

    {
        let refresh = refresh.clone();
        search.connect_search_changed(move |_| refresh());
    }
    for d in [&colour, &group] {
        let refresh = refresh.clone();
        d.connect_selected_notify(move |_| refresh());
    }
    {
        let chosen = chosen.clone();
        let refresh = refresh.clone();
        clear.connect_clicked(move |_| {
            chosen.borrow_mut().clear();
            refresh();
        });
    }

    // Colour menu: the four reading colours.
    {
        let popover = gtk4::Popover::new();
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        row.set_margin_top(6);
        row.set_margin_bottom(6);
        row.set_margin_start(6);
        row.set_margin_end(6);
        for (i, c) in HIGHLIGHT_COLORS.iter().enumerate() {
            let b = gtk4::Button::new();
            b.add_css_class("flat");
            b.set_child(Some(&palette::numbered_swatch(c.hex, i + 1)));
            b.set_tooltip_text(Some(&palette::highlight_label(i)));
            b.update_property(&[gtk4::accessible::Property::Label(&format!(
                "Recolour as {}",
                palette::highlight_label(i)
            ))]);
            let widgets = widgets.clone();
            let chosen = chosen.clone();
            let refresh = refresh.clone();
            let popover = popover.clone();
            let hex = c.hex.to_string();
            b.connect_clicked(move |_| {
                popover.popdown();
                bulk(
                    &widgets,
                    &chosen,
                    &notes_edit::Op::Colour(hex.clone()),
                    &refresh,
                );
            });
            row.append(&b);
        }
        popover.set_child(Some(&row));
        recolour.set_popover(Some(&popover));
    }
    for (button, adding) in [(&add_tag, true), (&remove_tag, false)] {
        let widgets = widgets.clone();
        let chosen = chosen.clone();
        let refresh = refresh.clone();
        button.connect_clicked(move |_| {
            let widgets = widgets.clone();
            let chosen = chosen.clone();
            let refresh = refresh.clone();
            let dialog = adw::MessageDialog::new(
                Some(&widgets.window),
                Some(if adding { "Add a tag" } else { "Remove a tag" }),
                Some("The tag, without the #."),
            );
            let entry = gtk4::Entry::new();
            entry.set_activates_default(true);
            dialog.set_extra_child(Some(&entry));
            dialog.add_responses(&[
                ("cancel", "Cancel"),
                ("ok", if adding { "Add" } else { "Remove" }),
            ]);
            dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("ok"));
            dialog.set_close_response("cancel");
            let field = entry.clone();
            dialog.connect_response(None, move |_, id| {
                let tag = field.text().trim().trim_start_matches('#').to_string();
                if id == "ok" && !tag.is_empty() {
                    let op = if adding {
                        notes_edit::Op::AddTag(tag)
                    } else {
                        notes_edit::Op::RemoveTag(tag)
                    };
                    bulk(&widgets, &chosen, &op, &refresh);
                }
            });
            dialog.present();
            entry.grab_focus();
        });
    }
    {
        let widgets = widgets.clone();
        let chosen = chosen.clone();
        let refresh = refresh.clone();
        delete.connect_clicked(move |_| {
            let n = chosen.borrow().len();
            let dialog = adw::MessageDialog::new(
                Some(&widgets.window),
                Some(&format!(
                    "Delete {n} annotation{}?",
                    if n == 1 { "" } else { "s" }
                )),
                Some("They are removed from their documents' annotation files."),
            );
            dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_close_response("cancel");
            let widgets = widgets.clone();
            let chosen = chosen.clone();
            let refresh = refresh.clone();
            dialog.connect_response(None, move |_, id| {
                if id == "delete" {
                    bulk(&widgets, &chosen, &notes_edit::Op::Delete, &refresh);
                }
            });
            dialog.present();
        });
    }
    {
        let widgets = widgets.clone();
        let chosen = chosen.clone();
        export.connect_clicked(move |_| {
            let keys: Vec<(String, String)> = chosen.borrow().iter().cloned().collect();
            crate::notes_export::show(&widgets, keys);
        });
    }

    NotesPage { root, refresh }
}

/// Apply `op` to the chosen annotations and say what happened.
fn bulk(
    widgets: &Rc<Widgets>,
    chosen: &Rc<RefCell<BTreeSet<(String, String)>>>,
    op: &notes_edit::Op,
    refresh: &Rc<dyn Fn()>,
) {
    let targets: Vec<(String, String)> = chosen.borrow().iter().cloned().collect();
    let report = notes_edit::apply(&targets, op, &|hash| {
        fond_read_gtk::existing_reader(hash).is_some()
    });
    let mut message = format!("Changed {}", report.changed);
    if report.skipped_open > 0 {
        message.push_str(&format!(
            "; {} left alone because their document is open — close it and try again",
            report.skipped_open
        ));
    }
    if report.failed > 0 {
        message.push_str(&format!("; {} could not be changed", report.failed));
    }
    toast(widgets, &message);
    if matches!(op, notes_edit::Op::Delete) {
        let skipped: BTreeSet<(String, String)> = targets
            .into_iter()
            .filter(|(h, _)| fond_read_gtk::existing_reader(h).is_some())
            .collect();
        *chosen.borrow_mut() = skipped;
    }
    refresh();
}
