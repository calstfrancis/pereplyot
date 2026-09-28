use std::cell::RefCell;
use std::rc::Rc;

use fond_read_gtk::palette::{self, HIGHLIGHT_COLORS};
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::config::Config;

pub fn show(parent: &adw::ApplicationWindow, config: Rc<RefCell<Config>>) {
    let window = adw::PreferencesWindow::builder()
        .title("Highlight labels")
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .default_height(520)
        .search_enabled(false)
        .build();

    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    group.set_title("Highlight colours");
    group.set_description(Some(
        "What each colour means, shown when you hover it in the reader's toolbar.",
    ));

    let rows: Rc<Vec<adw::EntryRow>> = Rc::new(
        HIGHLIGHT_COLORS
            .iter()
            .enumerate()
            .map(|(i, color)| {
                let row = adw::EntryRow::new();
                row.set_title(color.name);
                row.set_text(&palette::highlight_label(i));
                row.add_prefix(&palette::swatch(color.hex));
                group.add(&row);
                row
            })
            .collect(),
    );

    let save = {
        let rows = rows.clone();
        Rc::new(move || {
            let labels: Vec<String> = rows
                .iter()
                .zip(HIGHLIGHT_COLORS.iter())
                .map(|(row, color)| {
                    let text = row.text().trim().to_string();
                    if text == color.default_label {
                        String::new()
                    } else {
                        text
                    }
                })
                .collect();
            palette::set_highlight_labels(&labels);
            let mut config = config.borrow_mut();
            config.highlight_labels = labels;
            config.save();
        })
    };
    for row in rows.iter() {
        let save = save.clone();
        row.connect_changed(move |_| save());
    }

    let reset = gtk4::Button::with_label("Reset to defaults");
    reset.set_halign(gtk4::Align::Center);
    reset.set_margin_top(12);
    {
        let rows = rows.clone();
        reset.connect_clicked(move |_| {
            for (row, color) in rows.iter().zip(HIGHLIGHT_COLORS.iter()) {
                row.set_text(color.default_label);
            }
        });
    }

    page.add(&group);
    let reset_group = adw::PreferencesGroup::new();
    reset_group.add(&reset);
    page.add(&reset_group);
    window.add(&page);
    window.present();
}
