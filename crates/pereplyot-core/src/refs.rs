//! Reading a bibliography: splitting an entry into authors, year and title, and finding the places the
//! text cites it. Heuristics only (no model); a field that can't be read confidently stays empty.

use crate::identity::find_doi;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParsedRef {
    pub raw: String,
    pub label: Option<u32>,
    /// Last words of each author's family name, lower-cased, in order.
    pub families: Vec<String>,
    pub year: Option<i32>,
    pub suffix: Option<char>,
    pub title: String,
    pub doi: Option<String>,
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn is_year_at(c: &[char], i: usize) -> Option<(i32, Option<char>, usize)> {
    if i + 4 > c.len() || !c[i..i + 4].iter().all(char::is_ascii_digit) {
        return None;
    }
    if i > 0 && (c[i - 1].is_alphanumeric()) {
        return None;
    }
    let y: i32 = c[i..i + 4].iter().collect::<String>().parse().ok()?;
    if !(1000..=2100).contains(&y) {
        return None;
    }
    let mut end = i + 4;
    let mut suffix = None;
    if end < c.len()
        && c[end].is_ascii_lowercase()
        && !(end + 1 < c.len() && c[end + 1].is_alphabetic())
    {
        suffix = Some(c[end]);
        end += 1;
    } else if end < c.len() && (c[end].is_alphanumeric()) {
        return None;
    }
    Some((y, suffix, end))
}

fn strip_label(s: &str) -> (Option<u32>, &str) {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    if bytes.first() == Some(&b'[') {
        if let Some(close) = t.find(']') {
            if let Ok(n) = t[1..close].trim().parse::<u32>() {
                return (Some(n), t[close + 1..].trim_start());
            }
        }
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let rest = &t[digits..];
        if rest.starts_with(". ") || rest.starts_with(") ") || rest.starts_with(' ') && digits <= 3
        {
            let after = rest[1..].trim_start();
            // "1. Smith…" is a label; "1990 was…" is not (a year has four digits).
            if !after.is_empty() && !rest.starts_with(' ') {
                return (t[..digits].parse().ok(), after);
            }
        }
    }
    (None, t)
}

const NOT_ENDING: [&str; 14] = [
    "jr", "sr", "ed", "eds", "trans", "vol", "no", "pp", "p", "st", "dr", "ca", "cf", "ibid",
];

fn looks_like_initials(s: &str) -> bool {
    let t = s.trim().trim_end_matches('.');
    !t.is_empty()
        && t.split([' ', '.'])
            .filter(|p| !p.is_empty())
            .all(|p| p.chars().count() <= 2 && p.chars().next().is_some_and(char::is_uppercase))
}

/// Family-name last words from an author segment in any of: "Smith, J., & Lee, K." / "Smith, John S., and
/// Pheme Perkins" / "J. S. Kloppenborg and K. Lee".
pub fn families_of(seg: &str) -> Vec<String> {
    let seg = seg
        .replace(" & ", " and ")
        .replace(", and ", " and ")
        .replace(", &", " and ")
        .replace("et al.", "")
        .replace(';', " and ");
    let seg = seg.trim().trim_end_matches([',', '.']).trim();
    let mut out = Vec::new();
    for piece in seg.split(" and ") {
        let chunks: Vec<&str> = piece
            .split(',')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        let mut i = 0;
        while i < chunks.len() {
            let chunk = chunks[i];
            let words: Vec<&str> = chunk.split_whitespace().collect();
            let given_first = words.len() > 1 && looks_like_initials(words[0]);
            if let Some(f) = words.last().copied() {
                let f: String = f
                    .chars()
                    .filter(|c| c.is_alphabetic() || *c == '-' || *c == '\'')
                    .collect();
                if !f.is_empty() && f.chars().next().is_some_and(char::is_alphabetic) {
                    out.push(f.to_lowercase());
                }
            }
            // "Family, Given" — the next chunk is that author's given names.
            if !given_first && i + 1 < chunks.len() {
                let next = chunks[i + 1];
                let nw: Vec<&str> = next.split_whitespace().collect();
                let given_like = looks_like_initials(next)
                    || (i == 0
                        && chunks.len() == 2
                        && !chunk.contains(' ')
                        && nw.len() <= 3
                        && nw
                            .iter()
                            .all(|w| w.chars().next().is_some_and(char::is_uppercase)));
                if given_like {
                    i += 1;
                }
            }
            i += 1;
        }
    }
    out
}

const CONNECTORS: [&str; 20] = [
    "and", "&", "van", "von", "de", "der", "den", "del", "la", "le", "da", "di", "du", "et", "al.",
    "jr.", "sr.", "ed.", "eds.", "ii",
];
const TITLE_STARTERS: [&str; 9] = [
    "The",
    "A",
    "An",
    "On",
    "In",
    "Of",
    "Toward",
    "Towards",
    "Introduction",
];

fn is_initial_token(t: &str) -> bool {
    let b = t.trim_end_matches('.');
    t.ends_with('.') && b.chars().count() <= 2 && b.chars().next().is_some_and(char::is_uppercase)
}

/// The names at the start of an entry that has no parenthesised year: the byte offset where they end.
fn author_segment_end(body: &str) -> usize {
    let mut pos = 0;
    let mut prev_initial_period = false;
    let tokens: Vec<(usize, &str)> = body
        .split(' ')
        .scan(0usize, |p, t| {
            let at = *p;
            *p += t.len() + 1;
            Some((at, t))
        })
        .collect();
    for (i, (at, tok)) in tokens.iter().enumerate() {
        let lower = tok.to_lowercase();
        let first = tok.chars().next().unwrap_or(' ');
        if matches!(first, '“' | '"' | '*' | '_' | '(' | '‘' | '[') {
            return *at;
        }
        let connector = CONNECTORS.contains(&lower.as_str());
        let next_lower = tokens.get(i + 1).is_some_and(|(_, n)| {
            n.chars().next().is_some_and(char::is_lowercase)
                && !CONNECTORS.contains(&n.to_lowercase().as_str())
        });
        if i > 0 && !connector {
            if first.is_lowercase() {
                return *at;
            }
            if prev_initial_period
                && (TITLE_STARTERS.contains(&tok.trim_end_matches([',', '.'])) || next_lower)
            {
                return *at;
            }
        }
        pos = at + tok.len();
        let bare = tok.trim_end_matches(',');
        if bare.ends_with('.') && !is_initial_token(bare) && !connector && bare.chars().count() > 2
        {
            return pos;
        }
        prev_initial_period = is_initial_token(bare);
    }
    pos
}

/// Like [`sentences`] but a lone capital before a period ends a sentence ("…formation of Q. Harvard").
fn loose_first_sentence(s: &str) -> String {
    let c = chars(s);
    let mut i = 0;
    while i + 1 < c.len() {
        if c[i] == '.' && c[i + 1] == ' ' && i + 2 < c.len() && c[i + 2].is_uppercase() {
            let mut w = i;
            while w > 0 && c[w - 1].is_alphabetic() {
                w -= 1;
            }
            let word: String = c[w..i].iter().collect();
            let lone_initial_pair = word.chars().count() == 1 && i + 3 < c.len() && c[i + 3] == '.';
            if !NOT_ENDING.contains(&word.to_lowercase().as_str()) && !lone_initial_pair {
                return c[..i].iter().collect::<String>().trim().to_string();
            }
        }
        i += 1;
    }
    s.trim().trim_end_matches('.').to_string()
}

/// Splits a run of author–year entries that came out as one block (no hanging indent, no gaps) at each
/// ". Surname, X. (2019)" boundary.
pub fn split_entries(text: &str) -> Vec<String> {
    let c = chars(text);
    let mut starts = vec![0usize];
    let mut i = 0;
    while i + 3 < c.len() {
        if c[i] == '.' && c[i + 1] == ' ' && c[i + 2].is_uppercase() {
            let word_end = c[i + 2..]
                .iter()
                .position(|ch| !ch.is_alphabetic() && *ch != '\'' && *ch != '-')
                .map(|n| i + 2 + n);
            if let Some(we) = word_end {
                let named = c.get(we) == Some(&',') && we - (i + 2) >= 2;
                let window: String = c[we..(we + 90).min(c.len())].iter().collect();
                let year_soon = window.find('(').is_some_and(|p| {
                    let cc = chars(&window[p + 1..]);
                    is_year_at(&cc, 0).is_some()
                });
                if named && year_soon {
                    starts.push(i + 2);
                }
            }
        }
        i += 1;
    }
    starts.push(c.len());
    starts
        .windows(2)
        .map(|w| c[w[0]..w[1]].iter().collect::<String>().trim().to_string())
        .filter(|e| !e.is_empty())
        .collect()
}

fn strip_trailing_year(title: &str) -> String {
    let t = title.trim_end();
    let c = chars(t);
    if c.len() > 5 {
        if let Some((_, _, end)) = is_year_at(&c, c.len() - 4) {
            if end == c.len() {
                return c[..c.len() - 4]
                    .iter()
                    .collect::<String>()
                    .trim_end_matches(['.', ',', ' ', '('])
                    .to_string();
            }
        }
    }
    t.to_string()
}

pub fn parse_reference(raw: &str) -> ParsedRef {
    let raw = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let (label, body) = strip_label(&raw);
    let c = chars(body);
    let mut p = ParsedRef {
        raw: raw.clone(),
        label,
        doi: find_doi(body),
        ..Default::default()
    };
    // The year: one in parentheses near the start wins; otherwise the last one in the first 400 chars.
    let mut paren: Option<(usize, i32, Option<char>)> = None;
    let mut last: Option<(i32, Option<char>)> = None;
    for i in 0..c.len().min(400) {
        if let Some((y, suffix, end)) = is_year_at(&c, i) {
            if i > 0
                && c[i - 1] == '('
                && end < c.len()
                && c[end] == ')'
                && paren.is_none()
                && i < 160
            {
                paren = Some((i - 1, y, suffix));
            }
            last = Some((y, suffix));
        }
    }
    let (paren_at, year) = match paren {
        Some((at, y, s)) => (Some(at), Some((y, s))),
        None => (None, last),
    };
    p.year = year.map(|(y, _)| y);
    p.suffix = year.and_then(|(_, s)| s);

    let (seg, rest): (String, String) = match paren_at {
        Some(at) => {
            let head: String = c[..at].iter().collect();
            let tail: String = c[at..].iter().collect();
            let close = tail.find(')').map_or(0, |i| i + 1);
            (head, tail[close..].to_string())
        }
        None => {
            let end = author_segment_end(body);
            (body[..end].to_string(), body[end..].to_string())
        }
    };
    p.families = families_of(&seg);

    // Title: quoted or italic text anywhere in the entry first; else the sentence after the names (and any bare year).
    let mut title = String::new();
    if let Some(open) = body.find(['“', '"', '*', '_']) {
        let rest_q = &body[open..];
        let open_c = rest_q.chars().next().unwrap_or('"');
        let close_c = if open_c == '“' { '”' } else { open_c };
        let inner = &rest_q[open_c.len_utf8()..];
        if let Some(end) = inner.find(close_c) {
            title = inner[..end].trim().trim_end_matches([',', '.']).to_string();
        }
    }
    if title.is_empty() {
        let mut rest = rest.trim_start_matches(['.', ',', ' ']).to_string();
        // Chicago author-date: "Perkins, Pheme. 1990. Reading…" — the year stands alone.
        let rc = chars(&rest);
        if let Some((_, _, end)) = is_year_at(&rc, 0) {
            rest = rc[end..]
                .iter()
                .collect::<String>()
                .trim_start_matches(['.', ',', ' '])
                .to_string();
        }
        title = loose_first_sentence(&rest);
    }
    p.title = strip_trailing_year(title.trim().trim_matches(['.', ',', ' ']));
    p
}

// --------------------------------------------------------------------------- in-text citations

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiteItem {
    Number(u32),
    AuthorYear {
        surnames: Vec<String>,
        year: i32,
        suffix: Option<char>,
        locator: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InText {
    /// Char offsets into the scanned text.
    pub start: usize,
    pub end: usize,
    pub items: Vec<CiteItem>,
}

fn numbers_in(inner: &str) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return None;
        }
        let range: Vec<&str> = part.split(['–', '-', '—']).map(str::trim).collect();
        match range.as_slice() {
            [n] => out.push(n.parse().ok()?),
            [a, b] => {
                let (a, b): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
                if a > b || b - a > 50 {
                    return None;
                }
                out.extend(a..=b);
            }
            _ => return None,
        }
    }
    (!out.is_empty()).then_some(out)
}

