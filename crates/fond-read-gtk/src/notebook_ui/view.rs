use std::cell::{Cell, RefCell};

use gtk4::{gdk, glib};

use super::*;
use crate::notebook::{self, Block, Notebook};

const OBJECT: char = '\u{FFFC}';
const SAVE_DELAY_MS: u64 = 600;

pub(super) type ToastAction<'a> = (&'a str, Rc<dyn Fn()>);

pub(super) struct Card {
    pub(super) anchor: gtk4::TextChildAnchor,
    pub(super) widget: gtk4::Widget,
    pub(super) quote: Rc<RefCell<Quote>>,
    pub(super) subscription: u64,
}

#[derive(Default)]
struct State {
    nb: Option<Notebook>,
    cards: Vec<Card>,
}

/// The editor: a text view for the Typst you write, with a card in it for every quote.
pub struct NotebookView {
    root: adw::ToastOverlay,
    text: gtk4::TextView,
    title_button: gtk4::MenuButton,
    subtitle: gtk4::Label,
    hint: gtk4::Label,
    pub(super) picker: gtk4::Popover,
    pub(super) more: gtk4::Popover,
    dir: PathBuf,
    state: RefCell<State>,
    loading: Cell<bool>,
    timer: RefCell<Option<glib::SourceId>>,
    me: std::rc::Weak<NotebookView>,
}

/// How wide a quote card is in a text view `text_width` wide: the whole line.
fn card_width(text_width: i32) -> i32 {
    (text_width - 28).max(180)
}

fn last_path(dir: &std::path::Path) -> PathBuf {
    dir.join(".last")
}

impl NotebookView {
    pub fn new(dir: PathBuf) -> Rc<NotebookView> {
        let text = gtk4::TextView::new();
        text.set_wrap_mode(gtk4::WrapMode::WordChar);
        text.set_left_margin(12);
        text.set_right_margin(12);
        text.set_top_margin(8);
        text.set_bottom_margin(48);
        text.set_pixels_below_lines(2);
        text.set_vexpand(true);
        text.update_property(&[gtk4::accessible::Property::Label("Notebook text")]);

        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_child(Some(&text));
        scroll.set_vexpand(true);

        let hint = gtk4::Label::new(Some(
            "Write here. Drag highlights in from the Notes list, or use “Add to notebook”.",
        ));
        hint.set_wrap(true);
        hint.set_justify(gtk4::Justification::Center);
        hint.add_css_class("dim-label");
        hint.set_can_target(false);
        hint.set_halign(gtk4::Align::Center);
        hint.set_valign(gtk4::Align::Start);
        hint.set_margin_top(64);
        hint.set_margin_start(32);
        hint.set_margin_end(32);
        let overlay = gtk4::Overlay::new();
        overlay.set_child(Some(&scroll));
        overlay.add_overlay(&hint);

        let title_button = gtk4::MenuButton::new();
        title_button.add_css_class("flat");
        title_button.set_hexpand(true);
        title_button.set_tooltip_text(Some("Switch notebook, or start a new one"));
        let more_button = gtk4::MenuButton::new();
        more_button.set_icon_name("view-more-symbolic");
        more_button.add_css_class("flat");
        more_button.set_tooltip_text(Some("Notebook: rename, shelf, export, delete"));
        let picker = gtk4::Popover::new();
        let more = gtk4::Popover::new();
        title_button.set_popover(Some(&picker));
        more_button.set_popover(Some(&more));

        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        head.set_margin_start(6);
        head.set_margin_end(6);
        head.set_margin_top(4);
        head.append(&title_button);
        head.append(&more_button);
        let subtitle = gtk4::Label::new(None);
        subtitle.add_css_class("dim-label");
        subtitle.add_css_class("caption");
        subtitle.set_xalign(0.0);
        subtitle.set_margin_start(18);

        let column = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        column.append(&head);
        column.append(&subtitle);
        column.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
        column.append(&overlay);
        let root = adw::ToastOverlay::new();
        root.set_child(Some(&column));
        root.add_css_class("notebook");

        let view = Rc::new_cyclic(|me| NotebookView {
            root,
            text,
            title_button,
            subtitle,
            hint,
            picker,
            more,
            dir,
            state: RefCell::new(State::default()),
            loading: Cell::new(false),
            timer: RefCell::new(None),
            me: me.clone(),
        });
        view.wire();
        view
    }

