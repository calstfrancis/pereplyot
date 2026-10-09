use std::collections::HashMap;

use super::*;
use crate::notebook::{self, ExportOptions, Linked, Notebook};
use fond_annot::export::Format;

const FIGURES_DIR: &str = "pereplyot-figures-dir/";

/// Ask how to export the notebook, save it where the person says, and — for Zerkalo — open it
/// there. Every quote is cited; clipped areas are written as pictures beside the file.
pub(super) fn show(view: &Rc<NotebookView>, for_zerkalo: bool) {
    view.flush();
    let Some(nb) = view.snapshot() else { return };
    if nb.quotes().next().is_none() && nb.blocks.is_empty() {
        view.toast("Nothing to export yet", None);
        return;
    }
    let parent = view.widget().root().and_downcast::<gtk4::Window>();
    let dialog = adw::Window::new();
    dialog.set_title(Some(if for_zerkalo {
        "Open in Zerkalo"
    } else {
        "Export notebook"
    }));
    dialog.set_modal(true);
    dialog.set_default_size(460, -1);
    if let Some(p) = &parent {
        dialog.set_transient_for(Some(p));
    }
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    let go = gtk4::Button::with_label(if for_zerkalo {
        "Save and open…"
    } else {
        "Export…"
    });
    go.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&go);
    toolbar.add_top_bar(&header);

    let group = adw::PreferencesGroup::new();
    let labels: Vec<&str> = Format::ALL.iter().map(|f| f.label()).collect();
    let format_row = adw::ComboRow::new();
    format_row.set_title("Format");
    format_row.set_model(Some(&gtk4::StringList::new(&labels)));
    format_row.set_selected(0);
    format_row.set_visible(!for_zerkalo);
    let links_row = adw::SwitchRow::new();
    links_row.set_title("Link each quote back to Pereplyot");
    links_row.set_subtitle("Adds a ↗ that opens the passage, where the format can carry a link");
    links_row.set_active(true);
    let bib_row = adw::EntryRow::new();
    bib_row.set_title("Bibliography file (Typst; optional)");
    group.add(&format_row);
    group.add(&links_row);
    group.add(&bib_row);
    let hint = gtk4::Label::new(Some(
        "Quotes cite their document’s key when it has one (@key[p. 12] in Typst, \\autocite in \
         LaTeX, [@key, p. 12] in Markdown) and name the source in words when it doesn’t.",
    ));
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    hint.set_margin_start(12);
    hint.set_margin_end(12);
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(16);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&group);
    content.append(&hint);
    toolbar.set_content(Some(&content));
    dialog.set_content(Some(&toolbar));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let view = view.clone();
        go.connect_clicked(move |_| {
            let format = if for_zerkalo {
                Format::Typst
            } else {
                Format::ALL[(format_row.selected() as usize).min(Format::ALL.len() - 1)]
            };
            let bibliography = bib_row.text().trim().to_string();
            let links = links_row.is_active();
            dialog.close();
            save(&view, nb.clone(), format, links, bibliography, for_zerkalo);
        });
    }
    dialog.present();
}

struct Figure {
    id: String,
    pdf: PathBuf,
    page: u32,
    rect: [f64; 4],
}

