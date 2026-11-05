//! Keeping Pereplyot's data in a folder a sync client (Syncthing, Nextcloud, Dropbox) shares between
//! machines, and merging the conflict copies it leaves. No server and no account: the sync client does
//! the moving, this does the merging, and nothing is ever deleted: a copy it has merged or replaced
//! is moved to `conflicts-merged/` beside the data.

use std::fs;
use std::path::{Path, PathBuf};

use fond_annot::AnnotationSidecar;
use pereplyot_core::syncmerge::{conflict_original, merge_annotations, merge_bookmarks};

pub const KEPT: &str = "conflicts-merged";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    /// Conflict copies of notes whose marks were merged.
    pub merged: usize,
    /// Conflict copies of anything else, settled by keeping the newer file.
    pub settled: usize,
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for e in read.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 3 && p.file_name().is_some_and(|n| n != KEPT) {
                walk(&p, depth + 1, out);
            }
        } else {
            out.push(p);
        }
    }
}

fn modified(p: &Path) -> std::time::SystemTime {
    p.metadata()
        .and_then(|m| m.modified())
        .unwrap_or(std::time::UNIX_EPOCH)
}

fn set_aside(root: &Path, file: &Path) {
    let dir = root.join(KEPT);
    let _ = fs::create_dir_all(&dir);
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut target = dir.join(&name);
    let mut n = 1;
    while target.exists() {
        target = dir.join(format!("{n}-{name}"));
        n += 1;
    }
    if fs::rename(file, &target).is_err() && fs::copy(file, &target).is_ok() {
        let _ = fs::remove_file(file);
    }
}

fn merge_sidecar_files(original: &Path, conflict: &Path) -> Option<()> {
    let read = |p: &Path| -> Option<AnnotationSidecar> {
        AnnotationSidecar::parse(&fs::read_to_string(p).ok()?, p).ok()
    };
    let (mine, theirs) = (read(original)?, read(conflict)?);
    let mut merged = mine.clone();
    merged.annotations = merge_annotations(mine.annotations, theirs.annotations);
    let json = merged.to_json().ok()?;
    fond_read_gtk::fsutil::write_atomic(original, json.as_bytes()).ok()
}

/// Settles generic JSON: the newer file's fields, with the bookmarks of both.
fn merge_json_files(original: &Path, conflict: &Path) -> Option<()> {
    let load = |p: &Path| -> Option<serde_json::Value> {
        serde_json::from_str(&fs::read_to_string(p).ok()?).ok()
    };
    let (a, b) = (load(original)?, load(conflict)?);
    let (mut newer, other) = if modified(conflict) > modified(original) {
        (b, a)
    } else {
        (a, b)
    };
    let marks = |v: &serde_json::Value| -> Vec<u32> {
        v.get("bookmarks")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|n| n.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default()
    };
    if newer.get("bookmarks").is_some() || other.get("bookmarks").is_some() {
        let union = merge_bookmarks(marks(&newer), &marks(&other));
        newer["bookmarks"] = serde_json::json!(union);
    }
    fond_read_gtk::fsutil::write_atomic(
        original,
        serde_json::to_string_pretty(&newer).ok()?.as_bytes(),
    )
    .ok()
}

/// Merges every conflict copy under `root`. Blocking.
pub fn resolve_conflicts(root: &Path) -> Report {
    let mut files = Vec::new();
    walk(root, 0, &mut files);
    let mut report = Report::default();
    for conflict in files {
        let Some(name) = conflict
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            continue;
        };
        let Some(original_name) = conflict_original(&name) else {
            continue;
        };
        let original = conflict.with_file_name(original_name);
        if !original.exists() {
            // The other device made this one; it was never in conflict.
            let _ = fs::rename(&conflict, &original);
            continue;
        }
        let in_annotations = conflict
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "annotations");
        let is_json = conflict.extension().is_some_and(|e| e == "json");
        let done = if in_annotations && is_json {
            merge_sidecar_files(&original, &conflict).map(|()| report.merged += 1)
        } else if is_json {
            merge_json_files(&original, &conflict).map(|()| report.settled += 1)
        } else {
            // Typst notebooks and other text: keep the newer, set the other aside to read.
            if modified(&conflict) > modified(&original) {
                set_aside(root, &original);
                let _ = fs::rename(&conflict, &original);
                report.settled += 1;
                continue;
            }
            Some(()).inspect(|_| report.settled += 1)
        };
        if done.is_some() {
            set_aside(root, &conflict);
        }
    }
    report
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Local,
    Linked(PathBuf),
}

