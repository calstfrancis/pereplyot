//! Caret browsing on the page itself (F7): arrow keys move a text caret over the page's own
//! text and Shift+arrows select, so a mark can be made from the keyboard alone, on the real page
//! rather than only in the Text view. The selection is the ordinary one, so 1–4 mark it.

use super::*;

#[derive(Clone)]
pub(super) struct CharBox {
    pub ch: char,
    /// `[left, bottom, right, top]` in PDF user space.
    pub rect: [f64; 4],
}

#[derive(Clone)]
pub(super) struct Caret {
    pub page: u16,
    chars: Rc<Vec<CharBox>>,
    /// Indices of each line's characters, lines in reading order.
    lines: Rc<Vec<Vec<usize>>>,
    /// The caret stands before this character (`chars.len()` is the end of the page).
    pub at: usize,
    /// Where a selection began, if one is being extended.
    anchor: Option<usize>,
    /// The horizontal position kept while moving up and down.
    want_x: Option<f64>,
}

fn lines_of(chars: &[CharBox]) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut centre = f64::NAN;
    let mut height: f64 = 0.0;
    for (i, c) in chars.iter().enumerate() {
        let cy = (c.rect[1] + c.rect[3]) / 2.0;
        let h = (c.rect[3] - c.rect[1]).abs().max(1.0);
        if lines.is_empty() || (cy - centre).abs() > 0.6 * height.max(h) {
            lines.push(Vec::new());
            centre = cy;
            height = h;
        }
        lines.last_mut().expect("a line").push(i);
    }
    lines
}

/// The characters of `page` with their boxes, in the order PDFium reads them.
fn read_chars(r: &ReaderState, page: u16) -> Vec<CharBox> {
    let Some(doc) = r.doc.as_ref() else {
        return Vec::new();
    };
    let Ok(pdf_page) = doc.pages().get(page) else {
        return Vec::new();
    };
    let Ok(text) = pdf_page.text() else {
        return Vec::new();
    };
    text.chars()
        .iter()
        .filter_map(|c| {
            let ch = c.unicode_char()?;
            if ch.is_control() {
                return None;
            }
            let b = c.loose_bounds().ok()?;
            let rect = [
                b.left().value as f64,
                b.bottom().value as f64,
                b.right().value as f64,
                b.top().value as f64,
            ];
            (rect[2] > rect[0] || ch.is_whitespace()).then_some(CharBox { ch, rect })
        })
        .collect()
}

impl Caret {
    fn load(r: &ReaderState, page: u16, at_end: bool) -> Option<Caret> {
        let chars = read_chars(r, page);
        if chars.is_empty() {
            return None;
        }
        let lines = lines_of(&chars);
        let at = if at_end { chars.len() - 1 } else { 0 };
        Some(Caret {
            page,
            chars: Rc::new(chars),
            lines: Rc::new(lines),
            at,
            anchor: None,
            want_x: None,
        })
    }

    fn line_of(&self, i: usize) -> usize {
        let i = i.min(self.chars.len().saturating_sub(1));
        self.lines.iter().position(|l| l.contains(&i)).unwrap_or(0)
    }

    fn x_of(&self, i: usize) -> f64 {
        match self.chars.get(i) {
            Some(c) => c.rect[0],
            None => self.chars.last().map_or(0.0, |c| c.rect[2]),
        }
    }

    /// The caret as a thin quad in PDF space, for drawing.
    pub fn quad(&self) -> [f64; 8] {
        let c = self
            .chars
            .get(self.at)
            .or_else(|| self.chars.last())
            .expect("a caret has characters");
        let x = self.x_of(self.at);
        let (b, t) = (c.rect[1], c.rect[3]);
        [x, t, x + 1.4, t, x, b, x + 1.4, b]
    }

