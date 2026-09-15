use gtk4::prelude::GtkWindowExt;
use libadwaita as adw;

pub fn show_about(window: &adw::ApplicationWindow) {
    let about = gtk4::AboutDialog::builder()
        .program_name("Pereplyot")
        .version(env!("CARGO_PKG_VERSION"))
        .comments("Standalone PDF/EPUB reader with annotations — переплёт, \"binding\"")
        .website("https://github.com/calstfrancis/pereplyot")
        .transient_for(window)
        .modal(true)
        .build();
    about.present();
}
