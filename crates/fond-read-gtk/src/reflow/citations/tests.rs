use super::*;

fn page(n: u16, lines: &[(&str, f32, f32, f32)]) -> RawPage {
    let mut words = Vec::new();
    for &(text, x, y, size) in lines {
        let mut cx = x;
        for token in text.split(' ') {
            let sup = token.starts_with('^');
            let t = token.trim_start_matches('^');
            let s = if sup { size * 0.6 } else { size };
            let w = s * 0.5 * t.chars().count() as f32;
            let bottom = y + 0.2 * s - if sup { 0.35 * size } else { 0.0 };
            words.push(Word {
                text: t.to_string(),
                x0: cx,
                y0: bottom - s,
                x1: cx + w,
                y1: bottom,
                size: s,
                bold: false,
                italic: false,
            });
            cx += w + 0.3 * size;
        }
    }
    RawPage {
        page: n,
        width: 612.0,
        height: 792.0,
        words,
    }
}

fn centre_of(p: &RawPage, text: &str) -> (f32, f32) {
    let w = p.words.iter().find(|w| w.text == text).expect("word");
    ((w.x0 + w.x1) / 2.0, (w.y0 + w.y1) / 2.0)
}

fn cite(p: &RawPage, word: &str) -> Option<Citation> {
    let (x, y) = centre_of(p, word);
    citation_at(p, x, y)
}

#[test]
fn a_bracketed_number_cites_that_number() {
    let p = page(0, &[("as shown earlier [2] and then", 72.0, 200.0, 10.0)]);
    assert_eq!(cite(&p, "[2]"), Some(Citation::Numeric(vec![2])));
}

#[test]
fn a_list_in_brackets_gives_the_number_under_the_pointer() {
    let p = page(0, &[("see [3, 5, 7-9] for more", 72.0, 200.0, 10.0)]);
    assert_eq!(cite(&p, "5,"), Some(Citation::Numeric(vec![5])));
    assert_eq!(cite(&p, "7-9]"), Some(Citation::Numeric(vec![7, 8, 9])));
}

#[test]
fn a_superscript_number_is_a_citation() {
    let p = page(0, &[("a claim ^14 follows here", 72.0, 200.0, 10.0)]);
    assert_eq!(cite(&p, "14"), Some(Citation::Numeric(vec![14])));
}

#[test]
fn a_parenthetical_author_year_picks_the_item_under_the_pointer() {
    let p = page(
        0,
        &[(
            "as argued (Smith 2019; Jones and Lee 2015a) before",
            72.0,
            200.0,
            10.0,
        )],
    );
    assert_eq!(
        cite(&p, "2019;"),
        Some(Citation::AuthorYear {
            surnames: vec!["Smith".into()],
            year: "2019".into()
        })
    );
    assert_eq!(
        cite(&p, "Lee"),
        Some(Citation::AuthorYear {
            surnames: vec!["Jones".into(), "Lee".into()],
            year: "2015a".into()
        })
    );
}

#[test]
fn a_narrative_citation_is_found_from_the_name_or_the_year() {
    let p = page(
        0,
        &[("later Smith and Jones (2019) argue that", 72.0, 200.0, 10.0)],
    );
    let want = Some(Citation::AuthorYear {
        surnames: vec!["Smith".into(), "Jones".into()],
        year: "2019".into(),
    });
    assert_eq!(cite(&p, "Smith"), want);
    assert_eq!(cite(&p, "(2019)"), want);
    assert_eq!(cite(&p, "argue"), None);
}

#[test]
fn ordinary_words_and_dates_are_not_citations() {
    let p = page(
        0,
        &[(
            "it happened in 1999 on a Tuesday (a rainy one)",
            72.0,
            200.0,
            10.0,
        )],
    );
    assert_eq!(cite(&p, "happened"), None);
    assert_eq!(cite(&p, "rainy"), None);
}

fn bibliography(hanging: bool) -> Vec<RawPage> {
    let body = page(0, &[("body text here", 72.0, 200.0, 10.0)]);
    let indent = if hanging { 14.0 } else { 0.0 };
    let refs = page(
        1,
        &[
            ("References", 72.0, 100.0, 14.0),
            (
                "Jones, A. and Lee, B. (2015a). On narrative. Journal of Tests, 3, 1-20.",
                72.0,
                130.0,
                10.0,
            ),
            (
                "Second line of that entry continues here.",
                72.0 + indent,
                142.0,
                10.0,
            ),
            (
                "Jones, A. and Lee, B. (2015b). Another piece. Journal of Tests, 4, 5-9.",
                72.0,
                160.0,
                10.0,
            ),
            (
                "Smith, J. (2019). The Book of Examples. Example Press.",
                72.0,
                190.0,
                10.0,
            ),
            (
                "Smith, J. (2012). An older Smith work. Example Press.",
                72.0,
                220.0,
                10.0,
            ),
        ],
    );
    vec![body, refs]
}

#[test]
fn an_author_year_citation_finds_its_entry_in_a_hanging_indent_list() {
    let pages = bibliography(true);
    let found = find_entry(
        &pages,
        &Citation::AuthorYear {
            surnames: vec!["Smith".into()],
            year: "2019".into(),
        },
    )
    .expect("entry");
    assert!(found.text.contains("Book of Examples"), "{}", found.text);
    assert_eq!(found.page, 1);
}

#[test]
fn a_year_suffix_picks_between_two_works_by_the_same_authors() {
    let pages = bibliography(true);
    let a = find_entry(
        &pages,
        &Citation::AuthorYear {
            surnames: vec!["Jones".into(), "Lee".into()],
            year: "2015a".into(),
        },
    )
    .expect("entry");
    assert!(a.text.contains("On narrative"), "{}", a.text);
    assert!(
        a.text.contains("Second line"),
        "an entry keeps its continuation lines"
    );
    let b = find_entry(
        &pages,
        &Citation::AuthorYear {
            surnames: vec!["Jones".into(), "Lee".into()],
            year: "2015b".into(),
        },
    )
    .expect("entry");
    assert!(b.text.contains("Another piece"), "{}", b.text);
}

#[test]
fn a_citation_nobody_wrote_finds_nothing() {
    let pages = bibliography(true);
    assert!(find_entry(
        &pages,
        &Citation::AuthorYear {
            surnames: vec!["Nobody".into()],
            year: "2001".into()
        }
    )
    .is_none());
}

#[test]
fn a_numbered_list_is_searched_by_label() {
    let body = page(0, &[("body", 72.0, 200.0, 10.0)]);
    let refs = page(
        1,
        &[
            ("Bibliography", 72.0, 100.0, 14.0),
            ("[1] First, A. A first work. 2001.", 72.0, 130.0, 10.0),
            ("[2] Second, B. A second work that wraps", 72.0, 150.0, 10.0),
            ("onto a second line without any indent.", 72.0, 162.0, 10.0),
            ("[3] Third, C. A third work. 2003.", 72.0, 182.0, 10.0),
        ],
    );
    let found = find_entry(&[body, refs], &Citation::Numeric(vec![2])).expect("entry");
    assert!(
        found
            .text
            .contains("second work that wraps onto a second line"),
        "{}",
        found.text
    );
}
