//! Notes export: Markdown, Typst and LaTeX, optionally grouped by what each highlight colour
//! *means* (key idea / evidence / connection / problem) and cited against a bibliography key.

use std::cell::Cell;
use std::rc::Rc;

use fond_annot::AnnotationKind;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::ReaderHost;

use crate::palette::{self, HIGHLIGHT_COLORS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Typst,
    Latex,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Typst, Format::Markdown, Format::Latex];

    pub fn label(self) -> &'static str {
        match self {
            Format::Markdown => "Markdown (Pandoc citations)",
            Format::Typst => "Typst",
            Format::Latex => "LaTeX (biblatex)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Typst => "typ",
            Format::Latex => "tex",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    /// Printed page ("42", "xii") for a PDF, or a chapter ("3") for an EPUB.
    pub locator: String,
    pub is_chapter: bool,
    pub kind: AnnotationKind,
    pub color: Option<String>,
    pub quote: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub format: Format,
    pub group_by_colour: bool,
    /// Bibliography key to cite with; without one, the locator is shown as plain text.
    pub cite_key: Option<String>,
}

impl Item {
    fn locator_text(&self) -> String {
        if self.is_chapter {
            format!("ch. {}", self.locator)
        } else {
            format!("p. {}", self.locator)
        }
    }

    fn kind_tag(&self) -> Option<&'static str> {
        match self.kind {
            AnnotationKind::Underline => Some("underlined"),
            AnnotationKind::Strikeout => Some("struck out"),
            AnnotationKind::Note => Some("note"),
            AnnotationKind::Highlight => None,
        }
    }
}

/// `(heading, items)` groups in palette order; ungrouped output is one headingless group.
fn groups(items: &[Item], by_colour: bool) -> Vec<(Option<String>, Vec<&Item>)> {
    if !by_colour {
        return vec![(None, items.iter().collect())];
    }
    let mut out: Vec<(Option<String>, Vec<&Item>)> = Vec::new();
    for (i, color) in HIGHLIGHT_COLORS.iter().enumerate() {
        let hits: Vec<&Item> = items
            .iter()
            .filter(|it| {
                it.color
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(color.hex))
            })
            .collect();
        if !hits.is_empty() {
            out.push((Some(palette::highlight_label(i)), hits));
        }
    }
    let rest: Vec<&Item> = items
        .iter()
        .filter(|it| {
            !it.color.as_deref().is_some_and(|c| {
                HIGHLIGHT_COLORS
                    .iter()
                    .any(|k| k.hex.eq_ignore_ascii_case(c))
            })
        })
        .collect();
    if !rest.is_empty() {
        out.push((Some("Other".to_string()), rest));
    }
    out
}

pub fn render(title: &str, items: &[Item], bookmarks: &[String], opts: &Options) -> String {
    match opts.format {
        Format::Markdown => markdown(title, items, bookmarks, opts),
        Format::Typst => typst(title, items, bookmarks, opts),
        Format::Latex => latex(title, items, bookmarks, opts),
    }
}

// ---------------------------------------------------------------- Markdown

