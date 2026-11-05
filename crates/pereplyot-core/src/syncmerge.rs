//! Merging the copies a sync client leaves when two devices changed the same file: nothing is lost and
//! the later edit of any one mark wins.

use fond_annot::Annotation;

/// The file `name` was a conflicting copy of, if it is one: a Syncthing `x.sync-conflict-…`, or a Dropbox or
/// Nextcloud `x (conflicted copy …)`.
pub fn conflict_original(name: &str) -> Option<String> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !e.is_empty() && e.len() <= 8 => (s, Some(e)),
        _ => (name, None),
    };
    let original = match stem.find(".sync-conflict-") {
        Some(at) => stem[..at].to_string(),
        None => {
            let at = stem.find(" (")?;
            let tail = &stem[at + 2..];
            if !(tail.contains("conflicted copy") && tail.ends_with(')')) {
                return None;
            }
            stem[..at].to_string()
        }
    };
    if original.is_empty() {
        return None;
    }
    Some(match ext {
        Some(e) => format!("{original}.{e}"),
        None => original,
    })
}

fn stamp(a: &Annotation) -> &str {
    a.modified.as_deref().or(a.created.as_deref()).unwrap_or("")
}

/// Every mark of both lists, once each by id; when both have a mark the later `modified` stays (a tie
/// keeps `mine`). Order: `mine` as it was, then the marks only `theirs` has.
pub fn merge_annotations(mine: Vec<Annotation>, theirs: Vec<Annotation>) -> Vec<Annotation> {
    let mut out = mine;
    for t in theirs {
        match out.iter().position(|m| m.id == t.id) {
            Some(i) => {
                if stamp(&t) > stamp(&out[i]) {
                    out[i] = t;
                }
            }
            None => out.push(t),
        }
    }
    out
}

/// Bookmarks are a set: every page either device marked.
pub fn merge_bookmarks(mut mine: Vec<u32>, theirs: &[u32]) -> Vec<u32> {
    for p in theirs {
        if !mine.contains(p) {
            mine.push(*p);
        }
    }
    mine.sort_unstable();
    mine
}

#[cfg(test)]
mod tests {
    use super::*;
    use fond_annot::AnnotationKind;

    fn mark(id: &str, note: &str, modified: Option<&str>) -> Annotation {
        Annotation {
            id: id.into(),
            kind: AnnotationKind::Highlight,
            page: Some(1),
            chapter: None,
            quadpoints: vec![],
            snippet: Some("words".into()),
            snippet_prefix: None,
            snippet_suffix: None,
            color: None,
            note: Some(note.into()),
            created: Some("2026-01-01".into()),
            modified: modified.map(str::to_string),
            extra: Default::default(),
        }
    }

    #[test]
    fn conflict_copies_name_their_originals() {
        assert_eq!(
            conflict_original("abc.sync-conflict-20260512-101010-ABCDEFG.json").as_deref(),
            Some("abc.json")
        );
        assert_eq!(
            conflict_original("abc (conflicted copy 2026-05-12).json").as_deref(),
            Some("abc.json")
        );
        assert_eq!(
            conflict_original("abc (Jo's conflicted copy 2026-05-12 101010).json").as_deref(),
            Some("abc.json")
        );
        assert_eq!(conflict_original("abc.json"), None);
        assert_eq!(conflict_original("My Notes (draft).json"), None);
        assert_eq!(conflict_original(".sync-conflict-x.json"), None);
    }

    #[test]
    fn marks_from_both_devices_are_kept() {
        let mine = vec![mark("a", "mine", None), mark("b", "only here", None)];
        let theirs = vec![mark("a", "same", None), mark("c", "only there", None)];
        let merged = merge_annotations(mine, theirs);
        let ids: Vec<&str> = merged.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn the_later_edit_of_a_mark_wins_and_a_tie_keeps_mine() {
        let merged = merge_annotations(
            vec![
                mark("a", "old", Some("2026-02-01")),
                mark("b", "kept", Some("2026-03-01")),
            ],
            vec![
                mark("a", "new", Some("2026-02-09")),
                mark("b", "older", Some("2026-03-01")),
            ],
        );
        assert_eq!(merged[0].note.as_deref(), Some("new"));
        assert_eq!(merged[1].note.as_deref(), Some("kept"));
    }

    #[test]
    fn a_random_interleaving_of_edits_never_loses_a_mark() {
        // A deterministic pseudo-random walk: two devices add, edit and merge in turn; every mark ever
        // added by either must still be in the final merge.
        let mut seed = 0x9E3779B97F4A7C15u64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let mut devices: [Vec<Annotation>; 2] = [vec![], vec![]];
        let mut ever: Vec<String> = Vec::new();
        for step in 0..1000u64 {
            let d = next(2) as usize;
            match next(4) {
                0 | 1 => {
                    let id = format!("m{step}");
                    devices[d].push(mark(&id, "new", Some(&format!("2026-{:05}", step))));
                    ever.push(id);
                }
                2 if !devices[d].is_empty() => {
                    let i = next(devices[d].len() as u64) as usize;
                    devices[d][i].note = Some(format!("edit {step}"));
                    devices[d][i].modified = Some(format!("2026-{:05}", step));
                }
                _ => {
                    // The sync client runs: each device takes the merge of both.
                    let merged = merge_annotations(devices[0].clone(), devices[1].clone());
                    devices[0] = merged.clone();
                    devices[1] = merge_annotations(devices[1].clone(), devices[0].clone());
                }
            }
        }
        let merged = merge_annotations(devices[0].clone(), devices[1].clone());
        for id in &ever {
            assert!(merged.iter().any(|m| &m.id == id), "{id} was lost");
        }
        assert_eq!(merged.len(), ever.len(), "and nothing is duplicated");
    }

    #[test]
    fn bookmarks_are_a_union() {
        assert_eq!(merge_bookmarks(vec![5, 2], &[2, 9, 1]), [1, 2, 5, 9]);
    }
}
