//! Turning the words of a page into reading order, and the words of a document into a flow of
//! headings, paragraphs and notes. Pure functions over `RawPage`, tested without a PDF.

use std::collections::{HashMap, HashSet};

use super::model::*;

/// A baseline of words.
#[derive(Clone, Debug)]
pub struct Line {
    pub words: Vec<Word>,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub size: f32,
}

impl Line {
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut prev: Option<&Word> = None;
        for w in &self.words {
            if let Some(p) = prev {
                if w.x0 - p.x1 >= 0.1 * self.size {
                    out.push(' ');
                }
            }
            out.push_str(&w.text);
            prev = Some(w);
        }
        out
    }

    fn bold_fraction(&self) -> f32 {
        let total: usize = self.words.iter().map(|w| w.text.chars().count()).sum();
        if total == 0 {
            return 0.0;
        }
        let bold: usize = self
            .words
            .iter()
            .filter(|w| w.bold)
            .map(|w| w.text.chars().count())
            .sum();
        bold as f32 / total as f32
    }
}

/// Group words into lines by baseline, left to right within each line, top to bottom overall.
pub fn lines_of(words: &[Word]) -> Vec<Line> {
    let mut order: Vec<&Word> = words.iter().collect();
    order.sort_by(|a, b| {
        a.centre_y()
            .total_cmp(&b.centre_y())
            .then(a.x0.total_cmp(&b.x0))
    });
    let mut groups: Vec<Vec<&Word>> = Vec::new();
    let mut reference: Vec<(f32, f32)> = Vec::new();
    for w in order {
        let joined = match (groups.last(), reference.last()) {
            (Some(_), Some(&(centre, size))) => (w.centre_y() - centre).abs() <= 0.5 * size,
            _ => false,
        };
        if joined {
            let group = groups.last_mut().expect("a group");
            group.push(w);
            let n = group.len() as f32;
            let r = reference.last_mut().expect("a reference");
            r.0 += (w.centre_y() - r.0) / n;
            r.1 = r.1.max(w.size);
        } else {
            groups.push(vec![w]);
            reference.push((w.centre_y(), w.size));
        }
    }
    let mut lines: Vec<Line> = groups
        .into_iter()
        .map(|mut g| {
            g.sort_by(|a, b| a.x0.total_cmp(&b.x0));
            let size = dominant_size(&g);
            Line {
                x0: g.iter().map(|w| w.x0).fold(f32::INFINITY, f32::min),
                y0: g.iter().map(|w| w.y0).fold(f32::INFINITY, f32::min),
                x1: g.iter().map(|w| w.x1).fold(f32::NEG_INFINITY, f32::max),
                y1: g.iter().map(|w| w.y1).fold(f32::NEG_INFINITY, f32::max),
                size,
                words: g.into_iter().cloned().collect(),
            }
        })
        .collect();
    lines.sort_by(|a, b| a.y1.total_cmp(&b.y1).then(a.x0.total_cmp(&b.x0)));
    lines
}

/// The size most of a line's characters are set in.
fn dominant_size(words: &[&Word]) -> f32 {
    let mut weight: Vec<(f32, usize)> = Vec::new();
    for w in words {
        let n = w.text.chars().count().max(1);
        match weight.iter_mut().find(|(s, _)| (s - w.size).abs() < 0.3) {
            Some(entry) => entry.1 += n,
            None => weight.push((w.size, n)),
        }
    }
    weight
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(s, _)| s)
        .unwrap_or(10.0)
}

fn median_size(words: &[Word]) -> f32 {
    let mut sizes: Vec<f32> = words.iter().map(|w| w.size).collect();
    sizes.sort_by(f32::total_cmp);
    sizes.get(sizes.len() / 2).copied().unwrap_or(10.0)
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    X,
    Y,
}

