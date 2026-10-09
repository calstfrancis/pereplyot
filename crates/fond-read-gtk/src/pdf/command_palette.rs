use super::*;
use crate::commands::{self, Command};

fn clicked(button: &impl IsA<gtk4::Button>) -> impl Fn() + 'static {
    let button = button.clone().upcast::<gtk4::Button>();
    move || button.emit_clicked()
}

fn toggled(button: &impl IsA<gtk4::ToggleButton>) -> impl Fn() + 'static {
    let button = button.clone().upcast::<gtk4::ToggleButton>();
    move || button.set_active(!button.is_active())
}

fn short(text: &str, n: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > n {
        format!("{}…", flat.chars().take(n).collect::<String>())
    } else {
        flat
    }
}

/// Open the Ctrl+K palette for this reader.
pub(super) fn open(ui: &PdfUi) {
    let mut list: Vec<Command> = Vec::new();
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
    add(
        "Caret browsing on the page (select with the keyboard)",
        "F7",
        {
            let ui = ui.clone();
            Rc::new(move || caret::toggle(&ui))
        },
    );
    add("Reading mode", "T", Rc::new(toggled(&ui.text_toggle)));
    add("Notebook", "N", Rc::new(toggled(&ui.notebook_toggle)));
    add("Notes sidebar", "", Rc::new(toggled(&ui.notes_toggle)));
    add(
        "Contents and thumbnails",
        "",
        Rc::new(toggled(&ui.sidebar_toggle)),
    );
    add(
        "Continuous scroll",
        "View",
        Rc::new(toggled(&ui.continuous_toggle)),
    );
    add(
        "Two-page view",
        "View",
        Rc::new(toggled(&ui.two_page_toggle)),
    );
    add(
        "Split view: side by side",
        "View",
        Rc::new(clicked(&ui.split_side_button)),
    );
    add(
        "Split view: top and bottom",
        "View",
        Rc::new(clicked(&ui.split_stack_button)),
    );
    add("Rotate page", "View", Rc::new(clicked(&ui.rotate_button)));
    add(
        "Change page colours (dark, sepia, normal)",
        "View",
        Rc::new(toggled(&ui.invert_button)),
    );
    add("Fit width", "Zoom", Rc::new(clicked(&ui.zoom_fit_width)));
    add("Fit page", "Zoom", Rc::new(clicked(&ui.zoom_fit_page)));
    add("Zoom in", "Ctrl++", Rc::new(clicked(&ui.zoom_in)));
    add("Zoom out", "Ctrl+−", Rc::new(clicked(&ui.zoom_out)));
    add("Search in this document", "Ctrl+F", {
        let entry = ui.search_entry.clone();
        Rc::new(move || {
            entry.grab_focus();
        })
    });
    add(
        "Bookmark this page",
        "",
        Rc::new(clicked(&ui.bookmark_button)),
    );
    add(
        "Add a note on this page…",
        "",
        Rc::new(clicked(&ui.note_button)),
    );
    add(
        "Set page numbering…",
        "",
        Rc::new(clicked(&ui.page_num_button)),
    );
    add(
        "Back to where you followed a link from",
        "Alt+←",
        Rc::new(clicked(&ui.link_back)),
    );
    add("Forward again", "Alt+→", Rc::new(clicked(&ui.link_forward)));
    add("Export notes…", "", Rc::new(clicked(&ui.export_button)));
    add(
        "Save a copy with annotations…",
        "",
        Rc::new(clicked(&ui.copy_button)),
    );
    add(
        "Open in a new window",
        "",
        Rc::new(clicked(&ui.popout_button)),
    );
    for (i, colour) in crate::palette::HIGHLIGHT_COLORS.iter().enumerate() {
        let _ = colour;
        let ui = ui.clone();
        add(
            &format!("Mark the selection: {}", crate::palette::highlight_label(i)),
            &format!("{}", i + 1),
            Rc::new(move || {
                let mark = ui.quick_mark.borrow().clone();
                if let Some(mark) = mark {
                    mark(i);
                }
            }),
        );
    }

    for entry in ui.outline.borrow().iter() {
        let Some(page) = entry.page else { continue };
        let indent = "  ".repeat(entry.depth.min(4) as usize);
        let reader = ui.reader.clone();
        add(
            &format!("{indent}Go to: {}", entry.title),
            &format!("p. {page}"),
            Rc::new(move || {
                let target = page
                    .saturating_sub(1)
                    .min(reader.borrow().count.saturating_sub(1));
                jump(&reader, target, JumpKind::Outline);
            }),
        );
    }
    {
        let r = ui.reader.borrow();
        let mut notes: Vec<fond_annot::Annotation> = r
            .store
            .sidecar()
            .annotations
            .iter()
            .filter(|a| a.page.is_some())
            .cloned()
            .collect();
        notes.sort_by_key(|a| a.page);
        for a in notes {
            let page = a.page.unwrap_or(1);
            let label = r
                .page_labels
                .get((page as usize).saturating_sub(1))
                .cloned()
                .flatten()
                .unwrap_or_else(|| page.to_string());
            let title = a
                .snippet
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or(a.note.as_deref())
                .map_or_else(|| "(mark)".to_string(), |s| short(s, 70));
            let reader = ui.reader.clone();
            let id = a.id.clone();
            add(
                &format!("Note: {title}"),
                &format!("p. {label}"),
                Rc::new(move || {
                    let target = (page.saturating_sub(1))
                        .min(reader.borrow().count.saturating_sub(1) as u32)
                        as u16;
                    jump(&reader, target, JumpKind::Annotation);
                    mark_edit::flash(&reader, &id);
                }),
            );
        }
    }

    let dynamic: commands::Dynamic = {
        let reader = ui.reader.clone();
        let entry = ui.page_entry.clone();
        Rc::new(move |query: &str| {
            let q = query.trim();
            if q.is_empty() {
                return Vec::new();
            }
            let r = reader.borrow();
            let by_label = r
                .page_labels
                .iter()
                .position(|l| l.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(q)));
            let by_number = q
                .parse::<u32>()
                .ok()
                .filter(|n| *n >= 1 && *n as usize <= r.count as usize);
            if by_label.is_none() && by_number.is_none() {
                return Vec::new();
            }
            let entry = entry.clone();
            let text = q.to_string();
            vec![Command::new(format!("Go to page {q}"), "Page", move || {
                entry.set_text(&text);
                entry.emit_activate();
            })]
        })
    };
    commands::show(&ui.reader_window, list, Some(dynamic));
}