fn markdown(title: &str, items: &[Item], bookmarks: &[String], opts: &Options) -> String {
    let mut out = format!("# {title}\n\n");
    if !bookmarks.is_empty() {
        out.push_str("## Bookmarks\n\n");
        for b in bookmarks {
            out.push_str(&format!("- {b}\n"));
        }
        out.push('\n');
    }
    for (heading, group) in groups(items, opts.group_by_colour) {
        match heading {
            Some(h) => out.push_str(&format!("## {h}\n\n")),
            None if !items.is_empty() => out.push_str("## Notes & highlights\n\n"),
            None => {}
        }
        for it in group {
            if let Some(q) = it.quote.as_deref().filter(|q| !q.trim().is_empty()) {
                for line in q.lines() {
                    out.push_str(&format!("> {line}\n"));
                }
                let cite = match &opts.cite_key {
                    Some(k) => format!("[@{k}, {}]", it.locator_text()),
                    None => format!("({})", it.locator_text()),
                };
                out.push_str(&format!(">\n> — {cite}"));
                if let Some(tag) = it.kind_tag() {
                    out.push_str(&format!(" *({tag})*"));
                }
                out.push_str("\n\n");
            } else {
                out.push_str(&format!("**{}**\n\n", it.locator_text()));
            }
            if let Some(n) = it.note.as_deref().filter(|n| !n.trim().is_empty()) {
                out.push_str(&format!("{n}\n\n"));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ Typst

/// Escape text for Typst *markup* mode so it renders literally.
pub fn typst_escape(s: &str) -> String {
    s.split('\n')
        .map(typst_escape_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn typst_escape_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len() + 4);
    let lead = chars.iter().take_while(|c| c.is_whitespace()).count();
    out.extend(&chars[..lead]);
    let mut i = lead;
    // A line-leading `=`, `+`, `-` or `1.` would otherwise start a heading or list.
    match chars.get(i) {
        Some(&c @ ('=' | '+' | '-')) => {
            out.push('\\');
            out.push(c);
            i += 1;
        }
        Some(c) if c.is_ascii_digit() => {
            let digits = chars[i..].iter().take_while(|d| d.is_ascii_digit()).count();
            if chars.get(i + digits) == Some(&'.') {
                out.extend(&chars[i..i + digits]);
                out.push_str("\\.");
                i += digits + 1;
            }
        }
        _ => {}
    }
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' | '#' | '$' | '*' | '_' | '`' | '~' | '@' | '<' | '>' | '[' | ']' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '/' if matches!(chars.get(i + 1), Some('/') | Some('*')) => out.push_str("\\/"),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

fn typst_paragraphs(s: &str) -> String {
    s.split("\n\n")
        .map(|p| typst_escape(p.trim()))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn typst_attribution(it: &Item, cite_key: &Option<String>) -> String {
    match cite_key {
        Some(k) => format!("@{k}[{}]", it.locator_text()),
        None => typst_escape(&it.locator_text()),
    }
}

fn typst(title: &str, items: &[Item], bookmarks: &[String], opts: &Options) -> String {
    let mut out = format!("= {}\n\n", typst_escape(title));
    if !bookmarks.is_empty() {
        out.push_str("== Bookmarks\n\n");
        for b in bookmarks {
            out.push_str(&format!("- {}\n", typst_escape(b)));
        }
        out.push('\n');
    }
    for (heading, group) in groups(items, opts.group_by_colour) {
        match heading {
            Some(h) => out.push_str(&format!("== {}\n\n", typst_escape(&h))),
            None if !items.is_empty() => out.push_str("== Notes and highlights\n\n"),
            None => {}
        }
        for it in group {
            let attribution = typst_attribution(it, &opts.cite_key);
            let tag = it
                .kind_tag()
                .map(|t| format!(" _({t})_"))
                .unwrap_or_default();
            if let Some(q) = it.quote.as_deref().filter(|q| !q.trim().is_empty()) {
                out.push_str(&format!(
                    "#quote(block: true, attribution: [{attribution}{tag}])[{}]\n\n",
                    typst_paragraphs(q)
                ));
            } else {
                out.push_str(&format!("*{}*{tag}\n\n", typst_escape(&it.locator_text())));
            }
            if let Some(n) = it.note.as_deref().filter(|n| !n.trim().is_empty()) {
                out.push_str(&format!("{}\n\n", typst_paragraphs(n)));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ LaTeX

pub fn latex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' | '}' | '$' | '&' | '#' | '_' | '%' => {
                out.push('\\');
                out.push(c);
            }
            '^' => out.push_str("\\textasciicircum{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            _ => out.push(c),
        }
    }
    out
}

fn latex(title: &str, items: &[Item], bookmarks: &[String], opts: &Options) -> String {
    let mut out = String::new();
    out.push_str("\\section*{");
    out.push_str(&latex_escape(title));
    out.push_str("}\n\n");
    if !bookmarks.is_empty() {
        out.push_str("\\subsection*{Bookmarks}\n\\begin{itemize}\n");
        for b in bookmarks {
            out.push_str(&format!("  \\item {}\n", latex_escape(b)));
        }
        out.push_str("\\end{itemize}\n\n");
    }
    for (heading, group) in groups(items, opts.group_by_colour) {
        match heading {
            Some(h) => out.push_str(&format!("\\subsection*{{{}}}\n\n", latex_escape(&h))),
            None if !items.is_empty() => out.push_str("\\subsection*{Notes and highlights}\n\n"),
            None => {}
        }
        for it in group {
            let cite = match &opts.cite_key {
                Some(k) => {
                    let sup = if it.is_chapter { "ch.~" } else { "p.~" };
                    format!("\\autocite[{sup}{}]{{{k}}}", latex_escape(&it.locator))
                }
                None => format!("({})", latex_escape(&it.locator_text())),
            };
            let tag = it
                .kind_tag()
                .map(|t| format!(" \\emph{{({t})}}"))
                .unwrap_or_default();
            if let Some(q) = it.quote.as_deref().filter(|q| !q.trim().is_empty()) {
                out.push_str(&format!(
                    "\\begin{{quote}}\n{}\n{cite}{tag}\n\\end{{quote}}\n\n",
                    latex_escape(q.trim())
                ));
            } else {
                out.push_str(&format!(
                    "\\textbf{{{}}}{tag}\n\n",
                    latex_escape(&it.locator_text())
                ));
            }
            if let Some(n) = it.note.as_deref().filter(|n| !n.trim().is_empty()) {
                out.push_str(&format!("{}\n\n", latex_escape(n.trim())));
            }
        }
    }
    out
}

/// Annotations of a sidecar as export items, in reading order. PDF annotations are located by
/// printed page label (falling back to the file page); EPUB ones by `chapter_number` (spine
/// position), falling back to the chapter's file name.
pub fn items_from_sidecar(
    sidecar: &fond_annot::AnnotationSidecar,
    page_labels: &[Option<String>],
    chapter_number: &dyn Fn(&str) -> Option<usize>,
) -> Vec<Item> {
    type SortKey = (u8, usize, Option<String>);
    let mut keyed: Vec<(SortKey, Item)> = sidecar
        .annotations
        .iter()
        .map(|a| {
            let (order, locator, is_chapter) = match (a.page, a.chapter.as_deref()) {
                (Some(p), _) => {
                    let label = page_labels
                        .get((p as usize).saturating_sub(1))
                        .and_then(|l| l.clone())
                        .unwrap_or_else(|| p.to_string());
                    ((0u8, p as usize), label, false)
                }
                (None, Some(c)) => {
                    let n = chapter_number(c);
                    let label = n.map(|n| n.to_string()).unwrap_or_else(|| {
                        std::path::Path::new(c)
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_else(|| c.to_string())
                    });
                    ((1u8, n.unwrap_or(usize::MAX)), label, true)
                }
                (None, None) => ((2u8, 0), String::from("?"), false),
            };
            (
                (order.0, order.1, a.created.clone()),
                Item {
                    locator,
                    is_chapter,
                    kind: a.kind,
                    color: a.color.clone(),
                    quote: a.snippet.clone(),
                    note: a.note.clone(),
                },
            )
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.into_iter().map(|(_, i)| i).collect()
}

/// A quotation plus its citation, ready to paste into a document in `format`.
pub fn cite_snippet(
    format: Format,
    quote: &str,
    locator: &str,
    is_chapter: bool,
    cite_key: Option<&str>,
) -> String {
    let item = Item {
        locator: locator.to_string(),
        is_chapter,
        kind: AnnotationKind::Highlight,
        color: None,
        quote: None,
        note: None,
    };
    let quote = quote.split_whitespace().collect::<Vec<_>>().join(" ");
    match (format, cite_key) {
        (Format::Typst, Some(k)) => format!(
            "#quote(block: true, attribution: [@{k}[{}]])[{}]",
            item.locator_text(),
            typst_escape(&quote)
        ),
        (Format::Typst, None) => format!(
            "#quote(block: true, attribution: [{}])[{}]",
            typst_escape(&item.locator_text()),
            typst_escape(&quote)
        ),
        (Format::Latex, Some(k)) => format!(
            "\\enquote{{{}}}\\autocite[{}{}]{{{k}}}",
            latex_escape(&quote),
            if is_chapter { "ch.~" } else { "p.~" },
            latex_escape(locator)
        ),
        (Format::Latex, None) => format!(
            "\\enquote{{{}}} ({})",
            latex_escape(&quote),
            latex_escape(&item.locator_text())
        ),
        (Format::Markdown, Some(k)) => {
            format!("\"{quote}\" [@{k}, {}]", item.locator_text())
        }
        (Format::Markdown, None) => format!("\"{quote}\" ({})", item.locator_text()),
    }
}

/// The format last chosen in the export dialog (Typst until the user picks another).
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
            let text = render(
                &title,
                &items,
                &bookmarks,
                &Options {
                    format,
                    group_by_colour: grouped_row.is_active(),
                    cite_key: key,
                },
            );
            dialog.close();
            let file_dialog = gtk4::FileDialog::builder()
                .title("Export notes")
                .initial_name(format!("{title} — notes.{}", format.extension()))
                .build();
            let host = host.clone();
            file_dialog.save(Some(&parent), gtk4::gio::Cancellable::NONE, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        match crate::fsutil::write_atomic(&path, text.as_bytes()) {
                            Ok(()) => host.notify(&format!("Exported to {}", path.display())),
                            Err(e) => host.notify(&format!("Couldn't export: {e}")),
                        }
                    }
                }
            });
        });
    }
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(page: &str, color: &str, quote: &str, note: Option<&str>) -> Item {
        Item {
            locator: page.into(),
            is_chapter: false,
            kind: AnnotationKind::Highlight,
            color: Some(color.into()),
            quote: Some(quote.into()),
            note: note.map(str::to_string),
        }
    }

    fn opts(format: Format, group: bool, key: Option<&str>) -> Options {
        Options {
            format,
            group_by_colour: group,
            cite_key: key.map(str::to_string),
        }
    }

    #[test]
    fn typst_escapes_markup() {
        assert_eq!(typst_escape("a #b $c [d] @e"), "a \\#b \\$c \\[d\\] \\@e");
        assert_eq!(typst_escape("= not a heading"), "\\= not a heading");
        assert_eq!(typst_escape("3. not a list"), "3\\. not a list");
        assert_eq!(typst_escape("see http://x"), "see http:\\//x");
        assert_eq!(typst_escape("line\n- item"), "line\n\\- item");
    }

    #[test]
    fn typst_cites_with_supplement() {
        let it = item(
            "42",
            "#D6B86A",
            "Being is becoming.",
            Some("Whitehead's claim"),
        );
        let out = render(
            "Process",
            &[it],
            &[],
            &opts(Format::Typst, false, Some("whitehead1929")),
        );
        assert!(out.contains("attribution: [@whitehead1929[p. 42]]"));
        assert!(out.contains("Whitehead's claim"));
    }

    #[test]
    fn grouping_follows_palette_order_and_uses_labels() {
        let a = item("1", "#C98F78", "problem text", None);
        let b = item("2", "#D6B86A", "idea text", None);
        let c = item("3", "#123456", "other text", None);
        let out = render("T", &[a, b, c], &[], &opts(Format::Markdown, true, None));
        let idea = out.find("Key idea").unwrap();
        let problem = out.find("Problem").unwrap();
        let other = out.find("## Other").unwrap();
        assert!(idea < problem && problem < other);
    }

    #[test]
    fn latex_escapes_and_cites() {
        let it = item("7", "#D6B86A", "50% of $x_i$ & more", None);
        let out = render("T_1", &[it], &[], &opts(Format::Latex, false, Some("k")));
        assert!(out.contains("50\\% of \\$x\\_i\\$ \\& more"));
        assert!(out.contains("\\autocite[p.~7]{k}"));
        assert!(out.contains("\\section*{T\\_1}"));
    }

    #[test]
    fn cite_snippets() {
        let t = cite_snippet(Format::Typst, "a  b\nc #", "42", false, Some("k"));
        assert_eq!(
            t,
            "#quote(block: true, attribution: [@k[p. 42]])[a b c \\#]"
        );
        let l = cite_snippet(Format::Latex, "x", "3", true, Some("k"));
        assert_eq!(l, "\\enquote{x}\\autocite[ch.~3]{k}");
        assert_eq!(
            cite_snippet(Format::Markdown, "x", "7", false, None),
            "\"x\" (p. 7)"
        );
    }

    #[test]
    fn markdown_uses_pandoc_citation() {
        let it = item("5", "#D6B86A", "q", None);
        let out = render(
            "T",
            &[it],
            &[],
            &opts(Format::Markdown, false, Some("smith2020")),
        );
        assert!(out.contains("[@smith2020, p. 5]"));
    }

    #[test]
    fn dump_samples_for_external_compilers() {
        let Ok(dir) = std::env::var("EXPORT_DUMP_DIR") else {
            return;
        };
        let nasty = "Line one with #hash, $money$, *stars*, _under_, [brackets], @at, `tick`, a // slash, <angle>.\n= fake heading\n- fake item\n3. fake numbered\nAnd \"quotes\" -- dashes... ~tilde";
        let items = vec![
            item(
                "12",
                "#D6B86A",
                nasty,
                Some("My note: 50% of it & more.\n\nSecond paragraph #1"),
            ),
            item("3", "#C98F78", "Plain problem.", None),
            Item {
                kind: AnnotationKind::Underline,
                ..item("xii", "#91A9B8", "evidence", None)
            },
            Item {
                is_chapter: true,
                ..item("4", "#9CAF88", "connection", Some("see ch"))
            },
        ];
        for (fmt, ext) in [
            (Format::Typst, "typ"),
            (Format::Latex, "tex"),
            (Format::Markdown, "md"),
        ] {
            for (name, key) in [("cited", Some("whitehead1929")), ("plain", None)] {
                let out = render(
                    "Process & Reality: A_Study #1",
                    &items,
                    &["p. 3".into()],
                    &opts(fmt, true, key),
                );
                std::fs::write(format!("{dir}/{name}.{ext}"), out).unwrap();
            }
        }
    }
}