fn save(
    view: &Rc<NotebookView>,
    nb: Notebook,
    format: Format,
    links: bool,
    bibliography: String,
    for_zerkalo: bool,
) {
    let parent = view.widget().root().and_downcast::<gtk4::Window>();
    let file_dialog = gtk4::FileDialog::builder()
        .title("Export notebook")
        .initial_name(format!("{}.{}", nb.stem, format.extension()))
        .build();
    let view = view.clone();
    file_dialog.save(
        parent.as_ref(),
        gtk4::gio::Cancellable::NONE,
        move |result| {
            let Some(path) = result.ok().and_then(|f| f.path()) else {
                return;
            };
            let figures_name = format!("{}-figures", safe_stem(&path));
            let mut figures: HashMap<String, Figure> = HashMap::new();
            for q in nb.quotes().filter(|q| q.area) {
                let rect = live_annotation(&q.hash, &q.id).and_then(|a| a.rect());
                let pdf = document_path(&q.hash);
                if let (Some(rect), Some(pdf)) = (rect, pdf) {
                    figures.insert(
                        q.id.clone(),
                        Figure {
                            id: q.id.clone(),
                            pdf,
                            page: q.page,
                            rect,
                        },
                    );
                }
            }
            let connections = crate::connections::read(|c| c.clone());
            let image_for = |q: &Quote| {
                figures
                    .contains_key(&q.id)
                    .then(|| format!("{FIGURES_DIR}{}.png", q.id))
            };
            let connections_of = |q: &Quote| -> Vec<Linked> {
                connections
                    .of(&q.hash, &q.id)
                    .into_iter()
                    .map(|(conn, other)| Linked {
                        note: conn.note.clone(),
                        text: other.text.split_whitespace().collect::<Vec<_>>().join(" "),
                        whence: other.whence(),
                    })
                    .collect()
            };
            let key_for = |q: &Quote| cite_key_for(&q.hash);
            let options = ExportOptions {
                format,
                bibliography: (!bibliography.is_empty()).then_some(bibliography.as_str()),
                key_for: &key_for,
                image_for: &image_for,
                connections_of: &connections_of,
                links,
            };
            let text =
                notebook::export(&nb, &options).replace(FIGURES_DIR, &format!("{figures_name}/"));
            let used: Vec<Figure> = figures
                .into_values()
                .filter(|f| text.contains(&format!("{figures_name}/{}.png", f.id)))
                .collect();
            let dir = path
                .parent()
                .map(|p| p.join(&figures_name))
                .unwrap_or_else(|| figures_name.clone().into());
            let view = view.clone();
            glib::spawn_future_local(async move {
                let mut written = 0usize;
                for f in &used {
                    let (pdf, page, rect) = (f.pdf.clone(), f.page, f.rect);
                    let drawn =
                        gtk4::gio::spawn_blocking(move || crate::clip::render(&pdf, page, rect))
                            .await
                            .ok()
                            .flatten();
                    if let Some(drawn) = drawn {
                        if crate::clip::save_png(&drawn, &dir.join(format!("{}.png", f.id))).is_ok()
                        {
                            written += 1;
                        }
                    }
                }
                match crate::fsutil::write_atomic(&path, text.as_bytes()) {
                    Ok(()) => {
                        let note = if written == used.len() {
                            format!("Exported to {}", path.display())
                        } else {
                            format!(
                                "Exported to {}, but only {written} of {} figures could be drawn",
                                path.display(),
                                used.len()
                            )
                        };
                        if for_zerkalo {
                            view.toast(&note, None);
                            open_in_zerkalo(&view, &path);
                        } else if format == Format::Typst {
                            let again = view.clone();
                            let path = path.clone();
                            view.toast(
                                &note,
                                Some((
                                    "Open in Zerkalo",
                                    Rc::new(move || open_in_zerkalo(&again, &path)),
                                )),
                            );
                        } else {
                            view.toast(&note, None);
                        }
                    }
                    Err(e) => view.toast(&format!("Couldn't export: {e}"), None),
                }
            });
        },
    );
}

fn safe_stem(path: &std::path::Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('-').to_string();
    if cleaned.is_empty() {
        "notebook".to_string()
    } else {
        cleaned
    }
}

/// Open `path` in Zerkalo if it is installed, otherwise ask which program should open it.
fn open_in_zerkalo(view: &Rc<NotebookView>, path: &std::path::Path) {
    use gtk4::gio::prelude::*;
    let file = gtk4::gio::File::for_path(path);
    let zerkalo = gtk4::gio::AppInfo::all().into_iter().find(|a| {
        a.id()
            .is_some_and(|id| id == "io.github.calstfrancis.Zerkalo.desktop")
    });
    if let Some(app) = zerkalo {
        if app
            .launch(
                std::slice::from_ref(&file),
                gtk4::gio::AppLaunchContext::NONE,
            )
            .is_ok()
        {
            return;
        }
    }
    let launcher = gtk4::FileLauncher::new(Some(&file));
    let parent = view.widget().root().and_downcast::<gtk4::Window>();
    let view = view.clone();
    launcher.launch(
        parent.as_ref(),
        gtk4::gio::Cancellable::NONE,
        move |result| {
            if let Err(e) = result {
                if !e.matches(gtk4::DialogError::Dismissed) {
                    view.toast(&format!("Couldn't open it: {e}"), None);
                }
            }
        },
    );
}
