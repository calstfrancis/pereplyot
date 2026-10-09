//! Search across everything read: the text of every document in History and the Library, and
//! every note and highlight, in one list. Filters narrow it by shelf and colour, `#tag` in the
//! query by tag, and "quotes" make a phrase.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gtk4::prelude::*;

use fond_read_gtk::history::DocKind;
use fond_read_gtk::index::{self, DocEntry, Hit, Loaded, Query};
use fond_read_gtk::palette::{self, HIGHLIGHT_COLORS};

use crate::notes_index::{self, Filter};
use crate::search_index;
use crate::ui::notes_page::hit_row;
use crate::ui::window::{open_path_with_host, LaunchOptions};
use crate::ui::{toast, Widgets};

const TEXT_LIMIT: usize = 60;

pub struct SearchPage {
    pub root: gtk4::Box,
    pub entry: gtk4::SearchEntry,
    shown: Rc<dyn Fn()>,
}

impl SearchPage {
    /// Called when the tab is shown: bring the index up to date and refresh the filters.
    pub fn shown(&self) {
        (self.shown)();
    }
}

fn markup_with_marks(text: &str, marks: &[(usize, usize)]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    for &(s, e) in marks {
        if s < at || e > chars.len() || s >= e {
            continue;
        }
        out.push_str(&glib::markup_escape_text(
            &chars[at..s].iter().collect::<String>(),
        ));
        out.push_str("<b>");
        out.push_str(&glib::markup_escape_text(
            &chars[s..e].iter().collect::<String>(),
        ));
        out.push_str("</b>");
        at = e;
    }
    out.push_str(&glib::markup_escape_text(
        &chars[at..].iter().collect::<String>(),
    ));
    out
}

fn text_row(widgets: &Rc<Widgets>, entry: &DocEntry, unit: usize, hit: &Hit) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    outer.set_margin_top(8);
    outer.set_margin_bottom(8);
    outer.set_margin_start(10);
    outer.set_margin_end(10);
    let label = entry
        .labels
        .get(unit)
        .cloned()
        .unwrap_or_else(|| (unit + 1).to_string());
    let place = if entry.chapters {
        format!("ch. {label}")
    } else {
        format!("p. {label}")
    };
    let title = gtk4::Label::new(Some(&format!("{} · {place}", entry.title)));
    title.add_css_class("caption-heading");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    outer.append(&title);
    let snippet = gtk4::Label::new(None);
    snippet.set_markup(&markup_with_marks(&hit.snippet, &hit.marks));
    snippet.set_wrap(true);
    snippet.set_xalign(0.0);
    snippet.set_lines(3);
    snippet.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    outer.append(&snippet);
    row.set_child(Some(&outer));
    row.set_activatable(true);
    row.update_property(&[gtk4::accessible::Property::Label(&format!(
        "{}, {place}: {}",
        entry.title, hit.snippet
    ))]);
    let widgets = widgets.clone();
    let path = entry.path.clone();
    let start = match entry.kind {
        DocKind::Pdf => (unit + 1) as u32,
        DocKind::Epub => label.parse().unwrap_or(1),
    };
    row.connect_activate(move |_| {
        if path.is_file() {
            open_path_with_host(
                &widgets,
                path.clone(),
                LaunchOptions {
                    start_page: Some(start),
                    ..LaunchOptions::default()
                },
            );
        } else {
            toast(
                &widgets,
                "That document is no longer at its recorded location",
            );
        }
    });
    row
}

fn section(title: &str) -> (gtk4::Box, gtk4::Label, gtk4::ListBox) {
    let heading = gtk4::Label::new(Some(title));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    let b = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    b.append(&heading);
    b.append(&list);
    (b, heading, list)
}

fn clear(list: &gtk4::ListBox) {
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
}

