//! The Reading view: a PDF's flow shown in a read-only `TextView`, restyled from the shared
//! typography, with page numbers in a left margin and notes in a right margin level with the
//! lines that cite them.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use gtk4::prelude::*;

use super::margin::{Builder, MarginColumn};
use super::model::{Item, Paragraph, SourceWord};
use super::thread::{ReflowEvent, ReflowHandle};
use crate::typography::Typography;

const LEFT_WIDTH: i32 = 64;

type PageLabelClick = Rc<dyn Fn(u16, &gtk4::Widget)>;

/// One saved mark to paint onto the text: which page, the quoted text and (when known) where it
/// sits on the page, its colour and style.
pub struct TextMark {
    pub page: u16,
    pub quote: String,
    /// The mark's rectangles in page points as displayed (origin top left), when it has any.
    pub rects: Vec<[f32; 4]>,
    pub rgba: [u8; 4],
    pub style: MarkStyle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MarkStyle {
    Highlight,
    Underline,
    Strikeout,
}

/// Collapse every run of whitespace to one space.
pub fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct ParaRec {
    /// Buffer offset of the first char of the paragraph's text.
    start: i32,
    /// Buffer offset just past its last char.
    end: i32,
    /// (index into the paragraph's text, chars inserted there): the note markers in the text.
    inserted: Vec<(usize, usize)>,
    source: Vec<SourceWord>,
}

impl ParaRec {
    /// Buffer offset of the char at `idx`; markers at `idx` come before it when `before_markers`
    /// is false and after it when true.
    fn offset(&self, idx: usize, before_markers: bool) -> i32 {
        let shift: usize = self
            .inserted
            .iter()
            .filter(|(at, _)| {
                if before_markers {
                    *at < idx
                } else {
                    *at <= idx
                }
            })
            .map(|(_, len)| len)
            .sum();
        self.start + idx as i32 + shift as i32
    }
}

/// What the time-sliced loop works through, in order.
enum Entry {
    Item(Item),
    /// The layout has reached this page.
    Loaded(u16),
    Done,
}

struct MarkerRec {
    offset: i32,
    len: i32,
    note: Option<usize>,
}

pub struct ReadingView {
    pub scroll: gtk4::ScrolledWindow,
    pub text_view: gtk4::TextView,
    clamp: libadwaita::Clamp,
    left: Rc<MarginColumn>,
    right: Rc<MarginColumn>,
    right_width: Cell<i32>,
    total: u16,
    loaded_to: Cell<u16>,
    paras: RefCell<Vec<ParaRec>>,
    page_starts: RefCell<Vec<(i32, u16)>>,
    markers: RefCell<Vec<MarkerRec>>,
    css: gtk4::CssProvider,
    applied_tags: RefCell<Vec<String>>,
    handle: RefCell<Option<ReflowHandle>>,
    page_label: Rc<dyn Fn(u16) -> String>,
    /// Called when a page number in the left margin is clicked.
    pub on_page_label_click: RefCell<Option<PageLabelClick>>,
    /// Called once layout finishes with nothing to show, for a document with no text layer.
    pub on_no_text: RefCell<Option<Rc<dyn Fn()>>>,
    /// Called after more of the document has been laid out into the text.
    pub on_content: RefCell<Option<Rc<dyn Fn()>>>,
    relayout_pending: Cell<bool>,
    queue: RefCell<VecDeque<Entry>>,
    draining: Cell<bool>,
    content_pending: Cell<bool>,
    active_marker: Cell<Option<(i32, i32)>>,
}

fn escape(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

impl ReadingView {
    pub fn new(total: u16, page_label: Rc<dyn Fn(u16) -> String>) -> Rc<ReadingView> {
        crate::style::ensure();
        let text_view = gtk4::TextView::new();
        text_view.set_editable(false);
        text_view.set_cursor_visible(true);
        text_view.set_wrap_mode(gtk4::WrapMode::Word);
        text_view.set_widget_name("fond-reflow");
        text_view.set_top_margin(24);
        text_view.set_bottom_margin(48);
        text_view.set_hexpand(true);
        text_view.update_property(&[gtk4::accessible::Property::Label(
            "Document text. Select with Shift and the arrow keys, then press 1 to 4 to mark it.",
        )]);

        let left = MarginColumn::new(&text_view, LEFT_WIDTH);
        let right = MarginColumn::new(&text_view, 230);
        text_view.set_gutter(gtk4::TextWindowType::Left, Some(&left.fixed));
        text_view.set_gutter(gtk4::TextWindowType::Right, Some(&right.fixed));

        let clamp = libadwaita::Clamp::new();
        clamp.set_child(Some(&text_view));
        clamp.set_halign(gtk4::Align::Fill);
        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_hexpand(true);
        scroll.set_child(Some(&clamp));

        let css = gtk4::CssProvider::new();
        #[allow(deprecated)]
        text_view
            .style_context()
            .add_provider(&css, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);

        let buffer = text_view.buffer();
        let accent = {
            #[allow(deprecated)]
            text_view.style_context().lookup_color("accent_color")
        }
        .unwrap_or(gtk4::gdk::RGBA::BLUE);
        for (name, scale) in [("h1", 1.6f64), ("h2", 1.35), ("h3", 1.15)] {
            buffer.create_tag(
                Some(name),
                &[
                    ("scale", &scale),
                    ("weight", &700i32),
                    ("pixels-above-lines", &20i32),
                    ("pixels-below-lines", &10i32),
                ],
            );
        }
        buffer.create_tag(
            Some("marker"),
            &[
                ("scale", &0.7f64),
                ("rise", &5000i32),
                ("foreground-rgba", &accent),
            ],
        );
        buffer.create_tag(Some("para"), &[]);
        buffer.create_tag(
            Some("marker-active"),
            &[(
                "background-rgba",
                &gtk4::gdk::RGBA::new(accent.red(), accent.green(), accent.blue(), 0.25),
            )],
        );

        let view = Rc::new(ReadingView {
            scroll,
            text_view,
            clamp,
            left,
            right,
            right_width: Cell::new(230),
            total,
            loaded_to: Cell::new(0),
            paras: RefCell::new(Vec::new()),
            page_starts: RefCell::new(Vec::new()),
            markers: RefCell::new(Vec::new()),
            css,
            applied_tags: RefCell::new(Vec::new()),
            handle: RefCell::new(None),
            page_label,
            on_page_label_click: RefCell::new(None),
            on_no_text: RefCell::new(None),
            on_content: RefCell::new(None),
            relayout_pending: Cell::new(false),
            queue: RefCell::new(VecDeque::new()),
            draining: Cell::new(false),
            content_pending: Cell::new(false),
            active_marker: Cell::new(None),
        });
        view.install_signals();
        let weak: Weak<ReadingView> = Rc::downgrade(&view);
        crate::typography::shared().watch(move |t| {
            if let Some(view) = weak.upgrade() {
                view.apply_typography(t);
            }
        });
        view
    }

    fn install_signals(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.scroll.vadjustment().connect_value_changed({
            let this = this.clone();
            move |_| {
                if let Some(v) = this.upgrade() {
                    v.relayout_margins();
                }
            }
        });
        self.scroll.vadjustment().connect_changed({
            let this = this.clone();
            move |_| {
                if let Some(v) = this.upgrade() {
                    v.schedule_relayout();
                }
            }
        });
        let last_size = Cell::new((0, 0));
        self.text_view.add_tick_callback({
            let this = this.clone();
            move |widget, _| {
                let size = (widget.width(), widget.height());
                if size != last_size.get() {
                    last_size.set(size);
                    if let Some(v) = this.upgrade() {
                        v.schedule_relayout();
                    }
                }
                glib::ControlFlow::Continue
            }
        });

        let motion = gtk4::EventControllerMotion::new();
        motion.set_propagation_phase(gtk4::PropagationPhase::Capture);
        {
            let this = this.clone();
            motion.connect_motion(move |_, x, y| {
                if let Some(v) = this.upgrade() {
                    v.hover_at(x, y);
                }
            });
        }
        self.text_view.add_controller(motion);

        let click = gtk4::GestureClick::new();
        click.set_button(gtk4::gdk::BUTTON_PRIMARY);
        click.set_propagation_phase(gtk4::PropagationPhase::Capture);
        click.connect_released(move |_, _, x, y| {
            if let Some(v) = this.upgrade() {
                if let Some(note) = v.note_at(x, y) {
                    v.toggle_note(note);
                }
            }
        });
        self.text_view.add_controller(click);
    }

    fn schedule_relayout(self: &Rc<Self>) {
        if self.relayout_pending.replace(true) {
            return;
        }
        let this = self.clone();
        glib::idle_add_local_once(move || {
            this.relayout_pending.set(false);
            this.relayout_margins();
        });
    }

    fn relayout_margins(&self) {
        self.left.relayout();
        self.right.relayout();
    }

    fn marker_at_offset(&self, offset: i32) -> Option<(i32, i32, Option<usize>)> {
        let markers = self.markers.borrow();
        let i = markers.partition_point(|m| m.offset + m.len <= offset);
        markers
            .get(i)
            .filter(|m| m.offset <= offset)
            .map(|m| (m.offset, m.len, m.note))
    }

    fn offset_at(&self, x: f64, y: f64) -> Option<i32> {
        let (bx, by) = self.text_view.window_to_buffer_coords(
            gtk4::TextWindowType::Widget,
            x as i32,
            y as i32,
        );
        self.text_view
            .iter_at_location(bx, by)
            .map(|iter| iter.offset())
    }

    fn note_at(&self, x: f64, y: f64) -> Option<usize> {
        let offset = self.offset_at(x, y)?;
        self.marker_at_offset(offset).and_then(|(_, _, note)| note)
    }

    fn hover_at(&self, x: f64, y: f64) {
        let hit = self.offset_at(x, y).and_then(|o| self.marker_at_offset(o));
        let range = hit.map(|(offset, len, _)| (offset, len));
        if range == self.active_marker.get() {
            return;
        }
        self.set_active_marker(range);
        for index in 0..self.right.len() {
            if let Some(w) = self.right.widget(index) {
                match hit.and_then(|(_, _, note)| note) {
                    Some(n) if n == index => w.add_css_class("reading-note-active"),
                    _ => w.remove_css_class("reading-note-active"),
                }
            }
        }
    }

    fn set_active_marker(&self, range: Option<(i32, i32)>) {
        let buffer = self.text_view.buffer();
        if let Some(tag) = buffer.tag_table().lookup("marker-active") {
            buffer.remove_tag(&tag, &buffer.start_iter(), &buffer.end_iter());
            if let Some((offset, len)) = range {
                buffer.apply_tag(
                    &tag,
                    &buffer.iter_at_offset(offset),
                    &buffer.iter_at_offset(offset + len),
                );
            }
        }
        self.active_marker.set(range);
    }

    fn toggle_note(&self, note: usize) {
        if let Some(expanded) = self.right.expanded(note) {
            expanded.set(!expanded.get());
            self.right.refresh();
        }
    }

    fn apply_typography(self: &Rc<Self>, t: &Typography) {
        self.css.load_from_data(&t.text_view_css());
        let px = 18.0 * t.size;
        let buffer = self.text_view.buffer();
        if let Some(tag) = buffer.tag_table().lookup("para") {
            let (indent, below) = match t.paragraph {
                crate::typography::ParagraphStyle::Indent => ((px * 1.5) as i32, 2),
                crate::typography::ParagraphStyle::Space => (0, (px * 0.9) as i32),
            };
            tag.set_property("indent", indent);
            tag.set_property("pixels-below-lines", below);
            tag.set_property(
                "justification",
                if t.justify {
                    gtk4::Justification::Fill
                } else {
                    gtk4::Justification::Left
                },
            );
        }
        self.text_view.set_left_margin(t.margin as i32 + 8);
        self.text_view.set_right_margin(t.margin as i32 + 8);
        let note_width = ((t.width as f64 * 0.33) as i32).clamp(190, 300);
        self.right_width.set(note_width);
        self.right.set_width(note_width);
        let total = LEFT_WIDTH + 2 * (t.margin as i32 + 8) + t.width as i32 + note_width;
        self.clamp.set_maximum_size(total);
        self.clamp.set_tightening_threshold(total);
        self.left.refresh();
        self.right.refresh();
    }

    /// Scale the text by `factor` (1.25 = 25% larger).
    pub fn scale_font(&self, factor: f64) {
        crate::typography::shared().update(|t| t.size *= factor);
    }

    /// Start laying the document at `path` out and showing it as it is ready. A no-op once started.
    pub fn load(self: &Rc<Self>, path: std::path::PathBuf) {
        self.load_with(path, false);
    }

    /// As `load`, recognising text on pages that have none when `recognise` is set. A no-op while a
    /// layout is already under way.
    pub fn load_with(self: &Rc<Self>, path: std::path::PathBuf, recognise: bool) {
        if self.handle.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        let handle = super::thread::spawn(
            path,
            recognise,
            Rc::new(move |event| {
                let Some(view) = weak.upgrade() else {
                    return;
                };
                {
                    let mut queue = view.queue.borrow_mut();
                    match event {
                        ReflowEvent::Items(items, upto) => {
                            queue.extend(items.into_iter().map(Entry::Item));
                            queue.push_back(Entry::Loaded(upto));
                        }
                        ReflowEvent::Done => queue.push_back(Entry::Done),
                    }
                }
                view.drain_soon();
            }),
        );
        *self.handle.borrow_mut() = Some(handle);
    }

    /// Work through the queue a few milliseconds at a time between frames, so a long document
    /// arriving does not freeze the window.
    fn drain_soon(self: &Rc<Self>) {
        if self.draining.replace(true) {
            return;
        }
        let this = self.clone();
        glib::idle_add_local(move || {
            let started = std::time::Instant::now();
            while started.elapsed() < std::time::Duration::from_millis(6) {
                let Some(entry) = this.queue.borrow_mut().pop_front() else {
                    break;
                };
                match entry {
                    Entry::Item(item) => this.append(&item),
                    Entry::Loaded(upto) => {
                        this.loaded_to.set(upto.max(this.loaded_to.get()));
                        this.content_changed(false);
                    }
                    Entry::Done => {
                        this.loaded_to.set(this.total);
                        this.content_changed(true);
                        if this.paras.borrow().is_empty() && this.page_starts.borrow().is_empty() {
                            let on_no_text = this.on_no_text.borrow().clone();
                            if let Some(f) = on_no_text {
                                f();
                            }
                        }
                    }
                }
            }
            this.schedule_relayout();
            if this.queue.borrow().is_empty() {
                this.draining.set(false);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    /// Tell the owner more text is in, at most a few times a second (at once when `now`).
    fn content_changed(self: &Rc<Self>, now: bool) {
        let call = |this: &Rc<Self>| {
            let f = this.on_content.borrow().clone();
            if let Some(f) = f {
                f();
            }
        };
        if now {
            call(self);
        } else if !self.content_pending.replace(true) {
            let this = self.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                this.content_pending.set(false);
                call(&this);
            });
        }
    }

    fn note_page_start(&self, offset: i32, page: u16) {
        let mut starts = self.page_starts.borrow_mut();
        if starts.last().map_or(true, |(_, p)| *p < page) {
            starts.push((offset, page));
            drop(starts);
            let label = (self.page_label)(page);
            let click = self.on_page_label_click.borrow().clone();
            let build: Builder = Rc::new(move |_| {
                let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
                let text = gtk4::Label::new(None);
                text.set_markup(&format!("<span size=\"small\">{}</span>", escape(&label)));
                text.set_hexpand(true);
                text.set_xalign(1.0);
                text.add_css_class("dim-label");
                let tick = gtk4::Separator::new(gtk4::Orientation::Horizontal);
                tick.set_size_request(10, 1);
                tick.set_valign(gtk4::Align::Center);
                row.append(&text);
                row.append(&tick);
                if let Some(click) = click.clone() {
                    let g = gtk4::GestureClick::new();
                    let row_for_click = row.clone();
                    g.connect_released(move |_, _, _, _| {
                        click(page, row_for_click.upcast_ref());
                    });
                    row.add_controller(g);
                    row.set_cursor_from_name(Some("pointer"));
                }
                row.upcast()
            });
            self.left.add(offset, build);
        }
    }

    fn append(&self, item: &Item) {
        let buffer = self.text_view.buffer();
        match item {
            Item::Heading { level, text, page } => {
                let start = buffer.char_count();
                self.note_page_start(start, *page);
                let mut end = buffer.end_iter();
                let tag = match level {
                    1 => "h1",
                    2 => "h2",
                    _ => "h3",
                };
                buffer.insert_with_tags_by_name(&mut end, &format!("{text}\n"), &[tag]);
            }
            Item::Paragraph(p) => self.append_paragraph(p),
        }
    }

    fn append_paragraph(&self, p: &Paragraph) {
        let buffer = self.text_view.buffer();
        let start = buffer.char_count();
        let chars: Vec<char> = p.text.chars().collect();
        let mut markers = p.markers.clone();
        markers.sort_by_key(|m| m.at);
        let mut cursor = 0usize;
        let mut inserted: Vec<(usize, usize)> = Vec::new();
        let mut marker_offsets: Vec<(usize, String, i32, i32)> = Vec::new();
        for m in &markers {
            let upto = m.at.min(chars.len());
            if upto > cursor {
                let piece: String = chars[cursor..upto].iter().collect();
                buffer.insert(&mut buffer.end_iter(), &piece);
                cursor = upto;
            }
            let offset = buffer.char_count();
            let len = m.label.chars().count();
            buffer.insert_with_tags_by_name(&mut buffer.end_iter(), &m.label, &["marker"]);
            inserted.push((m.at, len));
            marker_offsets.push((m.at, m.label.clone(), offset, len as i32));
        }
        if cursor < chars.len() {
            let piece: String = chars[cursor..].iter().collect();
            buffer.insert(&mut buffer.end_iter(), &piece);
        }
        let end = buffer.char_count();
        buffer.insert(&mut buffer.end_iter(), "\n");
        if let Some(tag) = buffer.tag_table().lookup("para") {
            buffer.apply_tag(
                &tag,
                &buffer.iter_at_offset(start),
                &buffer.iter_at_offset(end + 1),
            );
        }
        let rec = ParaRec {
            start,
            end,
            inserted,
            source: p.source.clone(),
        };
        for b in &p.breaks {
            self.note_page_start(rec.offset(b.at, true), b.page);
        }
        // Notes in the right margin, level with the marker that cites them (or, if no marker
        // was found, with the end of the paragraph, flagged).
        let mut note_for_marker: Vec<(i32, usize)> = Vec::new();
        for note in &p.notes {
            let anchor = note.anchor.and_then(|at| {
                marker_offsets
                    .iter()
                    .find(|(a, label, _, _)| *a == at && *label == note.label)
                    .map(|(_, _, offset, _)| *offset)
            });
            let offset = anchor.unwrap_or(end);
            let matched = anchor.is_some();
            let label = note.label.clone();
            let text = note.text.clone();
            let add_index = self.right.add(
                offset,
                Rc::new(move |expanded| note_widget(&label, &text, matched, expanded)),
            );
            if let Some(o) = anchor {
                note_for_marker.push((o, add_index));
            }
        }
        {
            let mut recs = self.markers.borrow_mut();
            for (_, _, offset, len) in &marker_offsets {
                let note = note_for_marker
                    .iter()
                    .find(|(o, _)| o == offset)
                    .map(|(_, i)| *i);
                recs.push(MarkerRec {
                    offset: *offset,
                    len: *len,
                    note,
                });
            }
        }
        self.paras.borrow_mut().push(rec);
    }

    /// Forget a finished layout that found nothing, so `load_with` can run again.
    pub fn reset_empty(&self) {
        if self.paras.borrow().is_empty() {
            *self.handle.borrow_mut() = None;
        }
    }

    pub fn loaded_pages(&self) -> u16 {
        self.loaded_to.get()
    }

    fn page_start(&self, page: u16) -> Option<i32> {
        let starts = self.page_starts.borrow();
        starts
            .iter()
            .find(|(_, p)| *p >= page)
            .map(|(offset, _)| *offset)
    }

    pub fn scroll_to_page(&self, page: u16) {
        let Some(offset) = self.page_start(page) else {
            return;
        };
        let buffer = self.text_view.buffer();
        let iter = buffer.iter_at_offset(offset);
        let mark = buffer.create_mark(None, &iter, true);
        self.text_view.scroll_to_mark(&mark, 0.0, true, 0.0, 0.0);
        buffer.place_cursor(&iter);
        buffer.delete_mark(&mark);
    }

    pub fn page_at_offset(&self, offset: i32) -> u16 {
        let starts = self.page_starts.borrow();
        let i = starts.partition_point(|(o, _)| *o <= offset);
        starts
            .get(i.saturating_sub(1))
            .map(|(_, p)| *p)
            .unwrap_or(0)
            .min(self.total.saturating_sub(1))
    }

    /// The page whose text is at the top of the viewport.
    pub fn visible_page(&self) -> u16 {
        let iter = self
            .text_view
            .iter_at_location(0, 0)
            .unwrap_or_else(|| self.text_view.buffer().start_iter());
        self.page_at_offset(iter.offset())
    }

    fn in_marker(&self, offset: i32) -> bool {
        self.marker_at_offset(offset).is_some()
    }

    /// The selected text — note markers excluded — and the page its first real character lies on.
    pub fn selection(&self) -> Option<(u16, String)> {
        let buffer = self.text_view.buffer();
        let (start, end) = buffer.selection_bounds()?;
        let first = start.offset();
        let raw = buffer.text(&start, &end, false);
        let mut text = String::new();
        let mut first_real: Option<i32> = None;
        for (i, c) in raw.chars().enumerate() {
            let offset = first + i as i32;
            if self.in_marker(offset) {
                continue;
            }
            if first_real.is_none() && !c.is_whitespace() {
                first_real = Some(offset);
            }
            text.push(if c == '\n' { ' ' } else { c });
        }
        let collapsed = collapse(&text);
        let offset = first_real?;
        (!collapsed.is_empty()).then(|| (self.page_at_offset(offset), collapsed))
    }

    /// Where the selected words sit on their pages, as rectangles in page points (origin top
    /// left), neighbouring words on a line merged. Each comes with its page.
    pub fn selection_rects(&self) -> Vec<(u16, [f32; 4])> {
        let buffer = self.text_view.buffer();
        let Some((start, end)) = buffer.selection_bounds() else {
            return Vec::new();
        };
        let (s, e) = (start.offset(), end.offset());
        let paras = self.paras.borrow();
        let first = paras.partition_point(|p| p.end < s);
        let mut words: Vec<(u16, [f32; 4])> = Vec::new();
        for para in paras.iter().skip(first) {
            if para.start >= e {
                break;
            }
            for w in &para.source {
                let from = para.offset(w.start, true);
                let to = para.offset(w.end, false);
                if to > s && from < e {
                    words.push((w.page, w.bbox));
                }
            }
        }
        merge_rects(words)
    }

    /// Where on screen the selection ends, in the text view's own coordinates.
    pub fn selection_anchor(&self) -> (i32, i32) {
        let buffer = self.text_view.buffer();
        let iter = buffer
            .selection_bounds()
            .map(|(_, end)| end)
            .unwrap_or_else(|| buffer.iter_at_mark(&buffer.get_insert()));
        let rect = self.text_view.iter_location(&iter);
        self.text_view.buffer_to_window_coords(
            gtk4::TextWindowType::Widget,
            rect.x(),
            rect.y() + rect.height(),
        )
    }

    fn mark_tag(&self, mark: &TextMark) -> Option<String> {
        let buffer = self.text_view.buffer();
        let style = match mark.style {
            MarkStyle::Highlight => "h",
            MarkStyle::Underline => "u",
            MarkStyle::Strikeout => "s",
        };
        let name = format!(
            "mark-{style}-{:02x}{:02x}{:02x}{:02x}",
            mark.rgba[0], mark.rgba[1], mark.rgba[2], mark.rgba[3]
        );
        if buffer.tag_table().lookup(&name).is_none() {
            let [r, g, bl, al] = mark.rgba;
            let rgba = format!(
                "rgba({r},{g},{bl},{:.2})",
                (al as f64 / 255.0 * 1.4).min(1.0)
            );
            let colour = gtk4::gdk::RGBA::parse(rgba).unwrap_or(gtk4::gdk::RGBA::BLACK);
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
                MarkStyle::Strikeout => buffer.create_tag(Some(&name), &[("strikethrough", &true)]),
            };
            created?;
        }
        Some(name)
    }

    /// Buffer ranges of the words of `page` whose centres fall inside any of `rects`.
    fn ranges_for_rects(&self, page: u16, rects: &[[f32; 4]]) -> Vec<(i32, i32)> {
        let starts = self.page_starts.borrow();
        let Some(i) = starts.iter().position(|(_, p)| *p == page) else {
            return Vec::new();
        };
        let from = starts[i].0;
        let to = starts.get(i + 1).map(|(o, _)| *o).unwrap_or(i32::MAX);
        drop(starts);
        let paras = self.paras.borrow();
        let first = paras.partition_point(|p| p.end < from);
        let mut ranges: Vec<(i32, i32)> = Vec::new();
        for para in paras.iter().skip(first) {
            if para.start > to {
                break;
            }
            for w in para.source.iter().filter(|w| w.page == page) {
                let cx = (w.bbox[0] + w.bbox[2]) / 2.0;
                let cy = (w.bbox[1] + w.bbox[3]) / 2.0;
                if rects
                    .iter()
                    .any(|r| cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3])
                {
                    let a = para.offset(w.start, true);
                    let b = para.offset(w.end, false);
                    match ranges.last_mut() {
                        Some(last) if a - last.1 <= 1 => last.1 = b,
                        _ => ranges.push((a, b)),
                    }
                }
            }
        }
        ranges
    }

    /// Repaint saved marks onto the text, replacing whatever was painted before. A mark that
    /// knows where it sits on its page is painted on exactly the words there; any other is found
    /// by its quoted text.
    pub fn apply_marks(&self, marks: &[TextMark]) {
        let buffer = self.text_view.buffer();
        for name in self.applied_tags.borrow_mut().drain(..) {
            if let Some(tag) = buffer.tag_table().lookup(&name) {
                buffer.remove_tag(&tag, &buffer.start_iter(), &buffer.end_iter());
            }
        }
        for mark in marks {
            let Some(name) = self.mark_tag(mark) else {
                continue;
            };
            let mut ranges = if mark.rects.is_empty() {
                Vec::new()
            } else {
                self.ranges_for_rects(mark.page, &mark.rects)
            };
            if ranges.is_empty() {
                ranges = self.find_quote(mark).into_iter().collect();
            }
            for (a, b) in ranges {
                buffer.apply_tag_by_name(
                    &name,
                    &buffer.iter_at_offset(a),
                    &buffer.iter_at_offset(b),
                );
            }
            let mut applied = self.applied_tags.borrow_mut();
            if !applied.contains(&name) {
                applied.push(name);
            }
        }
    }

    fn find_quote(&self, mark: &TextMark) -> Option<(i32, i32)> {
        let needle = collapse(&mark.quote);
        if needle.is_empty() {
            return None;
        }
        let buffer = self.text_view.buffer();
        let starts = self.page_starts.borrow().clone();
        let i = starts.iter().position(|(_, p)| *p == mark.page)?;
        let from = starts[i].0;
        let to = starts
            .get(i + 1)
            .map(|(o, _)| *o)
            .unwrap_or_else(|| buffer.char_count());
        let region = buffer
            .text(
                &buffer.iter_at_offset(from),
                &buffer.iter_at_offset(to),
                false,
            )
            .to_string();
        let (collapsed, map) = collapse_with_map(&region);
        let byte_idx = collapsed.find(&needle)?;
        let first = collapsed[..byte_idx].chars().count();
        let last = first + needle.chars().count();
        let a = *map.get(first)?;
        let b = *map.get(last.saturating_sub(1))?;
        Some((from + a as i32, from + b as i32 + 1))
    }
}

fn note_widget(label: &str, text: &str, matched: bool, expanded: bool) -> gtk4::Widget {
    let body = gtk4::Label::new(None);
    let marker = if label.is_empty() {
        String::new()
    } else {
        format!("<sup>{}</sup> ", escape(label))
    };
    let flag = if matched {
        String::new()
    } else {
        "<i>unmatched note</i>  ".to_string()
    };
    body.set_markup(&format!(
        "<span size=\"smaller\">{flag}{marker}{}</span>",
        escape(text)
    ));
    body.set_wrap(true);
    body.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    body.set_xalign(0.0);
    body.set_halign(gtk4::Align::Start);
    body.set_margin_start(10);
    body.set_selectable(true);
    body.set_width_chars(8);
    body.set_max_width_chars(36);
    if !expanded {
        body.set_lines(7);
        body.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    }
    body.add_css_class("dim-label");
    let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    frame.add_css_class("reading-note");
    if !matched {
        frame.add_css_class("warning");
    }
    frame.append(&body);
    frame.upcast()
}

/// Merge words on the same line of the same page into one rectangle each.
fn merge_rects(words: Vec<(u16, [f32; 4])>) -> Vec<(u16, [f32; 4])> {
    let mut out: Vec<(u16, [f32; 4])> = Vec::new();
    for (page, b) in words {
        if let Some((p, last)) = out.last_mut() {
            let height = (last[3] - last[1]).max(1.0);
            let same_line =
                (b[1] - last[1]).abs() < 0.5 * height && (b[3] - last[3]).abs() < 0.5 * height;
            if *p == page && same_line && b[0] >= last[0] - 1.0 {
                last[0] = last[0].min(b[0]);
                last[1] = last[1].min(b[1]);
                last[2] = last[2].max(b[2]);
                last[3] = last[3].max(b[3]);
                continue;
            }
        }
        out.push((page, b));
    }
    out
}

/// `collapse`, plus for each char of the result the char offset it came from in `s`.
fn collapse_with_map(s: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len());
    let mut pending_space = false;
    for (i, c) in s.chars().enumerate() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
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
    fn collapse_maps_back_to_original_offsets() {
        let (c, map) = collapse_with_map("ab  \n cd");
        assert_eq!(c, "ab cd");
        assert_eq!(map[0], 0);
        assert_eq!(*map.last().unwrap(), 7);
    }

    #[test]
    fn neighbouring_words_on_a_line_merge_but_a_new_line_does_not() {
        let merged = merge_rects(vec![
            (0, [10.0, 100.0, 30.0, 110.0]),
            (0, [32.0, 100.0, 60.0, 110.0]),
            (0, [10.0, 112.0, 30.0, 122.0]),
            (1, [10.0, 112.0, 30.0, 122.0]),
        ]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].1, [10.0, 100.0, 60.0, 110.0]);
    }

    #[test]
    fn a_paragraph_maps_text_positions_around_inserted_markers() {
        let rec = ParaRec {
            start: 100,
            end: 120,
            inserted: vec![(5, 2)],
            source: Vec::new(),
        };
        assert_eq!(rec.offset(4, true), 104);
        assert_eq!(rec.offset(5, true), 105);
        assert_eq!(rec.offset(5, false), 107);
        assert_eq!(rec.offset(9, true), 111);
    }
}
