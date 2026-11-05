//! The synthesis matrix window: sources down the side, your questions across the top, the marks about
//! each in the cells, and exports that compile.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use fond_read_gtk::history::DocKind;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use pereplyot_core::matrix::{self, Def, Mark, Matrix, Source};
use pereplyot_core::precis::{self, BibEntry, PrecisMark};

use crate::library::Library;
use crate::matrix_store;
use crate::notes_index::{self, Filter};
use crate::ui::{toast, window::LaunchOptions, Widgets};

type Where = HashMap<String, (PathBuf, DocKind)>;

struct Gathered {
    sources: Vec<Source>,
    marks: Vec<Mark>,
    colours: Vec<(String, String, Option<String>, PrecisMark)>,
    places: Where,
}

/// Blocking: reads every annotation file.
fn gather(library: &Library, def: &Def) -> Gathered {
    let filter = Filter {
        shelf: def.shelf.clone().map(Some),
        limit: Some(usize::MAX),
        ..Default::default()
    };
    let hits = notes_index::search_with(library, &filter);
    let mut order: Vec<String> = Vec::new();
    let mut titles: HashMap<String, String> = HashMap::new();
    let mut places = Where::new();
    if let Some(shelf) = &def.shelf {
        for e in library
            .entries()
            .iter()
            .filter(|e| e.shelf.as_deref() == Some(shelf.as_str()))
        {
            if !order.contains(&e.hash) {
                order.push(e.hash.clone());
            }
            titles.insert(e.hash.clone(), e.title.clone());
            places.insert(e.hash.clone(), (e.path.clone(), e.kind));
        }
    }
    let mut marks = Vec::new();
    let mut colours = Vec::new();
    for h in &hits {
        if !order.contains(&h.hash) {
            order.push(h.hash.clone());
        }
        titles
            .entry(h.hash.clone())
            .or_insert_with(|| h.doc_title.clone());
        if let Some(path) = &h.path {
            let kind = notes_index::known_documents(library)
                .get(&h.hash)
                .map(|d| d.2)
                .unwrap_or(DocKind::Pdf);
            places.entry(h.hash.clone()).or_insert((path.clone(), kind));
        }
        let text = h
            .snippet
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| h.note.clone())
            .unwrap_or_default();
        let label = h
            .label
            .clone()
            .or_else(|| h.page.map(|p| p.to_string()))
            .unwrap_or_default();
        marks.push(Mark {
            hash: h.hash.clone(),
            id: h.annotation_id.clone(),
            label: label.clone(),
            text: text.clone(),
            note: h.note.clone().unwrap_or_default(),
            tags: h.tags.clone(),
        });
        colours.push((
            h.hash.clone(),
            h.annotation_id.clone(),
            h.color.clone(),
            PrecisMark {
                label,
                text,
                note: h.note.clone().unwrap_or_default(),
            },
        ));
    }
    let sources = order
        .into_iter()
        .map(|hash| {
            let record = crate::bib_store::load(&hash).filter(|r| !r.title.is_empty());
            Source {
                title: titles.get(&hash).cloned().unwrap_or_else(|| hash.clone()),
                key: record
                    .as_ref()
                    .map(|r| r.key.clone())
                    .filter(|k| !k.is_empty()),
                hash,
            }
        })
        .collect();
    Gathered {
        sources,
        marks,
        colours,
        places,
    }
}

struct State {
    stem: String,
    def: Def,
    gathered: Option<Rc<Gathered>>,
    matrix: Matrix,
}

fn open_mark(widgets: &Rc<Widgets>, places: &Where, mark: &Mark) {
    let Some((path, _)) = places.get(&mark.hash) else {
        toast(
            widgets,
            "That document is no longer at its recorded location",
        );
        return;
    };
    let page = mark.label.parse::<u32>().ok();
    crate::ui::window::open_path_with_host(
        widgets,
        path.clone(),
        LaunchOptions {
            start_annotation: Some(mark.id.clone()),
            start_page: page,
            ..LaunchOptions::default()
        },
    );
}

