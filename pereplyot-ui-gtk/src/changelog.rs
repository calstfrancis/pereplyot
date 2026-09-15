use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

const CHANGELOG: &str = include_str!("../../CHANGELOG.md");
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A small window rendering `CHANGELOG.md` verbatim, so the in-app view can never drift
/// from the file itself. Plain text rather than Zerkalo's fuller Markdown-to-Pango
/// renderer — not worth building that parser again for one window in a single-purpose app.
pub fn show_changelog(parent: &impl IsA<gtk4::Window>) {
    let win = adw::Window::new();
    win.set_title(Some("Changelog"));
    win.set_default_width(640);
    win.set_default_height(600);
    win.set_transient_for(Some(parent));

    let header = adw::HeaderBar::new();
    let title = adw::WindowTitle::new("Changelog", &format!("You're on v{CURRENT_VERSION}"));
    header.set_title_widget(Some(&title));

    let text_view = gtk4::TextView::new();
    text_view.set_editable(false);
    text_view.set_cursor_visible(false);
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_margin_start(16);
    text_view.set_margin_end(16);
    text_view.set_margin_top(12);
    text_view.set_margin_bottom(12);
    text_view.buffer().set_text(CHANGELOG);

    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_child(Some(&text_view));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroller));
    win.set_content(Some(&toolbar));

    win.present();
}
