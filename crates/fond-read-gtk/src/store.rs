use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fond_annot::{Annotation, AnnotationSidecar};

use crate::fsutil::{self, Loaded};
use crate::versions;

/// Remembers the file as this process last read or wrote it, so a save can tell what someone
/// else (another window, the vault's owner app, a sync) changed in between and keep it, instead
/// of overwriting it. Optionally keeps earlier versions of the file as it goes.
#[derive(Default)]
pub struct SidecarSync {
    base: RefCell<Option<Base>>,
    versions: Option<PathBuf>,
    kept_this_session: Cell<bool>,
}

struct Base {
    text: String,
    annotations: Vec<Annotation>,
}

pub struct LoadReport {
    pub sidecar: AnnotationSidecar,
    pub warning: Option<String>,
}

pub struct Saved {
    /// What was written, when it differs from what was handed in because changes made
    /// elsewhere were kept; the caller should take it as its own copy.
    pub merged: Option<AnnotationSidecar>,
    /// How many annotations were added, changed or removed elsewhere.
    pub outside: usize,
}

pub fn parse_sidecar(text: &str, path: &Path) -> Option<AnnotationSidecar> {
    AnnotationSidecar::parse(text, path).ok()
}

impl SidecarSync {
    /// Keeps earlier versions of the file in `dir` (see [`crate::versions`]).
    pub fn with_versions(dir: PathBuf) -> SidecarSync {
        SidecarSync {
            versions: Some(dir),
            ..SidecarSync::default()
        }
    }

    fn remember(&self, text: Option<String>, path: &Path) {
        *self.base.borrow_mut() = text.and_then(|text| {
            let annotations = parse_sidecar(&text, path)?.annotations;
            Some(Base { text, annotations })
        });
    }

    /// Load `path`; an unreadable file is set aside (never silently replaced), falling back to
    /// the previous-version backup, and the caller gets a warning to show.
    pub fn load(&self, path: &Path, fresh: impl Fn() -> AnnotationSidecar) -> LoadReport {
        let parse = |t: &str| parse_sidecar(t, path);
        let (sidecar, warning) = match fsutil::load_checked(path, parse) {
            Loaded::Ok(s) => (s, None),
            Loaded::Missing => (fresh(), None),
            Loaded::Corrupt {
                quarantined,
                recovered_from_backup,
            } => {
                let name = quarantined
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match recovered_from_backup {
                    Some(s) => (
                        s,
                        Some(format!(
                            "Annotations file was damaged; restored the previous version (damaged copy kept as {name})"
                        )),
                    ),
                    None => (
                        fresh(),
                        Some(format!(
                            "Annotations file was damaged and has been set aside as {name}"
                        )),
                    ),
                }
            }
        };
        self.remember(std::fs::read_to_string(path).ok(), path);
        LoadReport { sidecar, warning }
    }

    pub fn save(&self, path: &Path, sidecar: &AnnotationSidecar) -> Result<Saved, String> {
        let on_disk = match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            // Writing blind could overwrite whatever is there; better to fail this one save.
            Err(e) => return Err(format!("Could not read annotations before saving: {e}")),
        };
        let mut to_write = sidecar.clone();
        let mut outside = 0;
        if let Some(text) = &on_disk {
            let base = self.base.borrow();
            if base.as_ref().map(|b| b.text.as_str()) != Some(text.as_str()) {
                match parse_sidecar(text, path) {
                    Some(disk) => {
                        let empty = Vec::new();
                        let base_annotations = base.as_ref().map_or(&empty, |b| &b.annotations);
                        (to_write, outside) = merge(base_annotations, sidecar, &disk);
                    }
                    None => {
                        fsutil::set_aside(path);
                    }
                }
            }
            drop(base);
            if let Some(dir) = &self.versions {
                let now = versions::now();
                if !self.kept_this_session.get() || versions::due(dir, now) {
                    if parse_sidecar(text, path).is_some() {
                        let _ = versions::keep(dir, text, now);
                    }
                    self.kept_this_session.set(true);
                }
            }
        }
        let json = to_write.to_json().map_err(|e| e.to_string())?;
        fsutil::write_atomic(path, json.as_bytes())
            .map_err(|e| format!("Could not save annotations: {e}"))?;
        self.remember(Some(json), path);
        Ok(Saved {
            merged: (outside > 0).then_some(to_write),
            outside,
        })
    }
}