fn cell_widget(
    widgets: &Rc<Widgets>,
    state: &Rc<RefCell<State>>,
    places: &Rc<Where>,
    hash: &str,
    source: &str,
    column: &str,
    cell: &matrix::Cell,
) -> gtk4::Box {
    let b = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    b.add_css_class("matrix-cell");
    if cell.is_empty() {
        b.add_css_class("matrix-empty");
    }
    for mark in cell.marks.iter().take(3) {
        let label = gtk4::Label::new(None);
        let loc = if mark.label.is_empty() {
            String::new()
        } else {
            format!("  p. {}", mark.label)
        };
        label.set_markup(&format!(
            "“{}”<span alpha=\"60%\">{}</span>",
            glib::markup_escape_text(&mark.text.split_whitespace().collect::<Vec<_>>().join(" ")),
            glib::markup_escape_text(&loc)
        ));
        label.set_wrap(true);
        label.set_lines(4);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_xalign(0.0);
        label.set_max_width_chars(34);
        label.add_css_class("caption");
        let button = gtk4::Button::new();
        button.add_css_class("flat");
        button.set_child(Some(&label));
        button.set_tooltip_text(Some("Open this passage"));
        let (widgets, places, mark) = (widgets.clone(), places.clone(), mark.clone());
        button.connect_clicked(move |_| open_mark(&widgets, &places, &mark));
        b.append(&button);
    }
    if cell.marks.len() > 3 {
        let more = gtk4::Label::new(Some(&format!("+{} more", cell.marks.len() - 3)));
        more.add_css_class("dim-label");
        more.add_css_class("caption");
        more.set_xalign(0.0);
        b.append(&more);
    }
    let note = gtk4::Entry::new();
    note.set_placeholder_text(Some("Note"));
    note.update_property(&[gtk4::accessible::Property::Label(&format!(
        "Your note on {source} for {column}"
    ))]);
    note.set_text(&cell.note);
    let key = matrix::note_key(hash, column);
    let state = state.clone();
    note.connect_changed(move |e| {
        let mut s = state.borrow_mut();
        let text = e.text().to_string();
        if text.trim().is_empty() {
            s.def.notes.remove(&key);
        } else {
            s.def.notes.insert(key.clone(), text);
        }
        matrix_store::save(&s.stem, &s.def);
    });
    b.append(&note);
    b
}

fn rebuild(
    widgets: &Rc<Widgets>,
    state: &Rc<RefCell<State>>,
    grid_holder: &gtk4::ScrolledWindow,
    status: &gtk4::Label,
) {
    let def = state.borrow().def.clone();
    status.set_text("Reading the notes…");
    let (widgets, state, holder, status) = (
        widgets.clone(),
        state.clone(),
        grid_holder.clone(),
        status.clone(),
    );
    let library = widgets.library.borrow().clone();
    glib::spawn_future_local(async move {
        let gathered = {
            let def = def.clone();
            gtk4::gio::spawn_blocking(move || gather(&library, &def))
                .await
                .ok()
        };
        let Some(gathered) = gathered else {
            status.set_text("The notes couldn't be read.");
            return;
        };
        let gathered = Rc::new(gathered);
        let live = state.borrow().def.clone();
        let m = matrix::build(&live, &gathered.sources, &gathered.marks);
        let grid = gtk4::Grid::new();
        grid.set_column_spacing(8);
        grid.set_row_spacing(8);
        grid.set_margin_top(12);
        grid.set_margin_bottom(12);
        grid.set_margin_start(12);
        grid.set_margin_end(12);
        for (i, name) in m.columns.iter().enumerate() {
            let l = gtk4::Label::new(Some(name));
            l.add_css_class("heading");
            l.set_xalign(0.0);
            grid.attach(&l, i as i32 + 1, 0, 1, 1);
        }
        let places = Rc::new(
            gathered
                .places
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Where>(),
        );
        for (r, row) in m.rows.iter().enumerate() {
            let who = gtk4::Label::new(Some(&row.source.title));
            who.set_wrap(true);
            who.set_lines(3);
            who.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            who.set_max_width_chars(24);
            who.set_xalign(0.0);
            who.set_yalign(0.0);
            who.add_css_class("heading");
            grid.attach(&who, 0, r as i32 + 1, 1, 1);
            for (c, cell) in row.cells.iter().enumerate() {
                let w = cell_widget(
                    &widgets,
                    &state,
                    &places,
                    &row.source.hash,
                    &row.source.title,
                    &m.columns[c],
                    cell,
                );
                w.set_hexpand(true);
                grid.attach(&w, c as i32 + 1, r as i32 + 1, 1, 1);
            }
        }
        let empty = m
            .rows
            .iter()
            .flat_map(|r| &r.cells)
            .filter(|c| c.is_empty())
            .count();
        status.set_text(&if m.columns.is_empty() {
            "Name your questions above (for example “Method; Date of Q”), then tag marks with them.".to_string()
        } else if m.rows.is_empty() {
            "No sources yet: choose a shelf, or mark some passages first.".to_string()
        } else {
            format!(
                "{} sources × {} questions · {empty} empty cell(s) — what a source says nothing about",
                m.rows.len(),
                m.columns.len()
            )
        });
        holder.set_child(Some(&grid));
        let mut s = state.borrow_mut();
        s.gathered = Some(gathered);
        s.matrix = m;
    });
}

