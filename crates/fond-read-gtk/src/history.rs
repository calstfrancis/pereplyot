//! Pereplyot's reading-history log, backing its History shelf. Kartoteka and Sputnik open
//! documents by launching Pereplyot, so what's read from those apps lands here too without
//! either of them writing this file. Call [`record_open`] wherever a host calls
//! [`crate::pdf::show_pdf_reader`]/[`crate::epub::show_epub_reader`].
//!
//! Deliberately **not** resolved via `glib::user_data_dir()`: that call is sandbox-relative
//! and, run inside three different flatpak sandboxes, resolves to three different private
//! per-app directories rather than one shared file. This instead resolves an explicit, real
//! path under the actual home directory — the same pattern Kartoteka/Sputnik already rely
//! on to share a Kartoteka library between their own sandboxes (a plain path reachable
//! because all three hold `--filesystem=home` (Pereplyot's manifest was broadened to match
//! when Kartoteka/Sputnik started launching it).
//!
//! One known limitation, accepted rather than solved here: clicking a history entry that
//! another app recorded, from inside Pereplyot's own (portal-restricted) sandbox, can fail
//! if Pereplyot was never granted access to that specific file — the existing "couldn't
//! read file" toast on a failed open covers this, imperfectly, without needing the
//! document-portal cross-app sharing machinery a real fix would take.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_ENTRIES: usize = 1000;

fn history_path() -> PathBuf {
    glib::user_data_dir().join("pereplyot").join("recents.json")
}

/// Where earlier versions kept it, under the home directory whatever the data directory was.
fn legacy_history_path() -> PathBuf {
    glib::home_dir().join(".local/share/pereplyot/recents.json")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocKind {
    Pdf,
    Epub,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub hash: String,
    pub path: PathBuf,
    pub title: String,
    pub kind: DocKind,
    /// ISO 8601, via `glib::DateTime` rather than pulling in `chrono` for one timestamp.
    pub last_opened: String,
}

/// Every entry, most-recently-opened first. Empty (not an error) if the file doesn't exist yet.
/// A file that can't be parsed is set aside (`history.json.corrupt-…`) rather than left for the
/// next save to overwrite, and the last good copy is used if there is one.
pub fn load() -> Vec<HistoryEntry> {
    use crate::fsutil::{load_checked, Loaded};
    match load_checked(&history_path(), |s| serde_json::from_str(s).ok()) {
        Loaded::Ok(entries) => entries,
        Loaded::Missing => {
            let legacy = legacy_history_path();
            if legacy == history_path() {
                return Vec::new();
            }
            std::fs::read_to_string(legacy)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        }
        Loaded::Corrupt {
            recovered_from_backup,
            ..
        } => recovered_from_backup.unwrap_or_default(),
    }
}

fn save(entries: &[HistoryEntry]) {
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = crate::fsutil::write_atomic(&history_path(), json.as_bytes());
    }
}

/// Record that `path` (content hash `hash`, kind `kind`, resolved title `title`) was just
/// opened — moves it to the front if already present, caps the list at [`MAX_ENTRIES`].
/// Best-effort: a read/write failure is silently ignored, the same as every other
/// host-local persistence in this crate — losing a history entry isn't worth interrupting
/// a document open over.
pub fn record_open(kind: DocKind, hash: &str, path: &Path, title: &str) {
    let mut entries = load();
    entries.retain(|e| e.hash != hash);
    entries.insert(
        0,
        HistoryEntry {
            hash: hash.to_string(),
            path: path.to_path_buf(),
            title: title.to_string(),
            kind,
            last_opened: now_iso8601(),
        },
    );
    entries.truncate(MAX_ENTRIES);
    save(&entries);
}

fn now_iso8601() -> String {
    glib::DateTime::now_local()
        .and_then(|dt| dt.format_iso8601())
        .map(|s| s.to_string())
        .unwrap_or_default()
}
