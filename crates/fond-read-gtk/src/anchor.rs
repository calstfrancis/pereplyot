//! Finding a passage again in a text that may have changed since it was marked. A mark keeps the
//! words it covers plus a little text on each side (a W3C-style quote selector) and, where it can,
//! the position it had (a position selector); this puts them back together with fuzzy matching,
//! so an annotation survives a re-flowed, re-typeset or lightly corrected edition of a book.

const GRAM: usize = 2;

/// Text with every run of whitespace as one space, and for each kept character where it was in
/// the original (in chars).
pub(crate) struct Normal {
    pub chars: Vec<char>,
    origin: Vec<usize>,
}

pub(crate) fn normalise(text: &str) -> Normal {
    normalise_with(text, true)
}

/// As [`normalise`], optionally keeping one space at either end (for context, where a space
/// next to the passage is part of what matches).
fn normalise_with(text: &str, trim: bool) -> Normal {
    let mut chars = Vec::new();
    let mut origin = Vec::new();
    let mut in_space = trim;
    for (i, c) in text.chars().enumerate() {
        if c.is_whitespace() || c == '\u{a0}' {
            if !in_space {
                chars.push(' ');
                origin.push(i);
                in_space = true;
            }
        } else {
            chars.push(c);
            origin.push(i);
            in_space = false;
        }
    }
    while trim && chars.last() == Some(&' ') {
        chars.pop();
        origin.pop();
    }
    Normal { chars, origin }
}

fn find_all(hay: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let first = needle[0];
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i] == first && hay[i..i + needle.len()] == *needle {
            out.push(i);
        }
        i += 1;
    }
    out
}

/// Up to `n` characters of context on each side of the first place `snippet` appears in `text`.
pub(crate) fn context_around(
    text: &str,
    snippet: &str,
    n: usize,
) -> (Option<String>, Option<String>) {
    let hay = normalise(text);
    let needle = normalise(snippet);
    let Some(at) = find_all(&hay.chars, &needle.chars).first().copied() else {
        return (None, None);
    };
    let end = at + needle.chars.len();
    let before: String = hay.chars[at.saturating_sub(n)..at].iter().collect();
    let after: String = hay.chars[end..(end + n).min(hay.chars.len())]
        .iter()
        .collect();
    (
        (!before.is_empty()).then_some(before),
        (!after.is_empty()).then_some(after),
    )
}

/// How much text is kept on each side of a mark to find it again.
pub(crate) const CONTEXT_CHARS: usize = 40;