/// The widest empty strip across `words` on `axis` that is at least `min_gap` wide, as the
/// coordinate in its middle.
fn widest_gap(words: &[Word], axis: Axis, min_gap: f32) -> Option<f32> {
    let span = |w: &Word| match axis {
        Axis::X => (w.x0, w.x1),
        Axis::Y => (w.y0, w.y1),
    };
    let mut spans: Vec<(f32, f32)> = words.iter().map(span).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best: Option<(f32, f32)> = None;
    let mut end = spans.first()?.1;
    for &(lo, hi) in &spans[1..] {
        if lo - end >= min_gap && best.map_or(true, |(gap, _)| lo - end > gap) {
            best = Some((lo - end, (lo + end) / 2.0));
        }
        end = end.max(hi);
    }
    best.map(|(_, at)| at)
}

fn xy_cut(words: Vec<Word>, size: f32, out: &mut Vec<Vec<Word>>) {
    if words.len() < 2 {
        if !words.is_empty() {
            out.push(words);
        }
        return;
    }
    if let Some(at) = widest_gap(&words, Axis::Y, 1.1 * size) {
        let (top, bottom): (Vec<Word>, Vec<Word>) =
            words.into_iter().partition(|w| w.centre_y() < at);
        xy_cut(top, size, out);
        xy_cut(bottom, size, out);
        return;
    }
    if let Some(at) = widest_gap(&words, Axis::X, 1.0 * size) {
        let (left, right): (Vec<Word>, Vec<Word>) =
            words.into_iter().partition(|w| (w.x0 + w.x1) / 2.0 < at);
        if left.len() >= 6 && right.len() >= 6 {
            xy_cut(left, size, out);
            xy_cut(right, size, out);
            return;
        }
        let mut all = left;
        all.extend(right);
        out.push(all);
        return;
    }
    out.push(words);
}

/// A page's words as regions in reading order (columns left to right, full-width bands where
/// they fall), each region's lines top to bottom.
pub fn reading_order(words: &[Word]) -> Vec<Vec<Line>> {
    let mut regions = Vec::new();
    xy_cut(words.to_vec(), median_size(words), &mut regions);
    regions.iter().map(|r| lines_of(r)).collect()
}

fn is_roman(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 8
        && (s.chars().all(|c| "ivxlcdm".contains(c)) || s.chars().all(|c| "IVXLCDM".contains(c)))
}

/// "12", "xiv", "- 12 -": what a line holding only a page number looks like.
pub fn is_page_number(text: &str) -> bool {
    let t = text.trim_matches(|c: char| c.is_whitespace() || "-–—·•|()[]".contains(c));
    !t.is_empty()
        && ((t.len() <= 4 && t.chars().all(|c| c.is_ascii_digit()))
            || is_roman(t)
            || t.strip_prefix("Page ")
                .or_else(|| t.strip_prefix("page "))
                .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit())))
}