/// Three-way merge by annotation id: `base` is the file as this process last saw it, `ours`
/// what it now wants to write, `disk` what is there now. A change made on only one side wins;
/// where both sides changed the same annotation, ours wins, except that an annotation edited
/// elsewhere is kept even if we deleted it. Returns the result and how many outside changes
/// it took in.
pub fn merge(
    base: &[Annotation],
    ours: &AnnotationSidecar,
    disk: &AnnotationSidecar,
) -> (AnnotationSidecar, usize) {
    let base: HashMap<&str, &Annotation> = base.iter().map(|a| (a.id.as_str(), a)).collect();
    let on_disk: HashMap<&str, &Annotation> = disk
        .annotations
        .iter()
        .map(|a| (a.id.as_str(), a))
        .collect();
    let mine: HashSet<&str> = ours.annotations.iter().map(|a| a.id.as_str()).collect();
    let mut out = Vec::with_capacity(ours.annotations.len());
    let mut outside = 0;
    for a in &ours.annotations {
        match (base.get(a.id.as_str()), on_disk.get(a.id.as_str())) {
            (Some(b), Some(d)) if a == *b && d != b => {
                out.push((*d).clone());
                outside += 1;
            }
            (Some(b), None) if a == *b => outside += 1,
            _ => out.push(a.clone()),
        }
    }
    for d in &disk.annotations {
        if mine.contains(d.id.as_str()) {
            continue;
        }
        match base.get(d.id.as_str()) {
            Some(b) if d == *b => {}
            _ => {
                out.push(d.clone());
                outside += 1;
            }
        }
    }
    let mut merged = ours.clone();
    merged.annotations = out;
    if merged.pdf_hash.is_none() {
        merged.pdf_hash = disk.pdf_hash.clone();
    }
    (merged, outside)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fond_annot::AnnotationKind;

    fn ann(text: &str) -> Annotation {
        Annotation::drawn(
            AnnotationKind::Highlight,
            1,
            vec![[1.0, 2.5, 3.25, 4.0, 5.0, 6.0, 7.125, 8.0]],
            Some(text.into()),
            None,
            None,
        )
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pereplyot-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn on_disk(p: &Path) -> Vec<String> {
        let s = AnnotationSidecar::parse(&std::fs::read_to_string(p).unwrap(), p).unwrap();
        s.annotations
            .iter()
            .map(|a| {
                format!(
                    "{}{}",
                    a.snippet.clone().unwrap_or_default(),
                    a.note
                        .as_deref()
                        .map(|n| format!(":{n}"))
                        .unwrap_or_default()
                )
            })
            .collect()
    }

    /// Two windows on one file, each saving its own copy as the reader does.
    fn two(name: &str) -> (PathBuf, SidecarSync, SidecarSync) {
        let p = scratch(name).join("x.json");
        (p, SidecarSync::default(), SidecarSync::default())
    }

    #[test]
    fn a_saved_annotation_reads_back_equal() {
        let p = scratch("roundtrip").join("x.json");
        let mut s = AnnotationSidecar::new("k");
        let mut a = ann("quote");
        a.note = Some("a note".into());
        s.annotations.push(a);
        std::fs::write(&p, s.to_json().unwrap()).unwrap();
        let back = AnnotationSidecar::parse(&std::fs::read_to_string(&p).unwrap(), &p).unwrap();
        assert_eq!(back.annotations, s.annotations);
    }

    #[test]
    fn corrupt_file_is_not_silently_replaced() {
        let d = scratch("corrupt");
        let p = d.join("x.json");
        std::fs::write(&p, "{ broken").unwrap();
        let sync = SidecarSync::default();
        let r = sync.load(&p, || AnnotationSidecar::new("k"));
        assert!(r.warning.is_some());
        assert!(r.sidecar.annotations.is_empty());
        assert!(std::fs::read_dir(&d).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("corrupt")));
    }

    #[test]
    fn a_file_damaged_while_open_is_set_aside_before_saving() {
        let (p, a, _) = two("damaged");
        let mut sa = a.load(&p, || AnnotationSidecar::new("k")).sidecar;
        sa.annotations.push(ann("one"));
        a.save(&p, &sa).unwrap();
        std::fs::write(&p, "{ broken").unwrap();
        sa.annotations.push(ann("two"));
        a.save(&p, &sa).unwrap();
        assert_eq!(on_disk(&p), ["one", "two"]);
        assert!(std::fs::read_dir(p.parent().unwrap()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("corrupt")));
    }

    #[test]
    fn additions_made_elsewhere_survive_every_later_save() {
        let (p, a, b) = two("added");
        let mut sa = a.load(&p, || AnnotationSidecar::new("k")).sidecar;
        let mut sb = b.load(&p, || AnnotationSidecar::new("k")).sidecar;
        sa.annotations.push(ann("from a"));
        a.save(&p, &sa).unwrap();
        sb.annotations.push(ann("from b 1"));
        let saved = b.save(&p, &sb).unwrap();
        assert_eq!(saved.outside, 1);
        sb = saved.merged.expect("merged copy handed back");
        sb.annotations.push(ann("from b 2"));
        assert_eq!(b.save(&p, &sb).unwrap().outside, 0);
        assert_eq!(on_disk(&p), ["from b 1", "from a", "from b 2"]);
    }

    #[test]
    fn edits_and_deletions_made_elsewhere_are_kept() {
        let (p, a, b) = two("edits");
        let mut sa = a.load(&p, || AnnotationSidecar::new("k")).sidecar;
        sa.annotations.push(ann("keep"));
        sa.annotations.push(ann("edit"));
        sa.annotations.push(ann("drop"));
        a.save(&p, &sa).unwrap();
        let mut sb = b.load(&p, || AnnotationSidecar::new("k")).sidecar;
        sb.annotations[1].note = Some("edited in b".into());
        sb.annotations.remove(2);
        b.save(&p, &sb).unwrap();
        sa.annotations.push(ann("new in a"));
        let saved = a.save(&p, &sa).unwrap();
        assert_eq!(saved.outside, 2);
        assert_eq!(on_disk(&p), ["keep", "edit:edited in b", "new in a"]);
    }

    #[test]
    fn our_edit_wins_a_clash_and_their_edit_survives_our_delete() {
        let (p, a, b) = two("clash");
        let mut sa = a.load(&p, || AnnotationSidecar::new("k")).sidecar;
        sa.annotations.push(ann("both edit"));
        sa.annotations.push(ann("a deletes, b edits"));
        a.save(&p, &sa).unwrap();
        let mut sb = b.load(&p, || AnnotationSidecar::new("k")).sidecar;
        sb.annotations[0].note = Some("b".into());
        sb.annotations[1].note = Some("b".into());
        b.save(&p, &sb).unwrap();
        sa.annotations[0].note = Some("a".into());
        sa.annotations.remove(1);
        a.save(&p, &sa).unwrap();
        assert_eq!(on_disk(&p), ["both edit:a", "a deletes, b edits:b"]);
    }

    #[test]
    fn a_file_removed_elsewhere_is_not_read_as_everything_deleted() {
        let (p, a, _) = two("removed");
        let mut sa = a.load(&p, || AnnotationSidecar::new("k")).sidecar;
        sa.annotations.push(ann("one"));
        a.save(&p, &sa).unwrap();
        std::fs::remove_file(&p).unwrap();
        sa.annotations.push(ann("two"));
        a.save(&p, &sa).unwrap();
        assert_eq!(on_disk(&p), ["one", "two"]);
    }

    #[test]
    fn the_first_save_of_a_session_keeps_the_previous_file() {
        let d = scratch("versions");
        let p = d.join("x.json");
        let first = SidecarSync::with_versions(d.join("v"));
        let mut s = first.load(&p, || AnnotationSidecar::new("k")).sidecar;
        s.annotations.push(ann("one"));
        first.save(&p, &s).unwrap();
        assert!(versions::list(&d.join("v")).is_empty());
        let second = SidecarSync::with_versions(d.join("v"));
        let mut s = second.load(&p, || AnnotationSidecar::new("k")).sidecar;
        s.annotations.clear();
        second.save(&p, &s).unwrap();
        s.annotations.push(ann("two"));
        second.save(&p, &s).unwrap();
        let kept = versions::list(&d.join("v"));
        assert_eq!(kept.len(), 1);
        assert_eq!(on_disk(&kept[0].path), ["one"]);
    }
}
