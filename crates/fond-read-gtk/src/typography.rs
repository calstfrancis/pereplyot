//! Reading typography shared by the EPUB reader and the PDF Reading mode: one settings object,
//! one saved file and one panel, so both formats offer the same choices and remember them.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    /// New Computer Modern, justified with indented paragraphs: the look of Zerkalo's "LaTeX Look"
    /// template.
    #[default]
    Latex,
    Default,
    Serif,
    Sans,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ParagraphStyle {
    #[default]
    Indent,
    Space,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReadingTheme {
    #[default]
    Light,
    Sepia,
    Dark,
}

pub const SIZE_RANGE: (f64, f64) = (0.6, 3.0);
pub const LINE_RANGE: (f64, f64) = (1.1, 2.4);
pub const WIDTH_RANGE: (u32, u32) = (420, 1100);
pub const MARGIN_RANGE: (u32, u32) = (0, 120);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Typography {
    pub family: Family,
    /// 1.0 is the base size.
    pub size: f64,
    /// Line height as a multiple of the size.
    pub line_height: f64,
    /// Width of the text column in pixels.
    pub width: u32,
    /// Space either side of the column in pixels.
    pub margin: u32,
    pub justify: bool,
    pub paragraph: ParagraphStyle,
    pub theme: ReadingTheme,
}

impl Default for Typography {
    fn default() -> Self {
        Typography {
            family: Family::Latex,
            size: 1.0,
            line_height: 1.3,
            width: 680,
            margin: 24,
            justify: true,
            paragraph: ParagraphStyle::Indent,
            theme: ReadingTheme::Light,
        }
    }
}

fn path() -> PathBuf {
    glib::user_config_dir()
        .join("pereplyot")
        .join("reading.json")
}

impl Typography {
    pub fn load() -> Typography {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str::<Typography>(&s).ok())
            .unwrap_or_default()
            .clamped()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = crate::fsutil::write_atomic(&path(), json.as_bytes());
        }
    }

    fn clamped(mut self) -> Typography {
        self.size = self.size.clamp(SIZE_RANGE.0, SIZE_RANGE.1);
        self.line_height = self.line_height.clamp(LINE_RANGE.0, LINE_RANGE.1);
        self.width = self.width.clamp(WIDTH_RANGE.0, WIDTH_RANGE.1);
        self.margin = self.margin.clamp(MARGIN_RANGE.0, MARGIN_RANGE.1);
        self
    }

    /// Page and text colours of the reading theme, as CSS colours; `None` follows the app.
    pub fn colours(&self) -> Option<(&'static str, &'static str, &'static str)> {
        match self.theme {
            ReadingTheme::Light => None,
            ReadingTheme::Sepia => Some(("#f4ecd8", "#5b4636", "#8a6d3b")),
            ReadingTheme::Dark => Some(("#1e1e1e", "#dddddd", "#8ab4f8")),
        }
    }

    fn family_stack(&self) -> Option<&'static str> {
        match self.family {
            Family::Latex => Some(
                "'New Computer Modern', 'NewCM10', 'Latin Modern Roman', 'CMU Serif', \
                 'Computer Modern', Georgia, serif",
            ),
            Family::Default => None,
            Family::Serif => Some("Georgia, 'Times New Roman', serif"),
            Family::Sans => Some("-webkit-system-font, 'Helvetica Neue', Arial, sans-serif"),
            Family::Mono => Some("'DejaVu Sans Mono', Menlo, Consolas, monospace"),
        }
    }

    /// The stylesheet an EPUB chapter is shown with. Text size goes through the view's zoom
    /// instead (see `size`), so it is not part of this.
    pub fn epub_css(&self) -> String {
        self.epub_css_for(false)
    }

    /// The EPUB stylesheet; paginated, the page's own columns set the width and margins, so the
    /// text-width rule is left out.
    pub fn epub_css_for(&self, paginated: bool) -> String {
        let mut css = String::new();
        if let Some((bg, fg, link)) = self.colours() {
            css.push_str(&format!(
                "html, body {{ background: {bg} !important; color: {fg} !important; }} \
                 a, a:visited {{ color: {link} !important; }}\n"
            ));
        }
        if let Some(stack) = self.family_stack() {
            css.push_str(&format!(
                "body, p, div, span, li {{ font-family: {stack} !important; }}\n"
            ));
        }
        let indent = match self.paragraph {
            ParagraphStyle::Indent => "text-indent: 1.8em; margin: 0 0 0.45em;",
            ParagraphStyle::Space => "text-indent: 0; margin: 0 0 0.9em;",
        };
        let align = if self.justify { "justify" } else { "left" };
        if paginated {
            css.push_str(&format!(
                "body {{ line-height: {:.2} !important; }}\n\
                 p {{ {indent} text-align: {align} !important; line-height: {:.2} !important; }}\n",
                self.line_height, self.line_height
            ));
        } else {
            css.push_str(&format!(
                "body {{ max-width: {}px !important; margin: 0 auto !important; \
                 padding: 0 {}px !important; line-height: {:.2} !important; }}\n\
                 p {{ {indent} text-align: {align} !important; line-height: {:.2} !important; }}\n",
                self.width, self.margin, self.line_height, self.line_height
            ));
        }
        css
    }

    /// The CSS for the PDF Reading view's `TextView`.
    pub fn text_view_css(&self) -> String {
        let px = (18.0 * self.size).round();
        let mut css = format!(
            "textview {{ font-size: {px}px; line-height: {:.2}; ",
            self.line_height
        );
        if let Some(stack) = self.family_stack() {
            let stack = stack.replace("-webkit-system-font, ", "");
            css.push_str(&format!("font-family: {stack}; "));
        }
        css.push('}');
        if let Some((bg, fg, _)) = self.colours() {
            css.push_str(&format!(
                " textview, textview text {{ background-color: {bg}; color: {fg}; }}"
            ));
        }
        css
    }
}

