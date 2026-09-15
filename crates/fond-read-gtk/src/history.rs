//! A reading-history log shared across every app that embeds this reader — Kartoteka,
//! Sputnik, and Pereplyot's own launcher all record here, so Pereplyot's History shelf
//! shows what was opened anywhere, not just through Pereplyot itself. Call [`record_open`]
//! wherever a host calls [`crate::pdf::show_pdf_reader`]/[`crate::epub::show_epub_reader`].
//!
//! Deliberately **not** resolved via `glib::user_data_dir()`: that call is sandbox-relative
//! and, run inside three different flatpak sandboxes, resolves to three different private
//! per-app directories rather than one shared file. This instead resolves an explicit, real
//! path under the actual home directory — the same pattern Kartoteka/Sputnik already rely
//! on to share a Kartoteka library between their own sandboxes (a plain path reachable
//! because both hold `--filesystem=home`, not an XDG-portal-scoped location). Pereplyot
//! itself deliberately avoids `--filesystem=home` (root `CLAUDE.md`'s packaging notes) —
//! it grants a narrow `--filesystem=~/.local/share/pereplyot:create` specifically for this
//! file instead of the broader permission.
//!
//! One known limitation, accepted rather than solved here: clicking a history entry that
//! another app recorded, from inside Pereplyot's own (portal-restricted) sandbox, can fail
//! if Pereplyot was never granted access to that specific file — the existing "couldn't
//! read file" toast on a failed open covers this, imperfectly, without needing the
//! document-portal cross-app sharing machinery a real fix would take.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_ENTRIES: usize = 50;

fn history_path() -> PathBuf {
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

/// Every entry, most-recently-opened first. Empty (not an error) if the file doesn't
/// exist yet or can't be parsed.
pub fn load() -> Vec<HistoryEntry> {
    fs::read_to_string(history_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(entries: &[HistoryEntry]) {
    let path = history_path();
    if let Some(dir) = path.parent() {
        if fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = fs::write(path, json);
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
