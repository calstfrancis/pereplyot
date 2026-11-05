//! Review cards made from what you chose to remember, and how a review moves them along. Cards live
//! in their own files, never in the notes, so turning review off leaves no trace in any sidecar.

use serde::{Deserialize, Serialize};

use crate::fsrs::{Memory, Rating, Scheduler};

/// The tag that puts a mark into review.
pub const TAG: &str = "remember";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The marked words blanked out of their sentence.
    Cloze,
    /// A `Q: … A: …` written in a note.
    Question,
    /// The document's own few sentences.
    Summary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<String>,
    pub kind: Kind,
    pub title: String,
    /// The printed page, as cited.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub front: String,
    pub back: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stability: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<f64>,
    /// Day (days since the epoch, fractional) of the last review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<f64>,
    /// Day the card is next due.
    #[serde(default)]
    pub due: f64,
    #[serde(default)]
    pub reps: u32,
    #[serde(default)]
    pub lapses: u32,
}

impl Card {
    pub fn is_new(&self) -> bool {
        self.stability.is_none()
    }
}

/// A mark as card-making needs to know it.
#[derive(Debug, Clone)]
pub struct SourceMark {
    pub id: String,
    pub label: String,
    pub snippet: String,
    pub prefix: String,
    pub suffix: String,
    pub note: String,
    pub tags: Vec<String>,
}

fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

fn tail_words(s: &str, n: usize) -> String {
    let w = words(s);
    w[w.len().saturating_sub(n)..].join(" ")
}

fn head_words(s: &str, n: usize) -> String {
    words(s).into_iter().take(n).collect::<Vec<_>>().join(" ")
}

/// "Q: What is X? A: Y" in a note, if it has one.
pub fn question_in(note: &str) -> Option<(String, String)> {
    let at = note.find("Q:")?;
    let rest = &note[at + 2..];
    let split = rest.find("A:")?;
    let q = rest[..split].trim();
    let a = rest[split + 2..].trim();
    // A trailing "#remember" is the tag, not part of the answer.
    let a = a
        .split_whitespace()
        .filter(|w| !w.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    (!q.is_empty() && !a.is_empty()).then(|| (q.to_string(), a))
}

pub fn wants_review(tags: &[String]) -> bool {
    tags.iter()
        .any(|t| t.trim_start_matches('#').eq_ignore_ascii_case(TAG))
}

fn id_of(hash: &str, annotation: &str, kind: &str) -> String {
    format!("{hash}:{annotation}:{kind}")
}

/// The cards a document's marked passages (and its summary) call for. New cards are due now.
pub fn generate(
    hash: &str,
    title: &str,
    marks: &[SourceMark],
    summary: &str,
    now: f64,
) -> Vec<Card> {
    let mut cards = Vec::new();
    let mut any = false;
    for m in marks.iter().filter(|m| wants_review(&m.tags)) {
        any = true;
        let base = |kind, id: String, front: String, back: String| Card {
            id,
            hash: hash.into(),
            annotation: Some(m.id.clone()),
            kind,
            title: title.into(),
            label: m.label.clone(),
            front,
            back,
            stability: None,
            difficulty: None,
            last: None,
            due: now,
            reps: 0,
            lapses: 0,
        };
        if let Some((q, a)) = question_in(&m.note) {
            cards.push(base(Kind::Question, id_of(hash, &m.id, "question"), q, a));
        }
        let snippet = m.snippet.split_whitespace().collect::<Vec<_>>().join(" ");
        if snippet.split_whitespace().count() < 2 {
            continue;
        }
        let (before, after) = (tail_words(&m.prefix, 14), head_words(&m.suffix, 14));
        let front = if before.len() + after.len() >= 16 {
            format!(
                "{}{}[ … ]{}{}",
                ellipsis(&before, true),
                space(&before),
                space_before(&after),
                ellipsis(&after, false)
            )
        } else {
            let place = if m.label.is_empty() {
                String::new()
            } else {
                format!(", p. {}", m.label)
            };
            format!(
                "{title}{place}: which passage begins “{} …”?",
                head_words(&snippet, 3)
            )
        };
        cards.push(base(
            Kind::Cloze,
            id_of(hash, &m.id, "cloze"),
            front,
            snippet,
        ));
    }
    if any && !summary.trim().is_empty() {
        cards.push(Card {
            id: id_of(hash, "summary", "summary"),
            hash: hash.into(),
            annotation: None,
            kind: Kind::Summary,
            title: title.into(),
            label: String::new(),
            front: format!("What does {title} argue, and why does it matter?"),
            back: summary.trim().to_string(),
            stability: None,
            difficulty: None,
            last: None,
            due: now,
            reps: 0,
            lapses: 0,
        });
    }
    cards
}

fn ellipsis(s: &str, leading: bool) -> String {
    if s.is_empty() {
        String::new()
    } else if leading {
        format!("… {s}")
    } else {
        format!("{s} …")
    }
}

fn space(before: &str) -> &'static str {
    if before.is_empty() {
        ""
    } else {
        " "
    }
}

fn space_before(after: &str) -> &'static str {
    if after.is_empty() {
        ""
    } else {
        " "
    }
}

/// `fresh` (what the marks call for now) laid over `stored` (what was scheduled): a card keeps its
/// schedule but takes the current wording, and one the marks no longer call for is dropped.
pub fn merge(stored: &[Card], fresh: Vec<Card>) -> Vec<Card> {
    fresh
        .into_iter()
        .map(|mut c| {
            if let Some(old) = stored.iter().find(|s| s.id == c.id) {
                c.stability = old.stability;
                c.difficulty = old.difficulty;
                c.last = old.last;
                c.due = old.due;
                c.reps = old.reps;
                c.lapses = old.lapses;
            }
            c
        })
        .collect()
}

/// Applies a review made on day `now`.
pub fn review(card: &mut Card, rating: Rating, now: f64, sched: &Scheduler) {
    let memory = match (card.stability, card.difficulty, card.last) {
        (Some(stability), Some(difficulty), Some(last)) => sched.next(
            Memory {
                stability,
                difficulty,
            },
            (now - last).max(0.0),
            rating,
        ),
        _ => sched.first(rating),
    };
    card.stability = Some(memory.stability);
    card.difficulty = Some(memory.difficulty);
    card.last = Some(now);
    card.due = now + sched.interval(memory) as f64;
    card.reps += 1;
    if rating == Rating::Again {
        card.lapses += 1;
        // A forgotten card comes round again soon, in the same sitting if the session is still going.
        card.due = now + 10.0 / 1440.0;
    }
}

/// The cards due by `now`, those studied before first (most overdue first), then at most `new_limit` new ones.
pub fn due(cards: &[Card], now: f64, new_limit: usize) -> Vec<&Card> {
    let mut seen: Vec<&Card> = cards
        .iter()
        .filter(|c| !c.is_new() && c.due <= now)
        .collect();
    seen.sort_by(|a, b| a.due.total_cmp(&b.due));
    let fresh = cards
        .iter()
        .filter(|c| c.is_new() && c.due <= now)
        .take(new_limit);
    seen.into_iter().chain(fresh).collect()
}

#[cfg(test)]
mod tests;
