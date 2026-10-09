//! Finding the figures and tables of a page, so Reading mode can show them as pictures instead of
//! scrambling their labels and cells into the text.
//!
//! Pictures and lines are joined into regions; a region becomes a figure only when a caption
//! ("Figure 3.", "Table 2:") sits right against it, which keeps underlines, boxed words and
//! decorative rules out.

use super::layout::lines_of;
use super::model::{RawPage, Word};

type Rect = [f32; 4];

const CELL: f32 = 5.0;
/// Regions this near each other are one drawing (the bars of a chart, a chart and its axes).
const JOIN_GAP: f32 = 14.0;
/// The most a stack of thin rules, as in a table, may be spread out and still be one table.
const RULE_GAP: f32 = 60.0;
/// How far from its caption a figure may be.
const CAPTION_GAP: f32 = 50.0;
/// How far beyond its lines a figure's own labels may sit.
const LABEL_REACH: f32 = 12.0;

fn width(r: &Rect) -> f32 {
    r[2] - r[0]
}

fn height(r: &Rect) -> f32 {
    r[3] - r[1]
}

fn union(a: &Rect, b: &Rect) -> Rect {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn gap(a: &Rect, b: &Rect) -> (f32, f32) {
    (
        (a[0].max(b[0]) - a[2].min(b[2])).max(0.0),
        (a[1].max(b[1]) - a[3].min(b[3])).max(0.0),
    )
}

fn x_overlap(a: &Rect, b: &Rect) -> f32 {
    (a[2].min(b[2]) - a[0].max(b[0])).max(0.0)
}

fn centre_in(w: &Word, r: &Rect) -> bool {
    let (cx, cy) = ((w.x0 + w.x1) / 2.0, w.centre_y());
    cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3]
}

/// Join rectangles that touch, or come within a few points of touching, into the regions they
/// make together. Cheap however many rectangles there are, as a vector plot has thousands.
pub fn join_touching(rects: &[Rect], page_w: f32, page_h: f32) -> Vec<Rect> {
    if rects.is_empty() || page_w <= 0.0 || page_h <= 0.0 {
        return Vec::new();
    }
    let cols = (page_w / CELL).ceil() as usize + 3;
    let rows = (page_h / CELL).ceil() as usize + 3;
    let cell = |v: f32, max: usize| ((v / CELL).floor().max(0.0) as usize + 1).min(max - 1);
    let mut occupied = vec![false; cols * rows];
    for r in rects {
        let (c0, c1) = (cell(r[0], cols).saturating_sub(1), cell(r[2], cols) + 1);
        let (r0, r1) = (cell(r[1], rows).saturating_sub(1), cell(r[3], rows) + 1);
        for y in r0..=r1.min(rows - 1) {
            for x in c0..=c1.min(cols - 1) {
                occupied[y * cols + x] = true;
            }
        }
    }
    let mut label = vec![0usize; cols * rows];
    let mut next = 0usize;
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..occupied.len() {
        if !occupied[start] || label[start] != 0 {
            continue;
        }
        next += 1;
        label[start] = next;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % cols, i / cols);
            for (dx, dy) in [
                (-1i32, 0i32),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, -1),
                (1, 1),
                (-1, 1),
                (1, -1),
            ] {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= cols as i32 || ny >= rows as i32 {
                    continue;
                }
                let j = ny as usize * cols + nx as usize;
                if occupied[j] && label[j] == 0 {
                    label[j] = next;
                    stack.push(j);
                }
            }
        }
    }
    let mut boxes: Vec<Option<Rect>> = vec![None; next + 1];
    for r in rects {
        let id = label[cell((r[1] + r[3]) / 2.0, rows) * cols + cell((r[0] + r[2]) / 2.0, cols)];
        boxes[id] = Some(match boxes[id] {
            Some(b) => union(&b, r),
            None => *r,
        });
    }
    let mut out: Vec<Rect> = boxes.into_iter().flatten().collect();
    out.sort_by(|a, b| (height(b) * width(b)).total_cmp(&(height(a) * width(a))));
    out.truncate(400);
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Figure,
    Table,
}

