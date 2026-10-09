//! A search over the text of every document you have read. Each document's text is kept, a page
//! (or EPUB chapter) at a time, in a cache file that can always be rebuilt; a search reads those
//! and ranks the pages. Nothing here needs a database, and the cache is safe to delete.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::history::DocKind;

const UNIT_SEP: char = '\u{c}';

/// What the index knows about one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocEntry {
    pub hash: String,
    pub title: String,
    pub path: PathBuf,
    pub kind: DocKind,
    /// The printed page (or chapter number) of each unit, in order.
    pub labels: Vec<String>,
    /// Whether `labels` are chapters rather than pages.
    pub chapters: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Catalogue {
    docs: Vec<DocEntry>,
}

pub fn dir() -> PathBuf {
    glib::user_cache_dir().join("pereplyot").join("index")
}

fn catalogue_path(dir: &Path) -> PathBuf {
    dir.join("catalogue.json")
}

fn text_path(dir: &Path, hash: &str) -> PathBuf {
    dir.join("text").join(format!("{hash}.txt"))
}

pub fn load_catalogue(dir: &Path) -> Vec<DocEntry> {
    let parse = |t: &str| serde_json::from_str::<Catalogue>(t).ok();
    match fsutil::load_checked(&catalogue_path(dir), parse) {
        fsutil::Loaded::Ok(c) => c.docs,
        _ => Vec::new(),
    }
}

fn save_catalogue(dir: &Path, docs: &[DocEntry]) -> std::io::Result<()> {
    let json = serde_json::to_string(&Catalogue {
        docs: docs.to_vec(),
    })
    .map_err(std::io::Error::other)?;
    fsutil::write_atomic(&catalogue_path(dir), json.as_bytes())
}

/// Collapse whitespace so a phrase matches across a line or page break.
fn tidy(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Record a document's text (one entry per page or chapter) and its place in the catalogue.
pub fn store(dir: &Path, entry: DocEntry, units: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("text"))?;
    let body = units
        .iter()
        .map(|u| tidy(u))
        .collect::<Vec<_>>()
        .join(&UNIT_SEP.to_string());
    fsutil::write_atomic(&text_path(dir, &entry.hash), body.as_bytes())?;
    let mut docs = load_catalogue(dir);
    docs.retain(|d| d.hash != entry.hash);
    docs.push(entry);
    save_catalogue(dir, &docs)
}

/// Forget documents that are no longer wanted.
pub fn retain(dir: &Path, keep: &dyn Fn(&DocEntry) -> bool) {
    let docs = load_catalogue(dir);
    let (kept, dropped): (Vec<_>, Vec<_>) = docs.into_iter().partition(|d| keep(d));
    for d in &dropped {
        let _ = std::fs::remove_file(text_path(dir, &d.hash));
    }
    if !dropped.is_empty() {
        let _ = save_catalogue(dir, &kept);
    }
}

/// A document's text in memory, ready to search.
pub struct Loaded {
    pub entry: DocEntry,
    units: Vec<String>,
    lower: Vec<String>,
}

fn fold(c: char) -> char {
    let l = c.to_lowercase().next().unwrap_or(c);
    match l {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '’' | '‘' => '\'',
        '“' | '”' => '"',
        l => l,
    }
}

fn fold_str(s: &str) -> String {
    s.chars().map(fold).collect()
}

impl Loaded {
    pub fn load(dir: &Path, entry: &DocEntry) -> Option<Loaded> {
        let text = std::fs::read_to_string(text_path(dir, &entry.hash)).ok()?;
        let units: Vec<String> = text.split(UNIT_SEP).map(str::to_string).collect();
        let lower = units.iter().map(|u| fold_str(u)).collect();
        Some(Loaded {
            entry: entry.clone(),
            units,
            lower,
        })
    }

    pub fn from_units(entry: DocEntry, units: Vec<String>) -> Loaded {
        let units: Vec<String> = units.iter().map(|u| tidy(u)).collect();
        let lower = units.iter().map(|u| fold_str(u)).collect();
        Loaded {
            entry,
            units,
            lower,
        }
    }
}

/// Load every catalogued document (skipping ones whose text file has gone).
pub fn load_all(dir: &Path) -> Vec<Loaded> {
    load_catalogue(dir)
        .iter()
        .filter_map(|e| Loaded::load(dir, e))
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// Words every hit contains; the last may be a prefix as typed.
    pub words: Vec<String>,
    /// Exact phrases, from "double quotes".
    pub phrases: Vec<String>,
    /// `#tag`s, for the annotation side of a search.
    pub tags: Vec<String>,
}

