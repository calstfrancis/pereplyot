//! Earlier versions of each document's notes (kept by `fond_read_gtk::versions`), for the
//! "Earlier versions" window.

use std::fs;
use std::path::Path;

use fond_read_gtk::fsutil;
use fond_read_gtk::versions::{self, Version};

use crate::reader_host::{annotations_path, versions_dir};

pub struct Document {
    pub hash: String,
    pub versions: Vec<Version>,
}

/// Every document with earlier versions kept, most recently changed first.
pub fn documents() -> Vec<Document> {
    let root = crate::backup::data_dir().join("versions");
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut docs: Vec<Document> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| Document {
            hash: e.file_name().to_string_lossy().into_owned(),
            versions: versions::list(&e.path()),
        })
        .filter(|d| !d.versions.is_empty())
        .collect();
    docs.sort_by_key(|d| std::cmp::Reverse(d.versions[0].time));
    docs
}

/// How many annotations a notes file holds, if it can be read.
pub fn count(path: &Path) -> Option<usize> {
    let text = fs::read_to_string(path).ok()?;
    Some(
        fond_annot::AnnotationSidecar::parse(&text, path)
            .ok()?
            .annotations
            .len(),
    )
}

pub fn current_count(hash: &str) -> usize {
    count(&annotations_path(hash)).unwrap_or(0)
}

/// Make `version` the document's notes again. What is there now is kept as a version first, so
/// a restore can itself be taken back.
pub fn restore(hash: &str, version: &Path) -> Result<(), String> {
    restore_in(
        &annotations_path(hash),
        &versions_dir(hash),
        version,
        versions::now(),
    )
}

fn restore_in(current: &Path, kept: &Path, version: &Path, now: i64) -> Result<(), String> {
    let text = fs::read_to_string(version).map_err(|e| e.to_string())?;
    fond_annot::AnnotationSidecar::parse(&text, version)
        .map_err(|_| "That version can't be read".to_string())?;
    if let Ok(here) = fs::read_to_string(current) {
        versions::keep(kept, &here, now).map_err(|e| e.to_string())?;
    }
    fsutil::write_atomic(current, text.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fond_annot::{Annotation, AnnotationKind, AnnotationSidecar};

    fn notes(n: usize) -> String {
        let mut s = AnnotationSidecar::new("k");
        for i in 0..n {
            s.annotations.push(Annotation::drawn(
                AnnotationKind::Highlight,
                1,
                vec![],
                Some(format!("q{i}")),
                None,
                None,
            ));
        }
        s.to_json().unwrap()
    }

    #[test]
    fn restoring_keeps_what_was_there_so_it_can_be_undone() {
        let d = std::env::temp_dir().join(format!("pereplyot-nv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let current = d.join("x.json");
        let kept = d.join("v");
        versions::keep(&kept, &notes(3), 1_000).unwrap();
        fs::write(&current, notes(1)).unwrap();
        let old = versions::list(&kept)[0].path.clone();
        restore_in(&current, &kept, &old, 2_000).unwrap();
        assert_eq!(count(&current), Some(3));
        let all = versions::list(&kept);
        assert_eq!(all.len(), 2);
        assert_eq!(count(&all[0].path), Some(1));
        fs::write(d.join("bad.json"), "{").unwrap();
        assert!(restore_in(&current, &kept, &d.join("bad.json"), 3_000).is_err());
        assert_eq!(count(&current), Some(3));
    }
}