fn export(
    widgets: &Rc<Widgets>,
    state: &Rc<RefCell<State>>,
    parent: &adw::Window,
    which: &'static str,
) {
    let (stem, title) = {
        let s = state.borrow();
        (s.stem.clone(), s.def.title.clone())
    };
    let ext = match which {
        "csv" => "csv",
        "markdown" => "md",
        _ => "typ",
    };
    let suffix = if which == "bibliography" {
        "-annotated-bibliography"
    } else {
        ""
    };
    let dialog = gtk4::FileDialog::builder()
        .title("Export")
        .initial_name(format!("{stem}{suffix}.{ext}"))
        .build();
    let (widgets, state) = (widgets.clone(), state.clone());
    dialog.save(Some(parent), gtk4::gio::Cancellable::NONE, move |result| {
        let Some(path) = result.ok().and_then(|f| f.path()) else {
            return;
        };
        let text = {
            let s = state.borrow();
            let hashes: Vec<String> = s
                .matrix
                .rows
                .iter()
                .map(|r| r.source.hash.clone())
                .collect();
            match which {
                "csv" => matrix::to_csv(&s.matrix),
                "markdown" => matrix::to_markdown(&s.matrix),
                "typst" => {
                    let yaml = crate::bib_store::yaml_for(&hashes);
                    let mut out = matrix::to_typst(&s.matrix, yaml.is_some());
                    if let (Some(yaml), Some(dir)) = (yaml, path.parent()) {
                        let name = format!(
                            "{}-refs.yml",
                            path.file_stem()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        );
                        if fond_read_gtk::fsutil::write_atomic(&dir.join(&name), yaml.as_bytes())
                            .is_ok()
                        {
                            out.push_str(&format!("\n#bibliography(\"{name}\")\n"));
                        }
                    }
                    out
                }
                _ => annotated(&s, &title),
            }
        };
        match fond_read_gtk::fsutil::write_atomic(&path, text.as_bytes()) {
            Ok(()) => toast(&widgets, &format!("Exported to {}", path.display())),
            Err(e) => toast(&widgets, &format!("Couldn't export: {e}")),
        }
    });
}

/// The annotated bibliography for the matrix's sources: each reference in the chosen style, then your
/// summary, then the passages you marked under their meanings.
fn annotated(s: &State, title: &str) -> String {
    let style = crate::config::Config::load().citation_style;
    let palette: Vec<&str> = fond_read_gtk::palette::HIGHLIGHT_COLORS
        .iter()
        .map(|c| c.hex)
        .collect();
    let palette = [palette[0], palette[1], palette[2], palette[3]];
    let labels: Vec<String> = (0..4)
        .map(fond_read_gtk::palette::highlight_label)
        .collect();
    let mut entries: Vec<BibEntry> = Vec::new();
    for row in &s.matrix.rows {
        let record = crate::bib_store::load(&row.source.hash).filter(|r| !r.title.is_empty());
        let reference = record
            .as_ref()
            .and_then(|r| crate::cite::reference(r, &style).ok())
            .unwrap_or_else(|| row.source.title.clone());
        let summary = record
            .as_ref()
            .map(|r| r.summary.clone())
            .unwrap_or_default();
        let marks: Vec<(Option<String>, PrecisMark)> = s
            .gathered
            .as_ref()
            .map(|g| {
                g.colours
                    .iter()
                    .filter(|(h, ..)| *h == row.source.hash)
                    .map(|(_, _, colour, mark)| (colour.clone(), mark.clone()))
                    .collect()
            })
            .unwrap_or_default();
        entries.push(BibEntry {
            reference,
            precis: precis::classify(&marks, palette, &summary),
        });
    }
    entries.sort_by_key(|e| e.reference.to_lowercase());
    let heading = if title.is_empty() {
        "Annotated bibliography"
    } else {
        title
    };
    precis::annotated_bibliography_typst(
        heading,
        [&labels[0], &labels[1], &labels[2], &labels[3]],
        &entries,
        true,
    )
}

