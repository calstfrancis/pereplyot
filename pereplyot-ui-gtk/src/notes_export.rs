//! Export the notes chosen in the Notes tab: one section per document, each cited by its own key.

use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use fond_annot::export::{render, ColourGroup, Format, Item, Options};
use fond_read_gtk::palette::{self, HIGHLIGHT_COLORS};

use crate::notes_index::{self, Filter};
use crate::ui::{toast, Widgets};

/// The chosen annotations `(document hash, id)` as export text in `format`.
pub fn render_selection(
    widgets: &Rc<Widgets>,
    keys: &[(String, String)],
    format: Format,
    grouped: bool,
) -> String {
    let all = notes_index::search_with(&widgets.library.borrow(), &Filter::default());
    let mut out = String::new();
    let mut hashes: Vec<&str> = keys.iter().map(|k| k.0.as_str()).collect();
    hashes.sort_unstable();
    hashes.dedup();
    for hash in hashes {
        let mut hits: Vec<&notes_index::NoteHit> = all
            .iter()
            .filter(|h| {
                h.hash == hash && keys.iter().any(|k| k.0 == hash && k.1 == h.annotation_id)
            })
            .collect();
        hits.sort_by_key(|h| h.page);
        let Some(first) = hits.first() else { continue };
        let items: Vec<Item> = hits
            .iter()
            .map(|h| Item {
                locator: h.label.clone().unwrap_or_else(|| {
                    h.page.map_or_else(|| h.location.clone(), |p| p.to_string())
                }),
                is_chapter: h.page.is_none() && h.label.is_none(),
                kind: fond_annot::AnnotationKind::Highlight,
                color: h.color.clone(),
                quote: h.snippet.clone(),
                note: h.note.clone(),
                image: None,
                link: Some(fond_read_gtk::deeplink::build(
                    &fond_read_gtk::deeplink::DeepLink {
                        hash: h.hash.clone(),
                        annotation: Some(h.annotation_id.clone()),
                        page: None,
                    },
                )),
            })
            .collect();
        let options = Options {
            format,
            colour_groups: if grouped {
                HIGHLIGHT_COLORS
                    .iter()
                    .enumerate()
                    .map(|(i, c)| ColourGroup {
                        hex: c.hex.to_string(),
                        label: palette::highlight_label(i),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            cite_key: crate::reader_host::citation_key_of(hash),
        };
        out.push_str(&render(&first.doc_title, &items, &[], &options));
        out.push('\n');
    }
    out
}

pub fn show(widgets: &Rc<Widgets>, keys: Vec<(String, String)>) {
    if keys.is_empty() {
        return;
    }
    let dialog = adw::Window::new();
    dialog.set_title(Some("Export selected notes"));
    dialog.set_modal(true);
    dialog.set_transient_for(Some(&widgets.window));
    dialog.set_default_size(420, -1);
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    let go = gtk4::Button::with_label("Export…");
    go.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&go);
    toolbar.add_top_bar(&header);
    let group = adw::PreferencesGroup::new();
    let labels: Vec<&str> = Format::ALL.iter().map(|f| f.label()).collect();
    let format_row = adw::ComboRow::new();
    format_row.set_title("Format");
    format_row.set_model(Some(&gtk4::StringList::new(&labels)));
    let grouped = adw::SwitchRow::new();
    grouped.set_title("Group by colour meaning");
    grouped.set_active(true);
    group.add(&format_row);
    group.add(&grouped);
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(16);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&group);
    toolbar.set_content(Some(&content));
    dialog.set_content(Some(&toolbar));
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let widgets = widgets.clone();
        let dialog = dialog.clone();
        go.connect_clicked(move |_| {
            let format = Format::ALL[(format_row.selected() as usize).min(Format::ALL.len() - 1)];
            let text = render_selection(&widgets, &keys, format, grouped.is_active());
            dialog.close();
            let file_dialog = gtk4::FileDialog::builder()
                .title("Export selected notes")
                .initial_name(format!("notes.{}", format.extension()))
                .build();
            let widgets = widgets.clone();
            file_dialog.save(
                Some(&widgets.window.clone()),
                gio::Cancellable::NONE,
                move |result| {
                    let Some(path) = result.ok().and_then(|f| f.path()) else {
                        return;
                    };
                    match fond_read_gtk::fsutil::write_atomic(&path, text.as_bytes()) {
                        Ok(()) => toast(&widgets, &format!("Exported to {}", path.display())),
                        Err(e) => toast(&widgets, &format!("Couldn't export: {e}")),
                    }
                },
            );
        });
    }
    dialog.present();
}
