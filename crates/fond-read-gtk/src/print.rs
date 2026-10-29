//! Printing: a selection set as text in a face and size of the reader's choosing, or whole pages
//! of a PDF as they are. One dialog asks what to print and how; GTK's own print dialog (or its
//! preview) takes it from there.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::{glib, pango, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use serde::{Deserialize, Serialize};

const PT_PER_MM: f64 = 72.0 / 25.4;
const PAGE_DPI_SCALE: f64 = 200.0 / 72.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    #[default]
    Serif,
    Sans,
    Mono,
}

impl Face {
    const ALL: [Face; 3] = [Face::Serif, Face::Sans, Face::Mono];

    fn label(self) -> &'static str {
        match self {
            Face::Serif => "Serif",
            Face::Sans => "Sans",
            Face::Mono => "Monospace",
        }
    }

    fn pango(self) -> &'static str {
        match self {
            Face::Serif => "Serif",
            Face::Sans => "Sans",
            Face::Mono => "Monospace",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    #[default]
    Page,
    Actual,
}

/// How a print job looks. Remembered between jobs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub face: Face,
    pub size: f64,
    pub line_spacing: f64,
    pub margin_mm: f64,
    pub justify: bool,
    pub header: bool,
    pub page_numbers: bool,
    pub cite: bool,
    pub fit: Fit,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            face: Face::Serif,
            size: 12.0,
            line_spacing: 1.3,
            margin_mm: 22.0,
            justify: false,
            header: true,
            page_numbers: true,
            cite: true,
            fit: Fit::Page,
        }
    }
}

const SIZE_RANGE: (f64, f64) = (6.0, 36.0);
const SPACING_RANGE: (f64, f64) = (1.0, 2.5);
const MARGIN_RANGE: (f64, f64) = (0.0, 60.0);

impl Look {
    fn clamped(mut self) -> Look {
        self.size = self.size.clamp(SIZE_RANGE.0, SIZE_RANGE.1);
        self.line_spacing = self.line_spacing.clamp(SPACING_RANGE.0, SPACING_RANGE.1);
        self.margin_mm = self.margin_mm.clamp(MARGIN_RANGE.0, MARGIN_RANGE.1);
        self
    }

    fn path() -> PathBuf {
        glib::user_config_dir().join("pereplyot").join("print.json")
    }

    pub fn load() -> Look {
        use crate::fsutil::{lenient, load_checked, Loaded};
        match load_checked(&Look::path(), lenient::<Look>) {
            Loaded::Ok(l) => l,
            Loaded::Missing => Look::default(),
            Loaded::Corrupt {
                recovered_from_backup,
                ..
            } => recovered_from_backup.unwrap_or_default(),
        }
        .clamped()
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = crate::fsutil::write_atomic(&Look::path(), json.as_bytes());
        }
    }
}

/// Text picked out of the document.
#[derive(Clone)]
pub struct Selection {
    pub text: String,
    /// Where it comes from ("42", "chapter 3"), for the footer.
    pub locator: Option<String>,
}

/// A PDF whose pages can be printed as they are.
#[derive(Clone)]
pub struct Pages {
    pub path: PathBuf,
    /// 1-based.
    pub current: u32,
    pub count: u32,
}

pub type ChapterPrint = Rc<dyn Fn(&gtk4::Window)>;

pub struct Request {
    pub title: String,
    pub selection: Option<Selection>,
    pub pages: Option<Pages>,
    /// Prints what the reader shows by its own means (an EPUB chapter, through the web view),
    /// leaving the look to it.
    pub chapter: Option<ChapterPrint>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scope {
    Selection,
    Current,
    Range,
    All,
    Chapter,
}

impl Scope {
    fn label(self) -> &'static str {
        match self {
            Scope::Selection => "Selected text",
            Scope::Current => "This page",
            Scope::Range => "Page range",
            Scope::All => "Whole document",
            Scope::Chapter => "This chapter",
        }
    }
}

