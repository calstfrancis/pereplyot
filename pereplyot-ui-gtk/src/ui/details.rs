//! The editor for a document's bibliographic record: what it is, who wrote it, how to cite it.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use pereplyot_core::record::{split_authors, Kind, Record};

use crate::config::Config;

struct Form {
    kind: adw::ComboRow,
    title: adw::EntryRow,
    authors: adw::EntryRow,
    editors: adw::EntryRow,
    year: adw::EntryRow,
    container: adw::EntryRow,
    volume: adw::EntryRow,
    issue: adw::EntryRow,
    pages: adw::EntryRow,
    publisher: adw::EntryRow,
    place: adw::EntryRow,
    doi: adw::EntryRow,
    isbn: adw::EntryRow,
    key: adw::EntryRow,
}

fn entry(group: &adw::PreferencesGroup, title: &str, text: &str) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    row.set_text(text);
    group.add(&row);
    row
}

impl Form {
    fn write(&self, r: &Record) {
        self.kind
            .set_selected(Kind::ALL.iter().position(|k| *k == r.kind).unwrap_or(0) as u32);
        self.title.set_text(&r.title);
        self.authors.set_text(&r.authors.join("; "));
        self.editors.set_text(&r.editors.join("; "));
        self.year
            .set_text(&r.year.map(|y| y.to_string()).unwrap_or_default());
        self.container.set_text(&r.container);
        self.volume.set_text(&r.volume);
        self.issue.set_text(&r.issue);
        self.pages.set_text(&r.pages);
        self.publisher.set_text(&r.publisher);
        self.place.set_text(&r.place);
        self.doi.set_text(&r.doi);
        self.isbn.set_text(&r.isbn);
    }

    fn read(&self, base: &Record) -> Record {
        let text = |r: &adw::EntryRow| r.text().trim().to_string();
        let people = |r: &adw::EntryRow| split_authors(r.text().as_str());
        let mut record = base.clone();
        record.kind = Kind::ALL[(self.kind.selected() as usize).min(Kind::ALL.len() - 1)];
        record.title = text(&self.title);
        record.authors = people(&self.authors);
        record.editors = people(&self.editors);
        record.year = pereplyot_core::record::year_of(&text(&self.year));
        record.container = text(&self.container);
        record.volume = text(&self.volume);
        record.issue = text(&self.issue);
        record.pages = text(&self.pages);
        record.publisher = text(&self.publisher);
        record.place = text(&self.place);
        record.doi = text(&self.doi);
        record.isbn = text(&self.isbn);
        record.key = text(&self.key).trim_start_matches('@').to_string();
        record
    }
}

