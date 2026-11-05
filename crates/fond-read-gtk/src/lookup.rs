//! "Look up…" on a selection: a menu of web searches the person set up (Wikipedia, Logeion, PhilPapers, a
//! library catalogue), each a URL with `{q}` where the selected words go. Choosing one opens the browser;
//! nothing is sent until then.

use std::rc::Rc;

use gtk4::prelude::*;

use crate::ReaderHost;

/// `template` with `{q}` replaced by the escaped words of `query`.
pub fn url_for(template: &str, query: &str) -> String {
    let words = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let escaped = glib::Uri::escape_string(&words, None, false);
    template.replace("{q}", &escaped)
}

/// Only web addresses are opened: a template is text from a config file.
pub fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Shows the menu of lookups for `query` beside `parent`.
pub fn show_menu(parent: &gtk4::Widget, host: &Rc<dyn ReaderHost>, query: &str) {
    let templates = host.lookup_templates();
    if templates.is_empty() {
        host.notify("No lookup sites are set up (see lookup_templates in the config)");
        return;
    }
    let popover = gtk4::Popover::new();
    let rows = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    for (name, template) in templates {
        let url = url_for(&template, query);
        if !is_web(&url) {
            continue;
        }
        let b = gtk4::Button::with_label(&format!("Look up in {name}"));
        b.add_css_class("flat");
        if let Some(l) = b.child().and_then(|c| c.downcast::<gtk4::Label>().ok()) {
            l.set_xalign(0.0);
        }
        let (popover, parent) = (popover.clone(), parent.clone());
        b.connect_clicked(move |_| {
            popover.popdown();
            let window = parent.root().and_downcast::<gtk4::Window>();
            gtk4::UriLauncher::new(&url).launch(
                window.as_ref(),
                gtk4::gio::Cancellable::NONE,
                |_| {},
            );
        });
        rows.append(&b);
    }
    popover.set_child(Some(&rows));
    popover.set_parent(parent);
    popover.connect_closed(move |p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_escaped_into_the_template() {
        assert_eq!(
            url_for(
                "https://en.wikipedia.org/wiki/Special:Search?search={q}",
                "Q  source & more"
            ),
            "https://en.wikipedia.org/wiki/Special:Search?search=Q%20source%20%26%20more"
        );
        assert_eq!(
            url_for("https://logeion.uchicago.edu/{q}", "λόγος"),
            "https://logeion.uchicago.edu/%CE%BB%CF%8C%CE%B3%CE%BF%CF%82"
        );
    }

    #[test]
    fn only_web_addresses_are_opened() {
        assert!(is_web("https://example.org/?q=1"));
        assert!(!is_web("file:///etc/passwd"));
        assert!(!is_web("javascript:alert(1)"));
        assert!(!is_web("ftp://example.org"));
    }
}