fn capitalised(w: &str) -> bool {
    let w = w.trim_matches(|c: char| !c.is_alphabetic());
    w.chars().next().is_some_and(char::is_uppercase) && w.chars().count() >= 2
}

/// "Smith and Lee", "Smith et al.", "Smith" → surnames.
fn surnames_in(s: &str) -> Option<Vec<String>> {
    let s = s.replace(" & ", " and ").replace("et al.", "");
    let mut out = Vec::new();
    for part in s.split(" and ").flat_map(|p| p.split(',')) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let words: Vec<&str> = part.split_whitespace().collect();
        let last = words.last()?;
        if !capitalised(last) {
            return None;
        }
        out.push(
            last.chars()
                .filter(|c| c.is_alphabetic() || *c == '-' || *c == '\'')
                .collect::<String>()
                .to_lowercase(),
        );
    }
    (!out.is_empty()).then_some(out)
}

fn locator_of(rest: &str) -> Option<String> {
    let t = rest
        .trim()
        .trim_start_matches([',', ':'])
        .trim()
        .trim_start_matches("pp.")
        .trim_start_matches("p.")
        .trim();
    let ok = !t.is_empty()
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '–' | '-' | ',' | ' ' | '.'))
        && t.chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c.is_ascii_alphabetic());
    ok.then(|| t.trim_end_matches('.').to_string())
}

