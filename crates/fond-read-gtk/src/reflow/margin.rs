//! A column beside a `TextView` holding items level with the lines they belong to — page numbers
//! on the left, sidenotes on the right. Items are widgets built on demand for the part of the text
//! on screen, laid out from their anchors with collisions pushed down, and moved as the text
//! scrolls.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;

const GAP: f64 = 6.0;
/// Items before the first visible one that are still laid out, so a cluster of notes pushed down
/// by earlier ones lands where it would have with everything laid out.
const LEAD_IN: usize = 8;

/// Builds an item's widget; the flag says whether the item is open to its full height.
pub type Builder = Rc<dyn Fn(bool) -> gtk4::Widget>;

pub struct MarginItem {
    mark: gtk4::TextMark,
    build: Builder,
    /// Whether the item is open to its full height; collapsed items are capped by the builder.
    pub expanded: Rc<Cell<bool>>,
}

pub struct MarginColumn {
    pub fixed: gtk4::Fixed,
    view: gtk4::TextView,
    width: Cell<i32>,
    items: RefCell<Vec<MarginItem>>,
    shown: RefCell<HashMap<usize, gtk4::Widget>>,
}

impl MarginColumn {
    pub fn new(view: &gtk4::TextView, width: i32) -> Rc<MarginColumn> {
        let fixed = gtk4::Fixed::new();
        fixed.set_size_request(width, -1);
        fixed.set_overflow(gtk4::Overflow::Hidden);
        Rc::new(MarginColumn {
            fixed,
            view: view.clone(),
            width: Cell::new(width),
            items: RefCell::new(Vec::new()),
            shown: RefCell::new(HashMap::new()),
        })
    }

    /// Change the column's width; the items are rebuilt at the next `refresh`.
    pub fn set_width(&self, width: i32) {
        self.width.set(width);
        self.fixed.set_size_request(width, -1);
    }

    pub fn len(&self) -> usize {
        self.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.borrow().is_empty()
    }

    /// Add an item anchored at buffer offset `offset`, which must not be before the last one's.
    /// Returns its index.
    pub fn add(&self, offset: i32, build: Builder) -> usize {
        let buffer = self.view.buffer();
        // Left gravity: an anchor made where text is about to be inserted stays in front of it.
        let mark = buffer.create_mark(None, &buffer.iter_at_offset(offset), true);
        let mut items = self.items.borrow_mut();
        items.push(MarginItem {
            mark,
            build,
            expanded: Rc::new(Cell::new(false)),
        });
        items.len() - 1
    }

    pub fn expanded(&self, index: usize) -> Option<Rc<Cell<bool>>> {
        self.items.borrow().get(index).map(|i| i.expanded.clone())
    }

    /// The widget for item `index`, if it is on screen.
    pub fn widget(&self, index: usize) -> Option<gtk4::Widget> {
        self.shown.borrow().get(&index).cloned()
    }

    fn anchor_y(&self, item: &MarginItem) -> f64 {
        let buffer = self.view.buffer();
        let iter = buffer.iter_at_mark(&item.mark);
        let rect = self.view.iter_location(&iter);
        let (_, y) =
            self.view
                .buffer_to_window_coords(gtk4::TextWindowType::Widget, rect.x(), rect.y());
        y as f64
    }

    /// Drop the widgets and rebuild them, after a change that alters their look.
    pub fn refresh(&self) {
        for (_, w) in self.shown.borrow_mut().drain() {
            self.fixed.remove(&w);
        }
        self.relayout();
    }

    fn anchor_offset(&self, item: &MarginItem) -> i32 {
        self.view.buffer().iter_at_mark(&item.mark).offset()
    }

    /// The buffer offsets of the text a screen above and below the viewport. Items are found by
    /// offset, which costs nothing, so far-away text never has to be laid out just to place them.
    fn visible_offsets(&self) -> Option<(i32, i32)> {
        let scrolled = self
            .view
            .ancestor(gtk4::ScrolledWindow::static_type())?
            .downcast::<gtk4::ScrolledWindow>()
            .ok()?;
        let adj = scrolled.vadjustment();
        let (top, height) = (adj.value(), adj.page_size().max(1.0));
        let at = |y: f64| {
            self.view
                .iter_at_location(0, y.max(0.0) as i32)
                .map(|i| i.offset())
        };
        let from = at(top - height).unwrap_or(0);
        let to = at(top + 2.0 * height).unwrap_or(i32::MAX);
        Some((from, to))
    }

    /// Place the items near the viewport level with their anchors.
    pub fn relayout(&self) {
        let height = self.view.height() as f64;
        if height < 1.0 {
            return;
        }
        let items = self.items.borrow();
        if items.is_empty() {
            return;
        }
        let Some((from, to)) = self.visible_offsets() else {
            return;
        };
        let first = items.partition_point(|i| self.anchor_offset(i) < from);
        let start = first.saturating_sub(LEAD_IN);
        let mut shown = self.shown.borrow_mut();
        let mut keep: Vec<usize> = Vec::new();
        let mut bottom = f64::NEG_INFINITY;
        for (index, item) in items.iter().enumerate().skip(start) {
            if self.anchor_offset(item) > to {
                break;
            }
            let anchor = self.anchor_y(item);
            let widget = shown
                .entry(index)
                .or_insert_with(|| {
                    let w = (item.build)(item.expanded.get());
                    w.set_size_request(self.width.get(), -1);
                    self.fixed.put(&w, 0.0, anchor);
                    w
                })
                .clone();
            let (_, natural, _, _) = widget.measure(gtk4::Orientation::Vertical, self.width.get());
            let y = anchor.max(bottom + GAP);
            self.fixed.move_(&widget, 0.0, y);
            bottom = y + natural as f64;
            keep.push(index);
        }
        let stale: Vec<usize> = shown
            .keys()
            .copied()
            .filter(|i| !keep.contains(i))
            .collect();
        for index in stale {
            if let Some(w) = shown.remove(&index) {
                self.fixed.remove(&w);
            }
        }
    }
}