impl Query {
    pub fn parse(input: &str) -> Query {
        let mut q = Query::default();
        let mut rest = input.trim();
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix('"') {
                let (phrase, after) = r.split_once('"').unwrap_or((r, ""));
                let phrase = fold_str(&tidy(phrase));
                if !phrase.is_empty() {
                    q.phrases.push(phrase);
                }
                rest = after.trim_start();
                continue;
            }
            let (token, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if let Some(tag) = token.strip_prefix('#') {
                if !tag.is_empty() {
                    q.tags.push(tag.to_lowercase());
                }
            } else {
                let w: String = fold_str(token)
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '-')
                    .collect();
                if !w.is_empty() {
                    q.words.push(w);
                }
            }
            rest = after.trim_start();
        }
        q
    }

    pub fn is_empty_for_text(&self) -> bool {
        self.words.is_empty() && self.phrases.is_empty()
    }
}

/// How many times `word` starts a word in `hay`.
fn count_word_starts(hay: &str, word: &str) -> usize {
    let mut n = 0;
    let mut from = 0;
    while let Some(found) = hay[from..].find(word) {
        let at = from + found;
        let starts = hay[..at].chars().next_back().starts_a_word();
        if starts {
            n += 1;
        }
        from = at + word.len().max(1);
        while from < hay.len() && !hay.is_char_boundary(from) {
            from += 1;
        }
    }
    n
}

trait BreakBefore {
    fn starts_a_word(self) -> bool;
}

impl BreakBefore for Option<char> {
    fn starts_a_word(self) -> bool {
        self.map_or(true, |c| !c.is_alphanumeric())
    }
}

/// One page (or chapter) that matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub doc: usize,
    pub unit: usize,
    pub score: f64,
    /// The words around the first match, as a line to show.
    pub snippet: String,
    /// Where the matched terms are in `snippet`, as char ranges.
    pub marks: Vec<(usize, usize)>,
}

const SNIPPET_BEFORE: usize = 60;
const SNIPPET_LEN: usize = 200;

fn snippet_of(original: &str, lower: &str, needles: &[&str]) -> (String, Vec<(usize, usize)>) {
    let first = needles
        .iter()
        .filter_map(|n| lower.find(n).map(|b| (b, n.len())))
        .min_by_key(|(b, _)| *b);
    let start_char = first.map_or(0, |(b, _)| {
        lower[..b].chars().count().saturating_sub(SNIPPET_BEFORE)
    });
    let chars: Vec<char> = original.chars().collect();
    let end_char = (start_char + SNIPPET_LEN).min(chars.len());
    let mut text: String = chars[start_char..end_char].iter().collect();
    if start_char > 0 {
        text = format!("…{text}");
    }
    if end_char < chars.len() {
        text.push('…');
    }
    let shown_lower = fold_str(&text);
    let mut marks = Vec::new();
    for n in needles {
        let mut from = 0;
        while let Some(f) = shown_lower[from..].find(n) {
            let b = from + f;
            let s = shown_lower[..b].chars().count();
            marks.push((s, s + n.chars().count()));
            from = b + n.len().max(1);
        }
    }
    marks.sort_unstable();
    marks.dedup();
    (text, marks)
}

