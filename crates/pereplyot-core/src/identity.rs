use serde::{Deserialize, Serialize};

/// Fingerprints that recognise a document across renamed, re-saved or re-typeset copies.
/// Derived from the file, so it can always be rebuilt; the bibliographic record is separate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isbn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arxiv: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epub_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shingle: Vec<u64>,
}

/// What a file says about itself before any text is read.
#[derive(Debug, Clone, Default)]
pub struct RawFacts<'a> {
    pub title: Option<&'a str>,
    pub first_author: Option<&'a str>,
    pub year: Option<&'a str>,
    pub front_text: &'a str,
    pub pdf_id: Option<&'a str>,
    pub epub_id: Option<&'a str>,
}

impl Identity {
    pub fn build(hash: &str, facts: &RawFacts) -> Identity {
        let doi = find_doi(facts.front_text);
        let isbn = find_isbn(facts.front_text);
        let arxiv = find_arxiv(facts.front_text);
        let title_key = match facts.title {
            Some(t) => title_key(t, facts.first_author, facts.year),
            None => None,
        };
        Identity {
            hash: hash.to_string(),
            pdf_id: facts.pdf_id.map(str::to_string),
            doi,
            isbn,
            arxiv,
            epub_id: facts.epub_id.map(str::to_string),
            title_key,
            shingle: shingles(facts.front_text, 8, 64),
        }
    }

    /// How sure we are that `self` and `other` are two versions of one work.
    pub fn same_work(&self, other: &Identity) -> Option<Match> {
        if self.hash == other.hash {
            return Some(Match::Identical);
        }
        let eq = |a: &Option<String>, b: &Option<String>| a.is_some() && a == b;
        if eq(&self.doi, &other.doi) {
            return Some(Match::Doi);
        }
        if eq(&self.isbn, &other.isbn) {
            return Some(Match::Isbn);
        }
        if eq(&self.arxiv, &other.arxiv) {
            return Some(Match::Arxiv);
        }
        if eq(&self.pdf_id, &other.pdf_id) || eq(&self.epub_id, &other.epub_id) {
            return Some(Match::DocumentId);
        }
        if eq(&self.title_key, &other.title_key) {
            return Some(Match::TitleAuthorYear);
        }
        if jaccard(&self.shingle, &other.shingle) >= 0.6 {
            return Some(Match::Text);
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Identical,
    Doi,
    Isbn,
    Arxiv,
    DocumentId,
    TitleAuthorYear,
    Text,
}

/// Lower-cased title words (no stop words, no punctuation) + first author's surname + year.
pub fn title_key(title: &str, author: Option<&str>, year: Option<&str>) -> Option<String> {
    const STOP: [&str; 9] = ["the", "a", "an", "of", "and", "on", "in", "to", "for"];
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .filter(|w| !STOP.contains(&w.as_str()))
        .collect();
    if words.len() < 2 {
        return None;
    }
    let surname = author.map(surname_of).unwrap_or_default();
    let year = year.map(|y| y.trim().chars().take(4).collect::<String>());
    Some(format!(
        "{}|{}|{}",
        words.join(" "),
        surname,
        year.unwrap_or_default()
    ))
}

/// "Harrington, Daniel J." and "Daniel J. Harrington" both give "harrington".
pub fn surname_of(name: &str) -> String {
    let name = name.trim();
    let word = match name.split_once(',') {
        Some((family, _)) => family.trim(),
        None => name.split_whitespace().last().unwrap_or(""),
    };
    word.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_lowercase()
}

pub fn find_doi(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("10.") {
        let start = from + at;
        from = start + 3;
        if start > 0 && lower.as_bytes()[start - 1].is_ascii_digit() {
            continue;
        }
        let rest = &lower[start + 3..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits < 4 || rest.as_bytes().get(digits) != Some(&b'/') {
            continue;
        }
        let suffix_start = start + 3 + digits + 1;
        let end = lower[suffix_start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '<' | '>' | '{' | '}'))
            .map_or(lower.len(), |n| suffix_start + n);
        let doi = lower[start..end].trim_end_matches(['.', ',', ';', ')', ']', ':']);
        if doi.len() > suffix_start - start {
            return Some(doi.to_string());
        }
    }
    None
}

pub fn find_isbn(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("isbn") {
        let start = from + at + 4;
        from = start;
        let tail: String = lower[start..]
            .chars()
            .take(30)
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | ' ' | 'x'))
            .collect();
        let digits: String = tail.chars().filter(|c| *c != '-' && *c != ' ').collect();
        for len in [13usize, 10] {
            if digits.len() >= len {
                let candidate = &digits[..len];
                if isbn_valid(candidate) {
                    return Some(candidate.to_uppercase());
                }
            }
        }
    }
    None
}

fn isbn_valid(d: &str) -> bool {
    let vals: Vec<u32> = d
        .chars()
        .map(|c| {
            if c == 'x' {
                10
            } else {
                c.to_digit(10).unwrap_or(99)
            }
        })
        .collect();
    if vals.iter().any(|v| *v > 10) {
        return false;
    }
    match vals.len() {
        13 => {
            let sum: u32 = vals
                .iter()
                .enumerate()
                .map(|(i, v)| v * if i % 2 == 0 { 1 } else { 3 })
                .sum();
            sum % 10 == 0
        }
        10 => {
            let sum: u32 = vals
                .iter()
                .enumerate()
                .map(|(i, v)| v * (10 - i as u32))
                .sum();
            sum % 11 == 0
        }
        _ => false,
    }
}

