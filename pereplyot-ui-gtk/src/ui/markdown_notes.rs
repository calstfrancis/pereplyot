//! Keeping a Markdown file of each document's notes in a folder (an Obsidian or Logseq vault, say),
//! rewritten whenever the notes change. Off until a folder is chosen.

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::config::Config;

pub fn show(parent: &impl IsA<gtk4::Window>) {
    let window = adw::Window::builder()
        .title("Markdown notes")
        .transient_for(parent)
        .modal(true)
        .default_width(460)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    page.set_margin_top(12);
    page.set_margin_bottom(16);
    page.set_margin_start(12);
    page.set_margin_end(12);
    toolbar.set_content(Some(&page));
    window.set_content(Some(&toolbar));
    fill(&window, &page);
    window.present();
}

fn fill(window: &adw::Window, page: &gtk4::Box) {
    while let Some(c) = page.first_child() {
        page.remove(&c);
    }
    let folder = Config::load().markdown_folder;
    let intro = gtk4::Label::new(Some(
        "Pereplyot can keep one Markdown file per document in a folder of your choice, such as an \
         Obsidian or Logseq vault. Each highlight is a quote with a block id (^annot-…) and a link \
         that opens the passage here. The files are rewritten whenever the notes change, so edit \
         your notes in Pereplyot, not in the files.",
    ));
    intro.set_wrap(true);
    intro.set_xalign(0.0);
    page.append(&intro);
    let status = gtk4::Label::new(Some(&if folder.is_empty() {
        "Off".to_string()
    } else {
        format!("Writing to {folder}")
    }));
    status.set_xalign(0.0);
    status.add_css_class("dim-label");
    status.set_wrap(true);
    page.append(&status);

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let choose = gtk4::Button::with_label(if folder.is_empty() {
        "Choose a folder…"
    } else {
        "Change folder…"
    });
    {
        let window = window.clone();
        let page = page.clone();
        choose.connect_clicked(move |_| {
            let dialog = gtk4::FileDialog::builder()
                .title("Folder for Markdown notes")
                .build();
            let window = window.clone();
            let page = page.clone();
            dialog.select_folder(
                Some(&window.clone()),
                gtk4::gio::Cancellable::NONE,
                move |result| {
                    let Some(path) = result.ok().and_then(|f| f.path()) else {
                        return;
                    };
                    let mut config = Config::load();
                    config.markdown_folder = path.to_string_lossy().into_owned();
                    config.save();
                    crate::reader_host::write_all_markdown();
                    fill(&window, &page);
                },
            );
        });
    }
    buttons.append(&choose);
    if !folder.is_empty() {
        let off = gtk4::Button::with_label("Turn off");
        let window = window.clone();
        let page = page.clone();
        off.connect_clicked(move |_| {
            let mut config = Config::load();
            config.markdown_folder.clear();
            config.save();
            fill(&window, &page);
        });
        buttons.append(&off);
        let now = gtk4::Button::with_label("Write all now");
        let status = status.clone();
        now.connect_clicked(move |_| {
            let n = crate::reader_host::write_all_markdown();
            status.set_text(&format!("Wrote notes for {n} document(s)"));
        });
        buttons.append(&now);
    }
    page.append(&buttons);
}
