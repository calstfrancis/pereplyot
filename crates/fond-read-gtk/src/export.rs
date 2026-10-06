//! The export dialog; the rendering itself is `fond_annot::export`.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

pub use fond_annot::export::{cite_snippet, items_from_sidecar, Format, Item, Options};
use fond_annot::export::{render, ColourGroup};

use crate::palette::{self, HIGHLIGHT_COLORS};
use crate::ReaderHost;

/// The four reading colours with their current (possibly renamed) labels.
fn colour_groups() -> Vec<ColourGroup> {
    HIGHLIGHT_COLORS
        .iter()
        .enumerate()
        .map(|(i, c)| ColourGroup {
            hex: c.hex.to_string(),
            label: palette::highlight_label(i),
        })
        .collect()
}

pub fn preferred_format() -> Format {
    Format::ALL[(LAST_FORMAT.with(|f| f.get()) as usize).min(Format::ALL.len() - 1)]
}

thread_local! {
    static LAST_FORMAT: Cell<u32> = const { Cell::new(0) };
    static LAST_GROUPED: Cell<bool> = const { Cell::new(true) };
}

/// Ask how to export (format, grouping, citation key), then ask where to save and write it.
/// Typst is the default format.
pub fn show_export_dialog(
    host: &Rc<dyn ReaderHost>,
    parent: &impl IsA<gtk4::Window>,
    title: &str,
    items: Vec<Item>,
    bookmarks: Vec<String>,
) {
    if items.is_empty() && bookmarks.is_empty() {
        host.notify("Nothing to export yet");
        return;
    }
    let dialog = adw::Window::new();
    dialog.set_title(Some("Export notes"));
    dialog.set_modal(true);
    dialog.set_transient_for(Some(parent));
    dialog.set_default_size(440, -1);

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    let go = gtk4::Button::with_label("Export…");
    go.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&go);
    view.add_top_bar(&header);

    let group = adw::PreferencesGroup::new();
    let labels: Vec<&str> = Format::ALL.iter().map(|f| f.label()).collect();
    let format_row = adw::ComboRow::new();
    format_row.set_title("Format");
    format_row.set_model(Some(&gtk4::StringList::new(&labels)));
    format_row.set_selected(LAST_FORMAT.with(|f| f.get()));
    let grouped_row = adw::SwitchRow::new();
    grouped_row.set_title("Group by colour meaning");
    grouped_row
        .set_subtitle("Key ideas, evidence, connections and problems under their own headings");
    grouped_row.set_active(LAST_GROUPED.with(|g| g.get()));
    let key_row = adw::EntryRow::new();
    key_row.set_title("Citation key (optional)");
    key_row.set_text(&host.citation_key().unwrap_or_default());
    group.add(&format_row);
    group.add(&grouped_row);
    group.add(&key_row);
    let hint = gtk4::Label::new(Some(
        "With a key, quotes cite it — @key[p. 42] in Typst, \\autocite in LaTeX, [@key, p. 42] in Markdown.",
    ));
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    hint.set_margin_start(12);
    hint.set_margin_end(12);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(16);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&group);
    content.append(&hint);
    view.set_content(Some(&content));
    dialog.set_content(Some(&view));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let host = host.clone();
        let title = title.to_string();
        let dialog = dialog.clone();
        let parent = parent.clone().upcast::<gtk4::Window>();
        go.connect_clicked(move |_| {
            let format = Format::ALL[(format_row.selected() as usize).min(Format::ALL.len() - 1)];
            let key = key_row.text().trim().trim_start_matches('@').to_string();
            let key = (!key.is_empty()).then_some(key);
            LAST_FORMAT.with(|f| f.set(format_row.selected()));
            LAST_GROUPED.with(|g| g.set(grouped_row.is_active()));
            if key != host.citation_key() {
                host.set_citation_key(key.clone());
            }
            let text = render(
                &title,
                &items,
                &bookmarks,
                &Options {
                    format,
                    colour_groups: if grouped_row.is_active() {
                        colour_groups()
                    } else {
                        Vec::new()
                    },
                    cite_key: key,
                },
            );
            dialog.close();
            let file_dialog = gtk4::FileDialog::builder()
                .title("Export notes")
                .initial_name(format!("{title} — notes.{}", format.extension()))
                .build();
            let host = host.clone();
            file_dialog.save(Some(&parent), gtk4::gio::Cancellable::NONE, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        match crate::fsutil::write_atomic(&path, text.as_bytes()) {
                            Ok(()) => host.notify(&format!("Exported to {}", path.display())),
                            Err(e) => host.notify(&format!("Couldn't export: {e}")),
                        }
                    }
                }
            });
        });
    }
    dialog.present();
}