fn normalise(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_digit() {
                '#'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Band {
    Top,
    Bottom,
}

fn band_of(line: &Line, page_height: f32) -> Option<Band> {
    if line.y1 < 0.12 * page_height {
        Some(Band::Top)
    } else if line.y0 > 0.88 * page_height {
        Some(Band::Bottom)
    } else {
        None
    }
}

/// The lines in the top and bottom margins that repeat from page to page: running headers and
/// footers. Odd and even pages are counted apart, since books alternate them.
#[derive(Default, Debug)]
pub struct Furniture {
    repeated: HashSet<(Band, bool, String)>,
    /// Lines that repeat on pages of either parity.
    repeated_any: HashSet<(Band, String)>,
}

fn margin_candidates(page: &RawPage) -> Vec<(Band, Line)> {
    let lines = lines_of(&page.words);
    let mut top: Vec<&Line> = lines
        .iter()
        .filter(|l| band_of(l, page.height) == Some(Band::Top))
        .collect();
    top.truncate(2);
    let mut bottom: Vec<&Line> = lines
        .iter()
        .filter(|l| band_of(l, page.height) == Some(Band::Bottom))
        .collect();
    let keep = bottom.len().saturating_sub(2);
    let bottom = bottom.split_off(keep);
    top.into_iter()
        .map(|l| (Band::Top, l.clone()))
        .chain(bottom.into_iter().map(|l| (Band::Bottom, l.clone())))
        .collect()
}

pub fn detect_furniture(sample: &[RawPage]) -> Furniture {
    let mut counts: HashMap<(Band, bool, String), usize> = HashMap::new();
    let mut counts_any: HashMap<(Band, String), usize> = HashMap::new();
    let mut pages_by_parity = [0usize; 2];
    for page in sample {
        let odd = page.page % 2 == 1;
        pages_by_parity[odd as usize] += 1;
        let mut seen = HashSet::new();
        for (band, line) in margin_candidates(page) {
            let key = (band, odd, normalise(&line.text()));
            if !key.2.is_empty() && seen.insert(key.clone()) {
                *counts_any.entry((key.0, key.2.clone())).or_default() += 1;
                *counts.entry(key).or_default() += 1;
            }
        }
    }
    let repeated = counts
        .into_iter()
        .filter(|((_, odd, _), n)| {
            let pages = pages_by_parity[*odd as usize];
            *n >= 2 && *n as f32 >= 0.4 * pages as f32
        })
        .map(|(key, _)| key)
        .collect();
    let repeated_any = counts_any
        .into_iter()
        .filter(|(_, n)| *n >= 3 && *n as f32 >= 0.4 * sample.len() as f32)
        .map(|(key, _)| key)
        .collect();
    Furniture {
        repeated,
        repeated_any,
    }
}

impl Furniture {
    /// `page`'s words with its running headers, footers and page number removed.
    pub fn strip(&self, page: &RawPage) -> Vec<Word> {
        let odd = page.page % 2 == 1;
        let mut dropped: Vec<Line> = Vec::new();
        for (band, line) in margin_candidates(page) {
            let text = line.text();
            let key = normalise(&text);
            if is_page_number(&text)
                || self.repeated.contains(&(band, odd, key.clone()))
                || self.repeated_any.contains(&(band, key))
            {
                dropped.push(line);
            }
        }
        page.words
            .iter()
            .filter(|w| !dropped.iter().any(|l| l.words.iter().any(|d| d == *w)))
            .cloned()
            .collect()
    }
}

/// The size most body text is set in, taken over the sampled pages.
pub fn body_size(sample: &[RawPage]) -> f32 {
    let mut weight: HashMap<i32, usize> = HashMap::new();
    for page in sample {
        for w in &page.words {
            *weight.entry((w.size * 2.0).round() as i32).or_default() += w.text.chars().count();
        }
    }
    weight
        .into_iter()
        .max_by_key(|(size, n)| (*n, -*size))
        .map(|(size, _)| size as f32 / 2.0)
        .unwrap_or(10.0)
}

fn superscript_label(text: &str) -> Option<String> {
    const SUPERS: &str = "⁰¹²³⁴⁵⁶⁷⁸⁹";
    if !text.is_empty() && text.chars().all(|c| SUPERS.contains(c)) {
        return Some(
            text.chars()
                .filter_map(|c| SUPERS.chars().position(|s| s == c))
                .map(|d| char::from_digit(d as u32, 10).unwrap_or('0'))
                .collect(),
        );
    }
    let t = text.trim_matches(|c: char| ",.;:)]".contains(c));
    if t.is_empty() {
        return None;
    }
    let digits = t.len() <= 3 && t.chars().all(|c| c.is_ascii_digit());
    let symbols = t.chars().count() <= 3 && t.chars().all(|c| "*†‡§¶".contains(c));
    (digits || symbols).then(|| t.to_string())
}

fn is_marker(word: &Word, line_size: f32) -> Option<String> {
    (word.size < 0.78 * line_size)
        .then(|| superscript_label(&word.text))
        .flatten()
        .or_else(|| {
            word.text
                .chars()
                .all(|c| "⁰¹²³⁴⁵⁶⁷⁸⁹".contains(c))
                .then(|| superscript_label(&word.text))
                .flatten()
        })
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .ends_with(['.', '?', '!', ':', '"', '”', '’', ')', ']', '…'])
}

/// A paragraph under construction, and whether its last line stopped short of a full one.
struct Pending {
    paragraph: Paragraph,
    open: bool,
    last_page: u16,
}

/// A run of lines that belong together, before being turned into text.
struct Draft {
    lines: Vec<Line>,
    heading: Option<u8>,
    indented: bool,
    /// The last line runs to the edge of its column and does not end a sentence, so the text
    /// probably carries on in the next region or page.
    open: bool,
    x0: f32,
    x1: f32,
}

fn leaf_paragraphs(lines: &[Line], body: f32, captions: &[[f32; 4]]) -> Vec<Draft> {
    if lines.is_empty() {
        return Vec::new();
    }
    let mut lefts: Vec<f32> = lines.iter().map(|l| l.x0).collect();
    lefts.sort_by(f32::total_cmp);
    let left = lefts[lefts.len() / 10];
    let right = lines.iter().map(|l| l.x1).fold(f32::NEG_INFINITY, f32::max);
    let width = (right - left).max(1.0);
    let mut leadings: Vec<f32> = lines.windows(2).map(|p| p[1].y1 - p[0].y1).collect();
    leadings.sort_by(f32::total_cmp);
    let leading = leadings
        .get(leadings.len() / 2)
        .copied()
        .unwrap_or(body * 1.2);

    let heading_level = |l: &Line| -> Option<u8> {
        let text = l.text();
        let chars = text.chars().count();
        if chars == 0 || chars > 120 {
            return None;
        }
        if l.size >= 1.6 * body {
            Some(1)
        } else if l.size >= 1.3 * body {
            Some(2)
        } else if l.size >= 1.15 * body
            || (l.bold_fraction() > 0.85
                && l.size >= 0.98 * body
                && chars < 90
                && !ends_sentence(&text))
        {
            Some(3)
        } else {
            None
        }
    };

    let mut drafts: Vec<Draft> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let level = heading_level(line);
        let prev = i.checked_sub(1).map(|p| &lines[p]);
        let starts_caption = captions
            .iter()
            .any(|c| line.y0 >= c[1] - 1.0 && line.y1 <= c[3] + 1.0 && line.x0 >= c[0] - 1.0);
        let breaks = match (prev, drafts.last()) {
            (Some(p), Some(d)) => {
                let gap = line.y1 - p.y1;
                starts_caption
                    || d.heading != level
                    || gap > leading + 0.55 * body
                    || (line.x0 - left > 0.9 * body && level.is_none())
                    || (level.is_none() && (p.x1 - left) < 0.72 * width && ends_sentence(&p.text()))
            }
            _ => true,
        };
        if breaks {
            drafts.push(Draft {
                lines: vec![line.clone()],
                heading: level,
                indented: line.x0 - left > 0.9 * body,
                open: false,
                x0: left,
                x1: right,
            });
        } else if let Some(d) = drafts.last_mut() {
            d.lines.push(line.clone());
        }
    }
    for d in &mut drafts {
        if let Some(last) = d.lines.last() {
            d.open = d.heading.is_none()
                && (last.x1 - left) >= 0.85 * width
                && !ends_sentence(&last.text());
        }
    }
    drafts
}

fn append_word(p: &mut Paragraph, w: &Word, page: u16, joining_line: bool) {
    let prev_char = p.text.chars().last();
    let rejoin = joining_line
        && prev_char == Some('-')
        && p.text
            .chars()
            .rev()
            .nth(1)
            .is_some_and(|c| c.is_alphabetic())
        && w.text.chars().next().is_some_and(|c| c.is_lowercase());
    if rejoin {
        p.text.pop();
    } else if prev_char.is_some_and(|c| c != ' ') && !p.text.is_empty() {
        p.text.push(' ');
    }
    let start = p.text.chars().count();
    p.text.push_str(&w.text);
    p.source.push(SourceWord {
        start,
        end: start + w.text.chars().count(),
        page,
        bbox: [w.x0, w.y0, w.x1, w.y1],
    });
}

fn append_lines(p: &mut Paragraph, lines: &[Line], page: u16, continuing: bool) {
    for (li, line) in lines.iter().enumerate() {
        for (wi, w) in line.words.iter().enumerate() {
            if let Some(label) = is_marker(w, line.size) {
                if continuing || !p.text.is_empty() {
                    p.markers.push(Marker {
                        at: p.text.chars().count(),
                        label,
                    });
                    continue;
                }
            }
            append_word(p, w, page, (li > 0 || continuing) && wi == 0);
        }
    }
}

struct NoteDraft {
    label: String,
    text: String,
    page: u16,
}

fn note_label(line: &Line) -> Option<(String, usize)> {
    let first = line.words.first()?;
    let label = superscript_label(&first.text)
        .filter(|_| first.size < 0.78 * line.size || line.words.len() > 1)
        .or_else(|| {
            let t = first.text.trim_end_matches(['.', ')']);
            (line.words.len() > 1
                && !t.is_empty()
                && (t.len() <= 3 && t.chars().all(|c| c.is_ascii_digit())
                    || t.chars().all(|c| "*†‡§¶".contains(c))))
            .then(|| t.to_string())
        })?;
    Some((label, 1))
}

/// Builds the flow page by page, joining paragraphs that run over a column or page break and
/// attaching each note to the marker that cites it.
pub struct Assembler {
    furniture: Furniture,
    body: f32,
    items: Vec<Item>,
    /// Figures that come next in the flow, placed after the paragraph being built.
    waiting: Vec<Item>,
    pending: Option<Pending>,
    /// The most recent note placed, as (index of its paragraph, index in that paragraph's notes);
    /// an index equal to `items.len()` is the pending paragraph. A note that runs over to the next
    /// page carries on from here.
    last_note: Option<(usize, usize)>,
}

impl Assembler {
    pub fn new(furniture: Furniture, body: f32) -> Assembler {
        Assembler {
            furniture,
            body,
            items: Vec::new(),
            waiting: Vec::new(),
            pending: None,
            last_note: None,
        }
    }

    fn flush(&mut self) {
        if let Some(p) = self.pending.take() {
            self.items.push(Item::Paragraph(p.paragraph));
        }
        self.items.append(&mut self.waiting);
    }

    fn paragraph_at(&mut self, i: usize) -> Option<&mut Paragraph> {
        if i < self.items.len() {
            match &mut self.items[i] {
                Item::Paragraph(p) => Some(p),
                Item::Heading { .. } | Item::Figure { .. } => None,
            }
        } else if i == self.items.len() {
            self.pending.as_mut().map(|p| &mut p.paragraph)
        } else {
            None
        }
    }

    pub fn push_page(&mut self, page: &RawPage) {
        let mut words = self.furniture.strip(page);
        let found = super::figures::find_figures(page, &words);
        let captions = found.captions;
        let mut figures = found.regions;
        words.retain(|w| {
            let (cx, cy) = ((w.x0 + w.x1) / 2.0, w.centre_y());
            !figures
                .iter()
                .any(|f| cx >= f[0] && cx <= f[2] && cy >= f[1] && cy <= f[3])
        });
        let first_item = self.items.len();
        let mut notes: Vec<NoteDraft> = Vec::new();
        let mut previous_leaf: Option<(f32, f32)> = None;
        let mut carried: Option<(usize, usize)> = self.last_note;
        for leaf in reading_order(&words) {
            let (note_lines, body_lines): (Vec<Line>, Vec<Line>) =
                leaf.into_iter().partition(|l| {
                    l.size <= 0.9 * self.body
                        && l.y0 > 0.45 * page.height
                        && !captions
                            .iter()
                            .any(|c| l.y0 >= c[1] - 1.0 && l.y1 <= c[3] + 1.0)
                });
            for line in &note_lines {
                if let Some((label, skip)) = note_label(line) {
                    let words: Vec<String> = line
                        .words
                        .iter()
                        .skip(skip)
                        .map(|w| w.text.clone())
                        .collect();
                    carried = None;
                    notes.push(NoteDraft {
                        label,
                        text: words.join(" "),
                        page: page.page,
                    });
                } else if let Some(n) = notes.last_mut() {
                    join_text(&mut n.text, &line.text());
                } else if let Some((i, j)) = carried {
                    if let Some(n) = self.paragraph_at(i).and_then(|p| p.notes.get_mut(j)) {
                        join_text(&mut n.text, &line.text());
                        continue;
                    }
                    carried = None;
                    notes.push(NoteDraft {
                        label: String::new(),
                        text: line.text(),
                        page: page.page,
                    });
                } else {
                    notes.push(NoteDraft {
                        label: String::new(),
                        text: line.text(),
                        page: page.page,
                    });
                }
            }
            let leaf_x = (!body_lines.is_empty()).then(|| draft_x(&body_lines));
            for (n, draft) in leaf_paragraphs(&body_lines, self.body, &captions)
                .into_iter()
                .enumerate()
            {
                let (top, x0, x1) = (draft.lines[0].y0, draft.x0, draft.x1);
                while let Some(i) = figures
                    .iter()
                    .position(|f| f[1] <= top + 1.0 && overlap(f[0], f[2], x0, x1) >= 0.3)
                {
                    let bbox = figures.remove(i);
                    self.waiting.push(Item::Figure {
                        page: page.page,
                        bbox,
                    });
                }
                self.take_draft(draft, page.page, previous_leaf.filter(|_| n == 0));
            }
            if leaf_x.is_some() {
                previous_leaf = leaf_x;
            }
        }
        for bbox in figures {
            self.waiting.push(Item::Figure {
                page: page.page,
                bbox,
            });
        }
        self.flush_page_notes(notes, first_item, page.page);
    }

    /// `previous_leaf` is the column the last draft came from, given only for the first draft of
    /// a region, which is the only one that can continue a paragraph from another column.
    fn take_draft(&mut self, draft: Draft, page: u16, previous_leaf: Option<(f32, f32)>) {
        if let Some(level) = draft.heading {
            self.flush();
            let text = draft
                .lines
                .iter()
                .map(|l| l.text())
                .collect::<Vec<_>>()
                .join(" ");
            self.items.push(Item::Heading { level, text, page });
            return;
        }
        let first_in_region = previous_leaf.is_some();
        let continues = self.pending.as_ref().is_some_and(|p| {
            let other_column =
                previous_leaf.is_some_and(|(a, b)| overlap(a, b, draft.x0, draft.x1) <= 0.5);
            p.open && !draft.indented && (p.last_page != page || (first_in_region && other_column))
        });
        if continues {
            let Some(pending) = self.pending.as_mut() else {
                return;
            };
            if pending.last_page != page {
                pending.paragraph.breaks.push(PageBreak {
                    at: pending.paragraph.text.chars().count(),
                    page,
                });
            }
            append_lines(&mut pending.paragraph, &draft.lines, page, true);
            pending.open = draft.open;
            pending.last_page = page;
            return;
        }
        self.flush();
        let mut paragraph = Paragraph::default();
        paragraph.breaks.push(PageBreak { at: 0, page });
        append_lines(&mut paragraph, &draft.lines, page, false);
        self.pending = Some(Pending {
            paragraph,
            open: draft.open,
            last_page: page,
        });
    }

    fn flush_page_notes(&mut self, notes: Vec<NoteDraft>, first_item: usize, page: u16) {
        for note in notes {
            let total = self.items.len() + usize::from(self.pending.is_some());
            let mut placed = false;
            for i in first_item.saturating_sub(1)..total {
                let Some(p) = self.paragraph_at(i) else {
                    continue;
                };
                let used: Vec<usize> = p.notes.iter().filter_map(|n| n.anchor).collect();
                let Some(anchor) = p
                    .markers
                    .iter()
                    .find(|m| m.label == note.label && !used.contains(&m.at))
                    .map(|m| m.at)
                else {
                    continue;
                };
                p.notes.push(Note {
                    label: note.label.clone(),
                    text: note.text.clone(),
                    page: note.page,
                    anchor: Some(anchor),
                });
                self.last_note = Some((i, p.notes.len() - 1));
                placed = true;
                break;
            }
            if placed {
                continue;
            }
            let unmatched = Note {
                label: note.label,
                text: note.text,
                page: note.page,
                anchor: None,
            };
            let last = (0..total).rev().find(|&i| self.paragraph_at(i).is_some());
            match last.and_then(|i| self.paragraph_at(i).map(|p| (i, p))) {
                Some((i, p)) => {
                    p.notes.push(unmatched);
                    let j = p.notes.len() - 1;
                    self.last_note = Some((i, j));
                }
                None => {
                    self.items.push(Item::Paragraph(Paragraph {
                        notes: vec![unmatched],
                        breaks: vec![PageBreak { at: 0, page }],
                        ..Paragraph::default()
                    }));
                    self.last_note = Some((self.items.len() - 1, 0));
                }
            }
        }
    }

    /// Items complete before `page` starts, leaving the later ones for notes and joins to reach.
    pub fn take_ready(&mut self, page: u16) -> Vec<Item> {
        let last_page = |item: &Item| match item {
            Item::Heading { page, .. } | Item::Figure { page, .. } => *page,
            Item::Paragraph(p) => p
                .notes
                .iter()
                .map(|n| n.page)
                .chain(p.breaks.last().map(|b| b.page))
                .max()
                .unwrap_or(0),
        };
        let keep_from = self
            .items
            .iter()
            .position(|i| last_page(i) + 1 >= page)
            .unwrap_or(self.items.len());
        self.last_note = self
            .last_note
            .and_then(|(i, j)| i.checked_sub(keep_from).map(|i| (i, j)));
        self.items.drain(..keep_from).collect()
    }

    pub fn finish(mut self) -> Vec<Item> {
        self.flush();
        self.items
    }
}

fn draft_x(lines: &[Line]) -> (f32, f32) {
    (
        lines.iter().map(|l| l.x0).fold(f32::INFINITY, f32::min),
        lines.iter().map(|l| l.x1).fold(f32::NEG_INFINITY, f32::max),
    )
}

fn overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    let shared = (a1.min(b1) - a0.max(b0)).max(0.0);
    shared / (a1 - a0).min(b1 - b0).max(1.0)
}

fn join_text(into: &mut String, more: &str) {
    if into.ends_with('-')
        && into.chars().rev().nth(1).is_some_and(|c| c.is_alphabetic())
        && more.chars().next().is_some_and(|c| c.is_lowercase())
    {
        into.pop();
    } else if !into.is_empty() {
        into.push(' ');
    }
    into.push_str(more);
}

/// Lay out a whole document in memory: the pages set the furniture and body size, then every
/// page goes through the assembler.
pub fn flow_of(pages: &[RawPage]) -> Vec<Item> {
    let furniture = detect_furniture(pages);
    let body = body_size(pages);
    let mut asm = Assembler::new(furniture, body);
    for page in pages {
        asm.push_page(page);
    }
    asm.finish()
}

#[cfg(test)]
mod tests;