/// Pages named by "1-5, 8, 11-": 1-based, in order, without repeats, within `count`.
pub fn parse_ranges(spec: &str, count: u32) -> Option<Vec<u32>> {
    let mut out: Vec<u32> = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (from, to) = match part.split_once('-') {
            Some((a, b)) => {
                let a = a.trim();
                let b = b.trim();
                (
                    if a.is_empty() { 1 } else { a.parse().ok()? },
                    if b.is_empty() { count } else { b.parse().ok()? },
                )
            }
            None => {
                let n: u32 = part.parse().ok()?;
                (n, n)
            }
        };
        if from == 0 || from > to || to > count {
            return None;
        }
        for n in from..=to {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Selected text as paragraphs: the line breaks a page imposes are not the reader's to keep,
/// and a word split across lines by a hyphen is joined.
pub fn unwrap_lines(text: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
            continue;
        }
        if current.is_empty() {
            current.push_str(line);
        } else if current.ends_with('-')
            && current
                .chars()
                .rev()
                .nth(1)
                .is_some_and(|c| c.is_alphabetic())
            && line.chars().next().is_some_and(|c| c.is_lowercase())
        {
            current.pop();
            current.push_str(line);
        } else {
            current.push(' ');
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs.join("\n\n")
}

struct LineBox {
    top: f64,
    height: f64,
    baseline: f64,
}

struct Typeset {
    layout: pango::Layout,
    lines: Vec<LineBox>,
    pages: Vec<std::ops::Range<usize>>,
}

fn typeset(ctx: &gtk4::PrintContext, text: &str, look: &Look, area: (f64, f64)) -> Typeset {
    let layout = ctx.create_pango_layout();
    let scale = pango::SCALE as f64;
    layout.set_font_description(Some(&pango::FontDescription::from_string(&format!(
        "{} {}",
        look.face.pango(),
        look.size
    ))));
    layout.set_width((area.0 * scale) as i32);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_justify(look.justify);
    layout.set_spacing(((look.line_spacing - 1.0) * look.size * scale) as i32);
    layout.set_text(text);

    let mut lines = Vec::new();
    let mut iter = layout.iter();
    loop {
        let (_, logical) = iter.line_extents();
        lines.push(LineBox {
            top: logical.y() as f64 / scale,
            height: logical.height() as f64 / scale,
            baseline: iter.baseline() as f64 / scale,
        });
        if !iter.next_line() {
            break;
        }
    }

    let mut pages = Vec::new();
    let mut start = 0;
    for i in 0..lines.len() {
        if i > start && lines[i].top + lines[i].height - lines[start].top > area.1 {
            pages.push(start..i);
            start = i;
        }
    }
    pages.push(start..lines.len());
    Typeset {
        layout,
        lines,
        pages,
    }
}

fn small_layout(
    ctx: &gtk4::PrintContext,
    text: &str,
    size: f64,
    width: f64,
    align: pango::Alignment,
) -> pango::Layout {
    let layout = ctx.create_pango_layout();
    layout.set_font_description(Some(&pango::FontDescription::from_string(&format!(
        "Sans {size}"
    ))));
    layout.set_width((width * pango::SCALE as f64) as i32);
    layout.set_alignment(align);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout.set_text(text);
    layout
}

#[derive(Clone, Copy)]
struct Frame {
    margin: f64,
    header: f64,
    footer: f64,
}

impl Frame {
    fn of(look: &Look, with_header_footer: bool) -> Frame {
        let band = look.size.min(11.0) * 2.2;
        Frame {
            margin: look.margin_mm * PT_PER_MM,
            header: if with_header_footer && look.header {
                band
            } else {
                0.0
            },
            footer: if with_header_footer && (look.page_numbers || look.cite) {
                band
            } else {
                0.0
            },
        }
    }

    fn body(&self, page: (f64, f64)) -> (f64, f64) {
        (
            (page.0 - 2.0 * self.margin).max(36.0),
            (page.1 - 2.0 * self.margin - self.header - self.footer).max(36.0),
        )
    }
}

fn run_text_job(
    parent: &gtk4::Window,
    action: gtk4::PrintOperationAction,
    title: &str,
    selection: &Selection,
    look: &Look,
) {
    let body = unwrap_lines(&selection.text);
    let set: Rc<RefCell<Option<Typeset>>> = Rc::new(RefCell::new(None));
    let op = new_operation(title);
    let look = look.clone();
    let frame = Frame::of(&look, true);
    {
        let set = set.clone();
        let look = look.clone();
        let body = body.clone();
        op.connect_begin_print(move |op, ctx| {
            let area = frame.body((ctx.width(), ctx.height()));
            let typeset = typeset(ctx, &body, &look, area);
            op.set_n_pages(typeset.pages.len() as i32);
            *set.borrow_mut() = Some(typeset);
        });
    }
    {
        let title = title.to_string();
        let locator = selection.locator.clone();
        op.connect_draw_page(move |_, ctx, page_nr| {
            let guard = set.borrow();
            let Some(set) = guard.as_ref() else { return };
            let Some(range) = set.pages.get(page_nr as usize) else {
                return;
            };
            let cr = ctx.cairo_context();
            let (w, h) = (ctx.width(), ctx.height());
            let band = look.size.min(11.0) * 0.8;
            let inner = w - 2.0 * frame.margin;
            cr.set_source_rgb(0.0, 0.0, 0.0);
            let first_top = set.lines[range.start].top;
            for i in range.clone() {
                let line = &set.lines[i];
                if let Some(l) = set.layout.line_readonly(i as i32) {
                    cr.move_to(
                        frame.margin,
                        frame.margin + frame.header + line.baseline - first_top,
                    );
                    pangocairo::functions::show_layout_line(&cr, &l);
                }
            }
            cr.set_source_rgb(0.4, 0.4, 0.4);
            if look.header {
                let head = small_layout(ctx, &title, band, inner, pango::Alignment::Left);
                cr.move_to(frame.margin, frame.margin);
                pangocairo::functions::show_layout(&cr, &head);
                cr.set_line_width(0.5);
                let y = frame.margin + frame.header - band * 0.6;
                cr.move_to(frame.margin, y);
                cr.line_to(w - frame.margin, y);
                let _ = cr.stroke();
            }
            let foot_y = h - frame.margin - frame.footer + band * 0.6;
            if look.cite {
                if let Some(locator) = &locator {
                    let cite = small_layout(
                        ctx,
                        &format!("{title}, {locator}"),
                        band,
                        inner * 0.75,
                        pango::Alignment::Left,
                    );
                    cr.move_to(frame.margin, foot_y);
                    pangocairo::functions::show_layout(&cr, &cite);
                }
            }
            if look.page_numbers {
                let number = small_layout(
                    ctx,
                    &format!("{} / {}", page_nr + 1, set.pages.len()),
                    band,
                    inner,
                    pango::Alignment::Right,
                );
                cr.move_to(frame.margin, foot_y);
                pangocairo::functions::show_layout(&cr, &number);
            }
        });
    }
    launch(op, parent, action);
}

fn run_pages_job(
    parent: &gtk4::Window,
    action: gtk4::PrintOperationAction,
    title: &str,
    source: &Pages,
    pages: Vec<u32>,
    look: &Look,
) {
    let op = new_operation(title);
    op.set_n_pages(pages.len() as i32);
    let path = source.path.clone();
    let look = look.clone();
    op.connect_draw_page(move |_, ctx, page_nr| {
        let Some(&page) = pages.get(page_nr as usize) else {
            return;
        };
        let Some((clip, pt_w, pt_h)) = crate::clip::render_page(&path, page, PAGE_DPI_SCALE) else {
            return;
        };
        let mut data = clip.rgba;
        for px in data.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let stride = clip.width as i32 * 4;
        let Ok(surface) = gtk4::cairo::ImageSurface::create_for_data(
            data,
            gtk4::cairo::Format::ARgb32,
            clip.width as i32,
            clip.height as i32,
            stride,
        ) else {
            return;
        };
        let margin = look.margin_mm * PT_PER_MM;
        let (aw, ah) = (
            (ctx.width() - 2.0 * margin).max(36.0),
            (ctx.height() - 2.0 * margin).max(36.0),
        );
        let fit = match look.fit {
            Fit::Page => (aw / pt_w).min(ah / pt_h),
            Fit::Actual => 1.0,
        };
        let cr = ctx.cairo_context();
        cr.translate(
            margin + (aw - pt_w * fit) / 2.0,
            margin + (ah - pt_h * fit) / 2.0,
        );
        cr.scale(
            fit * pt_w / clip.width as f64,
            fit * pt_h / clip.height as f64,
        );
        let _ = cr.set_source_surface(&surface, 0.0, 0.0);
        cr.source().set_filter(gtk4::cairo::Filter::Good);
        let _ = cr.paint();
    });
    launch(op, parent, action);
}

thread_local! {
    static SETTINGS: RefCell<Option<gtk4::PrintSettings>> = const { RefCell::new(None) };
}

fn new_operation(title: &str) -> gtk4::PrintOperation {
    let op = gtk4::PrintOperation::new();
    op.set_unit(gtk4::Unit::Points);
    op.set_job_name(title);
    op.set_allow_async(true);
    op.set_embed_page_setup(true);
    SETTINGS.with(|s| op.set_print_settings(s.borrow().as_ref()));
    op
}

fn launch(op: gtk4::PrintOperation, parent: &gtk4::Window, action: gtk4::PrintOperationAction) {
    let weak_parent = parent.downgrade();
    op.connect_done(move |op, result| {
        SETTINGS.with(|s| *s.borrow_mut() = op.print_settings());
        if let (gtk4::PrintOperationResult::Error, Some(parent)) = (result, weak_parent.upgrade()) {
            gtk4::AlertDialog::builder()
                .message("Could not print")
                .detail("The print job failed.")
                .build()
                .show(Some(&parent));
        }
    });
    let _ = op.run(action, Some(parent));
}

/// Ask what to print and how, then print it.
pub fn show(parent: &impl IsA<gtk4::Window>, request: Request) {
    let parent: gtk4::Window = parent.clone().upcast();
    let mut scopes = Vec::new();
    if request.selection.is_some() {
        scopes.push(Scope::Selection);
    }
    if request.pages.is_some() {
        scopes.extend([Scope::Current, Scope::Range, Scope::All]);
    }
    if request.chapter.is_some() {
        scopes.push(Scope::Chapter);
    }
    if scopes.is_empty() {
        return;
    }

    let look = Rc::new(RefCell::new(Look::load()));
    let dialog = adw::Window::new();
    dialog.set_title(Some("Print"));
    dialog.set_modal(true);
    dialog.set_transient_for(Some(&parent));
    dialog.set_default_size(440, 640);

    let page = adw::PreferencesPage::new();

    let what = adw::PreferencesGroup::new();
    what.set_title("What to print");
    let scope_row = adw::ComboRow::new();
    scope_row.set_title("Print");
    scope_row.set_model(Some(&gtk4::StringList::new(
        &scopes.iter().map(|s| s.label()).collect::<Vec<_>>(),
    )));
    what.add(&scope_row);
    let range_row = adw::EntryRow::new();
    range_row.set_title("Pages (for example 1-5, 8, 11-)");
    if let Some(p) = &request.pages {
        range_row.set_text(&format!("1-{}", p.count));
    }
    what.add(&range_row);
    page.add(&what);

    let text_group = adw::PreferencesGroup::new();
    text_group.set_title("Text");
    let face_row = adw::ComboRow::new();
    face_row.set_title("Typeface");
    face_row.set_model(Some(&gtk4::StringList::new(
        &Face::ALL.iter().map(|f| f.label()).collect::<Vec<_>>(),
    )));
    let size_row = adw::SpinRow::with_range(SIZE_RANGE.0, SIZE_RANGE.1, 0.5);
    size_row.set_title("Size (pt)");
    let spacing_row = adw::SpinRow::with_range(SPACING_RANGE.0, SPACING_RANGE.1, 0.05);
    spacing_row.set_title("Line spacing");
    spacing_row.set_digits(2);
    let justify_row = adw::SwitchRow::new();
    justify_row.set_title("Justify");
    text_group.add(&face_row);
    text_group.add(&size_row);
    text_group.add(&spacing_row);
    text_group.add(&justify_row);
    page.add(&text_group);

    let layout_group = adw::PreferencesGroup::new();
    layout_group.set_title("Page");
    let margin_row = adw::SpinRow::with_range(MARGIN_RANGE.0, MARGIN_RANGE.1, 1.0);
    margin_row.set_title("Margins (mm)");
    layout_group.add(&margin_row);
    let fit_row = adw::ComboRow::new();
    fit_row.set_title("Scale");
    fit_row.set_model(Some(&gtk4::StringList::new(&[
        "Fit to page",
        "Actual size",
    ])));
    layout_group.add(&fit_row);
    let header_row = adw::SwitchRow::new();
    header_row.set_title("Title at the top");
    layout_group.add(&header_row);
    let numbers_row = adw::SwitchRow::new();
    numbers_row.set_title("Page numbers");
    layout_group.add(&numbers_row);
    let cite_row = adw::SwitchRow::new();
    cite_row.set_title("Source and page at the bottom");
    layout_group.add(&cite_row);
    page.add(&layout_group);

    {
        let l = look.borrow();
        face_row.set_selected(Face::ALL.iter().position(|f| *f == l.face).unwrap_or(0) as u32);
        size_row.set_value(l.size);
        spacing_row.set_value(l.line_spacing);
        justify_row.set_active(l.justify);
        margin_row.set_value(l.margin_mm);
        fit_row.set_selected(if l.fit == Fit::Page { 0 } else { 1 });
        header_row.set_active(l.header);
        numbers_row.set_active(l.page_numbers);
        cite_row.set_active(l.cite);
    }

    let scopes = Rc::new(scopes);
    let scope_of = {
        let scopes = scopes.clone();
        let scope_row = scope_row.clone();
        move || scopes[(scope_row.selected() as usize).min(scopes.len() - 1)]
    };
    let sync = {
        let range_row = range_row.clone();
        let text_group = text_group.clone();
        let fit_row = fit_row.clone();
        let header_row = header_row.clone();
        let numbers_row = numbers_row.clone();
        let cite_row = cite_row.clone();
        let layout_group = layout_group.clone();
        let margin_row = margin_row.clone();
        let scope_of = scope_of.clone();
        move || {
            let scope = scope_of();
            let text = scope == Scope::Selection;
            let pages = matches!(scope, Scope::Current | Scope::Range | Scope::All);
            range_row.set_visible(scope == Scope::Range);
            text_group.set_visible(text);
            layout_group.set_visible(text || pages);
            fit_row.set_visible(pages);
            header_row.set_visible(text);
            numbers_row.set_visible(text);
            cite_row.set_visible(text);
            margin_row.set_visible(text || pages);
        }
    };
    sync();
    scope_row.connect_selected_notify({
        let sync = sync.clone();
        move |_| sync()
    });

    let collect = {
        let look = look.clone();
        let face_row = face_row.clone();
        let size_row = size_row.clone();
        let spacing_row = spacing_row.clone();
        let justify_row = justify_row.clone();
        let margin_row = margin_row.clone();
        let fit_row = fit_row.clone();
        let header_row = header_row.clone();
        let numbers_row = numbers_row.clone();
        let cite_row = cite_row.clone();
        move || {
            let mut l = look.borrow_mut();
            l.face = Face::ALL[(face_row.selected() as usize).min(Face::ALL.len() - 1)];
            l.size = size_row.value();
            l.line_spacing = spacing_row.value();
            l.justify = justify_row.is_active();
            l.margin_mm = margin_row.value();
            l.fit = if fit_row.selected() == 0 {
                Fit::Page
            } else {
                Fit::Actual
            };
            l.header = header_row.is_active();
            l.page_numbers = numbers_row.is_active();
            l.cite = cite_row.is_active();
            let clamped = l.clone().clamped();
            *l = clamped.clone();
            clamped
        }
    };

    let go = {
        let dialog = dialog.clone();
        let parent = parent.clone();
        let request = Rc::new(request);
        let range_row = range_row.clone();
        move |action: gtk4::PrintOperationAction| {
            let look = collect();
            let scope = scope_of();
            let pages_job = |pages: Vec<u32>, source: &Pages| {
                look.save();
                dialog.close();
                run_pages_job(&parent, action, &request.title, source, pages, &look);
            };
            match (scope, &request.pages, &request.selection) {
                (Scope::Selection, _, Some(selection)) => {
                    look.save();
                    dialog.close();
                    run_text_job(&parent, action, &request.title, selection, &look);
                }
                (Scope::Current, Some(p), _) => pages_job(vec![p.current], p),
                (Scope::All, Some(p), _) => pages_job((1..=p.count).collect(), p),
                (Scope::Range, Some(p), _) => match parse_ranges(&range_row.text(), p.count) {
                    Some(pages) => pages_job(pages, p),
                    None => range_row.add_css_class("error"),
                },
                (Scope::Chapter, _, _) => {
                    dialog.close();
                    if let Some(run) = &request.chapter {
                        run(&parent);
                    }
                }
                _ => {}
            }
        }
    };
    range_row.connect_changed(|row| row.remove_css_class("error"));

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    header.set_show_start_title_buttons(false);
    let cancel = gtk4::Button::with_label("Cancel");
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    header.pack_start(&cancel);
    let print = gtk4::Button::with_label("Print…");
    print.add_css_class("suggested-action");
    {
        let go = go.clone();
        print.connect_clicked(move |_| go(gtk4::PrintOperationAction::PrintDialog));
    }
    header.pack_end(&print);
    let preview = gtk4::Button::with_label("Preview");
    preview.connect_clicked(move |_| go(gtk4::PrintOperationAction::Preview));
    header.pack_end(&preview);
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&page));
    dialog.set_content(Some(&toolbar));

    let keys = gtk4::EventControllerKey::new();
    {
        let dialog = dialog.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                dialog.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    dialog.add_controller(keys);
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_expand_in_order_without_repeats() {
        assert_eq!(parse_ranges("1-3, 8, 2", 10), Some(vec![1, 2, 3, 8]));
        assert_eq!(parse_ranges("8-", 10), Some(vec![8, 9, 10]));
        assert_eq!(parse_ranges("-2", 10), Some(vec![1, 2]));
    }

    #[test]
    fn ranges_outside_the_document_are_refused() {
        assert_eq!(parse_ranges("0", 10), None);
        assert_eq!(parse_ranges("5-11", 10), None);
        assert_eq!(parse_ranges("4-2", 10), None);
        assert_eq!(parse_ranges("a", 10), None);
        assert_eq!(parse_ranges("", 10), None);
    }

    #[test]
    fn page_line_breaks_are_unwrapped() {
        assert_eq!(
            unwrap_lines("The quick brown\nfox jumped over\n\nA new para-\ngraph here."),
            "The quick brown fox jumped over\n\nA new paragraph here."
        );
    }

    #[test]
    fn a_dash_before_a_capital_is_kept() {
        assert_eq!(unwrap_lines("well-\nKnown"), "well- Known");
    }

    #[test]
    fn look_survives_a_round_trip_and_clamps() {
        let l = Look {
            size: 500.0,
            ..Look::default()
        };
        assert_eq!(l.clamped().size, SIZE_RANGE.1);
        let json = serde_json::to_string(&Look::default()).unwrap();
        assert_eq!(
            serde_json::from_str::<Look>(&json).unwrap(),
            Look::default()
        );
    }
}