pub fn find_arxiv(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let at = lower.find("arxiv:")?;
    let id: String = lower[at + 6..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | 'v'))
        .collect();
    let base = id.split('v').next().unwrap_or("").trim_end_matches('.');
    let (head, tail) = base.split_once('.')?;
    let ok = head.len() == 4
        && head.bytes().all(|c| c.is_ascii_digit())
        && (4..=5).contains(&tail.len())
        && tail.bytes().all(|c| c.is_ascii_digit());
    ok.then(|| base.to_string())
}

/// The first half of a PDF trailer's `/ID [<a><b>]` (or `[(a)(b)]`), which other programs' incremental saves keep.
pub fn pdf_trailer_id(bytes: &[u8]) -> Option<String> {
    let tail = &bytes[bytes.len().saturating_sub(64 * 1024)..];
    let at = tail.windows(3).rposition(|w| w == b"/ID")?;
    let rest = &tail[at + 3..];
    let open = rest.iter().position(|b| *b == b'[')?;
    let rest = &rest[open + 1..];
    let rest = &rest[rest.iter().position(|b| !b.is_ascii_whitespace())?..];
    let (open, close) = match rest.first()? {
        b'<' => (b'<', b'>'),
        b'(' => (b'(', b')'),
        _ => return None,
    };
    let end = rest[1..].iter().position(|b| *b == close)?;
    let inner = &rest[1..1 + end];
    if inner.is_empty() || (open == b'<' && !inner.iter().all(u8::is_ascii_hexdigit)) {
        return None;
    }
    if open == b'<' {
        return Some(String::from_utf8_lossy(inner).to_lowercase());
    }
    Some(inner.iter().map(|b| format!("{b:02x}")).collect())
}

/// Hashes of overlapping word windows, thinned to a sample that is stable under small edits.
pub fn shingles(text: &str, window: usize, keep: usize) -> Vec<u64> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect();
    if words.len() < window {
        return Vec::new();
    }
    let mut all: Vec<u64> = words.windows(window).map(|w| fnv(&w.join(" "))).collect();
    all.sort_unstable();
    all.dedup();
    all.truncate(keep);
    all
}

fn fnv(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn jaccard(a: &[u64], b: &[u64]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.iter().filter(|x| b.binary_search(x).is_ok()).count();
    shared as f64 / (a.len() + b.len() - shared) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doi_found_and_trimmed() {
        assert_eq!(
            find_doi("see https://doi.org/10.1017/S0022046900123456."),
            Some("10.1017/s0022046900123456".into())
        );
        assert_eq!(find_doi("page 10.5 of 12"), None);
        assert_eq!(find_doi("v10.1234/abc"), Some("10.1234/abc".into()));
    }

    #[test]
    fn isbn_checksums() {
        assert_eq!(
            find_isbn("ISBN 978-0-14-044913-6"),
            Some("9780140449136".into())
        );
        assert_eq!(find_isbn("ISBN: 0-306-40615-2"), Some("0306406152".into()));
        assert_eq!(find_isbn("ISBN 978-0-14-044913-7"), None);
        assert_eq!(find_isbn("no label 9780140449136"), None);
    }

    #[test]
    fn arxiv_ids() {
        assert_eq!(
            find_arxiv("arXiv:2101.01234v2 [cs.CL]"),
            Some("2101.01234".into())
        );
        assert_eq!(find_arxiv("arXiv:12.3"), None);
    }

    #[test]
    fn trailer_id_hex_and_literal() {
        let hex = b"junk trailer << /Size 9 /ID [<DEADbeef01> <DEADbeef02>] >> startxref";
        assert_eq!(pdf_trailer_id(hex).as_deref(), Some("deadbeef01"));
        let lit = b"<< /ID [(ab) (cd)] >>";
        assert_eq!(pdf_trailer_id(lit).as_deref(), Some("6162"));
        assert_eq!(pdf_trailer_id(b"no id here"), None);
    }

    #[test]
    fn surnames() {
        assert_eq!(surname_of("Harrington, Daniel J."), "harrington");
        assert_eq!(surname_of("Daniel J. Harrington"), "harrington");
    }

    #[test]
    fn title_key_ignores_case_and_stop_words() {
        let a = title_key(
            "The Interpretation of the New Testament",
            Some("Harrington, D."),
            Some("1979"),
        );
        let b = title_key(
            "interpretation of new testament",
            Some("Daniel Harrington"),
            Some("1979-01"),
        );
        assert_eq!(a, b);
        assert!(title_key("Title", None, None).is_none());
    }

    #[test]
    fn same_work_orders_evidence() {
        let text = "Q source studies of the synoptic gospels. ".repeat(40);
        let a = Identity::build(
            "h1",
            &RawFacts {
                front_text: &text,
                pdf_id: Some("abc"),
                ..Default::default()
            },
        );
        let b = Identity::build(
            "h2",
            &RawFacts {
                front_text: &text,
                pdf_id: Some("abc"),
                ..Default::default()
            },
        );
        assert_eq!(a.same_work(&b), Some(Match::DocumentId));
        let c = Identity::build(
            "h3",
            &RawFacts {
                front_text: &text,
                ..Default::default()
            },
        );
        assert_eq!(a.same_work(&c), Some(Match::Text));
        let other = Identity::build("h4", &RawFacts { front_text: "entirely different words about something else altogether in a different field of study", ..Default::default() });
        assert_eq!(a.same_work(&other), None);
    }
}