type Listener = Rc<dyn Fn(&Typography)>;

/// The settings every open reader shares, with the callbacks that restyle each of them.
pub struct Shared {
    pub current: RefCell<Typography>,
    listeners: RefCell<Vec<Listener>>,
}

thread_local! {
    static SHARED: Rc<Shared> = Rc::new(Shared {
        current: RefCell::new(Typography::load()),
        listeners: RefCell::new(Vec::new()),
    });
}

pub fn shared() -> Rc<Shared> {
    SHARED.with(Rc::clone)
}

impl Shared {
    pub fn get(&self) -> Typography {
        self.current.borrow().clone()
    }

    /// Call `f` now and whenever the settings change.
    pub fn watch(&self, f: impl Fn(&Typography) + 'static) {
        let f: Listener = Rc::new(f);
        f(&self.get());
        self.listeners.borrow_mut().push(f);
    }

    pub fn update(&self, edit: impl FnOnce(&mut Typography)) {
        let mut next = self.get();
        edit(&mut next);
        let next = next.clamped();
        if *self.current.borrow() == next {
            return;
        }
        *self.current.borrow_mut() = next.clone();
        next.save();
        let listeners: Vec<_> = self.listeners.borrow().clone();
        for f in listeners {
            f(&next);
        }
    }
}

/// The "Aa" button and its panel: font, size, spacing, width, margins, justification, paragraph
/// style and reading theme.
pub fn button() -> gtk4::MenuButton {
    let settings = shared();
    let button = gtk4::MenuButton::new();
    button.set_label("Aa");
    button.add_css_class("flat");
    button.set_tooltip_text(Some(
        "Reading typography: font, size, spacing, width, theme",
    ));
    let popover = gtk4::Popover::new();
    popover.set_child(Some(&panel(&settings)));
    button.set_popover(Some(&popover));
    button
}

