use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const MAX_RECENTS: usize = 20;

fn data_dir() -> PathBuf {
    glib::user_data_dir().join("pereplyot")
}

fn recents_path() -> PathBuf {
    data_dir().join("recents.json")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocKind {
    Pdf,
    Epub,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    pub hash: String,
    pub path: PathBuf,
    pub title: String,
    pub kind: DocKind,
    pub last_opened: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Recents(Vec<RecentEntry>);

impl Recents {
    pub fn load() -> Recents {
        fs::read_to_string(recents_path())
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
            let _ = fs::write(recents_path(), json);
        }
    }

    pub fn entries(&self) -> &[RecentEntry] {
        &self.0
    }

    /// Move (or insert) `entry` to the front — most-recently-opened first — and cap the
    /// list, dropping the oldest once it grows past [`MAX_RECENTS`].
    pub fn touch(&mut self, entry: RecentEntry) {
        self.0.retain(|e| e.hash != entry.hash);
        self.0.insert(0, entry);
        self.0.truncate(MAX_RECENTS);
        self.save();
    }
}
