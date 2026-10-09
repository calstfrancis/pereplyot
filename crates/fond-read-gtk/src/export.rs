//! The export dialog; the rendering itself is `fond_annot::export`.

use std::cell::Cell;

use gtk4::glib;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

pub use crate::clip::AreaClip;
pub use fond_annot::export::{cite_snippet, items_from_sidecar, Format, Item, Options};
use fond_annot::export::{render, ColourGroup};

use crate::palette::{self, HIGHLIGHT_COLORS};
use crate::ReaderHost;

/// The four reading colours with their current (possibly renamed) labels.
fn colour_groups() -> Vec<ColourGroup> {
    HIGHLIGHT_COLORS
        .iter()
        .enumerate()
        .map(|(i, c)| ColourGroup {
            hex: c.hex.to_string(),
            label: palette::highlight_label(i),
        })
        .collect()
}

/// What stands for the figures folder in an item's image path until the export is saved and the
/// folder's real name is known.
const FIGURES_DIR: &str = "pereplyot-figures-dir/";

/// Export items for a PDF, with each clipped area pointing at the picture the export will write.
pub fn items_with_figures(
    sidecar: &fond_annot::AnnotationSidecar,
    page_labels: &[Option<String>],
    hash: Option<&str>,
) -> Vec<Item> {
    fond_annot::export::items_with_links(
        sidecar,
        page_labels,
        &|_| None,
        &|id| Some(format!("{FIGURES_DIR}{id}.png")),
        &|id| {
            hash.map(|h| {
                crate::deeplink::build(&crate::deeplink::DeepLink {
                    hash: h.to_string(),
                    annotation: Some(id.to_string()),
                    page: None,
                })
            })
        },
    )
}

/// The clipped areas of a sidecar.
pub fn clips_of(sidecar: &fond_annot::AnnotationSidecar) -> Vec<AreaClip> {
    sidecar
        .annotations
        .iter()
        .filter_map(|a| {
            Some(AreaClip {
                id: a.id.clone(),
                page: a.page?,
                rect: a.rect()?,
            })
        })
        .collect()
}

/// A file name made only of letters, digits, `-` and `_`, so the figures folder's name needs no
/// escaping in any of the formats.
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
        "notes".to_string()
    } else {
        cleaned
    }
}

pub fn preferred_format() -> Format {
    Format::ALL[(LAST_FORMAT.with(|f| f.get()) as usize).min(Format::ALL.len() - 1)]
}

thread_local! {
    static LAST_FORMAT: Cell<u32> = const { Cell::new(0) };
    static LAST_GROUPED: Cell<bool> = const { Cell::new(true) };
}

/// Ask how to export (format, grouping, citation key), then ask where to save and write it.
/// Typst is the default format.
pub fn show_export_dialog(
    host: &Rc<dyn ReaderHost>,
    parent: &impl IsA<gtk4::Window>,
    title: &str,
    items: Vec<Item>,
    bookmarks: Vec<String>,
    pdf: Option<std::path::PathBuf>,
    clips: Vec<AreaClip>,
) {
    if items.is_empty() && bookmarks.is_empty() {
        host.notify("Nothing to export yet");
        return;
    }
    let dialog = adw::Window::new();
    dialog.set_title(Some("Export notes"));
    dialog.set_modal(true);
    dialog.set_transient_for(Some(parent));
    dialog.set_default_size(440, -1);

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    let go = gtk4::Button::with_label("Export…");
    go.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&go);
    view.add_top_bar(&header);

    let group = adw::PreferencesGroup::new();
    let labels: Vec<&str> = Format::ALL.iter().map(|f| f.label()).collect();
    let format_row = adw::ComboRow::new();
    format_row.set_title("Format");
    format_row.set_model(Some(&gtk4::StringList::new(&labels)));
    format_row.set_selected(LAST_FORMAT.with(|f| f.get()));
    let grouped_row = adw::SwitchRow::new();
    grouped_row.set_title("Group by colour meaning");
    grouped_row
        .set_subtitle("Key ideas, evidence, connections and problems under their own headings");
    grouped_row.set_active(LAST_GROUPED.with(|g| g.get()));
    let key_row = adw::EntryRow::new();
    key_row.set_title("Citation key (optional)");
    key_row.set_text(&host.citation_key().unwrap_or_default());
    group.add(&format_row);
    group.add(&grouped_row);
    group.add(&key_row);
    let hint = gtk4::Label::new(Some(
        "With a key, quotes cite it — @key[p. 42] in Typst, \\autocite in LaTeX, [@key, p. 42] in Markdown.",
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
    view.set_content(Some(&content));
    dialog.set_content(Some(&view));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let host = host.clone();
        let title = title.to_string();
        let dialog = dialog.clone();
        let parent = parent.clone().upcast::<gtk4::Window>();
        go.connect_clicked(move |_| {
            let format = Format::ALL[(format_row.selected() as usize).min(Format::ALL.len() - 1)];
            let key = key_row.text().trim().trim_start_matches('@').to_string();
            let key = (!key.is_empty()).then_some(key);
            LAST_FORMAT.with(|f| f.set(format_row.selected()));
            LAST_GROUPED.with(|g| g.set(grouped_row.is_active()));
            if key != host.citation_key() {
                host.set_citation_key(key.clone());
            }
            let options = Options {
                format,
                colour_groups: if grouped_row.is_active() {
                    colour_groups()
                } else {
                    Vec::new()
                },
                cite_key: key,
            };
            dialog.close();
            let file_dialog = gtk4::FileDialog::builder()
                .title("Export notes")
                .initial_name(format!("{title} — notes.{}", format.extension()))
                .build();
            let host = host.clone();
            let items = items.clone();
            let bookmarks = bookmarks.clone();
            let title = title.clone();
            let pdf = pdf.clone();
            let clips = clips.clone();
            file_dialog.save(Some(&parent), gtk4::gio::Cancellable::NONE, move |result| {
                let Some(path) = result.ok().and_then(|f| f.path()) else {
                    return;
                };
                let figures_name = format!("{}-figures", safe_stem(&path));
                let used: Vec<AreaClip> = clips
                    .iter()
                    .filter(|c| {
                        items.iter().any(|i| {
                            i.image.as_deref() == Some(&format!("{FIGURES_DIR}{}.png", c.id))
                        })
                    })
                    .cloned()
                    .collect();
                let text = render(&title, &items, &bookmarks, &options)
                    .replace(FIGURES_DIR, &format!("{figures_name}/"));
                let Some(pdf) = pdf.filter(|_| !used.is_empty()) else {
                    write_export(&host, &path, &text, 0, 0);
                    return;
                };
                let dir = path
                    .parent()
                    .map(|p| p.join(&figures_name))
                    .unwrap_or_else(|| figures_name.clone().into());
                glib::spawn_future_local(async move {
                    let mut written = 0usize;
                    for clip in &used {
                        let pdf = pdf.clone();
                        let (page, rect) = (clip.page, clip.rect);
                        let drawn = gtk4::gio::spawn_blocking(move || {
                            crate::clip::render(&pdf, page, rect)
                        })
                        .await
                        .ok()
                        .flatten();
                        if let Some(drawn) = drawn {
                            let out = dir.join(format!("{}.png", clip.id));
                            if crate::clip::save_png(&drawn, &out).is_ok() {
                                written += 1;
                            }
                        }
                    }
                    write_export(&host, &path, &text, written, used.len());
                });
            });
        });
    }
    dialog.present();
}

