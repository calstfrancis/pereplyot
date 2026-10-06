use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;

use fond_annot::AnnotationSidecar;

use crate::fsutil::{self, Loaded};

/// Tracks what this process last read or wrote, so a save can notice that someone else
/// (another window, the vault's owner app) changed the file in between and fold their
/// additions in instead of overwriting them.
#[derive(Default)]
pub struct SidecarSync {
    known_ids: RefCell<HashSet<String>>,
    last_text: RefCell<Option<String>>,
}

pub struct LoadReport {
    pub sidecar: AnnotationSidecar,
    pub warning: Option<String>,
}

pub fn parse_sidecar(text: &str, path: &Path) -> Option<AnnotationSidecar> {
    AnnotationSidecar::parse(text, path).ok()
}

impl SidecarSync {
    pub fn note_known(&self, sidecar: &AnnotationSidecar, text: Option<String>) {
        *self.known_ids.borrow_mut() = sidecar.annotations.iter().map(|a| a.id.clone()).collect();
        *self.last_text.borrow_mut() = text;
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
        let text = std::fs::read_to_string(path).ok();
        self.note_known(&sidecar, text);
        LoadReport { sidecar, warning }
    }

    /// Merge annotations that appeared on disk since we last looked into `ours`, returning how
    /// many were folded in.
    pub fn merge_external(&self, path: &Path, ours: &mut AnnotationSidecar) -> usize {
        let Ok(disk_text) = std::fs::read_to_string(path) else {
            return 0;
        };
        if self.last_text.borrow().as_deref() == Some(disk_text.as_str()) {
            return 0;
        }
        let Some(disk) = parse_sidecar(&disk_text, path) else {
            return 0;
        };
        let known = self.known_ids.borrow();
        let mine: HashSet<&str> = ours.annotations.iter().map(|a| a.id.as_str()).collect();
        let added: Vec<_> = disk
            .annotations
            .into_iter()
            .filter(|a| !known.contains(&a.id) && !mine.contains(a.id.as_str()))
            .collect();
        drop(known);
        let n = added.len();
        ours.annotations.extend(added);
        n
    }

    pub fn save(&self, path: &Path, sidecar: &AnnotationSidecar) -> Result<usize, String> {
        let mut merged = sidecar.clone();
        let extra = self.merge_external(path, &mut merged);
        let json = merged.to_json().map_err(|e| e.to_string())?;
        fsutil::write_atomic(path, json.as_bytes())
            .map_err(|e| format!("Could not save annotations: {e}"))?;
        self.note_known(&merged, Some(json));
        Ok(extra)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fond_annot::{Annotation, AnnotationKind};

    fn ann(text: &str) -> Annotation {
        Annotation::drawn(
            AnnotationKind::Highlight,
            1,
            vec![],
            Some(text.into()),
            None,
            None,
        )
    }

    #[test]
    fn corrupt_file_is_not_silently_replaced() {
        let d = std::env::temp_dir().join(format!("pereplyot-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
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
    fn external_additions_survive_a_save() {
        let d = std::env::temp_dir().join(format!("pereplyot-store2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("x.json");
        let a = SidecarSync::default();
        let b = SidecarSync::default();
        let mut sa = a.load(&p, || AnnotationSidecar::new("k")).sidecar;
        let mut sb = b.load(&p, || AnnotationSidecar::new("k")).sidecar;
        sa.annotations.push(ann("from a"));
        a.save(&p, &sa).unwrap();
        sb.annotations.push(ann("from b"));
        assert_eq!(b.save(&p, &sb).unwrap(), 1);
        let on_disk = AnnotationSidecar::parse(&std::fs::read_to_string(&p).unwrap(), &p).unwrap();
        assert_eq!(on_disk.annotations.len(), 2);
    }
}
