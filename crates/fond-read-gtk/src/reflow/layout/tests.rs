use super::*;

struct L {
    text: String,
    x: f32,
    y: f32,
    size: f32,
    bold: bool,
}

fn l(text: &str, x: f32, y: f32, size: f32) -> L {
    L {
        text: text.to_string(),
        x,
        y,
        size,
        bold: false,
    }
}

/// `n` five-letter words: a full-width line at body size.
fn full(n: usize) -> String {
    ["alpha", "bravo", "delta", "gamma", "sigma"]
        .iter()
        .cycle()
        .take(n)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn page(n: u16, lines: &[L]) -> RawPage {
    let mut words = Vec::new();
    for line in lines {
        let mut x = line.x;
        for token in line.text.split(' ') {
            let marker = token.starts_with('^');
            let text = token.trim_start_matches('^');
            let size = if marker { line.size * 0.6 } else { line.size };
            let width = size * 0.5 * text.chars().count() as f32;
            let bottom = line.y + 0.2 * size - if marker { 0.35 * line.size } else { 0.0 };
            words.push(Word {
                text: text.to_string(),
                x0: x,
                y0: bottom - size,
                x1: x + width,
                y1: bottom,
                size,
                bold: line.bold,
                italic: false,
            });
            x += width + 0.3 * line.size;
        }
    }
    RawPage {
        page: n,
        width: 612.0,
        height: 792.0,
        words,
    }
}

fn paragraphs(items: &[Item]) -> Vec<&Paragraph> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect()
}

#[test]
fn paragraphs_split_on_indent_and_on_a_gap() {
    let p = page(
        0,
        &[
            l(&full(12), 72.0, 100.0, 10.0),
            l(&full(12), 72.0, 112.0, 10.0),
            l("alpha bravo delta.", 72.0, 124.0, 10.0),
            l(&full(12), 82.0, 136.0, 10.0),
            l(&full(12), 72.0, 148.0, 10.0),
            l("gamma sigma.", 72.0, 160.0, 10.0),
            l(&full(12), 72.0, 185.0, 10.0),
            l("alpha.", 72.0, 197.0, 10.0),
        ],
    );
    let items = flow_of(&[p]);
    assert_eq!(paragraphs(&items).len(), 3);
}

#[test]
fn a_word_hyphenated_across_lines_is_rejoined() {
    let p = page(
        0,
        &[
            l(&format!("{} theolo-", full(10)), 72.0, 100.0, 10.0),
            l("gical point of view.", 72.0, 112.0, 10.0),
        ],
    );
    let items = flow_of(&[p]);
    let text = &paragraphs(&items)[0].text;
    assert!(text.contains("theological point"), "{text}");
}