/// Whether `text` begins like a caption: a label word, then a number.
fn caption_kind(text: &str) -> Option<Kind> {
    let mut parts = text.split_whitespace();
    let label = parts.next()?;
    let (label, number) = match label.find(|c: char| c.is_ascii_digit()) {
        Some(i) if i > 0 => (&label[..i], Some(&label[i..])),
        _ => (label, parts.next()),
    };
    let number = number?;
    let numbered = number
        .trim_end_matches(['.', ':', ')', '-', '–', '—'])
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, 'I' | 'V' | 'X' | '.'))
        && number
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, 'I' | 'V' | 'X'));
    if !numbered {
        return None;
    }
    match label.trim_end_matches('.').to_lowercase().as_str() {
        "table" => Some(Kind::Table),
        "figure" | "fig" | "chart" | "plate" | "map" | "diagram" | "scheme" | "illustration" => {
            Some(Kind::Figure)
        }
        _ => None,
    }
}

/// Grow `seed` over the regions around it that belong to the same drawing.
fn grow(seed: Rect, others: &[Rect], keep_out: &Rect) -> Rect {
    let mut region = seed;
    let mut taken = vec![false; others.len()];
    loop {
        let mut changed = false;
        for (i, o) in others.iter().enumerate() {
            if taken[i] || gap(o, keep_out) == (0.0, 0.0) {
                continue;
            }
            let (gx, gy) = gap(&region, o);
            let near = gx <= JOIN_GAP && gy <= JOIN_GAP;
            let stacked = height(o) <= 3.0
                && height(&region) <= 3.0 + RULE_GAP
                && x_overlap(&region, o) >= 0.8 * width(o).min(width(&region))
                && gy <= RULE_GAP;
            if near || stacked {
                region = union(&region, o);
                taken[i] = true;
                changed = true;
            }
        }
        if !changed {
            return region;
        }
    }
}

/// The figures of a page, and the first lines of their captions.
#[derive(Default)]
pub struct Figures {
    /// The regions to show as pictures, top to bottom.
    pub regions: Vec<Rect>,
    /// Where each figure's caption starts, so the caption can be set as a paragraph of its own.
    pub captions: Vec<Rect>,
}

