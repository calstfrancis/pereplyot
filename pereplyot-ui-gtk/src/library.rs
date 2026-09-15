//! The Library: documents intentionally kept, separate from History's auto-populated
//! every-open list (`recents.rs`). Nothing lands here except via an explicit "Add to
//! Library" from a History row.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::recents::DocKind;

fn data_dir() -> PathBuf {
    glib::user_data_dir().join("pereplyot")
}

fn library_path() -> PathBuf {
    data_dir().join("library.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub hash: String,
    pub path: PathBuf,
    pub title: String,
    pub kind: DocKind,
    pub added_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library(Vec<LibraryEntry>);

impl Library {
    pub fn load() -> Library {
        fs::read_to_string(library_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = data_dir();
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(library_path(), json);
        }
    }

    pub fn entries(&self) -> &[LibraryEntry] {
        &self.0
    }

    pub fn contains(&self, hash: &str) -> bool {
        self.0.iter().any(|e| e.hash == hash)
    }

    /// No-op if `entry.hash` is already in the library — added-once semantics, not a
    /// most-recently-added reorder the way History's `touch` is.
    pub fn add(&mut self, entry: LibraryEntry) {
        if self.contains(&entry.hash) {
            return;
        }
        self.0.push(entry);
        self.save();
    }

    pub fn remove(&mut self, hash: &str) {
        self.0.retain(|e| e.hash != hash);
        self.save();
    }
}
