use super::*;

fn pages(first: &str, last: Option<&str>, note: Option<&str>) -> Locator {
    Locator::Pages {
        first: first.into(),
        last: last.map(str::to_string),
        note: note.map(str::to_string),
    }
}

#[test]
fn locators_in_every_form() {
    assert_eq!(parse_locator("45"), Some(pages("45", None, None)));
    assert_eq!(parse_locator("45–47"), Some(pages("45", Some("47"), None)));
    assert_eq!(
        parse_locator("345–47"),
        Some(pages("345", Some("347"), None))
    );
    assert_eq!(parse_locator("203n4"), Some(pages("203", None, Some("4"))));
    assert_eq!(
        parse_locator("203nn4–5"),
        Some(pages("203", None, Some("4–5")).tap_note())
    );
    assert_eq!(parse_locator("xii"), Some(pages("xii", None, None)));
    assert_eq!(parse_locator("passim"), Some(Locator::Passim));
    assert_eq!(parse_locator("45ff"), Some(pages("45", None, None)));
    assert_eq!(parse_locator("Abraham"), None);
    assert_eq!(parse_locator("1234567"), None);
}

trait Tap {
    fn tap_note(self) -> Self;
}
impl Tap for Locator {
    fn tap_note(self) -> Self {
        self
    }
}

const SAMPLE: &str = "Index
A
Abraham, 12, 45–47, 203n4
call of, 12–14
covenant with, 45, 60
Adams, John, passim
Q, see Sayings Source
Q (source), 33, 41,
57, 60–62
B
Barnabas, 88
";

#[test]
fn entries_sub_entries_see_and_continuation_lines() {
    let entries = parse_index(SAMPLE);
    let names: Vec<&str> = entries.iter().map(|e| e.term.as_str()).collect();
    assert_eq!(
        names,
        [
            "Abraham",
            "call of",
            "covenant with",
            "Adams, John",
            "Q",
            "Q (source)",
            "Barnabas"
        ]
    );
    let abraham = &entries[0];
    assert!(!abraham.sub);
    assert_eq!(abraham.locators.len(), 3);
    assert_eq!(abraham.locators[2], pages("203", None, Some("4")));
    assert!(entries[1].sub, "lowercase starts a sub-entry");
    assert_eq!(entries[3].locators, [Locator::Passim]);
    assert_eq!(entries[4].see.as_deref(), Some("Sayings Source"));
    let q = &entries[5];
    assert_eq!(
        q.locators.len(),
        4,
        "the continuation line's locators join the entry: {q:?}"
    );
    assert_eq!(q.locators[3], pages("60", Some("62"), None));
}

fn index_page(n: usize) -> String {
    (0..30)
        .map(|i| format!("Term{} number{}, {}, {}–{}\n", n, i, i + 3, i + 40, i + 42))
        .collect()
}

fn prose_page() -> String {
    "This is an ordinary page of running prose that never ends a line with a page number. "
        .repeat(12)
        .split(". ")
        .map(|s| format!("{s}.\n"))
        .collect()
}

#[test]
fn the_index_pages_are_found_at_the_back() {
    let mut book: Vec<String> = (0..20).map(|_| prose_page()).collect();
    book.extend((0..3).map(index_page));
    assert_eq!(index_pages(&book), Some((20, 22)));
    assert!(!looks_like_index(&prose_page()));
    let no_index: Vec<String> = (0..10).map(|_| prose_page()).collect();
    assert_eq!(index_pages(&no_index), None);
}

#[test]
fn a_locator_finds_its_pdf_page_by_printed_label() {
    let labels: Vec<Option<String>> = ["i", "ii", "1", "2", "3"]
        .iter()
        .map(|s| Some(s.to_string()))
        .collect();
    assert_eq!(page_of(&pages("2", None, None), &labels), Some(3));
    assert_eq!(page_of(&pages("ii", None, None), &labels), Some(1));
    assert_eq!(page_of(&pages("99", None, None), &labels), None);
    assert_eq!(page_of(&Locator::Passim, &labels), None);
}

#[test]
fn locators_read_back_as_written() {
    assert_eq!(pages("45", Some("47"), None).text(), "45–47");
    assert_eq!(pages("203", None, Some("4")).text(), "203n4");
}