/// One "Smith and Lee 2019a, p. 41" item of a parenthetical citation.
fn author_year_item(item: &str) -> Option<CiteItem> {
    let c = chars(item);
    let mut year_at = None;
    for i in 0..c.len() {
        if let Some((y, suffix, end)) = is_year_at(&c, i) {
            year_at = Some((i, y, suffix, end));
            break;
        }
    }
    let (i, year, suffix, end) = year_at?;
    let mut names: String = c[..i].iter().collect();
    for lead in [
        "see also", "see", "cf.", "e.g.,", "e.g.", "compare", "also", "but see",
    ] {
        let l = names.trim_start();
        if l.to_lowercase().starts_with(lead) {
            names = l[lead.len()..].to_string();
        }
    }
    let names = names.trim().trim_end_matches(',').trim();
    let surnames = surnames_in(names)?;
    let rest: String = c[end..].iter().collect();
    Some(CiteItem::AuthorYear {
        surnames,
        year,
        suffix,
        locator: locator_of(&rest),
    })
}

/// Citations in a stretch of running text, in order.
pub fn citations_in(text: &str) -> Vec<InText> {
    let c = chars(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '[' => {
                if let Some(rel) = c[i + 1..].iter().take(40).position(|ch| *ch == ']') {
                    let inner: String = c[i + 1..i + 1 + rel].iter().collect();
                    if let Some(numbers) = numbers_in(&inner) {
                        out.push(InText {
                            start: i,
                            end: i + rel + 2,
                            items: numbers.into_iter().map(CiteItem::Number).collect(),
                        });
                        i += rel + 2;
                        continue;
                    }
                }
            }
            '(' => {
                if let Some(rel) = c[i + 1..].iter().take(300).position(|ch| *ch == ')') {
                    let inner: String = c[i + 1..i + 1 + rel].iter().collect();
                    let cc = chars(inner.trim());
                    if is_year_at(&cc, 0).is_some() {
                        // Narrative: "Smith and Lee (2019, 41)" — the names stand outside the brackets.
                        if let Some(CiteItem::AuthorYear {
                            year,
                            suffix,
                            locator,
                            ..
                        }) = author_year_item(&format!("Zz {inner}"))
                        {
                            if let Some((names_start, surnames)) = narrative_names(&c[..i]) {
                                out.push(InText {
                                    start: names_start,
                                    end: i + rel + 2,
                                    items: vec![CiteItem::AuthorYear {
                                        surnames,
                                        year,
                                        suffix,
                                        locator,
                                    }],
                                });
                                i += rel + 2;
                                continue;
                            }
                        }
                    } else {
                        let parts: Vec<&str> = inner.split(';').collect();
                        let items: Vec<CiteItem> =
                            parts.iter().filter_map(|p| author_year_item(p)).collect();
                        if !items.is_empty() && items.len() == parts.len() {
                            out.push(InText {
                                start: i,
                                end: i + rel + 2,
                                items,
                            });
                            i += rel + 2;
                            continue;
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The capitalised names just before a "(2019)": "…as Smith and Lee" → (start, [smith, lee]).
fn narrative_names(before: &[char]) -> Option<(usize, Vec<String>)> {
    let s: String = before.iter().collect();
    let trimmed = s.trim_end();
    let end_chars = trimmed.chars().count();
    let words: Vec<(usize, &str)> = {
        let mut v = Vec::new();
        let mut pos = 0;
        for w in trimmed.split(' ') {
            v.push((pos, w));
            pos += w.chars().count() + 1;
        }
        v
    };
    let mut take = 0;
    let mut i = words.len();
    let mut after_name = false;
    while i > 0 && take < 6 {
        let w = words[i - 1].1;
        let bare = w.trim_matches(|c: char| !c.is_alphabetic());
        let connector = bare == "and" || bare == "&" || bare == "al" || w == "al." || bare == "et";
        if connector {
            take += 1;
            i -= 1;
            after_name = false;
        } else if (capitalised(w) && !w.ends_with('.') && !w.ends_with(','))
            || w.ends_with("et al.")
        {
            // Two capitalised words side by side are a sentence-opening word and a name ("Later Smith").
            if after_name {
                break;
            }
            take += 1;
            i -= 1;
            after_name = true;
        } else {
            break;
        }
    }
    if take == 0 {
        return None;
    }
    let names: Vec<&str> = words[i..].iter().map(|(_, w)| *w).collect();
    let surnames = surnames_in(&names.join(" "))?;
    let start = words[i].0;
    let _ = end_chars;
    Some((start, surnames))
}

/// Whether `item` cites `entry`.
pub fn cites(entry: &ParsedRef, item: &CiteItem) -> bool {
    match item {
        CiteItem::Number(n) => entry.label == Some(*n),
        CiteItem::AuthorYear {
            surnames,
            year,
            suffix,
            ..
        } => {
            if entry.year != Some(*year) || entry.families.is_empty() || surnames.is_empty() {
                return false;
            }
            if let (Some(a), Some(b)) = (suffix, &entry.suffix) {
                if a != b {
                    return false;
                }
            }
            if entry.families[0] != surnames[0] {
                return false;
            }
            surnames.iter().skip(1).all(|s| entry.families.contains(s))
        }
    }
}

// --------------------------------------------------------------------------- footnote-style citations

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// A full or short note that names its source: families, a title fragment and a page.
    Source {
        families: Vec<String>,
        title: String,
        page: Option<String>,
    },
    /// "Ibid." or "Ibid., 42".
    Ibid { page: Option<String> },
}

fn page_at_end(s: &str) -> Option<String> {
    let t = s.trim().trim_end_matches('.');
    let tail = t.rsplit([',', ' ']).next()?;
    (tail
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '–' | '-'))
        && !tail.is_empty()
        && tail.len() <= 8)
        .then(|| tail.to_string())
}

pub fn parse_note(text: &str) -> Option<Note> {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let (_, body) = strip_label(&joined);
    let body = body.trim();
    if body.to_lowercase().starts_with("ibid") {
        return Some(Note::Ibid {
            page: page_at_end(&body[4..]),
        });
    }
    // "Daniel J. Harrington, Interpreting the Old Testament (Wilmington: Glazier, 1981), 42." or the short
    // "Harrington, Interpreting the Old Testament, 57.": names, a comma, the title, then a page.
    let (names, rest) = body.split_once(", ")?;
    if names.split_whitespace().count() > 7 || !names.chars().next().is_some_and(char::is_uppercase)
    {
        return None;
    }
    let families = families_of(names);
    if families.is_empty() {
        return None;
    }
    let page = page_at_end(rest);
    let mut title = rest.split(" (").next().unwrap_or(rest).to_string();
    if page.is_some() && !rest.contains(" (") {
        if let Some((t, _)) = title.rsplit_once(',') {
            title = t.to_string();
        }
    }
    let title = title
        .trim()
        .trim_matches(['“', '”', '"', '*', '_', '.', ' '])
        .to_string();
    if title.is_empty() {
        return None;
    }
    Some(Note::Source {
        families,
        title,
        page,
    })
}

/// A resolved footnote: the source it cites (by its title and families) and the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNote {
    pub families: Vec<String>,
    pub title: String,
    pub page: Option<String>,
}

/// Resolves a run of footnotes in order: "Ibid." takes the previous note's source.
pub fn resolve_notes(notes: &[Note]) -> Vec<Option<ResolvedNote>> {
    let mut out: Vec<Option<ResolvedNote>> = Vec::new();
    let mut last: Option<ResolvedNote> = None;
    for n in notes {
        let resolved = match n {
            Note::Source {
                families,
                title,
                page,
            } => Some(ResolvedNote {
                families: families.clone(),
                title: title.clone(),
                page: page.clone(),
            }),
            Note::Ibid { page } => last.as_ref().map(|l| ResolvedNote {
                page: page.clone().or_else(|| l.page.clone()),
                ..l.clone()
            }),
        };
        if let Some(r) = &resolved {
            last = Some(r.clone());
        }
        out.push(resolved);
    }
    out
}

/// Whether a resolved note's source is this bibliography entry: same first family and a shared title word run.
pub fn note_cites(entry: &ParsedRef, note: &ResolvedNote) -> bool {
    if entry.families.is_empty()
        || note.families.is_empty()
        || entry.families[0] != note.families[0]
    {
        return false;
    }
    let norm = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .map(str::to_lowercase)
            .collect()
    };
    let (a, b) = (norm(&entry.title), norm(&note.title));
    if b.is_empty() {
        return a.is_empty();
    }
    let shared = b.iter().filter(|w| a.contains(w)).count();
    shared * 2 >= b.len()
}

/// A document the person owns, as far as matching a reference to it needs.
#[derive(Debug, Clone)]
pub struct Known {
    pub hash: String,
    pub families: Vec<String>,
    pub year: Option<i32>,
    pub title: String,
    pub doi: String,
}

fn words_of(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(str::to_lowercase)
        .collect()
}

/// Whether `entry` is the document `known`: the same DOI, or the same first author and year with a
/// title that mostly agrees.
pub fn is_same_work(entry: &ParsedRef, known: &Known) -> bool {
    if let Some(doi) = &entry.doi {
        if !known.doi.is_empty() && doi.eq_ignore_ascii_case(&known.doi) {
            return true;
        }
    }
    let (Some(a), Some(b)) = (entry.families.first(), known.families.first()) else {
        return false;
    };
    if a != b || entry.year != known.year || entry.year.is_none() {
        return false;
    }
    let (ew, kw) = (words_of(&entry.title), words_of(&known.title));
    if ew.is_empty() || kw.is_empty() {
        return false;
    }
    let shared = ew.iter().filter(|w| kw.contains(w)).count();
    shared * 10 >= ew.len().min(kw.len()) * 6
}

/// The sentence (or a window of text) around chars `start..end` of `text`, for showing a citation in context.
pub fn context_of(text: &str, start: usize, end: usize) -> String {
    let c: Vec<char> = text.chars().collect();
    let start = start.min(c.len());
    let end = end.min(c.len());
    let mut from = start;
    while from > 0 && start - from < 180 {
        if matches!(c[from - 1], '.' | '?' | '!')
            && c.get(from).is_some_and(|ch| ch.is_whitespace())
        {
            break;
        }
        from -= 1;
    }
    let mut to = end;
    while to < c.len() && to - end < 180 {
        if matches!(c[to], '.' | '?' | '!') && c.get(to + 1).map_or(true, |ch| ch.is_whitespace()) {
            to += 1;
            break;
        }
        to += 1;
    }
    c[from..to]
        .iter()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