pub fn state() -> State {
    let data = crate::paths::data_dir();
    match fs::read_link(&data) {
        Ok(target) => State::Linked(target),
        Err(_) => State::Local,
    }
}

fn copy_into(src: &Path, dst: &Path, stamp: &str) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)?.flatten() {
        let from = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_into(&from, &to, stamp)?;
        } else if !to.exists() {
            fs::copy(&from, &to)?;
        } else if fs::read(&from)? != fs::read(&to)? {
            // Both sides have this file and they differ: leave this one as a conflict copy to merge.
            let (stem, ext) = match name.rsplit_once('.') {
                Some((s, e)) => (s.to_string(), format!(".{e}")),
                None => (name.clone(), String::new()),
            };
            fs::copy(
                &from,
                dst.join(format!("{stem}.sync-conflict-{stamp}-LOCAL{ext}")),
            )?;
        }
    }
    Ok(())
}

fn stamp() -> String {
    glib::DateTime::now_local()
        .ok()
        .and_then(|d| d.format("%Y%m%d-%H%M%S").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "00000000-000000".into())
}

/// Moves Pereplyot's data into `folder` (merging with whatever is there) and links to it from where the app
/// keeps its data. Nothing is deleted: the old directory stays beside it as `…-before-sync`. Restart afterwards.
pub fn use_folder(folder: &Path) -> Result<(), String> {
    let data = crate::paths::data_dir();
    if matches!(state(), State::Linked(_)) {
        return Err("The data is already in a sync folder".into());
    }
    if !folder.is_dir() {
        return Err("That isn't a folder".into());
    }
    let target = folder.join("Pereplyot");
    if target == data || target.starts_with(&data) {
        return Err("Choose a folder outside Pereplyot's own data".into());
    }
    if data.exists() {
        copy_into(&data, &target, &stamp()).map_err(|e| e.to_string())?;
    } else {
        fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    }
    let _ = resolve_conflicts(&target);
    if data.exists() {
        let aside = data.with_file_name("pereplyot-before-sync");
        if aside.exists() {
            return Err(format!(
                "{} is already there; move it away first",
                aside.display()
            ));
        }
        fs::rename(&data, &aside).map_err(|e| e.to_string())?;
    }
    if let Some(parent) = data.parent() {
        let _ = fs::create_dir_all(parent);
    }
    std::os::unix::fs::symlink(&target, &data).map_err(|e| e.to_string())
}

