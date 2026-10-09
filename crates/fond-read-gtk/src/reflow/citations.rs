//! Finding what a citation under the pointer refers to, for PDFs whose citations are not links:
//! detect `[12]`, `(Smith 2019; Jones and Lee 2015a)`, `Smith (2019)` or a superscript number
//! near a point on a page, then look for the matching entry in the document's bibliography.

use super::layout::{lines_of, reading_order, Line};
use super::model::{RawPage, Word};

#[derive(Clone, Debug, PartialEq)]
pub enum Citation {
    Numeric(Vec<u32>),
    AuthorYear { surnames: Vec<String>, year: String },
}

/// Where a bibliography entry sits: its page and its box in page points as displayed.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryLocation {
    pub page: u16,
    pub bbox: [f32; 4],
    pub text: String,
}

struct Context {
    text: String,
    /// For each word: its char range in `text`.
    spans: Vec<(usize, usize)>,
    /// Index of the word under the pointer.
    hit: usize,
    /// The word under the pointer sits in smaller type than its line: a superscript.
    raised: bool,
}

fn contains(w: &Word, x: f32, y: f32) -> bool {
    x >= w.x0 - 1.5 && x <= w.x1 + 1.5 && y >= w.y0 - 1.5 && y <= w.y1 + 1.5
}

/// The line holding the word under `(x, y)` with its neighbours above and below joined, so a
/// group that wraps across lines is still whole.
fn context_at(page: &RawPage, x: f32, y: f32) -> Option<Context> {
    let lines = lines_of(&page.words);
    let at = lines
        .iter()
        .position(|l| l.words.iter().any(|w| contains(w, x, y)))?;
    let from = at.saturating_sub(1);
    let to = (at + 1).min(lines.len() - 1);
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut hit = None;
    let mut raised = false;
    for (li, line) in lines.iter().enumerate().take(to + 1).skip(from) {
        let near = (line.y1 - lines[at].y1).abs() < 3.0 * lines[at].size.max(1.0)
            && (li == at || line.x1 > lines[at].x0 - 2.0 * lines[at].size);
        if !near {
            continue;
        }
        let mut prev_x1: Option<f32> = None;
        for w in &line.words {
            if !text.is_empty() && prev_x1.map_or(true, |p| w.x0 - p >= 0.1 * line.size) {
                text.push(' ');
            }
            let start = text.chars().count();
            text.push_str(&w.text);
            spans.push((start, text.chars().count()));
            if li == at && contains(w, x, y) && hit.is_none() {
                hit = Some(spans.len() - 1);
                raised = w.size < 0.78 * line.size;
            }
            prev_x1 = Some(w.x1);
        }
        text.push(' ');
    }
    Some(Context {
        text,
        spans,
        hit: hit?,
        raised,
    })
}