pub fn show(widgets: &Rc<Widgets>) {
    fond_read_gtk::install_styles();
    let window = adw::Window::builder()
        .title("Synthesis matrix")
        .transient_for(&widgets.window)
        .default_width(1040)
        .default_height(680)
        .build();
    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let export_button = gtk4::MenuButton::new();
    export_button.set_label("Export");
    let pop = gtk4::Popover::new();
    let pop_rows = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    pop_rows.set_margin_top(6);
    pop_rows.set_margin_bottom(6);
    pop_rows.set_margin_start(6);
    pop_rows.set_margin_end(6);
    pop.set_child(Some(&pop_rows));
    export_button.set_popover(Some(&pop));
    header.pack_end(&export_button);
    view.add_top_bar(&header);

    let saved = matrix_store::list();
    let first = saved
        .first()
        .and_then(|(stem, _)| matrix_store::load(stem).map(|d| (stem.clone(), d)));
    let (stem, def) = first.unwrap_or_else(|| {
        (
            "matrix".to_string(),
            Def {
                title: "My matrix".into(),
                ..Default::default()
            },
        )
    });
    let state = Rc::new(RefCell::new(State {
        stem,
        def,
        gathered: None,
        matrix: Matrix::default(),
    }));

    let controls = adw::PreferencesGroup::new();
    controls.set_margin_top(12);
    controls.set_margin_start(12);
    controls.set_margin_end(12);
    let picker = adw::ComboRow::new();
    picker.set_title("Matrix");
    let name_row = adw::EntryRow::new();
    name_row.set_title("Name");
    name_row.set_text(&state.borrow().def.title);
    let shelf_row = adw::ComboRow::new();
    shelf_row.set_title("Sources");
    let shelves: Vec<String> = widgets.library.borrow().shelves().to_vec();
    let mut labels = vec!["Every document with notes".to_string()];
    labels.extend(shelves.iter().map(|s| format!("Shelf: {s}")));
    shelf_row.set_model(Some(&gtk4::StringList::new(
        &labels.iter().map(String::as_str).collect::<Vec<_>>(),
    )));
    shelf_row.set_selected(
        state
            .borrow()
            .def
            .shelf
            .as_ref()
            .and_then(|s| shelves.iter().position(|x| x == s))
            .map_or(0, |i| i as u32 + 1),
    );
    let columns_row = adw::EntryRow::new();
    columns_row.set_title("Questions: Name: tag, tag; Name …");
    columns_row.set_text(&matrix_store::columns_text(&state.borrow().def));
    columns_row.set_show_apply_button(true);
    controls.add(&picker);
    controls.add(&name_row);
    controls.add(&shelf_row);
    controls.add(&columns_row);

    let status = gtk4::Label::new(None);
    status.set_xalign(0.0);
    status.set_margin_start(14);
    status.set_margin_top(6);
    status.add_css_class("dim-label");
    status.add_css_class("caption");
    let holder = gtk4::ScrolledWindow::new();
    holder.set_vexpand(true);
    holder.set_hexpand(true);

    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    body.append(&controls);
    body.append(&status);
    body.append(&holder);
    view.set_content(Some(&body));
    window.set_content(Some(&view));

    let refresh = {
        let (widgets, state, holder, status) = (
            widgets.clone(),
            state.clone(),
            holder.clone(),
            status.clone(),
        );
        Rc::new(move || rebuild(&widgets, &state, &holder, &status))
    };
    // The picker lists the saved matrices and a way to start another; choosing one loads it into the form.
    let known: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let filling = Rc::new(std::cell::Cell::new(false));
    let fill_picker: Rc<dyn Fn()> = {
        let (picker, state, known, filling) = (
            picker.clone(),
            state.clone(),
            known.clone(),
            filling.clone(),
        );
        Rc::new(move || {
            filling.set(true);
            let mut saved = matrix_store::list();
            let current = state.borrow().stem.clone();
            if !saved.iter().any(|(s, _)| *s == current) {
                saved.insert(0, (current.clone(), state.borrow().def.title.clone()));
            }
            let mut names: Vec<String> = saved.iter().map(|(_, t)| t.clone()).collect();
            names.push("New matrix…".to_string());
            *known.borrow_mut() = saved.iter().map(|(s, _)| s.clone()).collect();
            picker.set_model(Some(&gtk4::StringList::new(
                &names.iter().map(String::as_str).collect::<Vec<_>>(),
            )));
            picker.set_selected(saved.iter().position(|(s, _)| *s == current).unwrap_or(0) as u32);
            filling.set(false);
        })
    };
    fill_picker();
    {
        let (state, known, filling, refresh, fill_picker) = (
            state.clone(),
            known.clone(),
            filling.clone(),
            refresh.clone(),
            fill_picker.clone(),
        );
        let (name_row, columns_row, shelf_row, shelves) = (
            name_row.clone(),
            columns_row.clone(),
            shelf_row.clone(),
            shelves.clone(),
        );
        picker.connect_selected_notify(move |p| {
            if filling.get() {
                return;
            }
            let at = p.selected() as usize;
            let stems = known.borrow().clone();
            let (stem, def) = match stems.get(at) {
                Some(stem) => match matrix_store::load(stem) {
                    Some(def) => (stem.clone(), def),
                    None => return,
                },
                None => {
                    let mut n = stems.len() + 1;
                    let (title, stem) = loop {
                        let title = format!("Matrix {n}");
                        let stem = matrix_store::stem_of(&title);
                        if !stems.contains(&stem) {
                            break (title, stem);
                        }
                        n += 1;
                    };
                    let def = Def {
                        title,
                        ..Default::default()
                    };
                    matrix_store::save(&stem, &def);
                    (stem, def)
                }
            };
            filling.set(true);
            name_row.set_text(&def.title);
            columns_row.set_text(&matrix_store::columns_text(&def));
            shelf_row.set_selected(
                def.shelf
                    .as_ref()
                    .and_then(|s| shelves.iter().position(|x| x == s))
                    .map_or(0, |i| i as u32 + 1),
            );
            filling.set(false);
            {
                let mut s = state.borrow_mut();
                s.stem = stem;
                s.def = def;
            }
            fill_picker();
            refresh();
        });
    }
    {
        let (state, refresh) = (state.clone(), refresh.clone());
        columns_row.connect_apply(move |row| {
            {
                let mut s = state.borrow_mut();
                s.def.columns = matrix_store::parse_columns(row.text().as_str());
                matrix_store::save(&s.stem, &s.def);
            }
            refresh();
        });
    }
    {
        let (state, refresh, shelves, filling) = (
            state.clone(),
            refresh.clone(),
            shelves.clone(),
            filling.clone(),
        );
        shelf_row.connect_selected_notify(move |row| {
            if filling.get() {
                return;
            }
            {
                let mut s = state.borrow_mut();
                s.def.shelf =
                    (row.selected() > 0).then(|| shelves[row.selected() as usize - 1].clone());
                matrix_store::save(&s.stem, &s.def);
            }
            refresh();
        });
    }
    {
        let (state, filling) = (state.clone(), filling.clone());
        name_row.connect_changed(move |row| {
            if filling.get() {
                return;
            }
            let mut s = state.borrow_mut();
            s.def.title = row.text().to_string();
            matrix_store::save(&s.stem, &s.def);
        });
    }
    for (label, which) in [
        ("Typst table", "typst"),
        ("Markdown table", "markdown"),
        ("CSV", "csv"),
        ("Annotated bibliography (Typst)", "bibliography"),
    ] {
        let b = gtk4::Button::with_label(label);
        b.add_css_class("flat");
        if let Some(l) = b.child().and_then(|c| c.downcast::<gtk4::Label>().ok()) {
            l.set_xalign(0.0);
        }
        let (widgets, state, window, pop) =
            (widgets.clone(), state.clone(), window.clone(), pop.clone());
        b.connect_clicked(move |_| {
            pop.popdown();
            export(&widgets, &state, &window, which);
        });
        pop_rows.append(&b);
    }
    refresh();
    window.present();
}