/// Pages containing every word (the last as a prefix) and every phrase, best first, at most
/// `limit`, within the documents `allow` accepts.
pub fn search(
    docs: &[Loaded],
    q: &Query,
    allow: &dyn Fn(&DocEntry) -> bool,
    limit: usize,
) -> Vec<Hit> {
    if q.is_empty_for_text() {
        return Vec::new();
    }
    let candidates: Vec<(usize, usize, Vec<usize>)> = docs
        .iter()
        .enumerate()
        .filter(|(_, d)| allow(&d.entry))
        .flat_map(|(di, d)| {
            d.lower.iter().enumerate().filter_map(move |(ui, hay)| {
                if q.phrases.iter().any(|p| !hay.contains(p.as_str())) {
                    return None;
                }
                let counts: Vec<usize> =
                    q.words.iter().map(|w| count_word_starts(hay, w)).collect();
                counts.iter().all(|&c| c > 0).then_some((di, ui, counts))
            })
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let total: usize = docs.iter().map(|d| d.units.len()).sum::<usize>().max(1);
    let idf: Vec<f64> = (0..q.words.len())
        .map(|i| {
            let df = candidates.iter().filter(|c| c.2[i] > 0).count() as f64;
            (1.0 + total as f64 / (1.0 + df)).ln()
        })
        .collect();
    let mut hits: Vec<Hit> = candidates
        .into_iter()
        .map(|(di, ui, counts)| {
            let words: f64 = counts
                .iter()
                .zip(&idf)
                .map(|(&c, i)| i * (c as f64 / (c as f64 + 1.2)))
                .sum();
            let phrase_bonus = 1.0 + q.phrases.len() as f64;
            let len_norm = 1.0 / (1.0 + (docs[di].lower[ui].len() as f64 / 4000.0));
            let needles: Vec<&str> = q
                .words
                .iter()
                .map(String::as_str)
                .chain(q.phrases.iter().map(String::as_str))
                .collect();
            let (snippet, marks) = snippet_of(&docs[di].units[ui], &docs[di].lower[ui], &needles);
            Hit {
                doc: di,
                unit: ui,
                score: (words + phrase_bonus) * (0.6 + 0.4 * len_norm),
                snippet,
                marks,
            }
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.doc.cmp(&b.doc))
            .then(a.unit.cmp(&b.unit))
    });
    hits.truncate(limit);
    hits
}

// ------------------------------------------------------------------------ extraction

/// A document's pages (PDF) or chapters (EPUB) as text, with the label of each. Blocking.
pub fn extract(path: &Path, kind: DocKind) -> Option<(Vec<String>, Vec<String>, bool)> {
    match kind {
        DocKind::Pdf => {
            let pdfium = crate::pdfium::get().ok()?;
            let bytes = std::fs::read(path).ok()?;
            let text = fond_doc::extract_text(pdfium, &bytes).ok()?;
            let labels = fond_doc::page_labels(pdfium, &bytes).unwrap_or_default();
            let labels = (0..text.pages.len())
                .map(|i| {
                    labels
                        .get(i)
                        .and_then(|l| l.clone())
                        .unwrap_or_else(|| (i + 1).to_string())
                })
                .collect();
            Some((text.pages, labels, false))
        }
        DocKind::Epub => {
            let book = fond_doc::epub::open_book(path).ok()?;
            let tmp = std::env::temp_dir().join(format!(
                "pereplyot-index-{}-{}",
                std::process::id(),
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
            let _ = std::fs::remove_dir_all(&tmp);
            fond_doc::epub::extract_all(path, &tmp).ok()?;
            let mut units = Vec::new();
            let mut labels = Vec::new();
            for (i, chapter) in book.spine.iter().enumerate() {
                if let Ok(xml) = std::fs::read_to_string(tmp.join(chapter)) {
                    let text = fond_doc::epub::strip_tags(&xml);
                    if !text.trim().is_empty() {
                        units.push(text);
                        labels.push((i + 1).to_string());
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&tmp);
            Some((units, labels, true))
        }
    }
}

/// Which documents need (re)indexing: those not catalogued, or catalogued from another path.
pub fn missing<'a>(
    dir: &Path,
    wanted: &'a [(String, PathBuf, DocKind, String)],
) -> Vec<&'a (String, PathBuf, DocKind, String)> {
    let have: HashMap<String, PathBuf> = load_catalogue(dir)
        .into_iter()
        .map(|d| (d.hash, d.path))
        .collect();
    wanted
        .iter()
        .filter(|(hash, path, _, _)| {
            have.get(hash).differs_or_missing(path) || !text_path(dir, hash).exists()
        })
        .collect()
}

trait DiffersFrom {
    fn differs_or_missing(self, path: &Path) -> bool;
}

impl DiffersFrom for Option<&PathBuf> {
    fn differs_or_missing(self, path: &Path) -> bool {
        self.map_or(true, |p| p != path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(hash: &str, title: &str, pages: &[&str]) -> Loaded {
        Loaded::from_units(
            DocEntry {
                hash: hash.into(),
                title: title.into(),
                path: PathBuf::from(format!("/{hash}.pdf")),
                kind: DocKind::Pdf,
                labels: (1..=pages.len()).map(|i| i.to_string()).collect(),
                chapters: false,
            },
            pages.iter().map(|s| s.to_string()).collect(),
        )
    }

    fn corpus() -> Vec<Loaded> {
        vec![
            doc(
                "a",
                "Liturgy and Life",
                &[
                    "The covenant community gathered for liturgy every week.",
                    "Scripture was read aloud. The covenant was renewed, and the Café sang.",
                    "Nothing about the topic at all.",
                ],
            ),
            doc(
                "b",
                "Narrative Theology",
                &[
                    "A narrative of grace. The covenant appears once here.",
                    "Liturgy again, liturgy twice, liturgy thrice.",
                ],
            ),
        ]
    }

    fn pages(hits: &[Hit]) -> Vec<(usize, usize)> {
        hits.iter().map(|h| (h.doc, h.unit)).collect()
    }

    #[test]
    fn every_word_must_be_on_the_page_and_the_last_may_be_a_prefix() {
        let docs = corpus();
        let hits = search(&docs, &Query::parse("covenant lit"), &|_| true, 10);
        assert_eq!(pages(&hits), vec![(0, 0)]);
        let hits = search(&docs, &Query::parse("covenant"), &|_| true, 10);
        assert_eq!(hits.len(), 3);
        assert!(search(&docs, &Query::parse("covenant astronomy"), &|_| true, 10).is_empty());
    }

    #[test]
    fn a_phrase_in_quotes_must_appear_as_written_even_across_a_line_break() {
        let docs = vec![doc(
            "c",
            "T",
            &["the covenant\ncommunity gathered", "community covenant"],
        )];
        let hits = search(
            &docs,
            &Query::parse("\"covenant community\""),
            &|_| true,
            10,
        );
        assert_eq!(pages(&hits), vec![(0, 0)]);
    }

    #[test]
    fn accents_and_case_do_not_matter() {
        let docs = corpus();
        let hits = search(&docs, &Query::parse("CAFE"), &|_| true, 10);
        assert_eq!(pages(&hits), vec![(0, 1)]);
        assert!(hits[0].snippet.contains("Café"));
    }

    #[test]
    fn a_page_that_says_it_more_ranks_higher_and_documents_can_be_excluded() {
        let docs = corpus();
        let hits = search(&docs, &Query::parse("liturgy"), &|_| true, 10);
        assert_eq!(pages(&hits)[0], (1, 1));
        let only_a = search(&docs, &Query::parse("liturgy"), &|e| e.hash == "a", 10);
        assert_eq!(pages(&only_a), vec![(0, 0)]);
    }

    #[test]
    fn the_snippet_marks_the_matched_words() {
        let docs = corpus();
        let hit = &search(&docs, &Query::parse("scripture"), &|_| true, 10)[0];
        let (s, e) = hit.marks[0];
        let shown: String = hit.snippet.chars().skip(s).take(e - s).collect();
        assert_eq!(shown, "Scripture");
    }

    #[test]
    fn queries_parse_words_phrases_and_tags() {
        let q = Query::parse("grace \"free gift\" #method Naïve");
        assert_eq!(q.words, vec!["grace", "naive"]);
        assert_eq!(q.phrases, vec!["free gift"]);
        assert_eq!(q.tags, vec!["method"]);
        assert!(Query::parse("#only").is_empty_for_text());
    }

    #[test]
    fn the_cache_round_trips_and_forgets_what_is_no_longer_wanted() {
        let dir = std::env::temp_dir().join(format!("pereplyot-index-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = doc("h1", "One", &["alpha beta", "gamma"]);
        store(
            &dir,
            d.entry.clone(),
            &["alpha beta".into(), "gamma".into()],
        )
        .unwrap();
        let d2 = doc("h2", "Two", &["delta"]);
        store(&dir, d2.entry.clone(), &["delta".into()]).unwrap();
        let loaded = load_all(&dir);
        assert_eq!(loaded.len(), 2);
        assert_eq!(
            search(&loaded, &Query::parse("gamma"), &|_| true, 5).len(),
            1
        );
        let wanted = vec![
            (
                "h1".to_string(),
                PathBuf::from("/h1.pdf"),
                DocKind::Pdf,
                "One".to_string(),
            ),
            (
                "h3".to_string(),
                PathBuf::from("/h3.pdf"),
                DocKind::Pdf,
                "Three".to_string(),
            ),
        ];
        assert_eq!(missing(&dir, &wanted).len(), 1);
        retain(&dir, &|e| e.hash == "h1");
        assert_eq!(load_all(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