fn chars_of(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn slice(chars: &[char], a: usize, b: usize) -> String {
    chars[a.min(chars.len())..b.min(chars.len())]
        .iter()
        .collect()
}

fn numbers_in(inner: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in inner.split([',', ';']) {
        let part = part.trim();
        let range: Vec<&str> = part.split(['-', '–', '—']).collect();
        match range.as_slice() {
            [a] => {
                if let Ok(n) = a.trim().parse() {
                    out.push(n);
                }
            }
            [a, b] => {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                    if a <= b && b - a < 30 {
                        out.extend(a..=b);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn is_year(s: &str) -> bool {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &s[digits.len()..];
    digits.len() == 4
        && (1000..=2100).contains(&digits.parse::<u32>().unwrap_or(0))
        && (rest.is_empty() || (rest.len() == 1 && rest.chars().all(|c| c.is_ascii_lowercase())))
}

const LEAD_WORDS: [&str; 8] = [
    "see",
    "also",
    "cf",
    "e.g",
    "i.e",
    "compare",
    "following",
    "after",
];

/// Surnames and a year from text like "see Smith and Jones 2015a" or "Smith et al., 2019".
fn author_year(item: &str) -> Option<Citation> {
    let tokens: Vec<&str> = item
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect();
    let year_at = tokens
        .iter()
        .position(|t| is_year(t.trim_matches(|c: char| ".;:)]".contains(c))))?;
    let year = tokens[year_at]
        .trim_matches(|c: char| ".;:)]".contains(c))
        .to_string();
    let mut surnames: Vec<String> = Vec::new();
    for t in &tokens[..year_at] {
        let word = t.trim_matches(|c: char| ".;:()[]&".contains(c));
        let lower = word.to_lowercase();
        if word.is_empty()
            || LEAD_WORDS.contains(&lower.as_str())
            || matches!(lower.as_str(), "and" | "et" | "al" | "p" | "pp" | "ibid")
        {
            continue;
        }
        if word.chars().next().is_some_and(|c| c.is_uppercase()) && word.len() > 1 {
            surnames.push(word.to_string());
        }
    }
    (!surnames.is_empty()).then_some(Citation::AuthorYear { surnames, year })
}

/// What the text under `(x, y)` on `page` cites, if it is a citation.
pub fn citation_at(page: &RawPage, x: f32, y: f32) -> Option<Citation> {
    let cx = context_at(page, x, y)?;
    let chars = chars_of(&cx.text);
    let (hs, he) = cx.spans[cx.hit];
    if cx.raised {
        let label: String = slice(&chars, hs, he);
        let n: u32 = label
            .trim_matches(|c: char| ",.;:)]".contains(c))
            .parse()
            .ok()?;
        return Some(Citation::Numeric(vec![n]));
    }
    // The nearest bracket pair around the pointer.
    let open = (0..=hs.min(chars.len().saturating_sub(1)))
        .rev()
        .find(|&i| matches!(chars[i], '(' | '['))
        .filter(|&i| hs - i < 160);
    if let Some(open) = open {
        let want = if chars[open] == '(' { ')' } else { ']' };
        let close = (he.saturating_sub(1)..chars.len()).find(|&i| chars[i] == want);
        if let Some(close) = close.filter(|&c| c > open && c - open < 240) {
            let inner = slice(&chars, open + 1, close);
            if chars[open] == '[' {
                let nums = numbers_in(&inner);
                if !nums.is_empty() {
                    // Several numbers: those in the token under the pointer, if that is part of the list.
                    let under = slice(&chars, hs, he);
                    let under = numbers_in(under.trim_matches(|c: char| "[]()".contains(c)));
                    return Some(Citation::Numeric(
                        if !under.is_empty() && under.iter().all(|n| nums.contains(n)) {
                            under
                        } else {
                            nums
                        },
                    ));
                }
            }
            // Author–year: the `;`-separated item the pointer is in.
            let mut offset = open + 1;
            for item in inner.split(';') {
                let len = item.chars().count();
                if hs < offset + len + 1 && he + 1 > offset {
                    if let Some(c) = author_year(item) {
                        return Some(c);
                    }
                }
                offset += len + 1;
            }
            if let Some(c) = author_year(&inner) {
                return Some(c);
            }
        }
    }
    narrative(&cx)
}

/// "Smith (2019)", "Smith and Jones (2019)", "Smith et al. (2019)" with the pointer on any part.
fn narrative(cx: &Context) -> Option<Citation> {
    let chars = chars_of(&cx.text);
    let words: Vec<String> = cx.spans.iter().map(|&(a, b)| slice(&chars, a, b)).collect();
    let year_idx = (cx.hit.saturating_sub(4)..(cx.hit + 4).min(words.len())).find(|&i| {
        words[i].starts_with('(') && is_year(words[i].trim_matches(|c: char| "();,.".contains(c)))
    })?;
    let year = words[year_idx]
        .trim_matches(|c: char| "();,.".contains(c))
        .to_string();
    let is_name = |w: &str| {
        let w = w.trim_matches(|c: char| ",.;:".contains(c));
        w.len() > 1 && w.chars().next().is_some_and(|c| c.is_uppercase())
    };
    // Work back from the year: "et al." or a surname, then any "and"/"," joined surnames before it.
    let mut surnames: Vec<String> = Vec::new();
    let mut i = year_idx;
    if i >= 2
        && words[i - 1]
            .trim_end_matches('.')
            .eq_ignore_ascii_case("al")
    {
        i -= 2; // "et al." sits between the name and the year
    }
    while i > 0 {
        let name = words[i - 1].as_str();
        if !is_name(name) {
            break;
        }
        surnames.insert(
            0,
            name.trim_matches(|c: char| ",.;:".contains(c)).to_string(),
        );
        i -= 1;
        let joiner = i
            .checked_sub(1)
            .map(|j| words[j].to_lowercase())
            .unwrap_or_default();
        if matches!(joiner.as_str(), "and" | "&" | "and,") {
            i -= 1;
        } else if !name.ends_with(',') && !words.get(i).is_some_and(|w| w.ends_with(',')) {
            break;
        }
    }
    // The pointer must be on the citation itself, not on a word that happens to be near it.
    let first = year_idx.saturating_sub(surnames.len() + 3);
    (!surnames.is_empty() && (first..=year_idx).contains(&cx.hit))
        .then_some(Citation::AuthorYear { surnames, year })
}

struct Draft {
    lines: Vec<Line>,
}

impl Draft {
    fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn bbox(&self) -> [f32; 4] {
        [
            self.lines
                .iter()
                .map(|l| l.x0)
                .fold(f32::INFINITY, f32::min),
            self.lines
                .iter()
                .map(|l| l.y0)
                .fold(f32::INFINITY, f32::min),
            self.lines
                .iter()
                .map(|l| l.x1)
                .fold(f32::NEG_INFINITY, f32::max),
            self.lines
                .iter()
                .map(|l| l.y1)
                .fold(f32::NEG_INFINITY, f32::max),
        ]
    }
}

/// The numeric label a line starts with: "[12]", "12.", "12)" or a bare "12" before more text.
fn leading_label(line: &Line) -> Option<u32> {
    let first = line.words.first()?;
    let t = first.text.trim_matches(|c: char| "[].)".contains(c));
    (t.len() <= 3 && !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) && line.words.len() > 1)
        .then(|| t.parse().ok())
        .flatten()
}

/// Split a page's reference list into entries: by number when they are numbered, by hanging
/// indent when the first line of each sticks out, otherwise by the space between them.
fn entries_of(page: &RawPage) -> Vec<Draft> {
    let mut out: Vec<Draft> = Vec::new();
    for leaf in reading_order(&page.words) {
        if leaf.is_empty() {
            continue;
        }
        let mut lefts: Vec<f32> = leaf.iter().map(|l| l.x0).collect();
        lefts.sort_by(f32::total_cmp);
        let left = lefts[lefts.len() / 10];
        let size = leaf[0].size.max(1.0);
        let numbered =
            leaf.iter().filter(|l| leading_label(l).is_some()).count() * 10 >= leaf.len() * 3;
        let hanging = leaf.iter().any(|l| l.x0 > left + 0.8 * size);
        let mut gaps: Vec<f32> = leaf.windows(2).map(|p| p[1].y1 - p[0].y1).collect();
        gaps.sort_by(f32::total_cmp);
        let leading = gaps.get(gaps.len() / 2).copied().unwrap_or(size * 1.2);
        for (i, line) in leaf.iter().enumerate() {
            let starts = if i == 0 {
                true
            } else if numbered {
                leading_label(line).is_some()
            } else if hanging {
                line.x0 <= left + 0.5 * size
            } else {
                line.y1 - leaf[i - 1].y1 > leading + 0.3 * size
            };
            match out.last_mut() {
                Some(d) if !starts => d.lines.push(line.clone()),
                _ => out.push(Draft {
                    lines: vec![line.clone()],
                }),
            }
        }
    }
    out
}

fn normal(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Where the bibliography begins among `pages` (consecutive, ending at the document's last).
fn bibliography_start(pages: &[RawPage]) -> usize {
    const HEADINGS: [&str; 7] = [
        "references",
        "bibliography",
        "workscited",
        "literaturecited",
        "selectedbibliography",
        "sources",
        "referencelist",
    ];
    for (i, page) in pages.iter().enumerate().rev() {
        if lines_of(&page.words)
            .iter()
            .any(|l| HEADINGS.contains(&normal(&l.text()).as_str()))
        {
            // The heading's own page may begin with a few lines of the previous section.
            return i;
        }
    }
    pages.len().saturating_sub((pages.len() / 4).max(2))
}

/// The bibliography entry that `citation` points to, searched for among `pages` — the last pages
/// of the document, in order.
pub fn find_entry(pages: &[RawPage], citation: &Citation) -> Option<EntryLocation> {
    let start = bibliography_start(pages);
    let mut best: Option<(i32, EntryLocation)> = None;
    for page in &pages[start..] {
        for draft in entries_of(page) {
            let text = draft.text();
            let score = match citation {
                Citation::Numeric(numbers) => {
                    let label = draft.lines.first().and_then(leading_label);
                    match (label, numbers.first()) {
                        (Some(l), Some(&n)) if l == n => 100,
                        _ => 0,
                    }
                }
                Citation::AuthorYear { surnames, year } => {
                    let head: String = normal(&text.chars().take(160).collect::<String>());
                    let first = normal(text.split([',', ' ']).next().unwrap_or(""));
                    let all_named = surnames.iter().all(|s| head.contains(&normal(s)));
                    let year_ok = text
                        .chars()
                        .take(260)
                        .collect::<String>()
                        .contains(year.as_str());
                    let bare_year: String = year.chars().take(4).collect();
                    let year_close = text
                        .chars()
                        .take(260)
                        .collect::<String>()
                        .contains(&bare_year);
                    let leads = surnames.first().is_some_and(|s| first == normal(s));
                    let mut score = 0;
                    if all_named {
                        score += 40;
                    }
                    if leads {
                        score += 30;
                    }
                    if year_ok {
                        score += 30;
                    } else if year_close {
                        score += 12;
                    }
                    if !(all_named && (year_ok || year_close)) {
                        score = score.min(35);
                    }
                    score
                }
            };
            if score >= 60 && best.as_ref().map_or(true, |(s, _)| score > *s) {
                best = Some((
                    score,
                    EntryLocation {
                        page: page.page,
                        bbox: draft.bbox(),
                        text,
                    },
                ));
            }
        }
    }
    best.map(|(_, loc)| loc)
}

#[cfg(test)]
mod tests;
