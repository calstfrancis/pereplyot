//! Remembers which content hashes each file path has had, most recent first, so that when a
//! file changes under the same path (re-saved, OCR'd, annotated elsewhere) its notes can be
//! offered for re-attachment instead of silently appearing to vanish.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fond_read_gtk::fsutil;

const KEEP_PER_PATH: usize = 8;

type Index = HashMap<String, Vec<String>>;

fn index_path() -> PathBuf {
    glib::user_data_dir().join("pereplyot").join("paths.json")
}

fn load() -> Index {
    match fsutil::load_checked(&index_path(), |t| serde_json::from_str::<Index>(t).ok()) {
        fsutil::Loaded::Ok(m) => m,
        _ => Index::new(),
    }
}

/// Earlier hashes of `path` other than `current`, most recent first.
pub fn earlier_hashes(path: &Path, current: &str) -> Vec<String> {
    load()
        .get(path.to_string_lossy().as_ref())
        .map(|v| v.iter().filter(|h| *h != current).cloned().collect())
        .unwrap_or_default()
}

pub fn record(path: &Path, hash: &str) {
    let mut map = load();
    let versions = map.entry(path.to_string_lossy().into_owned()).or_default();
    if versions.first().map(String::as_str) == Some(hash) {
        return;
    }
    versions.retain(|h| h != hash);
    versions.insert(0, hash.to_string());
    versions.truncate(KEEP_PER_PATH);
    if let Ok(json) = serde_json::to_string(&map) {
        let _ = fsutil::write_atomic(&index_path(), json.as_bytes());
    }
}