/// The parts of the page to show as pictures: each figure or table that has a caption beside it,
/// as the region its drawing and its own labels cover.
pub fn find_figures(page: &RawPage, words: &[Word]) -> Figures {
    if page.graphics.is_empty() {
        return Figures::default();
    }
    let lines = lines_of(words);
    let mut found: Vec<Rect> = Vec::new();
    let mut captions: Vec<Rect> = Vec::new();
    for line in &lines {
        let Some(kind) = caption_kind(&line.text()) else {
            continue;
        };
        let caption: Rect = [line.x0, line.y0, line.x1, line.y1];
        let reach = |below: bool| -> Option<Rect> {
            let seed = page
                .graphics
                .iter()
                .filter(|g| {
                    let distance = if below {
                        g[1] - caption[3]
                    } else {
                        caption[1] - g[3]
                    };
                    (-3.0..=CAPTION_GAP).contains(&distance)
                        && x_overlap(g, &caption) >= 0.3 * width(&caption).min(width(g)).max(1.0)
                        && gap(g, &caption).1 <= CAPTION_GAP
                })
                .min_by(|a, b| {
                    let d = |g: &Rect| {
                        if below {
                            g[1] - caption[3]
                        } else {
                            caption[1] - g[3]
                        }
                    };
                    d(a).total_cmp(&d(b))
                })?;
            let region = grow(*seed, &page.graphics, &caption);
            (width(&region) >= 40.0 && height(&region) >= 12.0).then_some(region)
        };
        let drawing = match kind {
            Kind::Figure => reach(false).or_else(|| reach(true)),
            Kind::Table => reach(true).or_else(|| reach(false)),
        };
        let Some(drawing) = drawing else {
            continue;
        };
        let mut area = drawing;
        let reachable = [
            drawing[0] - LABEL_REACH,
            drawing[1] - LABEL_REACH * 0.8,
            drawing[2] + LABEL_REACH,
            drawing[3] + LABEL_REACH * 0.8,
        ];
        for w in words {
            let inside = centre_in(w, &drawing);
            let touches = w.x1 >= reachable[0]
                && w.x0 <= reachable[2]
                && w.y1 >= reachable[1]
                && w.y0 <= reachable[3];
            let beside = touches
                && (w.y1 <= caption[1] + 0.5 * height(&caption)
                    || w.y0 >= caption[3] - 0.5 * height(&caption));
            let on_caption = w.y0 < caption[3]
                && w.y1 > caption[1]
                && w.x0 >= caption[0] - 1.0
                && w.x1 <= caption[2] + 1.0;
            if on_caption {
                continue;
            }
            if inside || beside {
                area = union(&area, &[w.x0, w.y0, w.x1, w.y1]);
            }
        }
        let area = [
            (area[0] - 3.0).max(0.0),
            (area[1] - 3.0).max(0.0),
            (area[2] + 3.0).min(page.width),
            (area[3] + 3.0).min(page.height),
        ];
        if found.iter().all(|f| gap(f, &area) != (0.0, 0.0)) {
            found.push(area);
            captions.push(caption);
        }
    }
    found.sort_by(|a, b| a[1].total_cmp(&b[1]));
    Figures {
        regions: found,
        captions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f32, y: f32, size: f32) -> Word {
        Word {
            text: text.into(),
            x0: x,
            y0: y,
            x1: x + text.len() as f32 * size * 0.5,
            y1: y + size,
            size,
            bold: false,
            italic: false,
        }
    }

    #[test]
    fn rectangles_a_few_points_apart_are_one_region_and_far_ones_are_not() {
        let rects = [
            [100.0, 100.0, 130.0, 160.0],
            [140.0, 100.0, 170.0, 140.0],
            [400.0, 500.0, 420.0, 520.0],
        ];
        let regions = join_touching(&rects, 612.0, 792.0);
        assert_eq!(regions.len(), 2);
        assert!(regions.contains(&[100.0, 100.0, 170.0, 160.0]));
        assert!(regions.contains(&[400.0, 500.0, 420.0, 520.0]));
    }

    #[test]
    fn captions_are_told_from_ordinary_lines() {
        assert!(caption_kind("Figure 3. The plot") == Some(Kind::Figure));
        assert!(caption_kind("Fig. 2: Bars") == Some(Kind::Figure));
        assert!(caption_kind("Table 12 Counts") == Some(Kind::Table));
        assert!(caption_kind("TABLE 1.") == Some(Kind::Table));
        assert!(caption_kind("Figure out the answer").is_none());
        assert!(caption_kind("Table of contents").is_none());
        assert!(caption_kind("The figure 3 shows").is_none());
    }

    fn page(graphics: Vec<Rect>) -> RawPage {
        RawPage {
            page: 0,
            width: 612.0,
            height: 792.0,
            words: Vec::new(),
            graphics,
        }
    }

    #[test]
    fn a_chart_with_its_axis_labels_is_found_by_the_caption_below_it() {
        let graphics = vec![[100.0, 200.0, 320.0, 300.0]];
        let words = vec![
            word("Figure", 72.0, 330.0, 9.0),
            word("1.", 110.0, 330.0, 9.0),
            word("Readers", 120.0, 330.0, 9.0),
            word("90", 82.0, 210.0, 8.0),
            word("Year", 205.0, 306.0, 8.0),
            word("Far", 82.0, 380.0, 10.0),
        ];
        let found = find_figures(&page(graphics), &words).regions;
        assert_eq!(found.len(), 1);
        let f = found[0];
        assert!(f[0] <= 82.0 && f[2] >= 320.0, "{f:?}");
        assert!(f[1] <= 200.0 && f[3] >= 314.0, "{f:?}");
        assert!(f[3] < 330.0, "the caption is inside the picture: {f:?}");
    }

    #[test]
    fn a_ruled_table_below_its_caption_is_one_region() {
        let graphics = vec![
            [72.0, 400.0, 360.0, 400.8],
            [72.0, 418.0, 360.0, 418.5],
            [72.0, 470.0, 360.0, 470.8],
        ];
        let words = vec![
            word("Table", 72.0, 380.0, 9.0),
            word("1.", 110.0, 380.0, 9.0),
            word("Counts", 125.0, 380.0, 9.0),
            word("Year", 80.0, 405.0, 9.0),
            word("2019", 80.0, 440.0, 9.0),
        ];
        let found = find_figures(&page(graphics), &words).regions;
        assert_eq!(found.len(), 1);
        assert!(
            found[0][1] >= 395.0 && found[0][3] >= 470.0,
            "{:?}",
            found[0]
        );
    }

    #[test]
    fn a_rule_with_no_caption_beside_it_is_not_a_figure() {
        let graphics = vec![[72.0, 400.0, 360.0, 400.8]];
        let words = vec![
            word("Just", 72.0, 380.0, 10.0),
            word("text", 100.0, 380.0, 10.0),
        ];
        assert!(find_figures(&page(graphics), &words).regions.is_empty());
    }
}
