use super::*;

fn mark(
    id: &str,
    snippet: &str,
    prefix: &str,
    suffix: &str,
    note: &str,
    tags: &[&str],
) -> SourceMark {
    SourceMark {
        id: id.into(),
        label: "41".into(),
        snippet: snippet.into(),
        prefix: prefix.into(),
        suffix: suffix.into(),
        note: note.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
    }
}

#[test]
fn only_marks_tagged_remember_make_cards() {
    let marks = [
        mark(
            "a",
            "Q was composed in stages",
            "Kloppenborg argued that",
            "beginning with wisdom sayings.",
            "",
            &["remember"],
        ),
        mark("b", "an aside worth keeping", "x", "y", "", &["method"]),
    ];
    let cards = generate("h", "Formation of Q", &marks, "", 100.0);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].kind, Kind::Cloze);
    assert!(cards[0].front.contains("[ … ]"), "{}", cards[0].front);
    assert!(cards[0].front.contains("Kloppenborg argued that"));
    assert!(
        !cards[0].front.contains("composed in stages"),
        "the answer is blanked"
    );
    assert_eq!(cards[0].back, "Q was composed in stages");
    assert!(cards[0].is_new() && cards[0].due == 100.0);
}

#[test]
fn a_mark_with_no_context_asks_which_passage_it_was() {
    let cards = generate(
        "h",
        "Formation of Q",
        &[mark(
            "a",
            "Q was composed in stages",
            "",
            "",
            "",
            &["#Remember"],
        )],
        "",
        0.0,
    );
    assert!(
        cards[0]
            .front
            .starts_with("Formation of Q, p. 41: which passage begins “Q was composed …”"),
        "{}",
        cards[0].front
    );
}

#[test]
fn your_own_question_and_answer_become_a_card() {
    let note = "Q: Who proposed the Farrer hypothesis? A: Austin Farrer, in 1955. #remember";
    assert_eq!(
        question_in(note),
        Some((
            "Who proposed the Farrer hypothesis?".into(),
            "Austin Farrer, in 1955.".into()
        ))
    );
    let cards = generate(
        "h",
        "T",
        &[mark(
            "a",
            "some highlighted words",
            "",
            "",
            note,
            &["remember"],
        )],
        "",
        0.0,
    );
    let kinds: Vec<Kind> = cards.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [Kind::Question, Kind::Cloze]);
    assert_eq!(cards[0].back, "Austin Farrer, in 1955.");
    assert_eq!(question_in("just a note"), None);
    assert_eq!(question_in("Q: only a question"), None);
}

#[test]
fn the_summary_becomes_a_card_only_when_something_is_remembered() {
    let m = mark("a", "words in a passage", "", "", "", &["remember"]);
    assert_eq!(
        generate("h", "T", std::slice::from_ref(&m), "It argues X.", 0.0).len(),
        2
    );
    assert_eq!(generate("h", "T", &[], "It argues X.", 0.0).len(), 0);
    assert_eq!(
        generate("h", "T", std::slice::from_ref(&m), "  ", 0.0).len(),
        1
    );
}

#[test]
fn a_regenerated_card_keeps_its_schedule_and_a_removed_one_goes() {
    let sched = Scheduler::default();
    let m = mark(
        "a",
        "words in a passage",
        "before the text",
        "after the text",
        "",
        &["remember"],
    );
    let mut stored = generate("h", "T", std::slice::from_ref(&m), "", 0.0);
    review(&mut stored[0], Rating::Good, 0.0, &sched);
    let due = stored[0].due;
    assert!(due > 0.0 && stored[0].reps == 1);
    let mut edited = m.clone();
    edited.suffix = "after the edited text".into();
    let merged = merge(
        &stored,
        generate("h", "T", std::slice::from_ref(&edited), "", 50.0),
    );
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].due, due, "the schedule is kept");
    assert!(merged[0].front.contains("edited"), "the wording is current");
    assert!(
        merge(&stored, generate("h", "T", &[], "", 50.0)).is_empty(),
        "no longer remembered"
    );
}

#[test]
fn reviews_move_cards_and_forgetting_brings_them_back_soon() {
    let sched = Scheduler::default();
    let m = mark("a", "words in a passage", "", "", "", &["remember"]);
    let mut c = generate("h", "T", &[m], "", 0.0).remove(0);
    review(&mut c, Rating::Good, 0.0, &sched);
    let first_wait = c.due;
    let when = c.due;
    review(&mut c, Rating::Good, when, &sched);
    assert!(
        c.due - first_wait > first_wait,
        "the second wait is longer: {first_wait} then {}",
        c.due - first_wait
    );
    let before = c.stability.unwrap();
    let now = c.due;
    review(&mut c, Rating::Again, now, &sched);
    assert_eq!(c.lapses, 1);
    assert!(c.stability.unwrap() < before);
    assert!(c.due - now < 1.0, "a forgotten card returns within the day");
}

#[test]
fn the_due_list_puts_overdue_first_and_caps_new_cards() {
    let sched = Scheduler::default();
    let marks: Vec<SourceMark> = (0..6)
        .map(|i| {
            mark(
                &format!("m{i}"),
                "some words in the passage",
                "",
                "",
                "",
                &["remember"],
            )
        })
        .collect();
    let mut cards = generate("h", "T", &marks, "", 0.0);
    review(&mut cards[0], Rating::Good, 0.0, &sched);
    review(&mut cards[1], Rating::Good, 0.0, &sched);
    cards[1].due = 5.0;
    cards[0].due = 3.0;
    let list = due(&cards, 10.0, 2);
    assert_eq!(list.len(), 4, "two studied, two of the four new");
    assert_eq!(list[0].id, cards[0].id, "most overdue first");
    assert_eq!(list[1].id, cards[1].id);
    assert!(list[2].is_new() && list[3].is_new());
    assert_eq!(due(&cards, 1.0, 0).len(), 0, "nothing studied is due yet");
}

#[test]
fn cards_round_trip_through_json() {
    let m = mark(
        "a",
        "some words in the passage",
        "x y z a b c d e",
        "f g h i j k l m",
        "",
        &["remember"],
    );
    let c = generate("h", "T", &[m], "", 7.5).remove(0);
    let back: Card = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back, c);
}
