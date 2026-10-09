use super::*;
use crate::notebook;
use crate::{popover_button, popover_separator};

fn rows() -> gtk4::Box {
    let rows = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);
    rows.set_width_request(260);
    rows
}

fn heading(text: &str) -> gtk4::Label {
    let l = gtk4::Label::new(Some(text));
    l.set_xalign(0.0);
    l.add_css_class("dim-label");
    l.add_css_class("caption-heading");
    l.set_margin_top(4);
    l
}

/// The notebooks, by shelf, and a way to start another.
pub(super) fn fill_picker(view: &Rc<NotebookView>) {
    view.flush();
    let list = rows();
    let current = view.stem();
    let all = notebook::list(view.dir());
    let mut shelves: Vec<Option<String>> = Vec::new();
    for s in &all {
        if !shelves.contains(&s.shelf) {
            shelves.push(s.shelf.clone());
        }
    }
    shelves.sort_by(|a, b| match (a, b) {
        (Some(a), Some(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    for shelf in shelves {
        list.append(&heading(shelf.as_deref().unwrap_or("Stand-alone")));
        for s in all.iter().filter(|s| s.shelf == shelf) {
            let mark = if current.as_deref() == Some(&s.stem) {
                "● "
            } else {
                ""
            };
            let row = popover_button(
                &format!(
                    "{mark}{}  ({} quote{})",
                    s.title,
                    s.quotes,
                    if s.quotes == 1 { "" } else { "s" }
                ),
                false,
            );
            let view = view.clone();
            let stem = s.stem.clone();
            row.connect_clicked(move |_| {
                view.picker.popdown();
                view.open(&stem);
            });
            list.append(&row);
        }
    }
    if !all.is_empty() {
        list.append(&popover_separator());
    }
    let new = popover_button("New notebook…", false);
    {
        let view = view.clone();
        new.connect_clicked(move |_| {
            view.picker.popdown();
            let parent = view.widget().root().and_downcast::<gtk4::Window>();
            let view = view.clone();
            let shelves = hooks().shelves.map(|f| f()).unwrap_or_default();
            prompt_new(parent.as_ref(), shelves, move |title, shelf| {
                view.flush();
                if let Ok(nb) = notebook::create(view.dir(), &title, shelf) {
                    view.open(&nb.stem);
                }
            });
        });
    }
    list.append(&new);
    view.picker.set_child(Some(&list));
}

/// Rename, shelf, export and delete for the notebook that is open.
pub(super) fn fill_more(view: &Rc<NotebookView>) {
    let list = rows();
    let open = view.stem().is_some();
    let rename = popover_button("Rename…", false);
    let export = popover_button("Export…", false);
    let zerkalo = popover_button("Open in Zerkalo", false);
    let delete = popover_button("Delete this notebook…", true);
    for b in [&rename, &export, &zerkalo, &delete] {
        b.set_sensitive(open);
    }
    {
        let view = view.clone();
        rename.connect_clicked(move |_| {
            view.more.popdown();
            let parent = view.widget().root().and_downcast::<gtk4::Window>();
            let initial = view.title();
            let view = view.clone();
            prompt(
                parent.as_ref(),
                "Rename notebook",
                &initial,
                "Rename",
                false,
                move |t| view.rename(&t),
            );
        });
    }
    {
        let view = view.clone();
        export.connect_clicked(move |_| {
            view.more.popdown();
            export::show(&view, false);
        });
    }
    {
        let view = view.clone();
        zerkalo.connect_clicked(move |_| {
            view.more.popdown();
            export::show(&view, true);
        });
    }
    {
        let view = view.clone();
        delete.connect_clicked(move |_| {
            view.more.popdown();
            let Some(stem) = view.stem() else { return };
            let parent = view.widget().root().and_downcast::<gtk4::Window>();
            let dialog = adw::MessageDialog::new(
                parent.as_ref(),
                Some("Delete this notebook?"),
                Some(&format!(
                    "“{}” will be deleted. The annotations in it stay where they are.",
                    view.title()
                )),
            );
            dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_close_response("cancel");
            let view = view.clone();
            dialog.connect_response(None, move |_, id| {
                if id == "delete" {
                    super::delete_notebook(&stem);
                    view.ensure_open();
                }
            });
            dialog.present();
        });
    }
    list.append(&rename);

    list.append(&heading("Shelf"));
    let shelf_now = view.shelf();
    let mut names: Vec<Option<String>> = vec![None];
    if let Some(f) = hooks().shelves {
        names.extend(f().into_iter().map(Some));
    }
    for name in names {
        let selected = shelf_now == name;
        let label = name.as_deref().unwrap_or("None (stand-alone)");
        let row = popover_button(
            &format!("{}{label}", if selected { "● " } else { "" }),
            false,
        );
        row.set_sensitive(open);
        let view = view.clone();
        row.connect_clicked(move |_| {
            view.more.popdown();
            view.set_shelf(name.clone());
        });
        list.append(&row);
    }
    list.append(&popover_separator());
    list.append(&export);
    list.append(&zerkalo);
    list.append(&popover_separator());
    list.append(&delete);
    view.more.set_child(Some(&list));
}
