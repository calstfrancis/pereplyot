use super::*;

fn mark(id: &str, page: u32, snippet: &str, prefix: &str, suffix: &str) -> MarkText {
    MarkText {
        id: id.into(),
        page,
        snippet: snippet.into(),
        prefix: prefix.into(),
        suffix: suffix.into(),
    }
}

/// A fixed body of prose, so two "editions" can lay out the same words differently.
fn prose() -> Vec<String> {
    let sentences = [
        "The synoptic problem concerns the literary relationship among the first three gospels.",
        "Markan priority holds that Mark was written first and used by Matthew and Luke.",
        "The hypothetical source Q accounts for material shared by Matthew and Luke but absent from Mark.",
        "Kloppenborg argued that Q was composed in stages, beginning with a collection of wisdom sayings.",
        "Critics of the two source hypothesis point to the minor agreements between Matthew and Luke against Mark.",
        "Farrer proposed instead that Luke used both Mark and Matthew, dispensing with Q altogether.",
        "The debate turns on whether the double tradition shows signs of Lukan editing of Matthean text.",
        "Redaction criticism examines how each evangelist shaped the traditions that came to hand.",
        "Form criticism had earlier classified the units of tradition into pronouncement stories and miracle tales.",
        "Any account of the sources must also reckon with the oral transmission that preceded them.",
        "The Gospel of Thomas offers an independent witness to sayings that also appear in Q.",
        "Scholars remain divided on whether Thomas depends upon the canonical gospels at all.",
    ];
    sentences.iter().map(|s| s.to_string()).collect()
}

fn paginate(sentences: &[String], per_page: usize) -> Vec<String> {
    sentences.chunks(per_page).map(|c| c.join(" ")).collect()
}

fn outcome<'a>(out: &'a [Outcome], id: &str) -> &'a Outcome {
    out.iter().find(|o| o.id == id).unwrap()
}

#[test]
fn the_same_text_on_the_same_page_is_exact() {
    let pages = paginate(&prose(), 3);
    let marks = [mark(
        "a",
        2,
        "Kloppenborg argued that Q was composed in stages",
        "",
        "",
    )];
    let out = reanchor(&marks, &pages, 4);
    assert_eq!(out[0].fit, Fit::Exact);
    assert_eq!(out[0].page, Some(2));
}

#[test]
fn a_re_paginated_edition_moves_marks_without_losing_them() {
    let old = paginate(&prose(), 3);
    let new = paginate(&prose(), 5);
    let marks = [
        mark("a", 2, "Kloppenborg argued that Q was composed in stages, beginning with a collection of wisdom sayings", "", ""),
        mark("b", 3, "Redaction criticism examines how each evangelist shaped the traditions", "", ""),
        mark("c", 4, "The Gospel of Thomas offers an independent witness", "", ""),
    ];
    let out = reanchor(&marks, &new, old.len() as u32);
    for o in &out {
        assert!(matches!(o.fit, Fit::Exact | Fit::Moved), "{o:?}");
    }
    assert_eq!(outcome(&out, "a").page, Some(1));
    assert_eq!(outcome(&out, "b").page, Some(2));
    assert_eq!(outcome(&out, "c").page, Some(3));
    assert_eq!(outcome(&out, "a").fit, Fit::Moved);
}

#[test]
fn ocr_noise_and_hyphenation_still_find_the_words() {
    let old = paginate(&prose(), 3);
    let mut noisy = prose();
    noisy[3] = "Kloppenborg argued that Q was com-\nposed in stages, begin-\nning with a coUection of wisdom sayings.".into();
    let new = paginate(&noisy, 3);
    let marks = [mark("a", 2, "Kloppenborg argued that Q was composed in stages, beginning with a collection of wisdom sayings", "", "")];
    let out = reanchor(&marks, &new, old.len() as u32);
    assert!(
        matches!(out[0].fit, Fit::Exact | Fit::Uncertain),
        "{:?}",
        out[0]
    );
    assert_eq!(out[0].page, Some(2));
    assert!(out[0].score > 0.8, "{:?}", out[0]);
}

