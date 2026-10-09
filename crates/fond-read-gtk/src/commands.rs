//! The Ctrl+K command palette: a search box over everything the window can do and everywhere it
//! can go — commands, headings, annotations, pages — in the house style of the other Fond apps.

use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

#[derive(Clone)]
pub struct Command {
    pub title: String,
    /// Where it is or what it does, shown dimly after the title ("Go to", "Ctrl+Z", "p. 42").
    pub hint: String,
    pub run: Rc<dyn Fn()>,
}

impl Command {
    pub fn new(
        title: impl Into<String>,
        hint: impl Into<String>,
        run: impl Fn() + 'static,
    ) -> Command {
        Command {
            title: title.into(),
            hint: hint.into(),
            run: Rc::new(run),
        }
    }
}

/// Commands that depend on what was typed (a page number to go to, say).
pub type Dynamic = Rc<dyn Fn(&str) -> Vec<Command>>;

const SHOWN: usize = 60;

/// The commands whose title contains every word of `query`, best (earliest, shortest) first.
/// An empty query keeps them all in the order given.
pub fn filter<'a>(commands: &'a [Command], query: &str) -> Vec<&'a Command> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut scored: Vec<(usize, usize, &Command)> = commands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let title = c.title.to_lowercase();
            let hint = c.hint.to_lowercase();
            if !words.iter().all(|w| title.contains(w) || hint.contains(w)) {
                return None;
            }
            let first = words
                .iter()
                .filter_map(|w| title.find(w))
                .min()
                .unwrap_or(usize::MAX / 2);
            Some((first, i, c))
        })
        .collect();
    if !words.is_empty() {
        scored.sort_by_key(|(first, i, c)| (*first, c.title.len(), *i));
    }
    scored.into_iter().map(|(_, _, c)| c).collect()
}

fn markup(title: &str, query: &str) -> String {
    let lower: Vec<char> = title.to_lowercase().chars().collect();
    let chars: Vec<char> = title.chars().collect();
    let mut bold = vec![false; chars.len()];
    if lower.len() == chars.len() {
        for w in query.split_whitespace().map(str::to_lowercase) {
            let w: Vec<char> = w.chars().collect();
            if w.is_empty() {
                continue;
            }
            let mut i = 0;
            while i + w.len() <= lower.len() {
                if lower[i..i + w.len()] == w[..] {
                    bold[i..i + w.len()].iter_mut().for_each(|b| *b = true);
                    i += w.len();
                } else {
                    i += 1;
                }
            }
        }
    }
    let mut out = String::new();
    let mut open = false;
    for (c, b) in chars.iter().zip(&bold) {
        if *b != open {
            out.push_str(if *b { "<b>" } else { "</b>" });
            open = *b;
        }
        out.push_str(&glib::markup_escape_text(&c.to_string()));
    }
    if open {
        out.push_str("</b>");
    }
    out
}

