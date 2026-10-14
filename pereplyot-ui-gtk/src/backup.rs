//! Backups of everything Pereplyot keeps (notes, bookmarks and reading places, notebooks, the
//! Library, earlier versions) into a folder of the user's choosing, as plain files: one folder
//! per backup, `Pereplyot backup 2026-10-14 15.30.12/`, the newest [`KEEP`] kept. A folder that
//! a sync client watches takes them off the computer.
//!
//! Restoring only adds: notes and documents missing here come back, and nothing already here is
//! changed except to gain annotations it lacks, kept as an earlier version first.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use fond_read_gtk::fsutil;
use fond_read_gtk::versions;
use serde::{Deserialize, Serialize};

pub const KEEP: usize = 14;
pub const EVERY_SECS: i64 = 24 * 60 * 60;
const PREFIX: &str = "Pereplyot backup ";
const MANIFEST: &str = "manifest.json";
const DIRS: [&str; 4] = ["annotations", "meta", "notebooks", "versions"];
const FILES: [&str; 4] = [
    "library.json",
    "paths.json",
    "connections.json",
    "recents.json",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    app: String,
    format: u32,
    /// Seconds since the Unix epoch.
    created: i64,
    version: String,
    files: usize,
    documents: usize,
}

pub fn data_dir() -> PathBuf {
    glib::user_data_dir().join("pereplyot")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub path: PathBuf,
    pub created: i64,
    pub documents: usize,
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    let m: Manifest = serde_json::from_str(&fs::read_to_string(dir.join(MANIFEST)).ok()?).ok()?;
    (m.app == "pereplyot").then_some(m)
}

/// The backups in `dest`, newest first.
pub fn list(dest: &Path) -> Vec<Backup> {
    let Ok(entries) = fs::read_dir(dest) else {
        return Vec::new();
    };
    let mut out: Vec<Backup> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(PREFIX))
        .filter_map(|e| {
            let m = read_manifest(&e.path())?;
            Some(Backup {
                path: e.path(),
                created: m.created,
                documents: m.documents,
            })
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.created));
    out
}

pub fn due(dest: &Path, now: i64) -> bool {
    list(dest)
        .first()
        .map_or(true, |b| now - b.created >= EVERY_SECS)
}

fn skipped(name: &str) -> bool {
    name.ends_with(".bak") || name.contains(".tmp-") || name.contains(".corrupt-")
}

fn copy_verified(from: &Path, to: &Path) -> std::io::Result<()> {
    let bytes = fs::read(from)?;
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = fs::File::create(to)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    if fs::read(to)? != bytes {
        return Err(std::io::Error::other(format!(
            "{} did not copy correctly",
            to.display()
        )));
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path, count: &mut usize) -> std::io::Result<()> {
    let Ok(entries) = fs::read_dir(from) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if skipped(&name.to_string_lossy()) {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &to.join(&name), count)?;
        } else if kind.is_file() {
            copy_verified(&entry.path(), &to.join(&name))?;
            *count += 1;
        }
    }
    Ok(())
}

fn name_for(now: i64) -> String {
    let local = chrono::DateTime::from_timestamp(now, 0)
        .unwrap_or_default()
        .with_timezone(&chrono::Local);
    format!("{PREFIX}{}", local.format("%Y-%m-%d %H.%M.%S"))
}

