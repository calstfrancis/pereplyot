//! Changes to many annotations at once, from the Notes tab. They are made in each document's
//! saved annotation file; a document open in a reader holds its own live copy, which a save from
//! here would be overwritten by, so those are left alone and counted.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fond_read_gtk::fsutil;

pub enum Op {
    Colour(String),
    AddTag(String),
    RemoveTag(String),
    Delete,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub changed: usize,
    /// In documents open in a reader.
    pub skipped_open: usize,
    pub failed: usize,
}

fn sidecar_path(hash: &str) -> PathBuf {
    glib::user_data_dir()
        .join("pereplyot")
        .join("annotations")
        .join(format!("{hash}.json"))
}

/// Apply `op` to the annotations `(document hash, annotation id)`.
pub fn apply(targets: &[(String, String)], op: &Op, is_open: &dyn Fn(&str) -> bool) -> Report {
    apply_in(&sidecar_path, targets, op, is_open)
}

fn apply_in(
    path_of: &dyn Fn(&str) -> PathBuf,
    targets: &[(String, String)],
    op: &Op,
    is_open: &dyn Fn(&str) -> bool,
) -> Report {
    let mut by_doc: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (hash, id) in targets {
        by_doc.entry(hash).or_default().push(id);
    }
    let mut report = Report::default();
    for (hash, ids) in by_doc {
        if is_open(hash) {
            report.skipped_open += ids.len();
            continue;
        }
        let path = path_of(hash);
        let Some(mut sidecar) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| fond_annot::AnnotationSidecar::parse(&t, &path).ok())
        else {
            report.failed += ids.len();
            continue;
        };
        let mut changed = 0;
        match op {
            Op::Delete => {
                let before = sidecar.annotations.len();
                sidecar
                    .annotations
                    .retain(|a| !ids.contains(&a.id.as_str()));
                changed = before - sidecar.annotations.len();
            }
            _ => {
                for a in sidecar
                    .annotations
                    .iter_mut()
                    .filter(|a| ids.contains(&a.id.as_str()))
                {
                    match op {
                        Op::Colour(hex) => a.color = Some(hex.clone()),
                        Op::AddTag(tag) => {
                            let mut tags = a.explicit_tags();
                            if !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                                tags.push(tag.clone());
                                a.set_explicit_tags(&tags);
                            }
                        }
                        Op::RemoveTag(tag) => {
                            let tags: Vec<String> = a
                                .explicit_tags()
                                .into_iter()
                                .filter(|t| !t.eq_ignore_ascii_case(tag))
                                .collect();
                            a.set_explicit_tags(&tags);
                        }
                        Op::Delete => {}
                    }
                    changed += 1;
                }
            }
        }
        match sidecar
            .to_json()
            .ok()
            .and_then(|json| fsutil::write_atomic(&path, json.as_bytes()).ok())
        {
            Some(()) => report.changed += changed,
            None => report.failed += ids.len(),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &std::path::Path, hash: &str, ids: &[&str]) {
        let mut s = fond_annot::AnnotationSidecar::new(hash);
        for id in ids {
            let mut a = fond_annot::Annotation::drawn_epub(
                fond_annot::AnnotationKind::Highlight,
                "c.xhtml".into(),
                format!("quote {id}"),
                None,
                None,
                None,
            );
            a.id = id.to_string();
            s.annotations.push(a);
        }
        std::fs::write(dir.join(format!("{hash}.json")), s.to_json().unwrap()).unwrap();
    }

    fn read(dir: &std::path::Path, hash: &str) -> fond_annot::AnnotationSidecar {
        let p = dir.join(format!("{hash}.json"));
        fond_annot::AnnotationSidecar::parse(&std::fs::read_to_string(&p).unwrap(), &p).unwrap()
    }

    #[test]
    fn bulk_recolour_tag_and_delete_across_documents_skipping_open_ones() {
        let dir = std::env::temp_dir().join(format!("pereplyot-bulk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write(&dir, "d1", &["a", "b"]);
        write(&dir, "d2", &["c"]);
        write(&dir, "open", &["x"]);
        let path_of = |h: &str| dir.join(format!("{h}.json"));
        let is_open = |h: &str| h == "open";
        let targets: Vec<(String, String)> =
            [("d1", "a"), ("d2", "c"), ("open", "x"), ("gone", "z")]
                .iter()
                .map(|(h, i)| (h.to_string(), i.to_string()))
                .collect();

        let r = apply_in(&path_of, &targets, &Op::Colour("#91A9B8".into()), &is_open);
        assert_eq!(
            r,
            Report {
                changed: 2,
                skipped_open: 1,
                failed: 1
            }
        );
        assert_eq!(
            read(&dir, "d1").annotations[0].color.as_deref(),
            Some("#91A9B8")
        );
        assert_ne!(
            read(&dir, "d1").annotations[1].color.as_deref(),
            Some("#91A9B8")
        );
        assert_ne!(
            read(&dir, "open").annotations[0].color.as_deref(),
            Some("#91A9B8")
        );

        apply_in(&path_of, &targets, &Op::AddTag("method".into()), &is_open);
        apply_in(&path_of, &targets, &Op::AddTag("Method".into()), &is_open);
        assert_eq!(
            read(&dir, "d1").annotations[0].explicit_tags(),
            vec!["method"]
        );
        apply_in(
            &path_of,
            &targets,
            &Op::RemoveTag("METHOD".into()),
            &is_open,
        );
        assert!(read(&dir, "d2").annotations[0].explicit_tags().is_empty());

        let r = apply_in(&path_of, &targets, &Op::Delete, &is_open);
        assert_eq!(r.changed, 2);
        assert_eq!(read(&dir, "d1").annotations.len(), 1);
        assert_eq!(read(&dir, "d2").annotations.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