/// How many characters at the end of `a` equal the end of `b`.
fn common_suffix(a: &[char], b: &[char]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

fn common_prefix(a: &[char], b: &[char]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn grams(s: &[char]) -> Vec<(char, char)> {
    s.windows(GRAM).map(|w| (w[0], w[1])).collect()
}

/// Dice similarity of the character pairs of two texts, 0 to 1.
fn similarity(a: &[(char, char)], b: &[char]) -> f64 {
    if a.is_empty() || b.len() < GRAM {
        return 0.0;
    }
    let mut pool: std::collections::HashMap<(char, char), i32> = std::collections::HashMap::new();
    for g in a {
        *pool.entry(*g).or_default() += 1;
    }
    let mut shared = 0;
    let mut total_b = 0;
    for w in b.windows(GRAM) {
        total_b += 1;
        if let Some(c) = pool.get_mut(&(w[0], w[1])) {
            if *c > 0 {
                *c -= 1;
                shared += 1;
            }
        }
    }
    2.0 * shared as f64 / (a.len() + total_b) as f64
}

/// Where a passage now is, as char offsets `(start, end)` into the original `text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Located {
    pub start: usize,
    pub end: usize,
    /// The words were found exactly, rather than approximately.
    pub exact: bool,
}

/// Find `snippet` in `text`. `prefix` and `suffix` are the text that stood before and after it,
/// and `hint` is where (char offsets, original text) it used to be.
pub(crate) fn locate(
    text: &str,
    snippet: &str,
    prefix: Option<&str>,
    suffix: Option<&str>,
    hint: Option<(usize, usize)>,
) -> Option<Located> {
    let hay = normalise(text);
    let needle = normalise(snippet);
    if needle.chars.is_empty() || hay.chars.is_empty() {
        return None;
    }
    let pre = prefix
        .map(|p| normalise_with(p, false).chars)
        .unwrap_or_default();
    let suf = suffix
        .map(|p| normalise_with(p, false).chars)
        .unwrap_or_default();
    // The hint is in original characters; find the nearest normalised index.
    let hint_norm = hint.map(|(s, _)| hay.origin.partition_point(|&o| o < s));
    let to_original = |s: usize, e: usize, exact: bool| Located {
        start: hay.origin[s],
        end: hay
            .origin
            .get(e)
            .copied()
            .unwrap_or_else(|| text.chars().count()),
        exact,
    };

    let exact = find_all(&hay.chars, &needle.chars);
    if !exact.is_empty() {
        let score = |at: usize| -> (usize, i64) {
            let end = at + needle.chars.len();
            let context =
                common_suffix(&hay.chars[..at], &pre) + common_prefix(&hay.chars[end..], &suf);
            let near = hint_norm.map_or(0, |h| -(at as i64 - h as i64).abs());
            (context, near)
        };
        let best = exact.iter().copied().max_by_key(|&at| score(at))?;
        return Some(to_original(best, best + needle.chars.len(), true));
    }

    // The words changed. First try the two ends of the passage on their own.
    let len = needle.chars.len();
    let end_len = (len / 3).clamp(8, 40).min(len);
    if len >= 16 {
        let head = &needle.chars[..end_len];
        let tail = &needle.chars[len - end_len..];
        let heads = find_all(&hay.chars, head);
        let tails = find_all(&hay.chars, tail);
        let mut best: Option<(i64, usize, usize)> = None;
        for &h in &heads {
            for &t in &tails {
                let end = t + end_len;
                if end <= h {
                    continue;
                }
                let span = end - h;
                if span * 10 < len * 6 || span * 10 > len * 16 {
                    continue;
                }
                let cost = (span as i64 - len as i64).abs()
                    + hint_norm.map_or(0, |p| (h as i64 - p as i64).abs() / 8);
                if best.map_or(true, |b| cost < b.0) {
                    best = Some((cost, h, end));
                }
            }
        }
        if let Some((_, h, end)) = best {
            return Some(to_original(h, end, false));
        }
    }

    // Otherwise the stretch of the text most like the passage, searched near where it was if we
    // know, and anywhere if we don't.
    let wanted = grams(&needle.chars);
    let (lo, hi) = match hint_norm {
        Some(h) => (
            h.saturating_sub(len * 2 + 200),
            (h + len * 3 + 200).min(hay.chars.len()),
        ),
        None => (0, hay.chars.len()),
    };
    let step = (len / 8).max(1);
    let mut best: Option<(f64, usize)> = None;
    let mut at = lo;
    while at < hi {
        let end = (at + len).min(hay.chars.len());
        let s = similarity(&wanted, &hay.chars[at..end]);
        if best.map_or(true, |b| s > b.0) {
            best = Some((s, at));
        }
        at += step;
    }
    let (score, mut at) = best?;
    if score < 0.55 {
        return None;
    }
    // Refine to the single-character position around the coarse hit.
    let mut refined = (score, at);
    for cand in at.saturating_sub(step)..(at + step).min(hay.chars.len()) {
        let end = (cand + len).min(hay.chars.len());
        let s = similarity(&wanted, &hay.chars[cand..end]);
        if s > refined.0 {
            refined = (s, cand);
        }
    }
    at = refined.1;
    Some(to_original(at, (at + len).min(hay.chars.len()), false))
}

/// Char offsets into `text` as the UTF-16 offsets a JavaScript string uses.
pub(crate) fn to_utf16(text: &str, char_offsets: &[usize]) -> Vec<usize> {
    let mut sorted: Vec<(usize, usize)> = char_offsets.iter().copied().enumerate().collect();
    sorted.sort_by_key(|&(_, c)| c);
    let mut out = vec![0; char_offsets.len()];
    let mut units = 0;
    let mut chars = text.chars();
    let mut at = 0;
    for (i, want) in sorted {
        while at < want {
            match chars.next() {
                Some(c) => units += c.len_utf16(),
                None => break,
            }
            at += 1;
        }
        out[i] = units;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOK: &str =
        "In the beginning the word was heard. The covenant community gathered for liturgy \
        and the reading of scripture. Then the covenant community gathered for liturgy again, \
        and the narrative continued.";

    fn found(
        text: &str,
        snippet: &str,
        pre: Option<&str>,
        suf: Option<&str>,
        hint: Option<(usize, usize)>,
    ) -> Option<String> {
        let l = locate(text, snippet, pre, suf, hint)?;
        Some(text.chars().skip(l.start).take(l.end - l.start).collect())
    }

    #[test]
    fn an_exact_passage_is_found_and_context_picks_between_repeats() {
        let snippet = "covenant community gathered for liturgy";
        let at = |l: Located| BOOK.chars().take(l.start).collect::<String>();
        assert!(locate(BOOK, snippet, None, None, None).unwrap().exact);
        let first = locate(
            BOOK,
            snippet,
            Some("was heard. The "),
            Some(" and the reading"),
            None,
        )
        .unwrap();
        let second = locate(
            BOOK,
            snippet,
            Some("scripture. Then the "),
            Some(" again, and"),
            None,
        )
        .unwrap();
        assert!(at(first).ends_with("The "), "{}", at(first));
        assert!(at(second).ends_with("Then the "), "{}", at(second));
        let by_position =
            locate(BOOK, snippet, None, None, Some((second.start, second.end))).unwrap();
        assert_eq!(by_position, second);
    }

    #[test]
    fn a_reflowed_edition_with_different_whitespace_still_matches() {
        let reflowed = BOOK.replace(". ", ".\n\n").replace(" for ", "\n for ");
        let l = locate(
            &reflowed,
            "covenant community gathered for liturgy and the reading",
            None,
            None,
            None,
        )
        .unwrap();
        assert!(l.exact);
        let got: String = reflowed
            .chars()
            .skip(l.start)
            .take(l.end - l.start)
            .collect();
        assert!(got.starts_with("covenant community gathered"), "{got}");
        assert!(got.ends_with("reading"), "{got}");
    }

    #[test]
    fn a_corrected_typo_is_still_found() {
        let edited = BOOK
            .replace("scripture", "Scripture")
            .replace("community", "communty");
        let l = locate(
            &edited,
            "the covenant community gathered for liturgy and the reading of scripture.",
            None,
            None,
            None,
        )
        .unwrap();
        assert!(!l.exact);
        let got: String = edited.chars().skip(l.start).take(l.end - l.start).collect();
        assert!(got.contains("covenant"), "{got}");
        assert!(got.contains("Scripture"), "{got}");
    }

    #[test]
    fn a_rewritten_middle_is_found_by_similarity_near_where_it_was() {
        let old = "the covenant community gathered for liturgy and the reading of scripture aloud";
        let edited = "In the beginning the word was heard. The covenant community assembled for the liturgy \
            and the reading of the scripture aloud. Then more.";
        let hint = BOOK
            .find("the covenant")
            .map(|b| (BOOK[..b].chars().count(), 0));
        let got = found(edited, old, None, None, hint).unwrap();
        assert!(
            got.contains("assembled") || got.contains("covenant"),
            "{got}"
        );
        assert!(got.chars().count() > 40);
    }

    #[test]
    fn a_passage_that_is_gone_is_not_found() {
        assert_eq!(
            locate(
                BOOK,
                "completely unrelated sentence about astronomy and tides",
                None,
                None,
                None
            ),
            None
        );
    }

    #[test]
    fn context_is_taken_from_around_the_first_match() {
        let (pre, suf) = context_around(BOOK, "covenant community gathered", 12);
        assert!(pre.unwrap().ends_with("The "));
        assert!(suf.unwrap().starts_with(" for liturgy"));
        assert_eq!(context_around(BOOK, "not here", 10), (None, None));
    }

    #[test]
    fn offsets_become_utf16_units() {
        let text = "a\u{1F600}b";
        assert_eq!(to_utf16(text, &[0, 1, 2, 3]), vec![0, 1, 3, 4]);
    }
}
