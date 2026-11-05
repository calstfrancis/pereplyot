//! Finding where a mark's words are in another version of a document (a preprint and its published
//! form, a first and a second edition, a scan and its OCR'd copy). Matching is by the words, not by
//! where they sat, so a mark that moved is found and one that is gone is reported, never dropped.

use std::collections::HashMap;

/// What a stored mark remembers about its place.
#[derive(Debug, Clone)]
pub struct MarkText {
    pub id: String,
    /// 1-based page in the version the mark was made on.
    pub page: u32,
    pub snippet: String,
    pub prefix: String,
    pub suffix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// The same words on the same page.
    Exact,
    /// The same words, somewhere else.
    Moved,
    /// Most of the words, or an ambiguous place: needs a look.
    Uncertain,
    /// Not found in this version.
    Orphaned,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub id: String,
    pub fit: Fit,
    /// 1-based page in the new version, when anything was found.
    pub page: Option<u32>,
    /// The words as they read in the new version.
    pub found: Option<String>,
    /// Where they are in that page's text, as a char range.
    pub range: Option<(usize, usize)>,
    pub score: f32,
}

const EXACT: f32 = 0.92;
const LIKELY: f32 = 0.6;

fn tokens(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in s.split_whitespace() {
        let w: String = w
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if !w.is_empty() {
            out.push(w);
        }
    }
    out
}

/// Tokens of a page with the char range each covers in `text`; a word broken by a line-end hyphen
/// ("inter-" / "pretation") is one token spanning both halves.
fn page_tokens(text: &str) -> Vec<(String, usize, usize)> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if !c[i].is_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        let mut word = String::new();
        loop {
            while i < c.len() && c[i].is_alphanumeric() {
                word.extend(c[i].to_lowercase());
                i += 1;
            }
            let hyphen_break = i + 1 < c.len() && c[i] == '-' && matches!(c[i + 1], '\n' | '\r');
            if !hyphen_break {
                break;
            }
            let mut j = i + 1;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if j < c.len() && c[j].is_lowercase() {
                i = j;
            } else {
                break;
            }
        }
        out.push((word, start, i));
    }
    out
}