/// Copy everything from `data` into a new backup in `dest`, then thin `dest` to the newest
/// [`KEEP`]. Written under a hidden name and renamed when complete, so a half-written backup
/// never looks like a real one.
pub fn back_up(data: &Path, dest: &Path, now: i64) -> Result<Backup, String> {
    let fail = |e: std::io::Error| format!("Backup failed: {e}");
    fs::create_dir_all(dest).map_err(fail)?;
    let name = name_for(now);
    let staging = dest.join(format!(".{name}.partial"));
    let _ = fs::remove_dir_all(&staging);
    let result = (|| {
        let mut files = 0;
        for dir in DIRS {
            copy_tree(&data.join(dir), &staging.join(dir), &mut files)?;
        }
        for file in FILES {
            if data.join(file).is_file() {
                copy_verified(&data.join(file), &staging.join(file))?;
                files += 1;
            }
        }
        let documents = fs::read_dir(staging.join("annotations"))
            .map(|e| e.flatten().count())
            .unwrap_or(0);
        let manifest = Manifest {
            app: "pereplyot".into(),
            format: 1,
            created: now,
            version: env!("CARGO_PKG_VERSION").into(),
            files,
            documents,
        };
        let json = serde_json::to_string_pretty(&manifest).map_err(std::io::Error::other)?;
        fsutil::write_atomic(&staging.join(MANIFEST), json.as_bytes())?;
        let final_path = dest.join(&name);
        if final_path.exists() {
            fs::remove_dir_all(&final_path)?;
        }
        fs::rename(&staging, &final_path)?;
        if let Ok(d) = fs::File::open(dest) {
            let _ = d.sync_all();
        }
        Ok(Backup {
            path: final_path,
            created: now,
            documents,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    let backup = result.map_err(fail)?;
    for old in list(dest).into_iter().skip(KEEP) {
        let _ = fs::remove_dir_all(old.path);
    }
    Ok(backup)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Restored {
    /// Documents whose notes were missing here and came back whole.
    pub documents: usize,
    /// Annotations added to documents that already had notes here.
    pub annotations: usize,
    pub notebooks: usize,
    pub other_files: usize,
    /// Documents left alone because they are open in a reader.
    pub skipped_open: usize,
    pub unreadable: usize,
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file() && !skipped(&p.file_name().unwrap_or_default().to_string_lossy())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn add_if_missing(from: &Path, to: &Path) -> std::io::Result<bool> {
    if to.exists() {
        return Ok(false);
    }
    fsutil::write_atomic(to, &fs::read(from)?)?;
    Ok(true)
}

/// Bring back from `backup` whatever `data` lacks. Library entries are left to the caller, which
/// holds the live Library.
pub fn restore(
    backup: &Path,
    data: &Path,
    is_open: &dyn Fn(&str) -> bool,
    now: i64,
) -> Result<Restored, String> {
    if read_manifest(backup).is_none() {
        return Err("That folder is not a Pereplyot backup".into());
    }
    let io = |e: std::io::Error| format!("Restore failed: {e}");
    let mut r = Restored::default();

    for from in files_in(&backup.join("annotations")) {
        let Some(name) = from.file_name() else {
            continue;
        };
        let hash = from
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let to = data.join("annotations").join(name);
        let Some(theirs) = fs::read_to_string(&from)
            .ok()
            .and_then(|t| fond_annot::AnnotationSidecar::parse(&t, &from).ok())
        else {
            r.unreadable += 1;
            continue;
        };
        if is_open(&hash) {
            r.skipped_open += 1;
            continue;
        }
        let here = fs::read_to_string(&to).ok();
        let Some(here_text) = here else {
            add_if_missing(&from, &to).map_err(io)?;
            r.documents += 1;
            continue;
        };
        let Ok(mut ours) = fond_annot::AnnotationSidecar::parse(&here_text, &to) else {
            fsutil::set_aside(&to);
            add_if_missing(&from, &to).map_err(io)?;
            r.documents += 1;
            continue;
        };
        let known: HashSet<String> = ours.annotations.iter().map(|a| a.id.clone()).collect();
        let missing: Vec<_> = theirs
            .annotations
            .into_iter()
            .filter(|a| !known.contains(&a.id))
            .collect();
        if missing.is_empty() {
            continue;
        }
        versions::keep(&data.join("versions").join(&hash), &here_text, now).map_err(io)?;
        r.annotations += missing.len();
        ours.annotations.extend(missing);
        let json = ours.to_json().map_err(|e| e.to_string())?;
        fsutil::write_atomic(&to, json.as_bytes()).map_err(io)?;
    }

    for from in files_in(&backup.join("notebooks")) {
        let Some(name) = from.file_name() else {
            continue;
        };
        if add_if_missing(&from, &data.join("notebooks").join(name)).map_err(io)? {
            r.notebooks += 1;
        }
    }
    for from in files_in(&backup.join("meta")) {
        let Some(name) = from.file_name() else {
            continue;
        };
        if add_if_missing(&from, &data.join("meta").join(name)).map_err(io)? {
            r.other_files += 1;
        }
    }
    if let Ok(docs) = fs::read_dir(backup.join("versions")) {
        for doc in docs.flatten().filter(|d| d.path().is_dir()) {
            for from in files_in(&doc.path()) {
                let Some(name) = from.file_name() else {
                    continue;
                };
                let to = data.join("versions").join(doc.file_name()).join(name);
                if add_if_missing(&from, &to).map_err(io)? {
                    r.other_files += 1;
                }
            }
        }
    }
    for file in ["paths.json", "connections.json", "recents.json"] {
        if backup.join(file).is_file()
            && add_if_missing(&backup.join(file), &data.join(file)).map_err(io)?
        {
            r.other_files += 1;
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fond_annot::{Annotation, AnnotationKind, AnnotationSidecar};

    fn scratch(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("pereplyot-backup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn ann(id: &str) -> Annotation {
        let mut a = Annotation::drawn(
            AnnotationKind::Highlight,
            1,
            vec![],
            Some(id.into()),
            None,
            None,
        );
        a.id = id.into();
        a
    }

    fn write_notes(data: &Path, hash: &str, ids: &[&str]) {
        let mut s = AnnotationSidecar::new(hash);
        s.annotations = ids.iter().map(|i| ann(i)).collect();
        let p = data.join("annotations").join(format!("{hash}.json"));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s.to_json().unwrap()).unwrap();
    }

    fn ids(data: &Path, hash: &str) -> Vec<String> {
        let p = data.join("annotations").join(format!("{hash}.json"));
        AnnotationSidecar::parse(&fs::read_to_string(&p).unwrap(), &p)
            .unwrap()
            .annotations
            .into_iter()
            .map(|a| a.id)
            .collect()
    }

    #[test]
    fn a_backup_holds_the_notes_and_skips_scratch_files() {
        let root = scratch("make");
        let data = root.join("data");
        write_notes(&data, "d1", &["a"]);
        fs::write(data.join("annotations").join("d1.json.bak"), "old").unwrap();
        fs::create_dir_all(data.join("versions").join("d1")).unwrap();
        fs::write(
            data.join("versions")
                .join("d1")
                .join("2026-01-01T00-00-00Z.json"),
            "{}",
        )
        .unwrap();
        fs::write(data.join("library.json"), "{}").unwrap();
        let b = back_up(&data, &root.join("dest"), 1_000_000).unwrap();
        assert_eq!(b.documents, 1);
        assert!(b.path.join("annotations").join("d1.json").is_file());
        assert!(!b.path.join("annotations").join("d1.json.bak").exists());
        assert!(b
            .path
            .join("versions")
            .join("d1")
            .join("2026-01-01T00-00-00Z.json")
            .is_file());
        assert!(b.path.join("library.json").is_file());
        assert_eq!(list(&root.join("dest")), [b]);
        assert!(!fs::read_dir(root.join("dest")).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".partial")));
    }

    #[test]
    fn only_the_newest_are_kept_and_other_folders_are_untouched() {
        let root = scratch("thin");
        let data = root.join("data");
        write_notes(&data, "d1", &["a"]);
        let dest = root.join("dest");
        fs::create_dir_all(dest.join("My own folder")).unwrap();
        fs::create_dir_all(dest.join(format!("{PREFIX}not ours"))).unwrap();
        for i in 0..(KEEP as i64 + 3) {
            back_up(&data, &dest, 1_000_000 + i * EVERY_SECS).unwrap();
        }
        assert_eq!(list(&dest).len(), KEEP);
        assert!(dest.join("My own folder").is_dir());
        assert!(dest.join(format!("{PREFIX}not ours")).is_dir());
        assert!(!due(&dest, 1_000_000 + (KEEP as i64 + 2) * EVERY_SECS + 60));
        assert!(due(&dest, 1_000_000 + (KEEP as i64 + 3) * EVERY_SECS));
    }

    #[test]
    fn restoring_adds_what_is_missing_and_changes_nothing_else() {
        let root = scratch("restore");
        let data = root.join("data");
        write_notes(&data, "gone", &["g1", "g2"]);
        write_notes(&data, "both", &["shared", "deleted later"]);
        write_notes(&data, "open", &["o1"]);
        fs::create_dir_all(data.join("notebooks")).unwrap();
        fs::write(data.join("notebooks").join("n.typ"), "notebook").unwrap();
        let b = back_up(&data, &root.join("dest"), 1_000_000).unwrap();

        fs::remove_file(data.join("annotations").join("gone.json")).unwrap();
        fs::remove_file(data.join("notebooks").join("n.typ")).unwrap();
        write_notes(&data, "both", &["shared", "new here"]);
        write_notes(&data, "open", &[]);
        write_notes(&data, "only here", &["h"]);

        let r = restore(&b.path, &data, &|h| h == "open", 2_000_000).unwrap();
        assert_eq!(r.documents, 1);
        assert_eq!(r.annotations, 1);
        assert_eq!(r.notebooks, 1);
        assert_eq!(r.skipped_open, 1);
        assert_eq!(ids(&data, "gone"), ["g1", "g2"]);
        assert_eq!(ids(&data, "both"), ["shared", "new here", "deleted later"]);
        assert!(ids(&data, "open").is_empty());
        assert_eq!(ids(&data, "only here"), ["h"]);
        let kept = versions::list(&data.join("versions").join("both"));
        assert_eq!(kept.len(), 1);

        let again = restore(&b.path, &data, &|_| false, 3_000_000).unwrap();
        assert_eq!(again.documents + again.annotations + again.notebooks, 1);
        assert!(restore(&root, &data, &|_| false, 0).is_err());
    }
}