#[test]
fn a_reworded_passage_is_flagged_not_trusted() {
    let old = paginate(&prose(), 3);
    let mut edited = prose();
    edited[5] =
        "Farrer proposed that Luke drew on Mark and Matthew, which makes Q unnecessary.".into();
    let new = paginate(&edited, 3);
    let marks = [mark("a", 2, "Farrer proposed instead that Luke used both Mark and Matthew, dispensing with Q altogether", "", "")];
    let out = reanchor(&marks, &new, old.len() as u32);
    assert!(
        matches!(out[0].fit, Fit::Uncertain | Fit::Orphaned),
        "{:?}",
        out[0]
    );
}

#[test]
fn a_passage_cut_from_the_new_edition_is_orphaned() {
    let old = paginate(&prose(), 3);
    let mut cut = prose();
    cut.remove(10);
    cut.remove(10);
    let new = paginate(&cut, 3);
    let marks = [mark(
        "a",
        4,
        "The Gospel of Thomas offers an independent witness to sayings that also appear in Q",
        "",
        "",
    )];
    let out = reanchor(&marks, &new, old.len() as u32);
    assert_eq!(out[0].fit, Fit::Orphaned, "{:?}", out[0]);
    assert_eq!(out[0].page, None);
}

#[test]
fn repeated_words_are_told_apart_by_their_context() {
    let pages = vec![
        "Introduction. As noted above, the point is plain. Later we return to it.".to_string(),
        "Elsewhere: the point is plain. And so the argument ends here.".to_string(),
    ];
    let marks = [mark(
        "a",
        2,
        "the point is plain",
        "Elsewhere",
        "And so the argument",
    )];
    let out = reanchor(&marks, &pages, 2);
    assert_eq!(out[0].page, Some(2));
    assert_ne!(out[0].fit, Fit::Orphaned);
    let marks = [mark(
        "b",
        1,
        "the point is plain",
        "As noted above",
        "Later we return",
    )];
    assert_eq!(reanchor(&marks, &pages, 2)[0].page, Some(1));
}

#[test]
fn a_one_word_mark_is_not_guessed() {
    let pages = paginate(&prose(), 3);
    let marks = [mark("a", 1, "Q", "", "")];
    assert_eq!(reanchor(&marks, &pages, 4)[0].fit, Fit::Orphaned);
}

#[test]
fn a_whole_set_survives_a_new_layout() {
    let old = paginate(&prose(), 3);
    let new = paginate(&prose(), 4);
    let sentences = prose();
    let marks: Vec<MarkText> = sentences
        .iter()
        .enumerate()
        .map(|(i, s)| mark(&format!("m{i}"), (i / 3 + 1) as u32, s, "", ""))
        .collect();
    let out = reanchor(&marks, &new, old.len() as u32);
    let good = out
        .iter()
        .filter(|o| matches!(o.fit, Fit::Exact | Fit::Moved))
        .count();
    assert_eq!(good, marks.len(), "{out:#?}");
    for (i, o) in out.iter().enumerate() {
        assert_eq!(o.page, Some((i / 4 + 1) as u32), "mark {i}");
    }
}

#[test]
fn the_range_points_at_the_words_in_the_page_text() {
    let pages = paginate(&prose(), 3);
    let snippet = "Kloppenborg argued that Q was composed in stages";
    let out = reanchor(&[mark("a", 2, snippet, "", "")], &pages, 4);
    let (from, to) = out[0].range.unwrap();
    let shown: String = pages[1].chars().skip(from).take(to - from).collect();
    assert_eq!(shown, snippet);
}

#[test]
fn a_range_covers_a_hyphen_broken_word_whole() {
    let pages = vec![
        "Opening lines here.\nThe interpre-\ntation of Q is contested among scholars today."
            .to_string(),
    ];
    let out = reanchor(
        &[mark("a", 1, "the interpretation of Q is contested", "", "")],
        &pages,
        1,
    );
    let (from, to) = out[0].range.unwrap();
    let shown: String = pages[0].chars().skip(from).take(to - from).collect();
    assert_eq!(shown, "The interpre-\ntation of Q is contested");
}
