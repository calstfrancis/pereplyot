//! Bringing a document's marks to another version of it: each one is found again by its words, given
//! fresh coordinates, and labelled by how sure that is. Nothing is dropped.

use std::path::Path;

use fond_annot::{Annotation, AnnotationKind, AnnotationSidecar};
use pereplyot_core::reanchor::{reanchor, Fit, MarkText};

/// Key in an annotation's `extra` that says it was carried over and not located with certainty.
pub const STATE_KEY: &str = "reanchor";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub exact: usize,
    pub moved: usize,
    pub uncertain: usize,
    pub orphaned: usize,
    /// Marks with no words to find them by (clipped areas, notes): kept where they were.
    pub unchecked: usize,
}

impl Summary {
    pub fn total(&self) -> usize {
        self.exact + self.moved + self.uncertain + self.orphaned + self.unchecked
    }

    /// Marks that need a look.
    pub fn to_review(&self) -> usize {
        self.uncertain + self.orphaned + self.unchecked
    }

    pub fn describe(&self) -> String {
        let mut parts = vec![format!("{} found again", self.exact + self.moved)];
        if self.uncertain > 0 {
            parts.push(format!("{} uncertain", self.uncertain));
        }
        if self.orphaned > 0 {
            parts.push(format!("{} not found in this version", self.orphaned));
        }
        if self.unchecked > 0 {
            parts.push(format!("{} kept where they were", self.unchecked));
        }
        parts.join(", ")
    }
}

struct PageChars {
    text: String,
    bounds: Vec<Option<[f32; 4]>>,
}

fn read_pages(path: &Path) -> Option<Vec<PageChars>> {
    let pdfium = crate::pdfium::get().ok()?;
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    let mut pages = Vec::new();
    for page in doc.pages().iter() {
        let mut text = String::new();
        let mut bounds = Vec::new();
        if let Ok(t) = page.text() {
            for c in t.chars().iter() {
                text.push(c.unicode_char().unwrap_or(' '));
                bounds.push(c.loose_bounds().ok().map(|r| {
                    [
                        r.left().value,
                        r.bottom().value,
                        r.right().value,
                        r.top().value,
                    ]
                }));
            }
        }
        pages.push(PageChars { text, bounds });
    }
    Some(pages)
}

/// One quad per line of the chars `from..to`, in PDF user space.
fn quads(page: &PageChars, from: usize, to: usize) -> Vec<[f64; 8]> {
    let mut lines: Vec<[f32; 4]> = Vec::new();
    for b in page.bounds.iter().take(to).skip(from).flatten() {
        let (l, bo, r, t) = (b[0], b[1], b[2], b[3]);
        if r - l <= 0.0 || t - bo <= 0.0 {
            continue;
        }
        let mid = (bo + t) / 2.0;
        match lines.last_mut() {
            Some(line) if mid >= line[1] && mid <= line[3] => {
                line[0] = line[0].min(l);
                line[1] = line[1].min(bo);
                line[2] = line[2].max(r);
                line[3] = line[3].max(t);
            }
            _ => lines.push([l, bo, r, t]),
        }
    }
    lines
        .iter()
        .map(|[l, b, r, t]| {
            [
                *l as f64, *t as f64, *r as f64, *t as f64, *l as f64, *b as f64, *r as f64,
                *b as f64,
            ]
        })
        .collect()
}

fn has_words(a: &Annotation) -> bool {
    matches!(
        a.kind,
        AnnotationKind::Highlight | AnnotationKind::Underline | AnnotationKind::Strikeout
    ) && a.page.is_some()
        && a.snippet
            .as_deref()
            .is_some_and(|s| s.split_whitespace().count() >= 2)
}

