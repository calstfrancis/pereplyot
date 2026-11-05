use super::*;

struct Case {
    style: &'static str,
    raw: &'static str,
    families: &'static [&'static str],
    year: i32,
    title: &'static str,
}

const CASES: &[Case] = &[
    Case {
        style: "APA",
        raw: "Kloppenborg, J. S., & Lee, K. (1987). The formation of Q. Harvard Theological Review, 80(3), 241–267. https://doi.org/10.1017/s0017816000030234",
        families: &["kloppenborg", "lee"],
        year: 1987,
        title: "The formation of Q",
    },
    Case {
        style: "Harvard",
        raw: "Harrington, D.J. (1981) Interpreting the Old Testament. Wilmington, DE: Glazier.",
        families: &["harrington"],
        year: 1981,
        title: "Interpreting the Old Testament",
    },
    Case {
        style: "Chicago NB bibliography",
        raw: "Kloppenborg, John S. The Formation of Q: Trajectories in Ancient Wisdom Collections. Philadelphia: Fortress, 1987.",
        families: &["kloppenborg"],
        year: 1987,
        title: "The Formation of Q: Trajectories in Ancient Wisdom Collections",
    },
    Case {
        style: "Chicago author-date",
        raw: "Perkins, Pheme. 1990. Reading the New Testament: An Introduction. New York: Paulist.",
        families: &["perkins"],
        year: 1990,
        title: "Reading the New Testament: An Introduction",
    },
    Case {
        style: "IEEE",
        raw: "[3] J. S. Kloppenborg and K. Lee, “The formation of Q,” Harvard Theol. Rev., vol. 80, no. 3, pp. 241–267, 1987.",
        families: &["kloppenborg", "lee"],
        year: 1987,
        title: "The formation of Q",
    },
    Case {
        style: "MLA",
        raw: "Kloppenborg, John S. “The Formation of Q.” Harvard Theological Review, vol. 80, no. 3, 1987, pp. 241–67.",
        families: &["kloppenborg"],
        year: 1987,
        title: "The Formation of Q",
    },
    Case {
        style: "SBL",
        raw: "Harrington, Daniel J. Interpreting the Old Testament. Wilmington, DE: Glazier, 1981.",
        families: &["harrington"],
        year: 1981,
        title: "Interpreting the Old Testament",
    },
    Case {
        style: "SBL with two authors",
        raw: "Boring, M. Eugene, and Fred B. Craddock. The People’s New Testament Commentary. Louisville: Westminster John Knox, 2004.",
        families: &["boring", "craddock"],
        year: 2004,
        title: "The People’s New Testament Commentary",
    },
];

