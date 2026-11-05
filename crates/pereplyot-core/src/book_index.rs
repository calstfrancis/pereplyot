//! Reading the index at the back of a book: finding its pages and splitting its lines into terms, locators
//! and cross-references, so each locator can be a link.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locator {
    /// A printed page or a range of them ("45", "45–47", "xii"); `note` is the footnote number, as in "203n4".
    Pages {
        first: String,
        last: Option<String>,
        note: Option<String>,
    },
    Passim,
}

impl Locator {
    pub fn text(&self) -> String {
        match self {
            Locator::Passim => "passim".to_string(),
            Locator::Pages { first, last, note } => {
                let mut s = first.clone();
                if let Some(l) = last {
                    s.push('–');
                    s.push_str(l);
                }
                if let Some(n) = note {
                    s.push('n');
                    s.push_str(n);
                }
                s
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub term: String,
    /// A sub-entry of the entry above it.
    pub sub: bool,
    pub locators: Vec<Locator>,
    /// "see X" / "see also X".
    pub see: Option<String>,
}

fn is_roman(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 8
        && s.chars()
            .all(|c| "ivxlcdm".contains(c.to_ascii_lowercase()))
}

fn is_page(s: &str) -> bool {
    let digits = s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() && s.len() <= 5;
    digits || is_roman(s)
}

/// One locator token: "45", "45–47", "203n4", "203nn4–5", "xii", "passim", "45f", "45ff".
pub fn parse_locator(tok: &str) -> Option<Locator> {
    let t = tok.trim().trim_end_matches('.');
    if t.eq_ignore_ascii_case("passim") {
        return Some(Locator::Passim);
    }
    let t = t.trim_end_matches("ff").trim_end_matches('f');
    let (pages, note) = match t.find('n') {
        Some(i) if i > 0 && t[..i].chars().all(|c| c.is_ascii_digit()) => {
            let n = t[i..].trim_start_matches('n');
            if n.is_empty() || !n.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return None;
            }
            (&t[..i], Some(n.to_string()))
        }
        _ => (t, None),
    };
    let parts: Vec<&str> = pages.split(['–', '-', '—']).collect();
    match parts.as_slice() {
        [a] if is_page(a) => Some(Locator::Pages {
            first: a.to_string(),
            last: None,
            note,
        }),
        [a, b] if is_page(a) && is_page(b) => Some(Locator::Pages {
            first: a.to_string(),
            last: Some(expand_range(a, b)),
            note,
        }),
        _ => None,
    }
}

/// "345–47" ends at 347.
fn expand_range(first: &str, last: &str) -> String {
    if first.chars().all(|c| c.is_ascii_digit())
        && last.chars().all(|c| c.is_ascii_digit())
        && last.len() < first.len()
    {
        format!("{}{}", &first[..first.len() - last.len()], last)
    } else {
        last.to_string()
    }
}

/// Splits "Kloppenborg, John S., 41, 45–47, 203n4" into the words and the trailing locators.
fn split_locators(line: &str) -> (String, Vec<Locator>) {
    let toks: Vec<&str> = line.split_whitespace().collect();
    let mut at = toks.len();
    let mut locators: Vec<Locator> = Vec::new();
    while at > 0 {
        let piece = toks[at - 1].trim_end_matches(',');
        // "45,47" with no space.
        let parts: Vec<&str> = piece.split(',').filter(|p| !p.is_empty()).collect();
        let parsed: Option<Vec<Locator>> = parts.iter().map(|p| parse_locator(p)).collect();
        match parsed {
            Some(ls) if !ls.is_empty() => {
                for l in ls.into_iter().rev() {
                    locators.push(l);
                }
                at -= 1;
            }
            _ => break,
        }
    }
    locators.reverse();
    let term = toks[..at]
        .join(" ")
        .trim_end_matches([',', ' '])
        .to_string();
    (term, locators)
}

fn only_locators(line: &str) -> bool {
    let (term, locs) = split_locators(line);
    term.is_empty() && !locs.is_empty()
}

/// Whether a page reads like index text: most of its lines end in page numbers.
pub fn looks_like_index(page: &str) -> bool {
    let lines: Vec<&str> = page
        .lines()
        .map(str::trim)
        .filter(|l| l.len() > 2)
        .collect();
    if lines.len() < 8 {
        return false;
    }
    let with = lines
        .iter()
        .filter(|l| {
            let (term, locs) = split_locators(l);
            !locs.is_empty() && !term.is_empty() || only_locators(l)
        })
        .count();
    with * 10 >= lines.len() * 6
}

/// The pages (0-based, inclusive) of the index at the end of the book, if there is one.
pub fn index_pages(pages: &[String]) -> Option<(usize, usize)> {
    let mut last = None;
    let mut first = None;
    for i in (pages.len() / 2..pages.len()).rev() {
        if looks_like_index(&pages[i]) {
            last.get_or_insert(i);
            first = Some(i);
        } else if last.is_some() {
            // One page of notes or a blank between index pages does not end it; two do.
            if i + 2 <= first.unwrap_or(i) && !looks_like_index(&pages[i + 1]) {
                break;
            }
            if pages[i].lines().filter(|l| l.trim().len() > 2).count() > 3 {
                break;
            }
        }
    }
    first.zip(last)
}

fn see_of(rest: &str) -> Option<(String, String)> {
    for marker in [
        ", see also ",
        ". See also ",
        ". See ",
        ", see ",
        " see also ",
        " see ",
    ] {
        if let Some(at) = rest.find(marker) {
            return Some((
                rest[..at].trim().to_string(),
                rest[at + marker.len()..]
                    .trim()
                    .trim_end_matches('.')
                    .to_string(),
            ));
        }
    }
    None
}

/// Parses the text of the index pages into entries, in order.
pub fn parse_index(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.len() < 2 || line.chars().all(|c| !c.is_alphanumeric()) {
            continue;
        }
        // A single capital letter heads a section ("A", "B").
        if line.chars().count() == 1 && line.chars().all(char::is_uppercase) {
            continue;
        }
        if only_locators(line) {
            if let Some(prev) = out.last_mut() {
                prev.locators.extend(split_locators(line).1);
            }
            continue;
        }
        let (term, locators) = split_locators(line);
        let (term, see) = match see_of(&term) {
            Some((t, s)) => (t, Some(s)),
            None => (term, None),
        };
        if term.is_empty() {
            continue;
        }
        let first = term.chars().next().unwrap_or(' ');
        let sub = first.is_lowercase() || matches!(first, '–' | '—' | '-' | '•');
        out.push(Entry {
            term: term
                .trim_start_matches(['–', '—', '-', '•', ' '])
                .to_string(),
            sub,
            locators,
            see,
        });
    }
    // A line with nothing to look up and no sub-entries under it is a heading ("Index"), not a term.
    let keep: Vec<bool> = out
        .iter()
        .enumerate()
        .map(|(i, e)| {
            !e.locators.is_empty() || e.see.is_some() || out.get(i + 1).is_some_and(|n| n.sub)
        })
        .collect();
    let mut keep = keep.into_iter();
    out.retain(|_| keep.next().unwrap_or(true));
    out
}

/// Where to find a locator's printed page among the document's page labels (0-based page index).
pub fn page_of(locator: &Locator, labels: &[Option<String>]) -> Option<usize> {
    let Locator::Pages { first, .. } = locator else {
        return None;
    };
    labels
        .iter()
        .position(|l| l.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(first)))
}

#[cfg(test)]
mod tests;
