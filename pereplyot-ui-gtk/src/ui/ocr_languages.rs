//! Which languages scanned pages are read in. English is bundled; others are added by the user
//! from a Tesseract language file they have downloaded themselves, and nothing is ever fetched
//! on their behalf.

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use fond_read_gtk::reflow::ocr;

pub fn show(parent: &impl IsA<gtk4::Window>) {
    let window = adw::Window::builder()
        .title("OCR languages")
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
    let group = adw::PreferencesGroup::new();
    group.set_title("Read scanned pages in");
    group.set_description(Some(
        "English is built in. To read another language, download its Tesseract language file \
         yourself (for example fra.traineddata from the tessdata_fast project on GitHub) and add \
         it here. Nothing is downloaded for you.",
    ));
    let english = adw::SwitchRow::new();
    english.set_title("English");
    english.set_subtitle("Built in");
    english.set_active(true);
    english.set_sensitive(false);
    group.add(&english);
    let chosen = ocr::chosen_languages();
    for code in ocr::added_languages() {
        let row = adw::SwitchRow::new();
        row.set_title(&code);
        row.set_subtitle("Added by you");
        row.set_active(chosen.contains(&code));
        let code = code.clone();
        row.connect_active_notify(move |r| {
            let mut now = ocr::chosen_languages();
            now.retain(|c| *c != code);
            if r.is_active() {
                now.push(code.clone());
            }
            ocr::choose_languages(&now);
        });
        group.add(&row);
    }
    page.append(&group);

    let add = gtk4::Button::with_label("Add a language file…");
    add.set_halign(gtk4::Align::Start);
    let status = gtk4::Label::new(None);
    status.add_css_class("dim-label");
    status.set_wrap(true);
    status.set_xalign(0.0);
    {
        let window = window.clone();
        let page = page.clone();
        add.connect_clicked(move |_| {
            let filter = gtk4::FileFilter::new();
            filter.add_pattern("*.traineddata");
            filter.set_name(Some("Tesseract language files"));
            let filters = gio::ListStore::new::<gtk4::FileFilter>();
            filters.append(&filter);
            let dialog = gtk4::FileDialog::builder()
                .title("Add a Tesseract language file")
                .filters(&filters)
                .build();
            let window = window.clone();
            let page = page.clone();
            let w = window.clone();
            dialog.open(Some(&w), gio::Cancellable::NONE, move |result| {
                let Some(path) = result.ok().and_then(|f| f.path()) else {
                    return;
                };
                match ocr::add_language_file(&path) {
                    Ok(code) => {
                        let mut now = ocr::chosen_languages();
                        if !now.contains(&code) {
                            now.push(code);
                        }
                        ocr::choose_languages(&now);
                        fill(&window, &page);
                    }
                    Err(e) => {
                        let dialog = adw::MessageDialog::new(
                            Some(&window),
                            Some("Couldn't add it"),
                            Some(&e),
                        );
                        dialog.add_response("ok", "OK");
                        dialog.present();
                    }
                }
            });
        });
    }
    page.append(&add);
    page.append(&status);
    let note = gtk4::Label::new(Some(
        "Pages already recognised stay as they were; open a scan and choose Recognise text to read it again in the languages chosen.",
    ));
    note.add_css_class("dim-label");
    note.add_css_class("caption");
    note.set_wrap(true);
    note.set_xalign(0.0);
    page.append(&note);
}