fn panel(settings: &Rc<Shared>) -> gtk4::Widget {
    let current = settings.get();
    let grid = gtk4::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    let mut row = 0;
    let mut add = |label: &str, widget: &gtk4::Widget| {
        let l = gtk4::Label::new(Some(label));
        l.set_halign(gtk4::Align::Start);
        l.add_css_class("dim-label");
        grid.attach(&l, 0, row, 1, 1);
        grid.attach(widget, 1, row, 1, 1);
        row += 1;
    };

    let family =
        gtk4::DropDown::from_strings(&["LaTeX", "System", "Serif", "Sans-serif", "Monospace"]);
    family.set_selected(current.family as u32);
    {
        let settings = settings.clone();
        family.connect_selected_notify(move |d| {
            let f = [
                Family::Latex,
                Family::Default,
                Family::Serif,
                Family::Sans,
                Family::Mono,
            ][d.selected().min(4) as usize];
            settings.update(|t| t.family = f);
        });
    }
    add("Font", family.upcast_ref());

    let slider = |range: (f64, f64), value: f64, step: f64, on: Rc<dyn Fn(f64)>| {
        let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, range.0, range.1, step);
        scale.set_value(value);
        scale.set_draw_value(false);
        scale.set_width_request(180);
        scale.connect_value_changed(move |s| on(s.value()));
        scale
    };
    {
        let settings = settings.clone();
        let s = slider(
            SIZE_RANGE,
            current.size,
            0.05,
            Rc::new(move |v| settings.update(|t| t.size = v)),
        );
        add("Size", s.upcast_ref());
    }
    {
        let settings = settings.clone();
        let s = slider(
            LINE_RANGE,
            current.line_height,
            0.05,
            Rc::new(move |v| settings.update(|t| t.line_height = v)),
        );
        add("Line spacing", s.upcast_ref());
    }
    {
        let settings = settings.clone();
        let s = slider(
            (WIDTH_RANGE.0 as f64, WIDTH_RANGE.1 as f64),
            current.width as f64,
            10.0,
            Rc::new(move |v| settings.update(|t| t.width = v as u32)),
        );
        add("Column width", s.upcast_ref());
    }
    {
        let settings = settings.clone();
        let s = slider(
            (MARGIN_RANGE.0 as f64, MARGIN_RANGE.1 as f64),
            current.margin as f64,
            4.0,
            Rc::new(move |v| settings.update(|t| t.margin = v as u32)),
        );
        add("Margins", s.upcast_ref());
    }

    let justify = gtk4::Switch::new();
    justify.set_active(current.justify);
    justify.set_halign(gtk4::Align::Start);
    {
        let settings = settings.clone();
        justify.connect_active_notify(move |s| settings.update(|t| t.justify = s.is_active()));
    }
    add("Justify", justify.upcast_ref());

    let paragraph = gtk4::DropDown::from_strings(&["Indented", "Spaced"]);
    paragraph.set_selected(current.paragraph as u32);
    {
        let settings = settings.clone();
        paragraph.connect_selected_notify(move |d| {
            let p = if d.selected() == 0 {
                ParagraphStyle::Indent
            } else {
                ParagraphStyle::Space
            };
            settings.update(|t| t.paragraph = p);
        });
    }
    add("Paragraphs", paragraph.upcast_ref());

    let themes = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    themes.add_css_class("linked");
    let mut first: Option<gtk4::ToggleButton> = None;
    for (label, theme) in [
        ("Light", ReadingTheme::Light),
        ("Sepia", ReadingTheme::Sepia),
        ("Dark", ReadingTheme::Dark),
    ] {
        let b = gtk4::ToggleButton::with_label(label);
        b.set_active(current.theme == theme);
        match &first {
            Some(f) => b.set_group(Some(f)),
            None => first = Some(b.clone()),
        }
        let settings = settings.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                settings.update(|t| t.theme = theme);
            }
        });
        themes.append(&b);
    }
    add("Theme", themes.upcast_ref());

    grid.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_round_trip_and_unknown_files_fall_back_to_defaults() {
        let t = Typography {
            family: Family::Serif,
            theme: ReadingTheme::Sepia,
            justify: true,
            ..Typography::default()
        };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Typography>(&json).unwrap(), t);
        assert_eq!(
            serde_json::from_str::<Typography>("{}").unwrap(),
            Typography::default()
        );
    }

    #[test]
    fn out_of_range_values_are_pulled_back() {
        let t = Typography {
            size: 50.0,
            width: 10,
            ..Typography::default()
        }
        .clamped();
        assert_eq!(t.size, SIZE_RANGE.1);
        assert_eq!(t.width, WIDTH_RANGE.0);
    }

    #[test]
    fn the_epub_stylesheet_follows_the_theme_and_paragraph_style() {
        let sepia = Typography {
            theme: ReadingTheme::Sepia,
            paragraph: ParagraphStyle::Space,
            ..Typography::default()
        };
        let css = sepia.epub_css();
        assert!(css.contains("#f4ecd8"));
        assert!(css.contains("text-indent: 0"));
        assert!(!Typography::default().epub_css().contains("background"));
    }
}