pub fn show(parent: &gtk4::Window, hash: &str) {
    let record = crate::bib_store::load(hash).unwrap_or_default();
    let window = adw::PreferencesWindow::builder()
        .title("Document details")
        .transient_for(parent)
        .modal(true)
        .default_width(560)
        .default_height(680)
        .search_enabled(false)
        .build();
    let page = adw::PreferencesPage::new();

    let about = adw::PreferencesGroup::new();
    about.set_title("What it is");
    let kind = adw::ComboRow::new();
    kind.set_title("Type");
    let labels: Vec<&str> = Kind::ALL.iter().map(|k| k.label()).collect();
    kind.set_model(Some(&gtk4::StringList::new(&labels)));
    kind.set_selected(
        Kind::ALL
            .iter()
            .position(|k| *k == record.kind)
            .unwrap_or(0) as u32,
    );
    about.add(&kind);
    let title = entry(&about, "Title", &record.title);
    let authors = entry(
        &about,
        "Authors (Family, Given; …)",
        &record.authors.join("; "),
    );
    let editors = entry(&about, "Editors", &record.editors.join("; "));
    let year = entry(
        &about,
        "Year",
        &record.year.map(|y| y.to_string()).unwrap_or_default(),
    );

    let part = adw::PreferencesGroup::new();
    part.set_title("Where it appeared");
    let container = entry(&part, "Journal or book it is in", &record.container);
    let volume = entry(&part, "Volume", &record.volume);
    let issue = entry(&part, "Issue", &record.issue);
    let pages = entry(&part, "Pages", &record.pages);
    let publisher = entry(&part, "Publisher", &record.publisher);
    let place = entry(&part, "Place", &record.place);

    let ids = adw::PreferencesGroup::new();
    ids.set_title("Identifiers");
    let doi = entry(&ids, "DOI", &record.doi);
    let isbn = entry(&ids, "ISBN", &record.isbn);
    let key = entry(&ids, "Citation key", &record.key);

    let style_group = adw::PreferencesGroup::new();
    style_group.set_title("How it reads");
    style_group.set_description(Some("Citation style applies to every document."));
    let styles = crate::cite::styles();
    let style_row = adw::ComboRow::new();
    style_row.set_title("Citation style");
    let names: Vec<&str> = styles.iter().map(|s| s.label.as_str()).collect();
    style_row.set_model(Some(&gtk4::StringList::new(&names)));
    let current = Config::load().citation_style;
    style_row.set_selected(styles.iter().position(|s| s.name == current).unwrap_or(0) as u32);
    style_group.add(&style_row);
    let preview = gtk4::Label::new(None);
    preview.set_wrap(true);
    preview.set_xalign(0.0);
    preview.set_selectable(true);
    preview.add_css_class("caption");
    preview.set_margin_top(6);
    style_group.add(&preview);

    let source_text = if record.sources.is_empty() {
        String::new()
    } else {
        record
            .sources
            .iter()
            .map(|(field, from)| format!("{field}: from {from}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let source_group = adw::PreferencesGroup::new();
    if !source_text.is_empty() {
        source_group.set_title("Where this came from");
        let l = gtk4::Label::new(Some(&source_text));
        l.set_xalign(0.0);
        l.add_css_class("caption");
        l.add_css_class("dim-label");
        source_group.add(&l);
    }

    let online = adw::PreferencesGroup::new();
    online.set_title("Look up online");
    online.set_description(Some(
        "Fills the empty fields from the DOI, arXiv id or ISBN above. Only that identifier is sent, to \
         CrossRef, arXiv or Open Library, and only when you press the button.",
    ));
    let lookup_row = adw::ActionRow::new();
    lookup_row.set_title("Look up");
    let lookup_button = gtk4::Button::with_label("Look up");
    lookup_button.set_valign(gtk4::Align::Center);
    lookup_row.add_suffix(&lookup_button);
    lookup_row.set_activatable_widget(Some(&lookup_button));
    online.add(&lookup_row);
    let offline = adw::SwitchRow::new();
    offline.set_title("Never go online");
    offline.set_subtitle("Hides this button");
    offline.set_active(Config::load().never_online);
    online.add(&offline);
    online.set_visible(true);
    lookup_row.set_visible(!offline.is_active());
    {
        let lookup_row = lookup_row.clone();
        offline.connect_active_notify(move |s| {
            let mut config = Config::load();
            config.never_online = s.is_active();
            config.save();
            lookup_row.set_visible(!s.is_active());
        });
    }

    let summary_group = adw::PreferencesGroup::new();
    summary_group.set_title("Your summary");
    summary_group.set_description(Some(
        "Two or three sentences on what it argues and why it matters. They head its entry in an annotated bibliography.",
    ));
    let summary_view = gtk4::TextView::new();
    summary_view.set_wrap_mode(gtk4::WrapMode::WordChar);
    summary_view.set_top_margin(8);
    summary_view.set_bottom_margin(8);
    summary_view.set_left_margin(10);
    summary_view.set_right_margin(10);
    summary_view.buffer().set_text(&record.summary);
    let summary_scroll = gtk4::ScrolledWindow::new();
    summary_scroll.set_min_content_height(96);
    summary_scroll.set_child(Some(&summary_view));
    summary_scroll.add_css_class("card");
    summary_group.add(&summary_scroll);

    page.add(&style_group);
    page.add(&about);
    page.add(&part);
    page.add(&ids);
    page.add(&summary_group);
    page.add(&online);
    if !source_text.is_empty() {
        page.add(&source_group);
    }
    window.add(&page);

    let form = Rc::new(Form {
        kind,
        title,
        authors,
        editors,
        year,
        container,
        volume,
        issue,
        pages,
        publisher,
        place,
        doi,
        isbn,
        key,
    });
    let base = Rc::new(RefCell::new(record));
    let styles = Rc::new(styles);

    let refresh = {
        let (form, base, styles, style_row, preview) = (
            form.clone(),
            base.clone(),
            styles.clone(),
            style_row.clone(),
            preview.clone(),
        );
        Rc::new(move || {
            let r = form.read(&base.borrow());
            let Some(style) = styles.get(style_row.selected() as usize) else {
                return;
            };
            if r.title.is_empty() {
                preview.set_text("Add a title to see how it will be cited.");
                return;
            }
            let note = crate::cite::footnote(&r, &style.name, Some("42"));
            let reference = crate::cite::reference(&r, &style.name);
            preview.set_text(&format!(
                "Footnote: {}\nReference: {}",
                note.unwrap_or_else(|e| format!("({e})")),
                reference.unwrap_or_else(|e| format!("({e})"))
            ));
        })
    };
    refresh();
    {
        let (form, base, refresh, lookup_button) = (
            form.clone(),
            base.clone(),
            refresh.clone(),
            lookup_button.clone(),
        );
        let window = window.clone();
        lookup_button.clone().connect_clicked(move |_| {
            let have = form.read(&base.borrow());
            lookup_button.set_sensitive(false);
            lookup_button.set_label("Looking up…");
            let (form, base, refresh, lookup_button, window) = (
                form.clone(),
                base.clone(),
                refresh.clone(),
                lookup_button.clone(),
                window.clone(),
            );
            glib::spawn_future_local(async move {
                let query = have.clone();
                let result = gio::spawn_blocking(move || crate::lookup::lookup(&query)).await;
                lookup_button.set_sensitive(true);
                lookup_button.set_label("Look up");
                match result {
                    Ok(Ok(found)) => {
                        let mut filled = have;
                        filled.fill_from(&found.record, found.service);
                        form.write(&filled);
                        *base.borrow_mut() = filled;
                        refresh();
                        window
                            .add_toast(adw::Toast::new(&format!("Filled from {}", found.service)));
                    }
                    Ok(Err(e)) => {
                        window.add_toast(adw::Toast::new(&format!("Couldn't look up: {e}")))
                    }
                    Err(_) => window.add_toast(adw::Toast::new("Couldn't look up")),
                }
            });
        });
    }
    for row in [
        &form.title,
        &form.authors,
        &form.editors,
        &form.year,
        &form.container,
        &form.volume,
        &form.issue,
        &form.pages,
        &form.publisher,
        &form.place,
    ] {
        let refresh = refresh.clone();
        row.connect_changed(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        form.kind.connect_selected_notify(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        style_row.connect_selected_notify(move |_| refresh());
    }

    let hash = hash.to_string();
    window.connect_close_request(move |_| {
        let mut record = form.read(&base.borrow());
        let buffer = summary_view.buffer();
        record.summary = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .to_string();
        if let Some(style) = styles.get(style_row.selected() as usize) {
            let mut config = Config::load();
            if config.citation_style != style.name {
                config.citation_style = style.name.clone();
                config.save();
            }
        }
        if record.key.is_empty() && !record.title.is_empty() {
            record.key = crate::bib_store::generate_key(&record, &hash);
        }
        // What the person typed is now theirs, not the file's or a service's.
        let before = base.borrow().clone();
        for (field, changed) in [
            ("title", record.title != before.title),
            ("authors", record.authors != before.authors),
            ("year", record.year != before.year),
        ] {
            if changed {
                record.sources.insert(field.into(), "your edit".into());
            }
        }
        if Some(&record) != crate::bib_store::load(&hash).as_ref() {
            crate::bib_store::save(&hash, &record);
        }
        glib::Propagation::Proceed
    });
    window.present();
}
