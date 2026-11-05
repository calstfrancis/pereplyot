//! Review cards on disk (`review/<hash>.json`, one file per document), kept apart from the notes so turning
//! review off leaves nothing behind in any sidecar.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fond_read_gtk::fsutil;
use pereplyot_core::fsrs::{Rating, Scheduler};
use pereplyot_core::review::{self, Card, SourceMark};

use crate::library::Library;
use crate::notes_index::{self, Filter};

fn dir() -> PathBuf {
    crate::paths::data_dir().join("review")
}

fn path_of(hash: &str) -> PathBuf {
    dir().join(format!("{hash}.json"))
}

/// Days since the epoch, with the time of day in the fraction.
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() / 86_400.0)
        .unwrap_or(0.0)
}

pub fn load(hash: &str) -> Vec<Card> {
    match fsutil::load_checked(&path_of(hash), |t| {
        serde_json::from_str::<Vec<Card>>(t).ok()
    }) {
        fsutil::Loaded::Ok(cards) => cards,
        fsutil::Loaded::Corrupt {
            recovered_from_backup,
            ..
        } => recovered_from_backup.unwrap_or_default(),
        fsutil::Loaded::Missing => Vec::new(),
    }
}

fn save(hash: &str, cards: &[Card]) {
    if cards.is_empty() {
        let _ = std::fs::remove_file(path_of(hash));
        let _ = std::fs::remove_file(path_of(hash).with_extension("json.bak"));
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(cards) {
        let _ = fsutil::write_atomic(&path_of(hash), json.as_bytes());
    }
}

/// Removes every review file: what turning review off does.
pub fn forget_all() {
    let _ = std::fs::remove_dir_all(dir());
}

/// Brings the cards up to date with what is tagged `#remember` now. Blocking: reads every sidecar.
pub fn sync(library: &Library) {
    let filter = Filter {
        tags: vec![review::TAG.to_string()],
        limit: Some(usize::MAX),
        ..Default::default()
    };
    let hits = notes_index::search_with(library, &filter);
    let mut by_doc: BTreeMap<String, (String, Vec<SourceMark>)> = BTreeMap::new();
    for h in hits {
        let entry = by_doc
            .entry(h.hash.clone())
            .or_insert_with(|| (h.doc_title.clone(), Vec::new()));
        entry.1.push(SourceMark {
            id: h.annotation_id.clone(),
            label: h
                .label
                .clone()
                .or_else(|| h.page.map(|p| p.to_string()))
                .unwrap_or_default(),
            snippet: h.snippet.clone().unwrap_or_default(),
            prefix: h.prefix.clone().unwrap_or_default(),
            suffix: h.suffix.clone().unwrap_or_default(),
            note: h.note.clone().unwrap_or_default(),
            tags: h.tags.clone(),
        });
    }
    let now = now();
    let mut have: Vec<String> = Vec::new();
    for (hash, (title, marks)) in &by_doc {
        let summary = crate::bib_store::load(hash)
            .map(|r| r.summary)
            .unwrap_or_default();
        let title = crate::bib_store::load(hash)
            .filter(|r| !r.title.is_empty())
            .map_or_else(|| title.clone(), |r| r.title);
        let fresh = review::generate(hash, &title, marks, &summary, now);
        let merged = review::merge(&load(hash), fresh);
        save(hash, &merged);
        have.push(hash.clone());
    }
    // Documents that no longer have anything marked to remember lose their cards.
    if let Ok(entries) = std::fs::read_dir(dir()) {
        for e in entries.flatten() {
            if let Some(stem) = e
                .path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
            {
                if e.path().extension().is_some_and(|x| x == "json") && !have.contains(&stem) {
                    save(&stem, &[]);
                }
            }
        }
    }
}

/// Every card, studied and new.
pub fn all() -> Vec<Card> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .flat_map(|h| load(&h))
        .collect()
}

pub const NEW_PER_SESSION: usize = 20;

pub fn due_now() -> Vec<Card> {
    let cards = all();
    review::due(&cards, now(), NEW_PER_SESSION)
        .into_iter()
        .cloned()
        .collect()
}

/// Records a rating and returns the card as it is now.
pub fn rate(card: &Card, rating: Rating) -> Card {
    let mut deck = load(&card.hash);
    let at = now();
    let mut updated = deck
        .iter()
        .find(|c| c.id == card.id)
        .cloned()
        .unwrap_or_else(|| card.clone());
    review::review(&mut updated, rating, at, &Scheduler::default());
    match deck.iter_mut().find(|c| c.id == card.id) {
        Some(slot) => *slot = updated.clone(),
        None => deck.push(updated.clone()),
    }
    save(&card.hash, &deck);
    updated
}
