//! The hamburger popover: hand-built rows, not a `gio::Menu` — house style
//! (root `CLAUDE.md`'s UI design standard), matching Kartoteka/Zerkalo's own hamburgers
//! rather than the earlier `gio::Menu` shortcut this app started with.

use std::cell::RefCell;
use std::rc::Rc;

use glib::variant::ToVariant;
use gtk4::prelude::*;

use crate::ui::Widgets;

/// A flat, left-aligned popover row — same shape as every other Fond app's helper of the
/// same name.
fn popover_button(label: &str) -> gtk4::Button {
    let button = gtk4::Button::new();
    button.add_css_class("flat");
    let lbl = gtk4::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_halign(gtk4::Align::Start);
    button.set_child(Some(&lbl));
    button
}

fn popover_separator() -> gtk4::Separator {
    let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    sep
}

/// A row that activates a `win.*` action and closes the popover — the same
/// `activate_row` shape Kartoteka's hamburger uses.
fn activate_row(
    rows: &gtk4::Box,
    popover: &gtk4::Popover,
    label: &str,
    action: &str,
) -> gtk4::Button {
    let row = popover_button(label);
    let popover = popover.clone();
    let action = action.to_string();
    row.connect_clicked(move |b| {
        popover.popdown();
        let _ = b.activate_action(&action, None);
    });
    rows.append(&row);
    row
}

pub fn build(widgets: &Rc<Widgets>) -> gtk4::Popover {
    let rows = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);
    rows.set_width_request(200);

    let popover = gtk4::Popover::new();
    popover.set_child(Some(&rows));

    activate_row(&rows, &popover, "Open…", "win.open");
    rows.append(&popover_separator());

    // Theme: three inline rows, not a submenu — state shown by the same
    // `.fond-toggle-active` bold marker Kartoteka's own theme rows use, kept in sync here
    // rather than by rebuilding the popover on every show.
    let current = widgets.config.borrow().theme.clone();
    let theme_buttons: Rc<RefCell<Vec<(String, gtk4::Button)>>> = Rc::new(RefCell::new(Vec::new()));
    for (label, name) in [("System", "system"), ("Light", "light"), ("Dark", "dark")] {
        let row = popover_button(label);
        if name == current {
            row.add_css_class("fond-toggle-active");
        }
        rows.append(&row);
        theme_buttons.borrow_mut().push((name.to_string(), row));
    }
    for (name, row) in theme_buttons.borrow().iter() {
        let popover = popover.clone();
        let name = name.clone();
        let all = theme_buttons.clone();
        row.connect_clicked(move |b| {
            popover.popdown();
            let _ = b.activate_action("win.theme", Some(&name.to_variant()));
            for (n, btn) in all.borrow().iter() {
                if *n == name {
                    btn.add_css_class("fond-toggle-active");
                } else {
                    btn.remove_css_class("fond-toggle-active");
                }
            }
        });
    }
    rows.append(&popover_separator());

    activate_row(&rows, &popover, "About Pereplyot", "win.about");

    popover
}