fn write_export(
    host: &Rc<dyn ReaderHost>,
    path: &std::path::Path,
    text: &str,
    figures_written: usize,
    figures_wanted: usize,
) {
    match crate::fsutil::write_atomic(path, text.as_bytes()) {
        Ok(()) if figures_written == figures_wanted => {
            host.notify(&format!("Exported to {}", path.display()))
        }
        Ok(()) => host.notify(&format!(
            "Exported to {}, but only {figures_written} of {figures_wanted} figures could be drawn",
            path.display()
        )),
        Err(e) => host.notify(&format!("Couldn't export: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_path(name: &str) -> Option<std::path::PathBuf> {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join(name))
                .find(|p| p.is_file())
        })
    }

    /// A clipped chart, exported as Typst with its picture beside it, compiles with the real
    /// `typst` (skipped when there is no PDFium or no `typst`).
    #[test]
    fn a_clipped_area_exports_to_typst_that_compiles() {
        let Some(typst) = on_path("typst") else {
            eprintln!("no typst; skipping");
            return;
        };
        if crate::pdfium::get().is_err() {
            eprintln!("no PDFium library; skipping");
            return;
        }
        let pdf = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/figures.pdf");
        let mut sidecar = fond_annot::AnnotationSidecar {
            schema: 1,
            key: "k".into(),
            pdf_hash: None,
            annotations: Vec::new(),
            extra: Default::default(),
        };
        let mut area = fond_annot::Annotation::area(
            1,
            [99.0, 559.0, 321.0, 661.0],
            Some("Readers per year".into()),
            Some("The #chart in question".into()),
            None,
        );
        area.id = "area-1".into();
        sidecar.annotations.push(area);
        let items = items_with_figures(&sidecar, &[], Some("abc"));
        let clips = clips_of(&sidecar);
        assert_eq!(clips.len(), 1);
        let dir = std::env::temp_dir().join(format!("pereplyot-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let drawn = crate::clip::render(&pdf, clips[0].page, clips[0].rect).expect("drawn");
        let bars = drawn
            .rgba
            .chunks_exact(4)
            .filter(|p| p[0] < 90 && (70..140).contains(&p[1]) && p[2] > 170)
            .count();
        assert!(
            bars > 2000,
            "the clip does not show the chart's bars ({bars})"
        );
        assert!(
            drawn.width > 800 && drawn.height > 350,
            "{}x{}",
            drawn.width,
            drawn.height
        );
        crate::clip::save_png(&drawn, &dir.join("my-notes-figures/area-1.png")).unwrap();
        let text = render(
            "Notes: a \"test\"",
            &items,
            &[],
            &Options {
                format: Format::Typst,
                colour_groups: Vec::new(),
                cite_key: None,
            },
        )
        .replace(FIGURES_DIR, "my-notes-figures/");
        assert!(
            text.contains("#figure(image(\"my-notes-figures/area-1.png\")"),
            "{text}"
        );
        std::fs::write(dir.join("my-notes.typ"), &text).unwrap();
        let out = std::process::Command::new(typst)
            .args(["compile", "my-notes.typ", "my-notes.pdf"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "typst failed: {}\n{text}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(dir.join("my-notes.pdf").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
