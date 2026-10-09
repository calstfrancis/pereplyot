use std::cell::RefCell;

use super::*;
use crate::notebook::Quote;

/// The card standing for a quote in the text: its source and page, its words and the thought
/// that went with it, the links it has, and a way back to the passage.
pub(super) fn build(
    view: &Rc<NotebookView>,
    quote: Rc<RefCell<Quote>>,
    anchor: &gtk4::TextChildAnchor,
) -> (gtk4::Widget, u64) {
    let outer = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    outer.add_css_class("notebook-quote");
    outer.set_margin_top(4);
    outer.set_margin_bottom(4);
    let colour = quote.borrow().colour.clone();
    if let Some(stripe) = crate::color_swatch((!colour.is_empty()).then_some(colour.as_str())) {
        if let Some(area) = stripe.downcast_ref::<gtk4::DrawingArea>() {
            area.set_content_width(4);
            area.set_content_height(0);
        }
        stripe.set_valign(gtk4::Align::Fill);
        outer.append(&stripe);
    }
    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    body.set_hexpand(true);

    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    let whence = gtk4::Label::new(None);
    whence.set_xalign(0.0);
    whence.set_hexpand(true);
    whence.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    whence.add_css_class("caption-heading");
    whence.add_css_class("dim-label");
    whence.set_tooltip_text(Some("Open this passage"));
    head.append(&whence);
    let remove = gtk4::Button::from_icon_name("window-close-symbolic");
    remove.add_css_class("flat");
    remove.add_css_class("circular");
    remove.set_tooltip_text(Some("Take this quote out of the notebook"));
    head.append(&remove);
    body.append(&head);

    let words = gtk4::Label::new(None);
    words.set_xalign(0.0);
    words.set_wrap(true);
    words.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    words.set_width_chars(24);
    words.set_max_width_chars(120);
    words.set_selectable(true);
    body.append(&words);
    let note = gtk4::Label::new(None);
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.set_max_width_chars(120);
    note.add_css_class("dim-label");
    note.add_css_class("caption");
    body.append(&note);
    let links = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    body.append(&links);
    outer.append(&body);

    let fill = {
        let quote = quote.clone();
        let whence = whence.clone();
        let words = words.clone();
        let note = note.clone();
        let links = links.clone();
        move || {
            let q = quote.borrow();
            whence.set_text(&format!(
                "{}p. {}",
                if q.title.is_empty() {
                    String::new()
                } else {
                    format!("{} · ", q.title)
                },
                q.locator()
            ));
            if q.text.trim().is_empty() {
                words.set_text(if q.area { "Figure" } else { "(no text)" });
                words.add_css_class("dim-label");
            } else {
                words.set_text(&q.text);
                words.remove_css_class("dim-label");
            }
            note.set_text(&q.note);
            note.set_visible(!q.note.trim().is_empty());
            while let Some(child) = links.first_child() {
                links.remove(&child);
            }
            links.append(&connect::rows(&q.hash, &q.id));
        }
    };
    fill();
    let subscription = crate::connections::subscribe(Rc::new(fill));

    {
        let quote = quote.clone();
        crate::make_jump(&whence, move || {
            let q = quote.borrow();
            open_source(&q.hash, &q.id, q.page);
        });
    }
    {
        let view = Rc::downgrade(view);
        let anchor = anchor.clone();
        remove.connect_clicked(move |_| {
            if let Some(view) = view.upgrade() {
                view.remove_card(&anchor);
            }
        });
    }
    (outer.upcast(), subscription)
}