pub fn build(widgets: &Rc<Widgets>) -> SearchPage {
    let entry = gtk4::SearchEntry::new();
    entry.set_placeholder_text(Some(
        "Search the text of everything you've read, and your notes",
    ));
    entry.set_hexpand(true);
    let scope =
        gtk4::DropDown::from_strings(&["Everything", "Text of documents", "Notes and highlights"]);
    scope.set_tooltip_text(Some("What to search"));
    let shelf = gtk4::DropDown::from_strings(&["Any shelf"]);
    shelf.set_tooltip_text(Some("Only documents on this shelf"));
    let when = gtk4::DropDown::from_strings(&["Any time", "Past week", "Past month", "Past year"]);
    when.set_tooltip_text(Some("Only notes made since"));
    let mut colours: Vec<String> = vec!["Any colour".to_string()];
    colours.extend((0..HIGHLIGHT_COLORS.len()).map(palette::highlight_label));
    let colour_refs: Vec<&str> = colours.iter().map(String::as_str).collect();
    let colour = gtk4::DropDown::from_strings(&colour_refs);
    colour.set_tooltip_text(Some("Only notes in this colour"));

    let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    bar.set_margin_top(8);
    bar.set_margin_start(12);
    bar.set_margin_end(12);
    bar.append(&entry);
    let filters = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    filters.set_margin_top(6);
    filters.set_margin_start(12);
    filters.set_margin_end(12);
    filters.append(&scope);
    filters.append(&shelf);
    filters.append(&colour);
    filters.append(&when);
    let status = gtk4::Label::new(None);
    status.add_css_class("dim-label");
    status.add_css_class("caption");
    status.set_hexpand(true);
    status.set_xalign(1.0);
    let rebuild = gtk4::Button::with_label("Rebuild");
    rebuild.add_css_class("flat");
    rebuild.set_tooltip_text(Some(
        "Read every document again and rebuild the search index",
    ));
    filters.append(&status);
    filters.append(&rebuild);

    let (notes_box, notes_heading, notes_list) = section("Notes and highlights");
    let (text_box, text_heading, text_list) = section("In the text");
    let empty = gtk4::Label::new(None);
    empty.add_css_class("dim-label");
    empty.set_wrap(true);
    empty.set_margin_top(32);
    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    body.set_margin_top(12);
    body.set_margin_bottom(12);
    body.set_margin_start(12);
    body.set_margin_end(12);
    body.append(&notes_box);
    body.append(&text_box);
    body.append(&empty);
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&body));
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&bar);
    root.append(&filters);
    root.append(&scroll);

    let docs: Rc<RefCell<Option<Arc<Vec<Loaded>>>>> = Rc::new(RefCell::new(None));
    let generation = Rc::new(Cell::new(0u64));
    let shelves_shown: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let run: Rc<dyn Fn()> = {
        let widgets = widgets.clone();
        let entry = entry.clone();
        let scope = scope.clone();
        let shelf = shelf.clone();
        let colour = colour.clone();
        let when = when.clone();
        let docs = docs.clone();
        let generation = generation.clone();
        let shelves_shown = shelves_shown.clone();
        let (notes_box, text_box, notes_list, text_list) = (
            notes_box.clone(),
            text_box.clone(),
            notes_list.clone(),
            text_list.clone(),
        );
        let (notes_heading, text_heading, empty) =
            (notes_heading.clone(), text_heading.clone(), empty.clone());
        Rc::new(move || {
            let query = Query::parse(&entry.text());
            let gen = generation.get() + 1;
            generation.set(gen);
            clear(&notes_list);
            clear(&text_list);
            let nothing_typed = query.is_empty_for_text() && query.tags.is_empty();
            notes_box.set_visible(false);
            text_box.set_visible(false);
            empty.set_visible(true);
            if nothing_typed {
                empty.set_text(
                    "Type to search. “Quotes” find a phrase; #tag finds notes with that tag.",
                );
                return;
            }
            empty.set_text("Searching…");
            let scope_i = scope.selected();
            let library = widgets.library.borrow().clone();
            let shelf_filter: Option<Option<String>> = match shelf.selected() {
                0 => None,
                1 => Some(None),
                i => shelves_shown
                    .borrow()
                    .get(i as usize - 2)
                    .cloned()
                    .map(Some),
            };
            let colour_hex = match colour.selected() {
                0 => None,
                i => HIGHLIGHT_COLORS
                    .get(i as usize - 1)
                    .map(|c| c.hex.to_string()),
            };
            let allowed: Option<std::collections::HashSet<String>> =
                shelf_filter.as_ref().map(|want| {
                    let mut set: std::collections::HashSet<String> = library
                        .entries()
                        .iter()
                        .filter(|e| e.shelf == *want)
                        .map(|e| e.hash.clone())
                        .collect();
                    if want.is_none() {
                        for s in search_index::sources(&library) {
                            if library.shelf_of(&s.hash).is_none() {
                                set.insert(s.hash);
                            }
                        }
                    }
                    set
                });
            let notes_filter = Filter {
                text: query
                    .words
                    .iter()
                    .chain(query.phrases.iter())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" "),
                colour: colour_hex.clone(),
                tags: query.tags.clone(),
                shelf: shelf_filter.clone(),
                hash: None,
                since: match when.selected() {
                    0 => None,
                    i => {
                        let days = [0, 7, 31, 366][i as usize];
                        glib::DateTime::now_local()
                            .ok()
                            .and_then(|n| n.add_days(-days).ok())
                            .and_then(|d| d.format("%Y-%m-%d").ok())
                            .map(|s| s.to_string())
                    }
                },
            };
            let want_notes = scope_i != 1
                && (colour_hex.is_some()
                    || notes_filter.since.is_some()
                    || !query.tags.is_empty()
                    || !query.is_empty_for_text());
            let want_text = scope_i != 2
                && !query.is_empty_for_text()
                && colour_hex.is_none()
                && query.tags.is_empty()
                && notes_filter.since.is_none();
            let widgets = widgets.clone();
            let docs = docs.clone();
            let generation = generation.clone();
            let (notes_box, text_box, notes_list, text_list) = (
                notes_box.clone(),
                text_box.clone(),
                notes_list.clone(),
                text_list.clone(),
            );
            let (notes_heading, text_heading, empty) =
                (notes_heading.clone(), text_heading.clone(), empty.clone());
            glib::spawn_future_local(async move {
                let note_hits = if want_notes {
                    let lib = library.clone();
                    gio::spawn_blocking(move || notes_index::search_with(&lib, &notes_filter))
                        .await
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let cached = docs.borrow().clone();
                let loaded = match cached {
                    Some(d) => d,
                    None => {
                        let d = Arc::new(
                            gio::spawn_blocking(|| index::load_all(&index::dir()))
                                .await
                                .unwrap_or_default(),
                        );
                        *docs.borrow_mut() = Some(d.clone());
                        d
                    }
                };
                let text_hits: Vec<(usize, Hit)> = if want_text {
                    let loaded = loaded.clone();
                    let query = query.clone();
                    gio::spawn_blocking(move || {
                        let allow =
                            |e: &DocEntry| allowed.as_ref().map_or(true, |s| s.contains(&e.hash));
                        index::search(&loaded, &query, &allow, TEXT_LIMIT)
                            .into_iter()
                            .map(|h| (h.doc, h))
                            .collect()
                    })
                    .await
                    .unwrap_or_default()
                } else {
                    Vec::new()
                };
                if generation.get() != gen {
                    return;
                }
                notes_heading.set_text(&format!("Notes and highlights ({})", note_hits.len()));
                for h in note_hits {
                    notes_list.append(&hit_row(&widgets, h));
                }
                notes_box.set_visible(notes_list.first_child().is_some());
                text_heading.set_text(&format!(
                    "In the text ({}{})",
                    text_hits.len(),
                    if text_hits.len() >= TEXT_LIMIT {
                        "+"
                    } else {
                        ""
                    }
                ));
                for (di, h) in &text_hits {
                    text_list.append(&text_row(&widgets, &loaded[*di].entry, h.unit, h));
                }
                text_box.set_visible(!text_hits.is_empty());
                let any = notes_box.is_visible() || text_box.is_visible();
                empty.set_visible(!any);
                empty.set_text("Nothing found. Documents are indexed in the background — try again in a moment if you have just added one.");
            });
        })
    };

    {
        let run = run.clone();
        let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        entry.connect_search_changed(move |_| {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let run = run.clone();
            let slot = pending.clone();
            let id =
                glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                    slot.borrow_mut().take();
                    run();
                });
            *pending.borrow_mut() = Some(id);
        });
    }
    for d in [&scope, &shelf, &colour, &when] {
        let run = run.clone();
        d.connect_selected_notify(move |_| run());
    }

    let shown: Rc<dyn Fn()> = {
        let widgets = widgets.clone();
        let shelf = shelf.clone();
        let status = status.clone();
        let docs = docs.clone();
        let shelves_shown = shelves_shown.clone();
        Rc::new(move || {
            let lib = widgets.library.borrow().clone();
            let names: Vec<String> = lib.shelves().to_vec();
            if *shelves_shown.borrow() != names {
                let mut labels = vec!["Any shelf".to_string(), "No shelf".to_string()];
                labels.extend(names.iter().cloned());
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                shelf.set_model(Some(&gtk4::StringList::new(&refs)));
                *shelves_shown.borrow_mut() = names;
            }
            let status = status.clone();
            let status_done = status.clone();
            let docs = docs.clone();
            let sources = search_index::sources(&lib);
            search_index::refresh(
                sources,
                move |done, total| {
                    if total > 0 && done < total {
                        status.set_text(&format!("Indexing {} of {total}…", done + 1));
                    }
                },
                move |changed| {
                    if changed {
                        *docs.borrow_mut() = None;
                    }
                    let n = index::load_catalogue(&index::dir()).len();
                    status_done.set_text(&format!(
                        "{n} document{} indexed",
                        if n == 1 { "" } else { "s" }
                    ));
                },
            );
        })
    };

    {
        let shown = shown.clone();
        let docs = docs.clone();
        rebuild.connect_clicked(move |_| {
            search_index::clear();
            *docs.borrow_mut() = None;
            shown();
        });
    }

    SearchPage { root, entry, shown }
}