#[test]
fn two_columns_read_left_then_right() {
    let mut lines = Vec::new();
    for i in 0..8 {
        lines.push(l(
            &format!("left{}", full(6)),
            72.0,
            100.0 + 12.0 * i as f32,
            10.0,
        ));
        lines.push(l(
            &format!("right{}", full(6)),
            340.0,
            100.0 + 12.0 * i as f32,
            10.0,
        ));
    }
    let items = flow_of(&[page(0, &lines)]);
    let all: String = paragraphs(&items)
        .iter()
        .map(|p| p.text.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    let last_left = all.rfind("left").unwrap();
    let first_right = all.find("right").unwrap();
    assert!(last_left < first_right, "{all}");
}

#[test]
fn running_headers_and_page_numbers_are_dropped() {
    let pages: Vec<RawPage> = (0..8)
        .map(|n| {
            page(
                n,
                &[
                    l("THE JOURNAL OF EXAMPLES", 200.0, 40.0, 9.0),
                    l(&full(12), 72.0, 200.0, 10.0),
                    l(&full(12), 72.0, 212.0, 10.0),
                    l(&format!("{}", n + 41), 300.0, 760.0, 9.0),
                ],
            )
        })
        .collect();
    let items = flow_of(&pages);
    let all: String = paragraphs(&items).iter().map(|p| p.text.clone()).collect();
    assert!(!all.contains("JOURNAL"), "{all}");
    assert!(!all.contains("41"), "{all}");
    assert!(all.contains("alpha"));
}

#[test]
fn larger_type_makes_a_heading() {
    let p = page(
        0,
        &[
            l("Chapter One", 72.0, 100.0, 18.0),
            l(&full(12), 72.0, 140.0, 10.0),
            l(&full(12), 72.0, 152.0, 10.0),
            l(&full(12), 72.0, 164.0, 10.0),
        ],
    );
    let items = flow_of(&[p]);
    assert!(matches!(&items[0], Item::Heading { text, .. } if text == "Chapter One"));
    assert!(matches!(&items[1], Item::Paragraph(_)));
}

#[test]
fn a_footnote_goes_with_the_marker_that_cites_it() {
    let p = page(
        0,
        &[
            l(&format!("{} ^1 {}", full(5), full(5)), 72.0, 100.0, 10.0),
            l(&full(12), 72.0, 112.0, 10.0),
            l(&full(12), 72.0, 124.0, 10.0),
            l("1 See Smith 2019, p. 4.", 72.0, 700.0, 8.0),
        ],
    );
    let items = flow_of(&[p]);
    let para = paragraphs(&items)[0];
    assert_eq!(para.markers.len(), 1);
    assert_eq!(para.notes.len(), 1);
    assert_eq!(para.notes[0].text, "See Smith 2019, p. 4.");
    assert_eq!(para.notes[0].anchor, Some(para.markers[0].at));
    assert!(!para.text.contains("^1"));
}

#[test]
fn a_note_with_no_marker_is_kept_and_marked_unmatched() {
    let p = page(
        0,
        &[
            l(&full(12), 72.0, 100.0, 10.0),
            l(&full(12), 72.0, 112.0, 10.0),
            l("7 Lost note text.", 72.0, 700.0, 8.0),
        ],
    );
    let items = flow_of(&[p]);
    let para = paragraphs(&items)[0];
    assert_eq!(para.notes.len(), 1);
    assert_eq!(para.notes[0].anchor, None);
    assert_eq!(para.notes[0].text, "Lost note text.");
}

#[test]
fn a_paragraph_that_runs_onto_the_next_page_stays_one_paragraph() {
    let first = page(
        0,
        &[
            l(&full(12), 72.0, 100.0, 10.0),
            l(&full(12), 72.0, 112.0, 10.0),
            l(&full(12), 72.0, 700.0, 10.0),
        ],
    );
    let second = page(
        1,
        &[
            l("carried on here.", 72.0, 100.0, 10.0),
            l(&full(12), 82.0, 124.0, 10.0),
            l("end.", 72.0, 136.0, 10.0),
        ],
    );
    let items = flow_of(&[first, second]);
    let paras = paragraphs(&items);
    assert_eq!(paras.len(), 3, "{paras:?}");
    assert!(paras[1].text.ends_with("carried on here."));
    assert_eq!(paras[1].breaks.last().unwrap().page, 1);
    assert!(paras[1].breaks.last().unwrap().at > 0);
}

#[test]
fn a_note_that_runs_over_a_page_break_stays_whole() {
    let first = page(
        0,
        &[
            l(&format!("{} ^1 {}", full(5), full(5)), 72.0, 100.0, 10.0),
            l("1 A long note that starts here and", 72.0, 720.0, 8.0),
        ],
    );
    let second = page(
        1,
        &[
            l("keeps going after the break.", 72.0, 700.0, 8.0),
            l(&full(12), 72.0, 100.0, 10.0),
        ],
    );
    let items = flow_of(&[first, second]);
    let note = &paragraphs(&items)[0].notes[0];
    assert!(
        note.text.contains("starts here and keeps going"),
        "{}",
        note.text
    );
}

#[test]
fn page_numbers_are_recognised() {
    for yes in ["12", "xiv", "- 12 -", "Page 3", "XIV"] {
        assert!(is_page_number(yes), "{yes}");
    }
    for no in ["Introduction", "2019 was", "12345"] {
        assert!(!is_page_number(no), "{no}");
    }
}
