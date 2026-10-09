//! "Resurface": a few of your highlights from past reading on the Library page, a different few
//! each day, for revision. Off until chosen in the menu.

use std::rc::Rc;

use gtk4::prelude::*;

use crate::notes_index::{self, Filter, NoteHit};
use crate::ui::window::{open_path_with_host, LaunchOptions};
use crate::ui::{toast, Widgets};

const SHOWN: usize = 3;

/// `count` of `hits`, chosen by `seed` so the same few show all day and a different few tomorrow.
pub fn pick(hits: &[NoteHit], seed: u64, count: usize) -> Vec<&NoteHit> {
    let mut keyed: Vec<(u64, &NoteHit)> = hits
        .iter()
        .filter(|h| {
            h.snippet
                .as_deref()
                .is_some_and(|s| s.split_whitespace().count() >= 4)
        })
        .map(|h| {
            let mut x = seed ^ 0x9E37_79B9_7F4A_7C15;
            for b in h.annotation_id.bytes().chain(h.hash.bytes()) {
                x = (x ^ b as u64).wrapping_mul(0x100_0000_01B3);
            }
            (x, h)
        })
        .collect();
    keyed.sort_by_key(|(k, _)| *k);
    keyed.into_iter().take(count).map(|(_, h)| h).collect()
}

fn card(widgets: &Rc<Widgets>, hit: &NoteHit) -> gtk4::Widget {
    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    outer.add_css_class("card");
    outer.set_margin_start(2);
    outer.set_margin_end(2);
    let inner = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    inner.set_margin_top(8);
    inner.set_margin_bottom(8);
    inner.set_margin_start(10);
    inner.set_margin_end(10);
    let quote = gtk4::Label::new(hit.snippet.as_deref());
    quote.set_wrap(true);
    quote.set_xalign(0.0);
    quote.set_lines(4);
    quote.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    inner.append(&quote);
    let source = gtk4::Button::with_label(&format!("{} · {}", hit.doc_title, hit.location));
    source.add_css_class("flat");
    source.add_css_class("caption");
    source.set_halign(gtk4::Align::Start);
    source.set_tooltip_text(Some("Open this passage"));
    let widgets = widgets.clone();
    let (path, page, id) = (hit.path.clone(), hit.page, hit.annotation_id.clone());
    source.connect_clicked(move |_| match &path {
        Some(p) if p.is_file() => {
            open_path_with_host(
                &widgets,
                p.clone(),
                LaunchOptions {
                    start_page: page,
                    start_annotation: page.is_none().then(|| id.clone()),
                    ..LaunchOptions::default()
                },
            );
        }
        _ => toast(
            &widgets,
            "That document is no longer at its recorded location",
        ),
    });
    inner.append(&source);
    outer.append(&inner);
    outer.upcast()
}

/// Fill `strip` with today's highlights, or hide it.
pub fn refresh(widgets: &Rc<Widgets>, strip: &gtk4::Box) {
    while let Some(c) = strip.first_child() {
        strip.remove(&c);
    }
    if !widgets.config.borrow().resurface {
        strip.set_visible(false);
        return;
    }
    let library = widgets.library.borrow().clone();
    let widgets = widgets.clone();
    let strip = strip.clone();
    glib::spawn_future_local(async move {
        let hits =
            gio::spawn_blocking(move || notes_index::search_with(&library, &Filter::default()))
                .await
                .unwrap_or_default();
        let seed = glib::DateTime::now_local()
            .map(|d| (d.to_unix() / 86_400) as u64)
            .unwrap_or(0);
        let chosen = pick(&hits, seed, SHOWN);
        if chosen.is_empty() {
            strip.set_visible(false);
            return;
        }
        let heading = gtk4::Label::new(Some("From your reading"));
        heading.add_css_class("heading");
        heading.set_xalign(0.0);
        strip.append(&heading);
        let row = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        for h in chosen {
            row.append(&card(&widgets, h));
        }
        strip.append(&row);
        strip.set_visible(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str, text: &str) -> NoteHit {
        NoteHit {
            hash: "h".into(),
            area: false,
            label: None,
            tags: Vec::new(),
            path: None,
            doc_title: "T".into(),
            location: "p. 1".into(),
            page: Some(1),
            annotation_id: id.into(),
            color: None,
            snippet: Some(text.into()),
            note: None,
        }
    }

    #[test]
    fn the_same_few_show_all_day_and_different_ones_tomorrow() {
        let hits: Vec<NoteHit> = (0..30)
            .map(|i| hit(&format!("a{i}"), "a quote with enough words in it"))
            .collect();
        let ids = |seed| {
            pick(&hits, seed, 3)
                .iter()
                .map(|h| h.annotation_id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(7), ids(7));
        assert_eq!(ids(7).len(), 3);
        assert_ne!(ids(7), ids(8));
    }

    #[test]
    fn fragments_are_not_worth_revisiting() {
        let hits = vec![hit("a", "two words"), hit("b", "this one has enough words")];
        let got = pick(&hits, 1, 5);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].annotation_id, "b");
    }
}
