use super::*;
use crate::commands::{self, Command};

fn short(text: &str, n: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > n {
        format!("{}…", flat.chars().take(n).collect::<String>())
    } else {
        flat
    }
}

/// Open the Ctrl+K palette for this book.
pub(super) fn open(ui: &EpubUi) {
    let mut list: Vec<Command> = Vec::new();
    let toggle = |b: &gtk4::ToggleButton| {
        let b = b.clone();
        Rc::new(move || b.set_active(!b.is_active())) as Rc<dyn Fn()>
    };
    let click = |b: &gtk4::Button| {
        let b = b.clone();
        Rc::new(move || b.emit_clicked()) as Rc<dyn Fn()>
    };
    let mut add = |title: &str, hint: &str, run: Rc<dyn Fn()>| {
        list.push(Command {
            title: title.to_string(),
            hint: hint.to_string(),
            run,
        });
    };
    for p in crate::posture::Posture::ALL {
        let notes = ui.notes_toggle.clone();
        let notebook = ui.notebook_toggle.clone();
        let sidebar = ui.sidebar_toggle.clone();
        add(
            &format!("Posture: {}", p.name()),
            p.tooltip(),
            Rc::new(move || {
                let (n, b) = p.panes();
                if p == crate::posture::Posture::Read {
                    sidebar.set_active(false);
                }
                notes.set_active(n);
                notebook.set_active(b);
            }),
        );
    }
    if let Some(run) = ui.host.kartoteka_handoff() {
        add("Add to Kartoteka", "", run);
    }
    add("Contents", "", toggle(&ui.sidebar_toggle));
    add("Notes sidebar", "", toggle(&ui.notes_toggle));
    add("Notebook", "N", toggle(&ui.notebook_toggle));
    add(
        "Paginated mode (turn pages)",
        "View",
        toggle(&ui.paged_toggle),
    );
    add("Search in this book", "Ctrl+F", toggle(&ui.search_toggle));
    add("Previous chapter", "←", click(&ui.prev));
    add("Next chapter", "→", click(&ui.next));
    add("Bookmark this chapter", "B", click(&ui.bookmark_button));
    add("Larger text", "Ctrl++", click(&ui.zoom_in_button));
    add("Smaller text", "Ctrl+−", click(&ui.zoom_out_button));

    let go = {
        let reader = ui.reader.clone();
        let view = ui.web_view.clone();
        let prev = ui.prev.clone();
        let next = ui.next.clone();
        let chapter_label = ui.chapter_label.clone();
        let bookmark_button = ui.bookmark_button.clone();
        Rc::new(move |target: &str| {
            epub_go_to(
                &reader,
                &view,
                &prev,
                &next,
                &chapter_label,
                &bookmark_button,
                target,
            )
        })
    };
    for entry in &ui.toc {
        let go = go.clone();
        let target = entry.target.clone();
        add(
            &format!("Go to: {}", entry.label),
            "Contents",
            Rc::new(move || go(&target)),
        );
    }
    {
        let r = ui.reader.borrow();
        let mut notes: Vec<fond_annot::Annotation> = r
            .store
            .sidecar()
            .annotations
            .iter()
            .filter(|a| a.chapter.is_some())
            .cloned()
            .collect();
        notes.sort_by_key(|a| {
            (
                a.chapter
                    .as_deref()
                    .and_then(|c| r.spine.iter().position(|p| p == c))
                    .unwrap_or(usize::MAX),
                a.created.clone(),
            )
        });
        for a in notes {
            let place = match crate::page_label_of(&a) {
                Some(l) => format!("p. {l}"),
                None => a
                    .chapter
                    .as_deref()
                    .and_then(|c| r.spine.iter().position(|p| p == c))
                    .map_or(String::new(), |i| format!("ch. {}", i + 1)),
            };
            let title = a
                .snippet
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or(a.note.as_deref())
                .map_or_else(|| "(mark)".to_string(), |s| short(s, 70));
            let hash = ui.hash.clone();
            let id = a.id.clone();
            add(
                &format!("Note: {title}"),
                &place,
                Rc::new(move || {
                    crate::jump_in_open_reader(&hash, 0, Some(&id));
                }),
            );
        }
    }
    let dynamic: commands::Dynamic = {
        let reader = ui.reader.clone();
        let go = go.clone();
        Rc::new(move |query: &str| {
            let q = query.trim();
            if q.is_empty() {
                return Vec::new();
            }
            let r = reader.borrow();
            let Some(page) = r.pages.iter().find(|p| p.label.eq_ignore_ascii_case(q)) else {
                return Vec::new();
            };
            let target = format!("{}#{}", page.chapter, page.id);
            let go = go.clone();
            vec![Command::new(
                format!("Go to page {}", page.label),
                "Printed page",
                move || go(&target),
            )]
        })
    };
    commands::show(&ui.reader_tab.host_window, list, Some(dynamic));
}