    pub fn widget(&self) -> &adw::ToastOverlay {
        &self.root
    }

    pub fn title(&self) -> String {
        self.state
            .borrow()
            .nb
            .as_ref()
            .map_or_else(|| "Notebook".to_string(), |n| n.title.clone())
    }

    pub(super) fn stem(&self) -> Option<String> {
        self.state.borrow().nb.as_ref().map(|n| n.stem.clone())
    }

    pub(super) fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub(super) fn toast(&self, message: &str, action: Option<ToastAction>) {
        let toast = adw::Toast::new(message);
        if let Some((label, run)) = action {
            toast.set_button_label(Some(label));
            toast.connect_button_clicked(move |_| run());
        }
        self.root.add_toast(toast);
    }

    fn wire(self: &Rc<Self>) {
        let buffer = self.text.buffer();
        {
            let me = self.me.clone();
            buffer.connect_changed(move |b| {
                let Some(me) = me.upgrade() else { return };
                me.hint.set_visible(b.char_count() == 0);
                if !me.loading.get() {
                    me.schedule_save();
                }
            });
        }
        let drop =
            gtk4::DropTarget::new(glib::BoxedAnyObject::static_type(), gdk::DragAction::COPY);
        {
            let me = self.me.clone();
            drop.connect_drop(move |target, value, x, y| {
                let Some(me) = me.upgrade() else { return false };
                me.text.remove_css_class("notebook-drop");
                let Ok(object) = value.get::<glib::BoxedAnyObject>() else {
                    return false;
                };
                let Some(quote) = object.try_borrow::<Quote>().ok().map(|q| q.clone()) else {
                    return false;
                };
                let _ = target;
                let (bx, by) = me.text.window_to_buffer_coords(
                    gtk4::TextWindowType::Widget,
                    x as i32,
                    y as i32,
                );
                let mut iter = me
                    .text
                    .iter_at_location(bx, by)
                    .unwrap_or_else(|| me.text.buffer().end_iter());
                me.insert_quote_at(&mut iter, quote);
                true
            });
        }
        {
            let me = self.me.clone();
            drop.connect_enter(move |_, _, _| {
                if let Some(me) = me.upgrade() {
                    me.text.add_css_class("notebook-drop");
                }
                gdk::DragAction::COPY
            });
        }
        {
            let me = self.me.clone();
            drop.connect_leave(move |_| {
                if let Some(me) = me.upgrade() {
                    me.text.remove_css_class("notebook-drop");
                }
            });
        }
        self.text.add_controller(drop);

        {
            let me = Rc::downgrade(self);
            self.picker.connect_show(move |_| {
                if let Some(me) = me.upgrade() {
                    picker::fill_picker(&me);
                }
            });
        }
        {
            let me = Rc::downgrade(self);
            self.more.connect_show(move |_| {
                if let Some(me) = me.upgrade() {
                    picker::fill_more(&me);
                }
            });
        }
        let me = Rc::downgrade(self);
        self.root.connect_unrealize(move |_| {
            if let Some(me) = me.upgrade() {
                me.flush();
            }
        });
        self.hint.set_visible(true);
        let me = self.me.clone();
        let last = Cell::new(0);
        self.text.add_tick_callback(move |text, _| {
            let width = text.width();
            if width != last.get() && width > 0 {
                last.set(width);
                if let Some(me) = me.upgrade() {
                    for card in me.state.borrow().cards.iter() {
                        card.widget.set_size_request(card_width(width), -1);
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    /// Open the notebook last used, or the newest, or start one. False if none could be opened.
    pub fn ensure_open(self: &Rc<Self>) -> bool {
        if self.state.borrow().nb.is_some() {
            return true;
        }
        let last = std::fs::read_to_string(last_path(&self.dir))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && notebook::path_of(&self.dir, s).exists());
        let stem = last
            .or_else(|| notebook::list(&self.dir).into_iter().next().map(|s| s.stem))
            .or_else(|| {
                notebook::create(&self.dir, "Notebook", None)
                    .ok()
                    .map(|n| n.stem)
            });
        match stem {
            Some(stem) => self.open(&stem),
            None => false,
        }
    }

    pub fn open(self: &Rc<Self>, stem: &str) -> bool {
        self.flush();
        let Some(nb) = notebook::load(&self.dir, stem) else {
            return false;
        };
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = crate::fsutil::write_atomic(&last_path(&self.dir), stem.as_bytes());
        self.populate(nb);
        true
    }

    fn clear_cards(&self) {
        for card in self.state.borrow_mut().cards.drain(..) {
            crate::connections::unsubscribe(card.subscription);
        }
    }

    fn populate(self: &Rc<Self>, nb: Notebook) {
        self.loading.set(true);
        self.clear_cards();
        let buffer = self.text.buffer();
        buffer.set_text("");
        for block in &nb.blocks {
            match block {
                Block::Text(t) => {
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, t);
                }
                Block::Quote(q) => {
                    let mut end = buffer.end_iter();
                    self.place_card(&mut end, q.clone());
                }
            }
        }
        self.loading.set(false);
        self.hint.set_visible(buffer.char_count() == 0);
        self.state.borrow_mut().nb = Some(nb);
        self.refresh_header();
    }

    pub(super) fn refresh_header(&self) {
        let st = self.state.borrow();
        let Some(nb) = st.nb.as_ref() else { return };
        self.title_button.set_label(&nb.title);
        let quotes = st.cards.len();
        let shelf = nb
            .shelf
            .as_deref()
            .map_or("Stand-alone".to_string(), |s| format!("Shelf: {s}"));
        self.subtitle.set_text(&format!(
            "{shelf} · {quotes} quote{}",
            if quotes == 1 { "" } else { "s" }
        ));
    }

    /// Put a card for `quote` at `iter`, which must start a line.
    fn place_card(self: &Rc<Self>, iter: &mut gtk4::TextIter, mut quote: Quote) {
        super::refresh_from_source(&mut quote);
        let buffer = self.text.buffer();
        let anchor = buffer.create_child_anchor(iter);
        buffer.insert(iter, "\n");
        let quote = Rc::new(RefCell::new(quote));
        let (widget, subscription) = card::build(self, quote.clone(), &anchor);
        self.text.add_child_at_anchor(&widget, &anchor);
        widget.set_size_request(card_width(self.text.width()), -1);
        self.state.borrow_mut().cards.push(Card {
            anchor,
            widget,
            quote,
            subscription,
        });
    }

    fn insert_quote_at(self: &Rc<Self>, iter: &mut gtk4::TextIter, quote: Quote) {
        if self.state.borrow().nb.is_none() && !self.ensure_open() {
            return;
        }
        let buffer = self.text.buffer();
        if !iter.starts_line() {
            buffer.insert(iter, "\n");
        }
        self.place_card(iter, quote);
        self.refresh_header();
        let mark = buffer.create_mark(None, iter, false);
        self.text.scroll_mark_onscreen(&mark);
        buffer.delete_mark(&mark);
    }

    /// Add a quote at the end of the notebook.
    pub fn add_quote(self: &Rc<Self>, quote: Quote) {
        let buffer = self.text.buffer();
        let mut end = buffer.end_iter();
        self.insert_quote_at(&mut end, quote);
        self.flush();
    }

    pub(super) fn remove_card(&self, anchor: &gtk4::TextChildAnchor) {
        let buffer = self.text.buffer();
        let (start, end) = buffer.bounds();
        let slice = buffer.slice(&start, &end, true);
        for (offset, ch) in slice.chars().enumerate() {
            if ch != OBJECT {
                continue;
            }
            let at = buffer.iter_at_offset(offset as i32);
            if at.child_anchor().as_ref() == Some(anchor) {
                let mut after = at;
                after.forward_char();
                if after.char() == '\n' {
                    after.forward_char();
                }
                let mut from = at;
                buffer.delete(&mut from, &mut after);
                break;
            }
        }
        let mut st = self.state.borrow_mut();
        if let Some(pos) = st.cards.iter().position(|c| &c.anchor == anchor) {
            let card = st.cards.remove(pos);
            crate::connections::unsubscribe(card.subscription);
        }
        drop(st);
        self.refresh_header();
        self.flush();
    }

    /// The notebook as it stands in the text view.
    pub(super) fn snapshot(&self) -> Option<Notebook> {
        let st = self.state.borrow();
        let mut nb = st.nb.clone()?;
        let buffer = self.text.buffer();
        let (start, end) = buffer.bounds();
        let slice = buffer.slice(&start, &end, true);
        let mut blocks: Vec<Block> = Vec::new();
        let mut segment = String::new();
        let mut skip_newline = false;
        for (offset, ch) in slice.chars().enumerate() {
            if ch == OBJECT {
                let anchor = buffer.iter_at_offset(offset as i32).child_anchor();
                let card = anchor.and_then(|a| st.cards.iter().find(|c| c.anchor == a));
                if let Some(card) = card {
                    if !segment.is_empty() {
                        blocks.push(Block::Text(std::mem::take(&mut segment)));
                    }
                    blocks.push(Block::Quote(card.quote.borrow().clone()));
                    skip_newline = true;
                }
                continue;
            }
            if skip_newline {
                skip_newline = false;
                if ch == '\n' {
                    continue;
                }
            }
            segment.push(ch);
        }
        if !segment.is_empty() {
            blocks.push(Block::Text(segment));
        }
        nb.blocks = blocks;
        Some(nb)
    }

    fn schedule_save(&self) {
        if let Some(old) = self.timer.borrow_mut().take() {
            old.remove();
        }
        let me = self.me.clone();
        let id = glib::timeout_add_local_once(
            std::time::Duration::from_millis(SAVE_DELAY_MS),
            move || {
                if let Some(me) = me.upgrade() {
                    me.timer.borrow_mut().take();
                    me.save_now();
                }
            },
        );
        *self.timer.borrow_mut() = Some(id);
    }

    fn save_now(&self) {
        if self.loading.get() {
            return;
        }
        let Some(nb) = self.snapshot() else { return };
        if notebook::save(&self.dir, &nb).is_ok() {
            self.state.borrow_mut().nb = Some(nb);
        } else {
            self.toast("Couldn't save the notebook", None);
        }
    }

    /// Write out anything not yet saved.
    pub fn flush(&self) {
        if let Some(t) = self.timer.borrow_mut().take() {
            t.remove();
        }
        self.save_now();
    }

    /// The open notebook was deleted from outside this view.
    pub(super) fn forget(&self, stem: &str) {
        if self.stem().as_deref() == Some(stem) {
            if let Some(t) = self.timer.borrow_mut().take() {
                t.remove();
            }
            self.loading.set(true);
            self.clear_cards();
            self.text.buffer().set_text("");
            self.loading.set(false);
            self.state.borrow_mut().nb = None;
            self.title_button.set_label("Notebook");
            self.subtitle.set_text("");
        }
    }

    /// Read the open notebook's file again (its shelf was changed behind this view's back).
    pub(super) fn reload(self: &Rc<Self>) {
        if let Some(stem) = self.stem() {
            if let Some(nb) = notebook::load(&self.dir, &stem) {
                self.populate(nb);
            }
        }
    }

    pub(super) fn rename(&self, title: &str) {
        if let Some(nb) = self.state.borrow_mut().nb.as_mut() {
            nb.title = title.to_string();
        }
        self.refresh_header();
        self.flush();
    }

    pub(super) fn set_shelf(&self, shelf: Option<String>) {
        if let Some(nb) = self.state.borrow_mut().nb.as_mut() {
            nb.shelf = shelf;
        }
        self.refresh_header();
        self.flush();
    }

    pub(super) fn shelf(&self) -> Option<String> {
        self.state
            .borrow()
            .nb
            .as_ref()
            .and_then(|n| n.shelf.clone())
    }
}
