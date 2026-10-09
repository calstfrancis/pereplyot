//! Welcome (first run) and What's New (after an update). `RELEASE_NAME` is the release's name
//! in `CHANGELOG.md`, the metainfo and the commit — updated at release time with the bullet list.

use gtk4::prelude::*;
use gtk4::Orientation;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::ui::Widgets;
use std::rc::Rc;

const VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const RELEASE_NAME: &str = "Gilt Spine";

/// Show the window if this version has not been seen: the welcome the first time ever, What's
/// New after an update. Called when the launcher is actually shown.
pub fn maybe_show(widgets: &Rc<Widgets>) {
    if std::env::var_os("PEREPLYOT_NO_WELCOME").is_some() {
        return;
    }
    let seen = widgets.config.borrow().last_seen_version.clone();
    if seen == VERSION {
        return;
    }
    {
        let mut c = widgets.config.borrow_mut();
        c.last_seen_version = VERSION.to_string();
        c.save();
    }
    show(&widgets.window, seen.is_empty());
}

pub fn show(parent: &impl IsA<gtk4::Window>, first_run: bool) {
    let window = adw::Window::builder()
        .title(if first_run {
            "Welcome to Pereplyot"
        } else {
            "What's New"
        })
        .transient_for(parent)
        .modal(true)
        .default_width(480)
        .default_height(560)
        .build();
    let body = gtk4::Box::new(Orientation::Vertical, 12);
    body.set_margin_start(24);
    body.set_margin_end(24);
    body.set_margin_top(20);
    body.set_margin_bottom(20);
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(450);
    clamp.set_child(Some(&body));
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_child(Some(&clamp));

    let title = gtk4::Label::new(Some("Pereplyot"));
    title.add_css_class("title-1");
    let sub = gtk4::Label::new(Some(&format!("Version {VERSION} “{RELEASE_NAME}”")));
    sub.add_css_class("dim-label");
    body.append(&title);
    body.append(&sub);
    body.append(&gtk4::Separator::new(Orientation::Horizontal));

    if first_run {
        body.append(&section("What Pereplyot is"));
        let about = gtk4::Label::new(Some(
            "A reader for people who read to write. Open a PDF or EPUB, mark it in four colours \
             that mean something, keep a Notebook beside it that cites every quote, and search \
             everything you have read. Your notes live in plain files, keyed by the document.",
        ));
        style(&about);
        body.append(&about);
        body.append(&section("Try this"));
        for (headline, detail) in TRY {
            body.append(&bullet(headline, detail));
        }
    } else {
        body.append(&section("What's new"));
        for (headline, detail) in NEW {
            body.append(&bullet(headline, detail));
        }
    }
    let close = gtk4::Button::with_label("Get started");
    close.add_css_class("suggested-action");
    close.set_halign(gtk4::Align::Center);
    close.set_margin_top(12);
    body.append(&close);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));
    {
        let window = window.clone();
        close.connect_clicked(move |_| window.close());
    }
    window.present();
}

/// Updated at release time, with `RELEASE_NAME`.
const NEW: &[(&str, &str)] = &[
    (
        "A Notebook beside your reading",
        "Press N, drag highlights in, click a quote to open its passage, connect annotations, and export to Typst, Markdown or LaTeX with citations.",
    ),
    (
        "Search everything you've read",
        "The new Search tab (Ctrl+F) searches the text of every document and all your notes, with filters; the Notes tab can now select, tag and recolour many at once.",
    ),
    (
        "EPUBs for scholars",
        "Printed page numbers, note popovers, a paginated mode and clipped images.",
    ),
    (
        "Reading mode in Zerkalo's LaTeX Look",
        "A PDF re-set as clean text in New Computer Modern, with page numbers in the left margin and footnotes in the right.",
    ),
    (
        "Ctrl+K and caret browsing",
        "A command palette for everything, and F7 to select and mark on the page from the keyboard alone.",
    ),
];

const TRY: &[(&str, &str)] = &[
    (
        "Select, then mark",
        "Drag over text and press 1–4, or pick a colour from the popover.",
    ),
    (
        "Press N",
        "Open the Notebook and drag a highlight from the Notes list into it.",
    ),
    (
        "Press Ctrl+K",
        "Run any command, or jump to a heading, a note or a page.",
    ),
    (
        "Add documents to the Library",
        "Then Search finds any passage in any of them.",
    ),
];

fn section(text: &str) -> gtk4::Label {
    let l = gtk4::Label::new(Some(text));
    l.add_css_class("heading");
    l.set_xalign(0.0);
    l.set_margin_top(8);
    l
}

fn style(l: &gtk4::Label) {
    l.set_wrap(true);
    l.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    l.set_xalign(0.0);
    l.set_hexpand(true);
}

fn bullet(headline: &str, detail: &str) -> gtk4::Box {
    let row = gtk4::Box::new(Orientation::Horizontal, 8);
    let dot = gtk4::Label::new(Some("•"));
    dot.add_css_class("dim-label");
    dot.set_valign(gtk4::Align::Start);
    row.append(&dot);
    let col = gtk4::Box::new(Orientation::Vertical, 2);
    let h = gtk4::Label::new(None);
    h.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(headline)));
    style(&h);
    col.append(&h);
    let d = gtk4::Label::new(Some(detail));
    d.add_css_class("dim-label");
    style(&d);
    col.append(&d);
    row.append(&col);
    row
}