#[test]
fn entries_split_into_authors_year_and_title_in_eight_styles() {
    let mut bad = Vec::new();
    for case in CASES {
        let p = parse_reference(case.raw);
        let ok = p.families == case.families && p.year == Some(case.year) && p.title == case.title;
        if !ok {
            bad.push(format!(
                "{}: families {:?} year {:?} title {:?}",
                case.style, p.families, p.year, p.title
            ));
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

#[test]
fn a_leading_label_is_read() {
    assert_eq!(
        parse_reference("[12] A. Author, “Title,” 2001.").label,
        Some(12)
    );
    assert_eq!(
        parse_reference("7. Smith, John. Book. Press, 1999.").label,
        Some(7)
    );
    assert_eq!(parse_reference("1999 was a year. Smith.").label, None);
}

#[test]
fn doi_is_found_in_an_entry() {
    assert_eq!(
        parse_reference(CASES[0].raw).doi.as_deref(),
        Some("10.1017/s0017816000030234")
    );
}

fn items(text: &str) -> Vec<CiteItem> {
    citations_in(text)
        .into_iter()
        .flat_map(|c| c.items)
        .collect()
}

fn ay(surnames: &[&str], year: i32, suffix: Option<char>, locator: Option<&str>) -> CiteItem {
    CiteItem::AuthorYear {
        surnames: surnames.iter().map(|s| s.to_string()).collect(),
        year,
        suffix,
        locator: locator.map(str::to_string),
    }
}

#[test]
fn parenthetical_author_year_with_locators_and_groups() {
    assert_eq!(
        items("as shown (Kloppenborg 1987, 41) and again (Smith and Lee 2015a; Jones 2001: 7)."),
        vec![
            ay(&["kloppenborg"], 1987, None, Some("41")),
            ay(&["smith", "lee"], 2015, Some('a'), None),
            ay(&["jones"], 2001, None, Some("7")),
        ]
    );
    assert_eq!(
        items("(see Perkins 1990, p. 12)"),
        vec![ay(&["perkins"], 1990, None, Some("12"))]
    );
    assert_eq!(
        items("(Smith et al. 2019)"),
        vec![ay(&["smith"], 2019, None, None)]
    );
}

#[test]
fn narrative_citations_take_the_names_before_the_year() {
    let found = citations_in("Earlier, Kloppenborg and Lee (1987, 41) argued otherwise.");
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].items,
        vec![ay(&["kloppenborg", "lee"], 1987, None, Some("41"))]
    );
    let text = "Earlier, Kloppenborg and Lee (1987, 41) argued otherwise.";
    let shown: String = text
        .chars()
        .skip(found[0].start)
        .take(found[0].end - found[0].start)
        .collect();
    assert_eq!(shown, "Kloppenborg and Lee (1987, 41)");
}

#[test]
fn numeric_citations_expand_ranges() {
    assert_eq!(
        items("see [3], [1, 4-6] and [12]"),
        [3, 1, 4, 5, 6, 12].map(CiteItem::Number).to_vec()
    );
    assert!(items("an array a[i] and [x]").is_empty());
}

#[test]
fn things_that_are_not_citations_are_left_alone() {
    assert!(items("In 1987 (a year) things changed (see below).").is_empty());
    assert!(items("(Table 3) and (Figure 2, 2019)").is_empty());
}

#[test]
fn citations_match_their_entries() {
    let kloppenborg = parse_reference(CASES[0].raw);
    let harrington = parse_reference(CASES[6].raw);
    assert!(cites(&kloppenborg, &ay(&["kloppenborg"], 1987, None, None)));
    assert!(cites(
        &kloppenborg,
        &ay(&["kloppenborg", "lee"], 1987, None, None)
    ));
    assert!(!cites(
        &kloppenborg,
        &ay(&["kloppenborg"], 1988, None, None)
    ));
    assert!(!cites(&kloppenborg, &ay(&["lee"], 1987, None, None)));
    assert!(!cites(&harrington, &ay(&["kloppenborg"], 1987, None, None)));
    assert!(cites(
        &parse_reference("[4] A. Author, “T,” 2001."),
        &CiteItem::Number(4)
    ));
}

#[test]
fn a_suffix_must_agree_when_both_have_one() {
    let a = parse_reference("Lee, K. (2015a). On narrative form. Journal, 1, 1–2.");
    let b = parse_reference("Lee, K. (2015b). More on form. Journal, 1, 3–4.");
    let cite_a = ay(&["lee"], 2015, Some('a'), None);
    assert!(cites(&a, &cite_a));
    assert!(!cites(&b, &cite_a));
}

#[test]
fn full_short_and_ibid_notes_resolve_to_one_source() {
    let raw = [
        "1. Daniel J. Harrington, Interpreting the Old Testament (Wilmington, DE: Glazier, 1981), 42.",
        "2. Harrington, Interpreting the Old Testament, 57.",
        "3. Ibid.",
        "4. Ibid., 60.",
        "5. John S. Kloppenborg, The Formation of Q (Philadelphia: Fortress, 1987), 12.",
    ];
    let notes: Vec<Note> = raw.iter().filter_map(|n| parse_note(n)).collect();
    assert_eq!(notes.len(), 5);
    let resolved = resolve_notes(&notes);
    let entry = parse_reference(CASES[6].raw);
    let hits: Vec<bool> = resolved
        .iter()
        .map(|r| r.as_ref().is_some_and(|r| note_cites(&entry, r)))
        .collect();
    assert_eq!(hits, [true, true, true, true, false]);
    assert_eq!(resolved[2].as_ref().unwrap().page.as_deref(), Some("57"));
    assert_eq!(resolved[3].as_ref().unwrap().page.as_deref(), Some("60"));
}

#[test]
fn a_reference_is_matched_to_an_owned_document() {
    let entry = parse_reference(CASES[6].raw);
    let owned = Known {
        hash: "h".into(),
        families: vec!["harrington".into()],
        year: Some(1981),
        title: "Interpreting the Old Testament: A Practical Guide".into(),
        doi: String::new(),
    };
    assert!(is_same_work(&entry, &owned));
    let other_year = Known {
        year: Some(1999),
        ..owned.clone()
    };
    assert!(!is_same_work(&entry, &other_year));
    let other_title = Known {
        title: "Something Else Entirely".into(),
        ..owned.clone()
    };
    assert!(!is_same_work(&entry, &other_title));
    let by_doi = Known {
        families: vec!["x".into()],
        year: None,
        title: String::new(),
        doi: "10.1017/S0017816000030234".into(),
        ..owned
    };
    assert!(is_same_work(&parse_reference(CASES[0].raw), &by_doi));
}

#[test]
fn a_citation_is_shown_in_its_sentence() {
    let text =
        "First sentence here. As Kloppenborg (1987, 41) argued, the layers differ. Then more.";
    let found = citations_in(text);
    let ctx = context_of(text, found[0].start, found[0].end);
    assert_eq!(ctx, "As Kloppenborg (1987, 41) argued, the layers differ.");
}

#[test]
fn a_run_of_entries_is_split_at_each_author_year_start() {
    let run = "Jones, A. and Lee, B. (2015a). On narrative form. Journal of Tests, 3, 1-20. Smith, J. (2019). The Book of Examples. Example Press, London. Zhang, W. (2020). A work nobody cites. Example Press.";
    let parts = split_entries(run);
    assert_eq!(parts.len(), 3, "{parts:?}");
    assert!(parts[1].starts_with("Smith, J. (2019)"));
    assert!(parts[2].starts_with("Zhang, W. (2020)"));
    assert_eq!(
        split_entries("Harrington, Daniel J. Interpreting. Glazier, 1981.").len(),
        1
    );
}

#[test]
fn a_year_ending_a_title_is_dropped() {
    assert_eq!(
        parse_reference("[1] Adams, C. A first work on the subject. 2001.").title,
        "A first work on the subject"
    );
}