/// `old`'s marks located in the PDF at `new_path`, as a sidecar for `new_hash`. Blocking.
pub fn carry(
    old: &AnnotationSidecar,
    old_pages: u32,
    new_path: &Path,
    new_hash: &str,
) -> Option<(AnnotationSidecar, Summary)> {
    let pages = read_pages(new_path)?;
    let texts: Vec<String> = pages.iter().map(|p| p.text.clone()).collect();
    let marks: Vec<MarkText> = old
        .annotations
        .iter()
        .filter(|a| has_words(a))
        .map(|a| MarkText {
            id: a.id.clone(),
            page: a.page.unwrap_or(1),
            snippet: a.snippet.clone().unwrap_or_default(),
            prefix: a.snippet_prefix.clone().unwrap_or_default(),
            suffix: a.snippet_suffix.clone().unwrap_or_default(),
        })
        .collect();
    let outcomes = reanchor(&marks, &texts, old_pages);

    let mut sidecar = old.clone();
    sidecar.key = new_hash.to_string();
    if sidecar.pdf_hash.is_some() {
        sidecar.pdf_hash = Some(new_hash.to_string());
    }
    let mut summary = Summary::default();
    for a in &mut sidecar.annotations {
        a.extra.remove(STATE_KEY);
        if !has_words(a) {
            if a.page.is_some() {
                summary.unchecked += 1;
                a.extra.insert(STATE_KEY.into(), "unchecked".into());
            }
            continue;
        }
        let Some(o) = outcomes.iter().find(|o| o.id == a.id) else {
            continue;
        };
        let located = o.page.zip(o.range).and_then(|(page, (from, to))| {
            let q = quads(pages.get(page as usize - 1)?, from, to);
            (!q.is_empty()).then_some((page, q))
        });
        let (fit, label) = match (o.fit, &located) {
            (Fit::Exact, Some(_)) => (Fit::Exact, None),
            (Fit::Moved, Some(_)) => (Fit::Moved, None),
            (Fit::Exact | Fit::Moved | Fit::Uncertain, _) => (Fit::Uncertain, Some("uncertain")),
            (Fit::Orphaned, _) => (Fit::Orphaned, Some("orphaned")),
        };
        match fit {
            Fit::Exact => summary.exact += 1,
            Fit::Moved => summary.moved += 1,
            Fit::Uncertain => summary.uncertain += 1,
            Fit::Orphaned => summary.orphaned += 1,
        }
        if let (Fit::Exact | Fit::Moved | Fit::Uncertain, Some((page, q))) = (fit, located) {
            a.page = Some(page);
            a.quadpoints = q;
        } else if fit == Fit::Uncertain {
            if let Some(page) = o.page {
                a.page = Some(page);
            }
        }
        if let Some(label) = label {
            a.extra.insert(STATE_KEY.into(), label.into());
        }
    }
    Some((sidecar, summary))
}

/// The state a carried mark was left in, if it still needs a look.
pub fn state_of(a: &Annotation) -> Option<&str> {
    a.extra.get(STATE_KEY).and_then(|v| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    fn annotation(id: &str, page: u32, snippet: &str) -> Annotation {
        let mut a = Annotation {
            id: id.into(),
            kind: AnnotationKind::Highlight,
            page: Some(page),
            chapter: None,
            quadpoints: vec![[0.0; 8]],
            snippet: Some(snippet.into()),
            snippet_prefix: None,
            snippet_suffix: None,
            color: None,
            note: None,
            created: None,
            modified: None,
            extra: Default::default(),
        };
        a.extra.clear();
        a
    }

    #[test]
    fn marks_are_found_again_with_real_coordinates() {
        if crate::pdfium::get().is_err() {
            eprintln!("no PDFium library; skipping");
            return;
        }
        let mut sidecar = AnnotationSidecar::new("old");
        sidecar.annotations = vec![
            annotation("a", 3, "Page 3 of the mixed document"),
            annotation("b", 1, "words that appear nowhere in the other file at all"),
            annotation("c", 1, "Page 4 of the mixed document"),
        ];
        let Some((new, summary)) = carry(&sidecar, 5, &fixture("mixed.pdf"), "new") else {
            panic!("could not read the fixture");
        };
        assert_eq!(new.key, "new");
        assert_eq!(summary.total(), 3, "{summary:?}");
        assert_eq!(
            (summary.exact, summary.moved, summary.orphaned),
            (1, 1, 1),
            "{summary:?}"
        );
        let (a, c) = (&new.annotations[0], &new.annotations[2]);
        assert_eq!((a.page, c.page), (Some(3), Some(4)));
        assert!(a.quadpoints.iter().all(|q| q != &[0.0; 8]) && !a.quadpoints.is_empty());
        assert!(c.quadpoints.iter().all(|q| q != &[0.0; 8]) && !c.quadpoints.is_empty());
        assert!(state_of(a).is_none() && state_of(c).is_none());
        assert_eq!(state_of(&new.annotations[1]), Some("orphaned"));
        assert_eq!(new.annotations.len(), 3, "nothing is dropped");
    }
}
