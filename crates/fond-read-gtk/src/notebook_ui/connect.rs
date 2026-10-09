//! Connecting two annotations: press Connect on one, then on the other. The links show on both
//! annotations' cards, in the Notes list and in the notebook.

use super::*;
use crate::connections::{self, End};

/// What pressing Connect on `end` does: starts a link, cancels it, or finishes one begun on
/// another annotation (asking how the two relate).
pub fn press(parent: Option<&gtk4::Window>, end: End, notify: Rc<dyn Fn(&str)>) {
    match connections::pending() {
        None => {
            connections::set_pending(Some(end));
            notify("Now press Connect on the annotation it relates to");
        }
        Some(first) if first.is(&end.hash, &end.id) => {
            connections::set_pending(None);
            notify("Connection cancelled");
        }
        Some(first) => {
            let parent = parent.cloned();
            super::prompt(
                parent.as_ref(),
                &format!("How does this relate to {}?", first.whence()),
                "",
                "Connect",
                true,
                move |note| {
                    connections::set_pending(None);
                    connections::edit(|c| {
                        c.connect(first.clone(), end.clone(), &note);
                    });
                    notify("Connected");
                },
            );
        }
    }
}

/// The Connect button for an annotation, which says what it will do and follows the pending link.
pub fn button(
    parent: Rc<dyn Fn() -> Option<gtk4::Window>>,
    end: End,
    notify: Rc<dyn Fn(&str)>,
) -> gtk4::Button {
    let pending_here = connections::pending().is_some_and(|p| p.is(&end.hash, &end.id));
    let pending_elsewhere = !pending_here && connections::pending().is_some();
    let button = gtk4::Button::with_label(if pending_here {
        "Cancel connecting"
    } else if pending_elsewhere {
        "Connect to the one I started"
    } else {
        "Connect…"
    });
    button.add_css_class("flat");
    button.add_css_class("caption");
    button.set_halign(gtk4::Align::Start);
    button.set_tooltip_text(Some(if pending_here {
        "Connecting from here — press Connect on the other annotation (press again to cancel)"
    } else if pending_elsewhere {
        "Connect to the annotation you started from"
    } else {
        "Connect this to another annotation, in any document"
    }));
    if pending_here {
        button.add_css_class("suggested-action");
    }
    button.connect_clicked(move |_| press(parent().as_ref(), end.clone(), notify.clone()));
    button
}

fn snippet(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 90 {
        format!("{}…", flat.chars().take(90).collect::<String>())
    } else {
        flat
    }
}

/// One row per connection of the annotation: the note, where it points, and the words there.
pub fn rows(hash: &str, id: &str) -> gtk4::Box {
    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    let mine: Vec<(String, End, String)> = connections::read(|c| {
        c.of(hash, id)
            .into_iter()
            .map(|(conn, other)| (conn.id.clone(), other.clone(), conn.note.clone()))
            .collect()
    });
    for (conn_id, other, note) in mine {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let label = gtk4::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        label.set_max_width_chars(120);
        label.add_css_class("caption");
        let mut markup = format!(
            "<span foreground=\"{}\">↔</span> ",
            crate::palette::HIGHLIGHT_COLORS[2].hex
        );
        if !note.is_empty() {
            markup.push_str(&format!("<i>{}</i> — ", glib::markup_escape_text(&note)));
        }
        markup.push_str(&format!(
            "<b>{}</b>",
            glib::markup_escape_text(&other.whence())
        ));
        if !other.text.trim().is_empty() {
            markup.push_str(&format!(
                ": {}",
                glib::markup_escape_text(&snippet(&other.text))
            ));
        }
        label.set_markup(&markup);
        label.set_tooltip_text(Some("Open the connected passage"));
        {
            let other = other.clone();
            crate::make_jump(&label, move || {
                open_source(&other.hash, &other.id, other.page)
            });
        }
        row.append(&label);
        let drop = gtk4::Button::from_icon_name("window-close-symbolic");
        drop.add_css_class("flat");
        drop.add_css_class("circular");
        drop.set_tooltip_text(Some("Remove this connection"));
        drop.connect_clicked(move |_| connections::edit(|c| c.remove(&conn_id)));
        row.append(&drop);
        list.append(&row);
    }
    list
}