    /// The selected characters' text and one quad per line.
    fn selection(&self) -> Option<(String, Vec<[f64; 8]>)> {
        let a = self.anchor?;
        let (lo, hi) = (a.min(self.at), a.max(self.at));
        if lo == hi {
            return None;
        }
        let mut text = String::new();
        let mut quads = Vec::new();
        let mut last_line = usize::MAX;
        let mut rect: Option<[f64; 4]> = None;
        let flush = |rect: &mut Option<[f64; 4]>, quads: &mut Vec<[f64; 8]>| {
            if let Some(r) = rect.take() {
                quads.push([r[0], r[3], r[2], r[3], r[0], r[1], r[2], r[1]]);
            }
        };
        for i in lo..hi.min(self.chars.len()) {
            let line = self.line_of(i);
            if line != last_line {
                flush(&mut rect, &mut quads);
                if !text.is_empty() && !text.ends_with(' ') {
                    text.push(' ');
                }
                last_line = line;
            }
            let c = &self.chars[i];
            text.push(c.ch);
            rect = Some(match rect {
                Some(r) => [
                    r[0].min(c.rect[0]),
                    r[1].min(c.rect[1]),
                    r[2].max(c.rect[2]),
                    r[3].max(c.rect[3]),
                ],
                None => c.rect,
            });
        }
        flush(&mut rect, &mut quads);
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!text.is_empty()).then_some((text, quads))
    }

    fn word_forward(&self, mut i: usize) -> usize {
        let n = self.chars.len();
        while i < n && !self.chars[i].ch.is_whitespace() {
            i += 1;
        }
        while i < n && self.chars[i].ch.is_whitespace() {
            i += 1;
        }
        i
    }

    fn word_back(&self, mut i: usize) -> usize {
        while i > 0 && self.chars[i - 1].ch.is_whitespace() {
            i -= 1;
        }
        while i > 0 && !self.chars[i - 1].ch.is_whitespace() {
            i -= 1;
        }
        i
    }

    fn nearest_in_line(&self, line: usize, x: f64) -> usize {
        self.lines[line]
            .iter()
            .copied()
            .min_by(|a, b| {
                (self.x_of(*a) - x)
                    .abs()
                    .partial_cmp(&(self.x_of(*b) - x).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(0)
    }
}

/// Make the page's selection and the drawn caret follow `caret`.
fn apply(reader: &Rc<RefCell<ReaderState>>) {
    {
        let mut r = reader.borrow_mut();
        let selection = r
            .caret
            .as_ref()
            .and_then(|c| c.selection().map(|s| (c.page, s)));
        r.last_selection = selection.map(|(page, (text, quads))| (page, text, quads));
    }
    mark_layer::redraw_all(reader);
}

pub(super) fn is_active(reader: &Rc<RefCell<ReaderState>>) -> bool {
    reader.borrow().caret.is_some()
}

pub(super) fn stop(reader: &Rc<RefCell<ReaderState>>) {
    let had = reader.borrow_mut().caret.take().is_some();
    if had {
        reader.borrow_mut().last_selection = None;
        mark_layer::redraw_all(reader);
    }
}

/// F7: start caret browsing at the top of the page being read, or end it.
pub(super) fn toggle(ui: &PdfUi) {
    if is_active(&ui.reader) {
        stop(&ui.reader);
        ui.host.notify("Caret browsing off");
        return;
    }
    let caret = {
        let r = ui.reader.borrow();
        Caret::load(&r, r.page, false)
    };
    match caret {
        Some(c) => {
            ui.reader.borrow_mut().caret = Some(c);
            apply(&ui.reader);
            ui.host.notify(
                "Caret browsing: arrows move, Shift+arrows select, 1–4 mark, F7 or Esc ends",
            );
        }
        None => ui.host.notify("This page has no text to browse"),
    }
}

/// A key while caret browsing; true if it was used.
pub(super) fn handle_key(ui: &PdfUi, key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
    let reader = &ui.reader;
    let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
    let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
    if matches!(key, gdk::Key::Escape | gdk::Key::F7) {
        stop(reader);
        return true;
    }
    let moves = matches!(
        key,
        gdk::Key::Left
            | gdk::Key::Right
            | gdk::Key::Up
            | gdk::Key::Down
            | gdk::Key::Home
            | gdk::Key::End
            | gdk::Key::KP_Left
            | gdk::Key::KP_Right
            | gdk::Key::KP_Up
            | gdk::Key::KP_Down
    );
    if !moves || modifiers.contains(gdk::ModifierType::ALT_MASK) {
        return false;
    }
    let Some(mut caret) = reader.borrow().caret.clone() else {
        return false;
    };
    // A mark made since consumed the selection.
    if reader.borrow().last_selection.is_none() {
        caret.anchor = None;
    }
    let before = caret.at;
    let n = caret.chars.len();
    let mut keep_x = false;
    let mut cross: Option<(i32, f64)> = None;
    match key {
        gdk::Key::Right | gdk::Key::KP_Right => {
            if caret.at + 1 >= n && !ctrl {
                cross = Some((1, f64::NAN));
            } else {
                caret.at = if ctrl {
                    caret.word_forward(caret.at).min(n - 1)
                } else {
                    caret.at + 1
                };
            }
        }
        gdk::Key::Left | gdk::Key::KP_Left => {
            if caret.at == 0 {
                cross = Some((-1, f64::NAN));
            } else {
                caret.at = if ctrl {
                    caret.word_back(caret.at)
                } else {
                    caret.at - 1
                };
            }
        }
        gdk::Key::Home => {
            let line = caret.line_of(caret.at);
            caret.at = caret.lines[line][0];
        }
        gdk::Key::End => {
            let line = caret.line_of(caret.at);
            caret.at = *caret.lines[line].last().unwrap_or(&0);
        }
        gdk::Key::Up | gdk::Key::KP_Up | gdk::Key::Down | gdk::Key::KP_Down => {
            let down = matches!(key, gdk::Key::Down | gdk::Key::KP_Down);
            let x = caret.want_x.unwrap_or_else(|| caret.x_of(caret.at));
            let line = caret.line_of(caret.at);
            let target = if down { line + 1 } else { line.wrapping_sub(1) };
            if target < caret.lines.len() {
                caret.at = caret.nearest_in_line(target, x);
                caret.want_x = Some(x);
                keep_x = true;
            } else {
                cross = Some((if down { 1 } else { -1 }, x));
            }
        }
        _ => {}
    }
    if let Some((dir, x)) = cross {
        let (page, count) = {
            let r = reader.borrow();
            (r.page as i32 + dir, r.count as i32)
        };
        if page < 0 || page >= count {
            return true;
        }
        let page = page as u16;
        let loaded = {
            let r = reader.borrow();
            Caret::load(&r, page, dir < 0)
        };
        let Some(mut next) = loaded else {
            return true;
        };
        if !x.is_nan() {
            let line = if dir > 0 { 0 } else { next.lines.len() - 1 };
            next.at = next.nearest_in_line(line, x);
            next.want_x = Some(x);
        } else if dir < 0 {
            next.at = next.chars.len() - 1;
        }
        next.anchor = if shift {
            caret.anchor.filter(|_| false)
        } else {
            None
        };
        jump(reader, page, JumpKind::Key);
        reader.borrow_mut().caret = Some(next);
        apply(reader);
        return true;
    }
    if !keep_x {
        caret.want_x = None;
    }
    if shift {
        caret.anchor.get_or_insert(before);
    } else {
        caret.anchor = None;
    }
    reader.borrow_mut().caret = Some(caret);
    apply(reader);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caret_over(text: &[&str]) -> Caret {
        let mut chars = Vec::new();
        for (row, line) in text.iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                let x = col as f64 * 6.0;
                let y = 700.0 - row as f64 * 14.0;
                chars.push(CharBox {
                    ch,
                    rect: [x, y, x + 6.0, y + 12.0],
                });
            }
        }
        let lines = lines_of(&chars);
        Caret {
            page: 0,
            chars: Rc::new(chars),
            lines: Rc::new(lines),
            at: 0,
            anchor: None,
            want_x: None,
        }
    }

    #[test]
    fn characters_group_into_lines_in_reading_order() {
        let c = caret_over(&["one two", "three", "four"]);
        assert_eq!(c.lines.len(), 3);
        assert_eq!(c.lines[0].len(), 7);
        assert_eq!(c.line_of(8), 1);
    }

    #[test]
    fn a_selection_is_the_words_between_anchor_and_caret_with_a_quad_per_line() {
        let mut c = caret_over(&["one two", "three"]);
        c.anchor = Some(4);
        c.at = 10;
        let (text, quads) = c.selection().unwrap();
        assert_eq!(text, "two thr");
        assert_eq!(quads.len(), 2);
        c.anchor = None;
        assert!(c.selection().is_none());
    }

    #[test]
    fn words_and_vertical_moves_keep_the_column() {
        let c = caret_over(&["one two", "three four"]);
        assert_eq!(c.word_forward(0), 4);
        assert_eq!(c.word_back(6), 4);
        let x = c.x_of(4);
        assert_eq!(c.nearest_in_line(1, x), c.lines[1][4]);
    }
}
