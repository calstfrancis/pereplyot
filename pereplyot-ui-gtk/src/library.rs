//! The Library: documents intentionally kept, separate from History's shared,
//! cross-app-populated log (`fond_read_gtk::history`). Nothing lands here except via an
//! explicit "Add to Library" from a History row — and, unlike History, this is
//! Pereplyot-private, so it stays resolved through the normal sandbox-relative data dir.
//! Entries can be filed on named shelves.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use fond_read_gtk::fsutil::{self, Loaded};
use fond_read_gtk::history::DocKind;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shelf: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    shelves: Vec<String>,
    #[serde(default)]
    entries: Vec<LibraryEntry>,
}

/// The on-disk file was a bare array of entries before shelves existed.
#[derive(Deserialize)]
#[serde(untagged)]
enum OnDisk {
    Current(Library),
    Legacy(Vec<LibraryEntry>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Added,
    Title,
}

impl Library {
    pub fn load() -> Library {
        let parse = |t: &str| {
            serde_json::from_str::<OnDisk>(t).ok().map(|d| match d {
                OnDisk::Current(l) => l,
                OnDisk::Legacy(entries) => Library {
                    shelves: Vec::new(),
                    entries,
                },
            })
        };
        match fsutil::load_checked(&library_path(), parse) {
            Loaded::Ok(l) => l,
            Loaded::Missing => Library::default(),
            Loaded::Corrupt {
                recovered_from_backup,
                ..
            } => recovered_from_backup.unwrap_or_default(),
        }
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fsutil::write_atomic(&library_path(), json.as_bytes());
        }
    }

    pub fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    /// Entries on `shelf` (`None` = every entry), in the requested order.
    pub fn view(&self, shelf: Option<&str>, sort: Sort) -> Vec<LibraryEntry> {
        let mut v: Vec<LibraryEntry> = self
            .entries
            .iter()
            .filter(|e| shelf.map_or(true, |s| e.shelf.as_deref() == Some(s)))
            .cloned()
            .collect();
        match sort {
            Sort::Added => v.sort_by_key(|e| std::cmp::Reverse(e.added_at)),
            Sort::Title => v.sort_by_key(|e| e.title.to_lowercase()),
        }
        v
    }

    pub fn shelves(&self) -> &[String] {
        &self.shelves
    }

    /// The shelf a document is on, if it is in the Library and on one.
    pub fn shelf_of(&self, hash: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|e| e.hash == hash)
            .and_then(|e| e.shelf.clone())
    }

    pub fn count_on(&self, shelf: &str) -> usize {
        self.entries
            .iter()
            .filter(|e| e.shelf.as_deref() == Some(shelf))
            .count()
    }

    pub fn contains(&self, hash: &str) -> bool {
        self.entries.iter().any(|e| e.hash == hash)
    }

    /// No-op if `entry.hash` is already in the library — added-once semantics, not a
    /// most-recently-added reorder the way History's `touch` is.
    pub fn add(&mut self, entry: LibraryEntry) {
        if self.contains(&entry.hash) {
            return;
        }
        self.entries.push(entry);
        self.save();
    }

    pub fn remove(&mut self, hash: &str) {
        self.entries.retain(|e| e.hash != hash);
        self.save();
    }

    /// Adds a shelf; false if the name is blank or already taken (case-insensitively).
    pub fn add_shelf(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.shelf_exists(name) {
            return false;
        }
        self.shelves.push(name.to_string());
        self.save();
        true
    }

    pub fn rename_shelf(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        if new.is_empty() || (!old.eq_ignore_ascii_case(new) && self.shelf_exists(new)) {
            return false;
        }
        let Some(slot) = self.shelves.iter_mut().find(|s| *s == old) else {
            return false;
        };
        *slot = new.to_string();
        for e in self
            .entries
            .iter_mut()
            .filter(|e| e.shelf.as_deref() == Some(old))
        {
            e.shelf = Some(new.to_string());
        }
        self.save();
        true
    }

    /// Deleting a shelf keeps its documents; they just become unshelved.
    pub fn delete_shelf(&mut self, name: &str) {
        self.shelves.retain(|s| s != name);
        for e in self
            .entries
            .iter_mut()
            .filter(|e| e.shelf.as_deref() == Some(name))
        {
            e.shelf = None;
        }
        self.save();
    }

    pub fn set_shelf(&mut self, hash: &str, shelf: Option<&str>) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.hash == hash) {
            e.shelf = shelf.map(str::to_string);
            self.save();
        }
    }

    fn shelf_exists(&self, name: &str) -> bool {
        self.shelves.iter().any(|s| s.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hash: &str, title: &str) -> LibraryEntry {
        LibraryEntry {
            hash: hash.into(),
            path: PathBuf::from("/x.pdf"),
            title: title.into(),
            kind: DocKind::Pdf,
            added_at: chrono::Utc::now(),
            shelf: None,
        }
    }

    fn lib() -> Library {
        let mut l = Library::default();
        l.entries.push(entry("a", "Zebra"));
        l.entries.push(entry("b", "apple"));
        l
    }

    #[test]
    fn legacy_array_loads() {
        let json = serde_json::to_string(&vec![entry("a", "A")]).unwrap();
        let parsed: OnDisk = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, OnDisk::Legacy(v) if v.len() == 1));
    }

    #[test]
    fn shelves_filter_and_sort() {
        let mut l = lib();
        l.shelves.push("Course".into());
        l.entries[0].shelf = Some("Course".into());
        assert_eq!(l.view(Some("Course"), Sort::Title).len(), 1);
        let all = l.view(None, Sort::Title);
        assert_eq!(all[0].title, "apple");
    }

    #[test]
    fn rename_moves_entries_and_delete_keeps_them() {
        let mut l = lib();
        assert!(l.shelves.is_empty());
        l.shelves.push("Old".into());
        l.entries[0].shelf = Some("Old".into());
        assert!(l.rename_shelf("Old", "New"));
        assert_eq!(l.entries[0].shelf.as_deref(), Some("New"));
        l.delete_shelf("New");
        assert!(l.shelves.is_empty());
        assert_eq!(l.entries.len(), 2);
        assert!(l.entries[0].shelf.is_none());
    }

    #[test]
    fn duplicate_shelf_names_are_refused() {
        let mut l = lib();
        l.shelves.push("Course".into());
        assert!(!l.add_shelf("course"));
        assert!(!l.add_shelf("  "));
    }
}
