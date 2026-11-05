//! The References tab of the left sidebar: the document's bibliography, how often and where each
//! entry is cited, and which of them the person already owns.

use std::cell::Cell;

use super::*;
use crate::refindex::{self, Entry, RefIndex};
use crate::OwnedCopy;

fn plain(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

fn label_of(reader: &Rc<RefCell<ReaderState>>, page: u16) -> String {
    reader
        .borrow()
        .page_labels
        .get(page as usize)
        .and_then(|l| l.clone())
        .unwrap_or_else(|| (page + 1).to_string())
}

fn heading(e: &Entry) -> String {
    let who = e.parsed.families.join(", ");
    let who = if who.is_empty() {
        e.parsed.label.map(|n| format!("[{n}]")).unwrap_or_default()
    } else {
        who
    };
    let year = e.parsed.year.map(|y| format!(" ({y})")).unwrap_or_default();
    format!("{}{year}", capitalise(&who))
}

fn capitalise(s: &str) -> String {
    s.split(", ")
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn row_for(
    reader: &Rc<RefCell<ReaderState>>,
    host: &Rc<dyn ReaderHost>,
    e: &Entry,
    from_notes: bool,
) -> adw::ExpanderRow {
    let owned: Option<OwnedCopy> = host.find_owned(&e.parsed);
    let row = adw::ExpanderRow::new();
    let title = if e.parsed.title.is_empty() {
        e.parsed.raw.chars().take(90).collect::<String>()
    } else {
        e.parsed.title.clone()
    };
    row.set_title(&plain(&heading(e)));
    row.set_subtitle(&plain(&title));
    row.set_subtitle_lines(2);
    if owned.is_some() {
        let dot = gtk4::Label::new(Some("●"));
        dot.set_tooltip_text(Some("In your library"));
        dot.update_property(&[gtk4::accessible::Property::Label("In your library")]);
        dot.add_css_class("accent");
        row.add_prefix(&dot);
    }
    let n = e.cited.len();
    let count = gtk4::Label::new(Some(&if n == 0 {
        "–".to_string()
    } else {
        format!("{n}×")
    }));
    count.add_css_class("dim-label");
    count.set_tooltip_text(Some(if n == 0 {
        "Not cited in the text"
    } else {
        "Times cited in the text"
    }));
    count.update_property(&[gtk4::accessible::Property::Label(&if n == 0 {
        "Not cited in the text".to_string()
    } else {
        format!("Cited {n} times in the text")
    })]);
    row.add_suffix(&count);

    for cite in &e.cited {
        let item = adw::ActionRow::new();
        item.set_title(&plain(&cite.context));
        item.set_title_lines(3);
        item.set_subtitle(&format!("p. {}", label_of(reader, cite.page)));
        item.set_activatable(true);
        let (reader, page) = (reader.clone(), cite.page);
        item.connect_activated(move |_| jump(&reader, page, JumpKind::Link));
        row.add_row(&item);
    }
    if !from_notes && e.bbox != [0.0; 4] {
        let item = adw::ActionRow::new();
        item.set_title("Go to this entry");
        item.set_subtitle(&format!("p. {}", label_of(reader, e.page)));
        item.set_activatable(true);
        let (reader, page) = (reader.clone(), e.page);
        item.connect_activated(move |_| jump(&reader, page, JumpKind::Link));
        row.add_row(&item);
    }
    match &owned {
        Some(copy) => {
            let mut locators: Vec<Option<String>> = Vec::new();
            for l in e.cited.iter().filter_map(|c| c.locator.clone()) {
                if !locators.contains(&Some(l.clone())) && locators.len() < 3 {
                    locators.push(Some(l));
                }
            }
            if locators.is_empty() {
                locators.push(None);
            }
            for locator in locators {
                let item = adw::ActionRow::new();
                item.set_title(&match &locator {
                    Some(l) => format!("Open your copy at p. {l}"),
                    None => "Open your copy".to_string(),
                });
                item.set_subtitle(&plain(&copy.title));
                item.set_activatable(true);
                let (host, copy) = (host.clone(), copy.clone());
                item.connect_activated(move |_| host.open_owned(&copy, locator.as_deref()));
                row.add_row(&item);
            }
        }
        None => {
            let item = adw::ActionRow::new();
            item.set_title("Copy reference");
            item.set_activatable(true);
            let (host, text) = (host.clone(), e.parsed.raw.clone());
            item.connect_activated(move |_| {
                if let Some(display) = gdk::Display::default() {
                    display.clipboard().set_text(&text);
                }
                host.notify("Copied to clipboard");
            });
            row.add_row(&item);
            if let Some(doi) = &e.parsed.doi {
                let item = adw::ActionRow::new();
                item.set_title("Open DOI in the browser");
                item.set_subtitle(&plain(doi));
                item.set_activatable(true);
                let url = format!("https://doi.org/{doi}");
                item.connect_activated(move |r| {
                    let window = r.root().and_downcast::<gtk4::Window>();
                    gtk4::UriLauncher::new(&url).launch(
                        window.as_ref(),
                        gtk4::gio::Cancellable::NONE,
                        |_| {},
                    );
                });
                row.add_row(&item);
            }
        }
    }
    row
}

fn fill(
    list: &gtk4::ListBox,
    status: &gtk4::Label,
    index: &RefIndex,
    reader: &Rc<RefCell<ReaderState>>,
    host: &Rc<dyn ReaderHost>,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if index.entries.is_empty() {
        status.set_text("No reference list was found in this document.");
        status.set_visible(true);
        return;
    }
    let owned = index
        .entries
        .iter()
        .filter(|e| host.find_owned(&e.parsed).is_some())
        .count();
    status.set_text(&format!(
        "{} {}{}",
        index.entries.len(),
        if index.from_notes {
            "sources in the notes"
        } else {
            "references"
        },
        if owned > 0 {
            format!(" · {owned} in your library")
        } else {
            String::new()
        }
    ));
    status.set_visible(true);
    for e in &index.entries {
        list.append(&row_for(reader, host, e, index.from_notes));
    }
}

/// The tab's page and a function that loads (or builds) the references the first time it is shown.
pub(super) fn build_references_tab(
    reader: &Rc<RefCell<ReaderState>>,
    host: &Rc<dyn ReaderHost>,
) -> (gtk4::Box, Rc<dyn Fn()>) {
    let page = gtk4::Box::new(Orientation::Vertical, 6);
    page.set_margin_top(6);
    page.set_margin_start(6);
    page.set_margin_end(6);
    let status = gtk4::Label::new(Some("Reading the reference list…"));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("dim-label");
    status.add_css_class("caption");
    let search = gtk4::SearchEntry::new();
    search.set_placeholder_text(Some("Filter references"));
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    list.set_margin_bottom(6);
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_child(Some(&list));
    page.append(&search);
    page.append(&status);
    page.append(&scroll);
    {
        let query: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let filter_query = query.clone();
        list.set_filter_func(move |row| {
            let q = filter_query.borrow();
            if q.is_empty() {
                return true;
            }
            let Some(expander) = row.downcast_ref::<adw::ExpanderRow>() else {
                return true;
            };
            let hay = format!("{} {}", expander.title(), expander.subtitle()).to_lowercase();
            q.split_whitespace().all(|w| hay.contains(w))
        });
        let list = list.clone();
        search.connect_search_changed(move |s| {
            *query.borrow_mut() = s.text().to_lowercase();
            list.invalidate_filter();
        });
    }

    let started = Rc::new(Cell::new(false));
    let trigger: Rc<dyn Fn()> = {
        let (reader, host, list, status) =
            (reader.clone(), host.clone(), list.clone(), status.clone());
        Rc::new(move || {
            if started.replace(true) {
                return;
            }
            let (hash, path) = {
                let r = reader.borrow();
                (
                    r.doc_ref
                        .as_ref()
                        .map(|d| d.hash.clone())
                        .unwrap_or_default(),
                    r.path.clone(),
                )
            };
            let (reader, host, list, status) =
                (reader.clone(), host.clone(), list.clone(), status.clone());
            glib::spawn_future_local(async move {
                let hash2 = hash.clone();
                let index = gtk4::gio::spawn_blocking(move || {
                    refindex::load(&hash2).or_else(|| {
                        let built = refindex::build(&path)?;
                        refindex::save(&hash2, &built);
                        Some(built)
                    })
                })
                .await
                .ok()
                .flatten();
                match index {
                    Some(index) => fill(&list, &status, &index, &reader, &host),
                    None => status.set_text("The reference list couldn't be read."),
                }
            });
        })
    };
    (page, trigger)
}

fn path_and_labels(reader: &Rc<RefCell<ReaderState>>) -> (std::path::PathBuf, Vec<Option<String>>) {
    let r = reader.borrow();
    (r.path.clone(), r.page_labels.clone())
}

fn section_page(placeholder: &str) -> (gtk4::Box, gtk4::Label, gtk4::ListBox, gtk4::SearchEntry) {
    let page = gtk4::Box::new(Orientation::Vertical, 6);
    page.set_margin_start(6);
    page.set_margin_end(6);
    let search = gtk4::SearchEntry::new();
    search.set_placeholder_text(Some(placeholder));
    let status = gtk4::Label::new(Some("Reading…"));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("dim-label");
    status.add_css_class("caption");
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    list.set_margin_bottom(6);
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_child(Some(&list));
    page.append(&search);
    page.append(&status);
    page.append(&scroll);
    {
        let query: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let filter_query = query.clone();
        list.set_filter_func(move |row| {
            let q = filter_query.borrow();
            if q.is_empty() {
                return true;
            }
            let hay = if let Some(e) = row.downcast_ref::<adw::ExpanderRow>() {
                format!("{} {}", e.title(), e.subtitle())
            } else if let Some(a) = row.downcast_ref::<adw::ActionRow>() {
                format!("{} {}", a.title(), a.subtitle().unwrap_or_default())
            } else {
                return true;
            }
            .to_lowercase();
            q.split_whitespace().all(|w| hay.contains(w))
        });
        let list = list.clone();
        search.connect_search_changed(move |s| {
            *query.borrow_mut() = s.text().to_lowercase();
            list.invalidate_filter();
        });
    }
    (page, status, list, search)
}

fn index_tab(reader: &Rc<RefCell<ReaderState>>) -> (gtk4::Box, Rc<dyn Fn()>) {
    use pereplyot_core::book_index::{page_of, Entry};
    let (page, status, list, _search) = section_page("Filter the index");
    let started = Rc::new(Cell::new(false));
    let trigger: Rc<dyn Fn()> = {
        let (reader, status, list) = (reader.clone(), status.clone(), list.clone());
        Rc::new(move || {
            if started.replace(true) {
                return;
            }
            let (path, labels) = path_and_labels(&reader);
            let (reader, status, list) = (reader.clone(), status.clone(), list.clone());
            glib::spawn_future_local(async move {
                let entries =
                    gtk4::gio::spawn_blocking(move || crate::bookaids::back_of_book_index(&path))
                        .await
                        .ok()
                        .flatten();
                let Some(entries) = entries.filter(|e| !e.is_empty()) else {
                    status.set_text("No index was found at the back of this document.");
                    return;
                };
                status.set_text(&format!("{} entries", entries.len()));
                let labels = Rc::new(labels);
                let go = {
                    let reader = reader.clone();
                    move |p: usize| jump(&reader, p as u16, JumpKind::Link)
                };
                for e in &entries {
                    let e: &Entry = e;
                    let pages: Vec<(String, Option<usize>)> = e
                        .locators
                        .iter()
                        .map(|l| (l.text(), page_of(l, &labels)))
                        .collect();
                    let term = if e.sub {
                        format!("    {}", e.term)
                    } else {
                        e.term.clone()
                    };
                    let subtitle = match (&e.see, pages.is_empty()) {
                        (Some(see), true) => format!("see {see}"),
                        (Some(see), false) => format!(
                            "{} · see {see}",
                            pages
                                .iter()
                                .map(|(t, _)| t.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        (None, _) => pages
                            .iter()
                            .map(|(t, _)| t.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    };
                    let linked: Vec<&(String, Option<usize>)> =
                        pages.iter().filter(|(_, p)| p.is_some()).collect();
                    if linked.len() > 1 {
                        let row = adw::ExpanderRow::new();
                        row.set_title(&plain(&term));
                        row.set_subtitle(&plain(&subtitle));
                        for (text, p) in linked {
                            let item = adw::ActionRow::new();
                            item.set_title(&format!("p. {text}"));
                            item.set_activatable(true);
                            let (go, p) = (go.clone(), p.unwrap_or(0));
                            item.connect_activated(move |_| go(p));
                            row.add_row(&item);
                        }
                        list.append(&row);
                    } else {
                        let row = adw::ActionRow::new();
                        row.set_title(&plain(&term));
                        row.set_subtitle(&plain(&subtitle));
                        if let Some((_, Some(p))) = linked.first() {
                            row.set_activatable(true);
                            let (go, p) = (go.clone(), *p);
                            row.connect_activated(move |_| go(p));
                        }
                        list.append(&row);
                    }
                }
            });
        })
    };
    (page, trigger)
}

fn copy_table(
    host: &Rc<dyn ReaderHost>,
    path: std::path::PathBuf,
    figure: crate::bookaids::FigureItem,
    format: &'static str,
) {
    let host = host.clone();
    glib::spawn_future_local(async move {
        let cells = gtk4::gio::spawn_blocking(move || {
            crate::bookaids::table_at(&path, figure.page, figure.region)
        })
        .await
        .ok()
        .flatten();
        let Some(cells) = cells else {
            host.notify("This table's words couldn't be read as cells");
            return;
        };
        let text = match format {
            "csv" => pereplyot_core::table::to_csv(&cells),
            "typst" => pereplyot_core::table::to_typst(&cells),
            _ => pereplyot_core::table::to_tsv(&cells),
        };
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(&text);
        }
        host.notify("Copied the table");
    });
}

fn figures_tab(
    reader: &Rc<RefCell<ReaderState>>,
    host: &Rc<dyn ReaderHost>,
) -> (gtk4::Box, Rc<dyn Fn()>) {
    let (page, status, list, _search) = section_page("Filter figures and tables");
    let started = Rc::new(Cell::new(false));
    let trigger: Rc<dyn Fn()> = {
        let (reader, host, status, list) =
            (reader.clone(), host.clone(), status.clone(), list.clone());
        Rc::new(move || {
            if started.replace(true) {
                return;
            }
            let (path, _) = path_and_labels(&reader);
            let (reader, host, status, list) =
                (reader.clone(), host.clone(), status.clone(), list.clone());
            glib::spawn_future_local(async move {
                let figures = {
                    let path = path.clone();
                    gtk4::gio::spawn_blocking(move || crate::bookaids::figures(&path))
                        .await
                        .ok()
                        .flatten()
                };
                let Some(figures) = figures.filter(|f| !f.is_empty()) else {
                    status.set_text("No captioned figures or tables were found.");
                    return;
                };
                status.set_text(&format!("{} figures and tables", figures.len()));
                for f in figures {
                    let row = adw::ActionRow::new();
                    row.set_title(&plain(&f.caption));
                    row.set_title_lines(2);
                    row.set_subtitle(&format!(
                        "p. {} · {}",
                        label_of(&reader, f.page),
                        if f.is_table { "table" } else { "figure" }
                    ));
                    row.set_activatable(true);
                    {
                        let (reader, page) = (reader.clone(), f.page);
                        row.connect_activated(move |_| jump(&reader, page, JumpKind::Link));
                    }
                    if f.is_table {
                        let menu = gtk4::MenuButton::new();
                        menu.set_icon_name("edit-copy-symbolic");
                        menu.set_valign(gtk4::Align::Center);
                        menu.add_css_class("flat");
                        menu.set_tooltip_text(Some("Copy this table's cells"));
                        menu.update_property(&[gtk4::accessible::Property::Label(
                            "Copy this table's cells",
                        )]);
                        let pop = gtk4::Popover::new();
                        let rows = gtk4::Box::new(Orientation::Vertical, 2);
                        for (label, format) in [
                            ("Copy as tab-separated", "tsv"),
                            ("Copy as CSV", "csv"),
                            ("Copy as Typst table", "typst"),
                        ] {
                            let b = gtk4::Button::with_label(label);
                            b.add_css_class("flat");
                            let (host, path, f, pop) =
                                (host.clone(), path.clone(), f.clone(), pop.clone());
                            b.connect_clicked(move |_| {
                                pop.popdown();
                                copy_table(&host, path.clone(), f.clone(), format);
                            });
                            rows.append(&b);
                        }
                        pop.set_child(Some(&rows));
                        menu.set_popover(Some(&pop));
                        row.add_suffix(&menu);
                    }
                    list.append(&row);
                }
            });
        })
    };
    (page, trigger)
}

/// The sidebar's "More" tab: the document's references, its back-of-book index, and its figures and
/// tables, one at a time under a chooser.
pub(super) fn build_more_tab(
    reader: &Rc<RefCell<ReaderState>>,
    host: &Rc<dyn ReaderHost>,
) -> (gtk4::Box, Rc<dyn Fn()>) {
    let (refs_page, refs_trigger) = build_references_tab(reader, host);
    let (index_page, index_trigger) = index_tab(reader);
    let (figures_page, figures_trigger) = figures_tab(reader, host);
    let chooser = gtk4::DropDown::from_strings(&["References", "Index", "Figures and tables"]);
    chooser.update_property(&[gtk4::accessible::Property::Label(
        "What to show in this tab",
    )]);
    chooser.set_margin_top(6);
    chooser.set_margin_start(6);
    chooser.set_margin_end(6);
    let stack = gtk4::Stack::new();
    stack.set_vexpand(true);
    stack.add_named(&refs_page, Some("references"));
    stack.add_named(&index_page, Some("index"));
    stack.add_named(&figures_page, Some("figures"));
    let show: Rc<dyn Fn()> = {
        let (chooser, stack) = (chooser.clone(), stack.clone());
        Rc::new(move || match chooser.selected() {
            1 => {
                stack.set_visible_child_name("index");
                index_trigger();
            }
            2 => {
                stack.set_visible_child_name("figures");
                figures_trigger();
            }
            _ => {
                stack.set_visible_child_name("references");
                refs_trigger();
            }
        })
    };
    {
        let show = show.clone();
        chooser.connect_selected_notify(move |_| show());
    }
    let page = gtk4::Box::new(Orientation::Vertical, 0);
    page.append(&chooser);
    page.append(&stack);
    (page, show)
}
