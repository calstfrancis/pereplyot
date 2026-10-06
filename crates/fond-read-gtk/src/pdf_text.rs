//! The PDF reader's Text view: the document's text reflowed into a real, read-only text
//! widget. Screen readers can read it, it can be enlarged with the text rewrapping, and text
//! can be selected from the keyboard — none of which a page image allows.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;

/// Join a page's raw text lines into paragraphs: a line that ends well short of the page's
/// usual width and finishes a sentence ends the paragraph, blank lines always do, and a
/// word hyphenated across lines is rejoined.
pub fn reflow(raw: &str) -> Vec<String> {
    let lines: Vec<&str> = raw.lines().map(str::trim).collect();
    let max_len = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let mut paragraphs: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in lines {
        if line.is_empty() {
            if !cur.is_empty() {
                paragraphs.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if cur.is_empty() {
            cur.push_str(line);
        } else if cur.ends_with('-')
            && cur.chars().rev().nth(1).is_some_and(|c| c.is_alphabetic())
            && line.chars().next().is_some_and(|c| c.is_lowercase())
        {
            cur.pop();
            cur.push_str(line);
        } else {
            cur.push(' ');
            cur.push_str(line);
        }
        let short = (line.chars().count() as f64) < max_len as f64 * 0.6;
        let sentence_end = line.ends_with(['.', '?', '!', ':', '"', '”', '’', ')']);
        if short && sentence_end {
            paragraphs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        paragraphs.push(cur);
    }
    paragraphs
}

/// Collapse every run of whitespace to one space, for matching quotes against reflowed text.
pub fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One saved mark to paint onto the text: which page, the quoted text, its colour and style.
pub struct TextMark {
    pub page: u16,
    pub quote: String,
    pub rgba: [u8; 4],
    pub style: MarkStyle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MarkStyle {
    Highlight,
    Underline,
    Strikeout,
}

pub struct ReflowView {
    pub scroll: gtk4::ScrolledWindow,
    pub text_view: gtk4::TextView,
    total: u16,
    /// Char offset at which each loaded page's heading starts.
    offsets: RefCell<Vec<i32>>,
    started: Cell<bool>,
    scale: Cell<f64>,
    css: gtk4::CssProvider,
    applied_tags: RefCell<Vec<String>>,
}

impl ReflowView {
    pub fn new(total: u16) -> Rc<ReflowView> {
        let text_view = gtk4::TextView::new();
        text_view.set_editable(false);
        text_view.set_cursor_visible(true);
        text_view.set_wrap_mode(gtk4::WrapMode::Word);
        text_view.set_widget_name("fond-reflow");
        text_view.set_left_margin(48);
        text_view.set_right_margin(48);
        text_view.set_top_margin(24);
        text_view.set_bottom_margin(48);
        text_view.set_pixels_below_lines(6);
        text_view.update_property(&[gtk4::accessible::Property::Label(
            "Document text. Select with Shift and the arrow keys, then press 1 to 4 to mark it.",
        )]);

        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_hexpand(true);
        let column = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        column.set_halign(gtk4::Align::Center);
        text_view.set_size_request(760, -1);
        column.append(&text_view);
        scroll.set_child(Some(&column));

        let css = gtk4::CssProvider::new();
        #[allow(deprecated)]
        text_view
            .style_context()
            .add_provider(&css, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);

        let view = Rc::new(ReflowView {
            scroll,
            text_view,
            total,
            offsets: RefCell::new(Vec::new()),
            started: Cell::new(false),
            scale: Cell::new(1.0),
            css,
            applied_tags: RefCell::new(Vec::new()),
        });
        view.apply_font();
        view
    }

    fn apply_font(&self) {
        let px = (18.0 * self.scale.get()).round();
        self.css.load_from_data(&format!(
            "textview {{ font-size: {px}px; line-height: 1.5; }}"
        ));
    }

    /// Scale the text by `factor` (1.25 = 25% larger), within sensible limits.
    pub fn scale_font(&self, factor: f64) {
        self.scale.set((self.scale.get() * factor).clamp(0.6, 4.0));
        self.apply_font();
    }

    /// Fill the buffer page by page, one page per idle tick so even a long book doesn't block
    /// the UI. A no-op after the first call. `on_page` reports how many pages are in.
    pub fn start_loading(
        self: &Rc<Self>,
        page_text: Rc<dyn Fn(u16) -> String>,
        page_label: Rc<dyn Fn(u16) -> String>,
        on_progress: Rc<dyn Fn(u16)>,
    ) {
        if self.started.replace(true) {
            return;
        }
        let buffer = self.text_view.buffer();
        let heading = buffer
            .create_tag(
                Some("page-heading"),
                &[
                    ("weight", &700i32),
                    ("scale", &0.85f64),
                    ("foreground", &"#808080"),
                    ("pixels-above-lines", &18i32),
                ],
            )
            .expect("tag");
        let _ = heading;
        self.load_next(0, page_text, page_label, on_progress);
    }

    fn load_next(
        self: &Rc<Self>,
        page: u16,
        page_text: Rc<dyn Fn(u16) -> String>,
        page_label: Rc<dyn Fn(u16) -> String>,
        on_progress: Rc<dyn Fn(u16)>,
    ) {
        if page >= self.total {
            return;
        }
        let buffer = self.text_view.buffer();
        let start_offset = buffer.char_count();
        self.offsets.borrow_mut().push(start_offset);
        let mut end = buffer.end_iter();
        buffer.insert_with_tags_by_name(
            &mut end,
            &format!("Page {}\n", page_label(page)),
            &["page-heading"],
        );
        let paragraphs = reflow(&page_text(page));
        let mut end = buffer.end_iter();
        if paragraphs.is_empty() {
            buffer.insert(&mut end, "(no text on this page)\n\n");
        } else {
            for p in paragraphs {
                buffer.insert(&mut end, &p);
                buffer.insert(&mut end, "\n\n");
            }
        }
        on_progress(page + 1);
        let this = self.clone();
        gtk4::glib::idle_add_local_once(move || {
            this.load_next(page + 1, page_text, page_label, on_progress);
        });
    }

    pub fn loaded_pages(&self) -> u16 {
        self.offsets.borrow().len() as u16
    }

    pub fn scroll_to_page(&self, page: u16) {
        let offset = match self.offsets.borrow().get(page as usize) {
            Some(o) => *o,
            None => return,
        };
        let buffer = self.text_view.buffer();
        let iter = buffer.iter_at_offset(offset);
        let mark = buffer.create_mark(None, &iter, true);
        self.text_view.scroll_to_mark(&mark, 0.0, true, 0.0, 0.0);
        buffer.place_cursor(&iter);
        buffer.delete_mark(&mark);
    }

    pub fn page_at_offset(&self, offset: i32) -> u16 {
        let offsets = self.offsets.borrow();
        offsets
            .partition_point(|o| *o <= offset)
            .saturating_sub(1)
            .min(self.total.saturating_sub(1) as usize) as u16
    }

    /// The page whose text is at the top of the viewport.
    pub fn visible_page(&self) -> u16 {
        let iter = self
            .text_view
            .iter_at_location(0, 0)
            .unwrap_or_else(|| self.text_view.buffer().start_iter());
        self.page_at_offset(iter.offset())
    }

    /// The selected text — page headings excluded — and the page its first real character
    /// lies on, if anything is selected.
    pub fn selection(&self) -> Option<(u16, String)> {
        let buffer = self.text_view.buffer();
        let (mut start, end) = buffer.selection_bounds()?;
        let heading = buffer.tag_table().lookup("page-heading")?;
        let mut text = String::new();
        let mut first_offset: Option<i32> = None;
        while start.offset() < end.offset() {
            if start.has_tag(&heading) {
                if !start.forward_to_tag_toggle(Some(&heading)) {
                    break;
                }
                continue;
            }
            let mut stop = start;
            if !stop.forward_to_tag_toggle(Some(&heading)) || stop.offset() > end.offset() {
                stop = end;
            }
            let segment = buffer.text(&start, &stop, false);
            if first_offset.is_none() && !segment.trim().is_empty() {
                first_offset = Some(start.offset());
            }
            text.push_str(&segment);
            text.push(' ');
            start = stop;
        }
        let collapsed = collapse(&text);
        let offset = first_offset?;
        (!collapsed.is_empty()).then(|| (self.page_at_offset(offset), collapsed))
    }

    /// Where on screen the selection ends, in the text view's own coordinates.
    pub fn selection_anchor(&self) -> (i32, i32) {
        let buffer = self.text_view.buffer();
        let iter = buffer
            .selection_bounds()
            .map(|(_, end)| end)
            .unwrap_or_else(|| buffer.iter_at_mark(&buffer.get_insert()));
        let rect = self.text_view.iter_location(&iter);
        let (x, y) = self.text_view.buffer_to_window_coords(
            gtk4::TextWindowType::Widget,
            rect.x(),
            rect.y() + rect.height(),
        );
        (x, y)
    }

    /// Repaint saved marks onto the text, replacing whatever was painted before.
    pub fn apply_marks(&self, marks: &[TextMark]) {
        let buffer = self.text_view.buffer();
        for name in self.applied_tags.borrow_mut().drain(..) {
            if let Some(tag) = buffer.tag_table().lookup(&name) {
                buffer.remove_tag(&tag, &buffer.start_iter(), &buffer.end_iter());
            }
        }
        let offsets = self.offsets.borrow().clone();
        for mark in marks {
            let Some(&page_start) = offsets.get(mark.page as usize) else {
                continue;
            };
            let page_end = offsets
                .get(mark.page as usize + 1)
                .copied()
                .unwrap_or_else(|| buffer.char_count());
            let region = buffer
                .text(
                    &buffer.iter_at_offset(page_start),
                    &buffer.iter_at_offset(page_end),
                    false,
                )
                .to_string();
            let needle = collapse(&mark.quote);
            if needle.is_empty() {
                continue;
            }
            // The page region keeps its line structure; find the quote tolerating whitespace
            // differences by matching on a collapsed copy and mapping back by char position.
            let (collapsed, map) = collapse_with_map(&region);
            let Some(byte_idx) = collapsed.find(&needle) else {
                continue;
            };
            let first = collapsed[..byte_idx].chars().count();
            let last = first + needle.chars().count();
            let (Some(&a), Some(&b)) = (map.get(first), map.get(last.saturating_sub(1))) else {
                continue;
            };
            let name = tag_name(mark);
            if buffer.tag_table().lookup(&name).is_none() {
                let [r, g, bl, al] = mark.rgba;
                let rgba = format!(
                    "rgba({r},{g},{bl},{:.2})",
                    (al as f64 / 255.0 * 1.4).min(1.0)
                );
                let colour = gtk4::gdk::RGBA::parse(&rgba).unwrap_or(gtk4::gdk::RGBA::BLACK);
                let created = match mark.style {
                    MarkStyle::Highlight => {
                        buffer.create_tag(Some(&name), &[("background-rgba", &colour)])
                    }
                    MarkStyle::Underline => buffer.create_tag(
                        Some(&name),
                        &[
                            ("underline", &gtk4::pango::Underline::Single),
                            ("underline-rgba", &colour),
                        ],
                    ),
                    MarkStyle::Strikeout => {
                        buffer.create_tag(Some(&name), &[("strikethrough", &true)])
                    }
                };
                if created.is_none() {
                    continue;
                }
            }
            buffer.apply_tag_by_name(
                &name,
                &buffer.iter_at_offset(page_start + a as i32),
                &buffer.iter_at_offset(page_start + b as i32 + 1),
            );
            let mut applied = self.applied_tags.borrow_mut();
            if !applied.contains(&name) {
                applied.push(name);
            }
        }
    }
}

fn tag_name(mark: &TextMark) -> String {
    let style = match mark.style {
        MarkStyle::Highlight => "h",
        MarkStyle::Underline => "u",
        MarkStyle::Strikeout => "s",
    };
    format!(
        "mark-{style}-{:02x}{:02x}{:02x}{:02x}",
        mark.rgba[0], mark.rgba[1], mark.rgba[2], mark.rgba[3]
    )
}

/// `collapse`, plus for each char of the result the char offset it came from in `s`.
fn collapse_with_map(s: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len());
    let mut pending_space = false;
    for (i, c) in s.chars().enumerate() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            if pending_space {
                // remember where the space is, used only if it ends up inside the output
            }
            continue;
        }
        if pending_space {
            out.push(' ');
            map.push(i.saturating_sub(1));
            pending_space = false;
        }
        out.push(c);
        map.push(i);
    }
    (out, map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_wrapped_lines_and_splits_paragraphs() {
        let raw = "This is a long line of text that wraps around\nand continues here until it ends.\nShort end.\nNext paragraph starts here and goes on for\nquite a while longer.";
        let p = reflow(raw);
        assert_eq!(p.len(), 2);
        assert!(p[0].starts_with("This is a long line of text that wraps around and continues"));
        assert!(p[0].ends_with("Short end."));
        assert!(p[1].starts_with("Next paragraph"));
    }

    #[test]
    fn rejoins_hyphenated_words_only_before_lowercase() {
        let p = reflow("a theolo-\ngical point\nthat goes on and on and on for ever");
        assert!(p[0].contains("theological"));
        let q = reflow("see the Kant-\nHegel debate and then some more words");
        assert!(q[0].contains("Kant- Hegel"));
    }

    #[test]
    fn blank_lines_break_paragraphs() {
        assert_eq!(reflow("one\n\ntwo").len(), 2);
        assert!(reflow("").is_empty());
    }

    #[test]
    fn collapse_maps_back_to_original_offsets() {
        let (c, map) = collapse_with_map("ab  \n cd");
        assert_eq!(c, "ab cd");
        assert_eq!(map[0], 0);
        assert_eq!(*map.last().unwrap(), 7);
    }
}