/// Takes the data back out of the sync folder: a copy goes where the app keeps its data and the link is removed.
/// The sync folder is left as it is.
pub fn stop() -> Result<(), String> {
    let data = crate::paths::data_dir();
    let State::Linked(target) = state() else {
        return Err("The data isn't in a sync folder".into());
    };
    let staging = data.with_file_name("pereplyot-leaving-sync");
    let _ = fs::remove_dir_all(&staging);
    copy_into(&target, &staging, "0").map_err(|e| e.to_string())?;
    fs::remove_file(&data).map_err(|e| e.to_string())?;
    fs::rename(&staging, &data).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fond_annot::{Annotation, AnnotationKind};

    fn mark(id: &str, note: &str) -> Annotation {
        Annotation {
            id: id.into(),
            kind: AnnotationKind::Highlight,
            page: Some(1),
            chapter: None,
            quadpoints: vec![],
            snippet: Some("some words".into()),
            snippet_prefix: None,
            snippet_suffix: None,
            color: None,
            note: Some(note.into()),
            created: Some("2026-01-01".into()),
            modified: None,
            extra: Default::default(),
        }
    }

    fn sidecar(key: &str, marks: Vec<Annotation>) -> String {
        let mut s = AnnotationSidecar::new(key);
        s.annotations = marks;
        s.to_json().unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pereplyot-sync-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_conflicting_sidecar_is_merged_and_the_copy_set_aside() {
        let root = scratch("merge");
        fs::create_dir_all(root.join("annotations")).unwrap();
        fs::write(
            root.join("annotations/h.json"),
            sidecar("h", vec![mark("a", "laptop")]),
        )
        .unwrap();
        fs::write(
            root.join("annotations/h.sync-conflict-20260101-101010-ABC.json"),
            sidecar("h", vec![mark("a", "laptop"), mark("b", "desktop")]),
        )
        .unwrap();
        let report = resolve_conflicts(&root);
        assert_eq!(report.merged, 1);
        let merged = AnnotationSidecar::parse(
            &fs::read_to_string(root.join("annotations/h.json")).unwrap(),
            Path::new("h"),
        )
        .unwrap();
        assert_eq!(merged.annotations.len(), 2);
        assert!(!root
            .join("annotations/h.sync-conflict-20260101-101010-ABC.json")
            .exists());
        assert_eq!(
            fs::read_dir(root.join(KEPT)).unwrap().count(),
            1,
            "the copy is kept, not deleted"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bookmarks_join_and_a_copy_for_a_file_that_is_not_there_is_adopted() {
        let root = scratch("meta");
        fs::create_dir_all(root.join("meta")).unwrap();
        fs::write(
            root.join("meta/h.json"),
            r#"{"bookmarks":[5,2],"reading_mode":true}"#,
        )
        .unwrap();
        fs::write(
            root.join("meta/h (conflicted copy 2026-02-02).json"),
            r#"{"bookmarks":[2,9]}"#,
        )
        .unwrap();
        fs::write(
            root.join("meta/new (conflicted copy 2026-02-02).json"),
            r#"{"bookmarks":[1]}"#,
        )
        .unwrap();
        let report = resolve_conflicts(&root);
        assert_eq!(report.settled, 1);
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("meta/h.json")).unwrap()).unwrap();
        assert_eq!(v["bookmarks"], serde_json::json!([2, 5, 9]));
        assert!(
            root.join("meta/new.json").exists(),
            "never in conflict, so just renamed"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn copying_into_a_folder_that_has_the_file_leaves_a_conflict_copy_to_merge() {
        let (src, dst) = (scratch("src"), scratch("dst"));
        fs::create_dir_all(src.join("annotations")).unwrap();
        fs::create_dir_all(dst.join("annotations")).unwrap();
        fs::write(
            src.join("annotations/h.json"),
            sidecar("h", vec![mark("a", "here")]),
        )
        .unwrap();
        fs::write(src.join("annotations/only-here.json"), "{}").unwrap();
        fs::write(
            dst.join("annotations/h.json"),
            sidecar("h", vec![mark("b", "there")]),
        )
        .unwrap();
        copy_into(&src, &dst, "20260101-101010").unwrap();
        assert!(dst.join("annotations/only-here.json").exists());
        assert!(dst
            .join("annotations/h.sync-conflict-20260101-101010-LOCAL.json")
            .exists());
        let report = resolve_conflicts(&dst);
        assert_eq!(report.merged, 1);
        let merged = AnnotationSidecar::parse(
            &fs::read_to_string(dst.join("annotations/h.json")).unwrap(),
            Path::new("h"),
        )
        .unwrap();
        assert_eq!(merged.annotations.len(), 2, "both devices' marks survive");
        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }
}