/// Show the palette over `parent`. Enter runs the highlighted command, Escape closes.
pub fn show(parent: &impl IsA<gtk4::Window>, commands: Vec<Command>, dynamic: Option<Dynamic>) {
    let window = adw::Window::new();
    window.set_modal(true);
    window.set_transient_for(Some(parent));
    window.set_default_size(560, 420);
    window.set_title(Some("Command palette"));
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let entry = gtk4::SearchEntry::new();
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some("Type a command, a heading, a page or a note"));
    header.set_title_widget(Some(&entry));
    toolbar.add_top_bar(&header);
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    list.add_css_class("navigation-sidebar");
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&list));
    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));

    let shown: Rc<std::cell::RefCell<Vec<Command>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
    let rebuild: Rc<dyn Fn()> = {
        let entry = entry.clone();
        let list = list.clone();
        let shown = shown.clone();
        Rc::new(move || {
            let query = entry.text().to_string();
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            let mut found: Vec<Command> = filter(&commands, &query).into_iter().cloned().collect();
            if let Some(d) = &dynamic {
                let mut extra = d(&query);
                extra.extend(found);
                found = extra;
            }
            found.truncate(SHOWN);
            for c in &found {
                let row = gtk4::ListBoxRow::new();
                let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
                line.set_margin_top(6);
                line.set_margin_bottom(6);
                line.set_margin_start(10);
                line.set_margin_end(10);
                let title = gtk4::Label::new(None);
                title.set_markup(&markup(&c.title, &query));
                title.set_xalign(0.0);
                title.set_hexpand(true);
                title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                line.append(&title);
                let hint = gtk4::Label::new(Some(&c.hint));
                hint.add_css_class("dim-label");
                hint.add_css_class("caption");
                line.append(&hint);
                row.set_child(Some(&line));
                row.update_property(&[gtk4::accessible::Property::Label(&format!(
                    "{}, {}",
                    c.title, c.hint
                ))]);
                list.append(&row);
            }
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
            }
            *shown.borrow_mut() = found;
        })
    };
    rebuild();
    {
        let rebuild = rebuild.clone();
        entry.connect_search_changed(move |_| rebuild());
    }
    let run_at: Rc<dyn Fn(i32)> = {
        let shown = shown.clone();
        let window = window.clone();
        Rc::new(move |i: i32| {
            let command = usize::try_from(i)
                .ok()
                .and_then(|i| shown.borrow().get(i).cloned());
            if let Some(c) = command {
                window.close();
                // After the palette has gone, so a command that opens its own dialog or moves
                // focus is not fighting it.
                glib::idle_add_local_once(move || (c.run)());
            }
        })
    };
    {
        let run_at = run_at.clone();
        list.connect_row_activated(move |_, row| run_at(row.index()));
    }
    {
        let list = list.clone();
        let run_at = run_at.clone();
        entry.connect_activate(move |_| {
            if let Some(row) = list.selected_row() {
                run_at(row.index());
            }
        });
    }
    {
        let list = list.clone();
        let closer = window.clone();
        let key = gtk4::EventControllerKey::new();
        key.set_propagation_phase(gtk4::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _, _| {
            let step = match keyval {
                gtk4::gdk::Key::Down => 1,
                gtk4::gdk::Key::Up => -1,
                gtk4::gdk::Key::Escape => {
                    closer.close();
                    return glib::Propagation::Stop;
                }
                _ => return glib::Propagation::Proceed,
            };
            let at = list.selected_row().map_or(-1, |r| r.index());
            if let Some(next) = list.row_at_index((at + step).max(0)) {
                list.select_row(Some(&next));
            }
            glib::Propagation::Stop
        });
        window.add_controller(key);
    }
    window.present();
    entry.grab_focus();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(title: &str, hint: &str) -> Command {
        Command::new(title, hint, || {})
    }

    #[test]
    fn every_word_must_match_and_earlier_matches_come_first() {
        let all = vec![
            c("Export notes…", "Notes"),
            c("Go to Chapter 3: Method", "Heading"),
            c("Toggle notebook", "N"),
            c("Notebook: add selection", ""),
        ];
        let titles = |q: &str| {
            filter(&all, q)
                .iter()
                .map(|c| c.title.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(titles("").len(), 4);
        assert_eq!(titles("note").len(), 3);
        assert_eq!(titles("go method"), vec!["Go to Chapter 3: Method"]);
        assert!(titles("zebra").is_empty());
        assert_eq!(titles("notebook")[0], "Notebook: add selection");
    }

    #[test]
    fn the_hint_is_searchable_and_matches_are_bolded() {
        let all = vec![c("Mark selection", "Ctrl+M")];
        assert_eq!(filter(&all, "ctrl").len(), 1);
        assert_eq!(markup("Mark selection", "sel"), "Mark <b>sel</b>ection");
        assert_eq!(markup("a < b", "b"), "a &lt; <b>b</b>");
    }
}