fn lcs_len(a: &[String], b: &[String]) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for x in a {
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y {
                prev[j] + 1
            } else {
                cur[j].max(prev[j + 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

struct Doc {
    /// Every token of every page, in order.
    tokens: Vec<String>,
    /// For each token, its 1-based page.
    page_of: Vec<u32>,
    /// For each token, its char range in its page's text.
    spans: Vec<(usize, usize)>,
    index: HashMap<String, Vec<usize>>,
    pages: u32,
}

impl Doc {
    fn new(pages: &[String]) -> Doc {
        let mut tokens = Vec::new();
        let mut page_of = Vec::new();
        let mut spans = Vec::new();
        for (i, text) in pages.iter().enumerate() {
            for (t, from, to) in page_tokens(text) {
                tokens.push(t);
                page_of.push(i as u32 + 1);
                spans.push((from, to));
            }
        }
        let mut index: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, t) in tokens.iter().enumerate() {
            index.entry(t.clone()).or_default().push(i);
        }
        Doc {
            tokens,
            page_of,
            spans,
            index,
            pages: pages.len() as u32,
        }
    }
}

struct Candidate {
    start: usize,
    end: usize,
    score: f32,
    context: f32,
}

fn context_score(doc: &Doc, start: usize, end: usize, prefix: &[String], suffix: &[String]) -> f32 {
    let before = &doc.tokens[start.saturating_sub(prefix.len().max(1) + 2)..start];
    let after = &doc.tokens[end..(end + suffix.len().max(1) + 2).min(doc.tokens.len())];
    let p = if prefix.is_empty() {
        0.0
    } else {
        lcs_len(prefix, before) as f32 / prefix.len() as f32
    };
    let s = if suffix.is_empty() {
        0.0
    } else {
        lcs_len(suffix, after) as f32 / suffix.len() as f32
    };
    (p + s) / 2.0
}

fn candidates(
    doc: &Doc,
    snippet: &[String],
    prefix: &[String],
    suffix: &[String],
) -> Vec<Candidate> {
    let m = snippet.len();
    if m == 0 {
        return Vec::new();
    }
    // Starting points: wherever the snippet's rarest words occur, backed off to where the snippet would begin.
    let mut rare: Vec<(usize, usize)> = snippet
        .iter()
        .enumerate()
        .filter_map(|(i, t)| doc.index.get(t).map(|v| (v.len(), i)))
        .collect();
    rare.sort_unstable();
    let mut starts: Vec<usize> = Vec::new();
    for (_, offset) in rare.into_iter().take(3) {
        for &pos in doc
            .index
            .get(&snippet[offset])
            .map(|v| &v[..v.len().min(400)])
            .unwrap_or(&[])
        {
            starts.push(pos.saturating_sub(offset));
        }
    }
    starts.sort_unstable();
    starts.dedup();
    let mut out = Vec::new();
    for start in starts {
        let end = (start + m + m / 5 + 2).min(doc.tokens.len());
        let window = &doc.tokens[start..end];
        let lcs = lcs_len(snippet, window);
        let score = lcs as f32 / m as f32;
        if score >= LIKELY * 0.8 {
            out.push(Candidate {
                start,
                end: (start + m).min(doc.tokens.len()),
                score,
                context: context_score(
                    doc,
                    start,
                    (start + m).min(doc.tokens.len()),
                    prefix,
                    suffix,
                ),
            });
        }
    }
    out
}

/// Finds each mark's words in `pages` (the new version's text, one string per page).
pub fn reanchor(marks: &[MarkText], pages: &[String], old_pages: u32) -> Vec<Outcome> {
    let doc = Doc::new(pages);
    marks
        .iter()
        .map(|mark| {
            let snippet = tokens(&mark.snippet);
            let none = Outcome {
                id: mark.id.clone(),
                fit: Fit::Orphaned,
                page: None,
                found: None,
                range: None,
                score: 0.0,
            };
            if snippet.len() < 2 {
                return none;
            }
            let (prefix, suffix) = (tokens(&mark.prefix), tokens(&mark.suffix));
            let cands = candidates(&doc, &snippet, &prefix, &suffix);
            // Where the old page would sit if the new version were the same length.
            let expected = if old_pages > 0 && doc.pages > 0 {
                mark.page as f32 / old_pages as f32 * doc.pages as f32
            } else {
                mark.page as f32
            };
            let best = cands
                .iter()
                .map(|c| {
                    let page = doc.page_of.get(c.start).copied().unwrap_or(1);
                    let nearness = 1.0
                        / (1.0 + (page as f32 - expected).abs() / doc.pages.max(1) as f32 * 10.0);
                    (c.score * 10.0 + c.context * 2.0 + nearness * 0.5, c)
                })
                .max_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, c)) = best else { return none };
            let page = doc.page_of.get(c.start).copied().unwrap_or(1);
            let ties = cands
                .iter()
                .filter(|o| {
                    (o.score - c.score).abs() < 0.001
                        && o.start != c.start
                        && doc.page_of[o.start] != page
                })
                .count();
            let fit = if c.score >= EXACT && ties == 0 {
                if page == mark.page {
                    Fit::Exact
                } else {
                    Fit::Moved
                }
            } else if c.score >= EXACT {
                // The same words occur in more than one place; only the context can say which.
                if c.context >= 0.5 {
                    Fit::Moved
                } else {
                    Fit::Uncertain
                }
            } else if c.score >= LIKELY {
                Fit::Uncertain
            } else {
                Fit::Orphaned
            };
            if fit == Fit::Orphaned {
                return Outcome {
                    score: c.score,
                    ..none
                };
            }
            // The words may run on to the next page; the range is the part on the page they begin on.
            let last = (c.start..c.end)
                .take_while(|&t| doc.page_of[t] == page)
                .last()
                .unwrap_or(c.start);
            Outcome {
                id: mark.id.clone(),
                fit,
                page: Some(page),
                found: Some(doc.tokens[c.start..c.end].join(" ")),
                range: Some((doc.spans[c.start].0, doc.spans[last].1)),
                score: c.score,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
