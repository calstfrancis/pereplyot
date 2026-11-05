//! A précis of one document, drawn from what was marked in it by meaning (key idea, evidence, connection,
//! problem) with the person's own few sentences on top, and the annotated bibliography built from them.

use fond_annot::export::typst_escape;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecisMark {
    /// The printed page, as cited.
    pub label: String,
    pub text: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Precis {
    pub summary: String,
    pub key_ideas: Vec<PrecisMark>,
    pub evidence: Vec<PrecisMark>,
    pub connections: Vec<PrecisMark>,
    pub problems: Vec<PrecisMark>,
}

impl Precis {
    pub fn is_empty(&self) -> bool {
        self.summary.trim().is_empty()
            && self.key_ideas.is_empty()
            && self.evidence.is_empty()
            && self.connections.is_empty()
            && self.problems.is_empty()
    }

    pub fn count(&self) -> usize {
        self.key_ideas.len() + self.evidence.len() + self.connections.len() + self.problems.len()
    }
}

/// Sorts marks into the four meanings by colour; `palette` is the four colours in meaning order. A mark in
/// another colour belongs to no meaning and is left out.
pub fn classify(
    marks: &[(Option<String>, PrecisMark)],
    palette: [&str; 4],
    summary: &str,
) -> Precis {
    let mut p = Precis {
        summary: summary.trim().to_string(),
        ..Default::default()
    };
    for (colour, mark) in marks {
        let Some(c) = colour else { continue };
        let slot = match palette.iter().position(|p| p.eq_ignore_ascii_case(c)) {
            Some(0) => &mut p.key_ideas,
            Some(1) => &mut p.evidence,
            Some(2) => &mut p.connections,
            Some(3) => &mut p.problems,
            _ => continue,
        };
        slot.push(mark.clone());
    }
    p
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BibEntry {
    /// The reference as the citation style words it.
    pub reference: String,
    pub precis: Precis,
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn marks_block(out: &mut String, heading: &str, marks: &[PrecisMark]) {
    if marks.is_empty() {
        return;
    }
    out.push_str(&format!(
        "  #text(weight: \"bold\", size: 0.9em)[{}]\n  #list(\n",
        typst_escape(heading)
    ));
    for m in marks {
        let loc = if m.label.is_empty() {
            String::new()
        } else {
            format!(" (p. {})", typst_escape(&m.label))
        };
        out.push_str(&format!(
            "    [“{}”{loc}],\n",
            typst_escape(&one_line(&m.text))
        ));
    }
    out.push_str("  )\n");
}

/// An annotated bibliography as one Typst document that compiles by itself: each reference, then the
/// person's summary, then (with `with_marks`) the marked passages under their meanings.
pub fn annotated_bibliography_typst(
    title: &str,
    labels: [&str; 4],
    entries: &[BibEntry],
    with_marks: bool,
) -> String {
    let mut out = String::new();
    out.push_str("#set par(justify: true)\n");
    out.push_str(&format!("= {}\n\n", typst_escape(title)));
    for e in entries {
        out.push_str(&format!(
            "#block(breakable: true, below: 1.4em)[\n  #par(hanging-indent: 1.5em)[{}]\n",
            typst_escape(&one_line(&e.reference))
        ));
        if !e.precis.summary.trim().is_empty() {
            out.push_str(&format!(
                "  #block(inset: (left: 1.5em))[{}]\n",
                typst_escape(&one_line(&e.precis.summary))
            ));
        }
        if with_marks && e.precis.count() > 0 {
            out.push_str("  #block(inset: (left: 1.5em))[\n");
            marks_block(&mut out, labels[0], &e.precis.key_ideas);
            marks_block(&mut out, labels[1], &e.precis.evidence);
            marks_block(&mut out, labels[2], &e.precis.connections);
            marks_block(&mut out, labels[3], &e.precis.problems);
            out.push_str("  ]\n");
        }
        out.push_str("]\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PALETTE: [&str; 4] = ["#f5c211", "#62a0ea", "#57e389", "#ed333b"];

    fn m(label: &str, text: &str) -> PrecisMark {
        PrecisMark {
            label: label.into(),
            text: text.into(),
            note: String::new(),
        }
    }

    #[test]
    fn marks_sort_into_the_four_meanings() {
        let marks = [
            (Some("#F5C211".to_string()), m("1", "idea")),
            (Some("#62a0ea".to_string()), m("2", "evidence")),
            (Some("#ed333b".to_string()), m("3", "problem")),
            (Some("#000000".to_string()), m("4", "other colour")),
            (None, m("5", "no colour")),
        ];
        let p = classify(&marks, PALETTE, "  In short.  ");
        assert_eq!(
            (
                p.key_ideas.len(),
                p.evidence.len(),
                p.connections.len(),
                p.problems.len()
            ),
            (1, 1, 0, 1)
        );
        assert_eq!(p.summary, "In short.");
        assert_eq!(p.count(), 3);
    }

    fn entries() -> Vec<BibEntry> {
        let marks = [
            (
                Some("#f5c211".to_string()),
                m("41", "Q was composed in \"stages\" #1."),
            ),
            (
                Some("#ed333b".to_string()),
                m("50", "But the layers are *disputed*."),
            ),
        ];
        vec![
            BibEntry {
                reference: "Kloppenborg, John S. The Formation of Q. Philadelphia: Fortress, 1987.".into(),
                precis: classify(&marks, PALETTE, "The classic stratigraphy of Q: three layers, wisdom first."),
            },
            BibEntry {
                reference: "Farrer, A. M. “On Dispensing with Q.” In Studies in the Gospels, 55–88. Oxford: Blackwell, 1955.".into(),
                precis: Precis::default(),
            },
        ]
    }

    #[test]
    fn the_annotated_bibliography_has_each_reference_summary_and_marks() {
        let t = annotated_bibliography_typst(
            "Annotated bibliography",
            ["Key ideas", "Evidence", "Connections", "Problems"],
            &entries(),
            true,
        );
        assert!(t.contains("Kloppenborg, John S. The Formation of Q"), "{t}");
        assert!(t.contains("The classic stratigraphy"));
        assert!(t.contains("Key ideas"));
        assert!(t.contains("(p. 41)"));
        assert!(!t.contains("Evidence"), "an empty meaning gets no heading");
        let short = annotated_bibliography_typst("T", ["a", "b", "c", "d"], &entries(), false);
        assert!(!short.contains("(p. 41)"));
    }

    fn on_path(name: &str) -> Option<std::path::PathBuf> {
        std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(name))
                .find(|f| f.is_file())
        })
    }

    #[test]
    fn the_annotated_bibliography_compiles() {
        let Some(typst) = on_path("typst") else {
            eprintln!("no typst on PATH; skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("pereplyot-precis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (i, with_marks) in [true, false].into_iter().enumerate() {
            let file = dir.join(format!("bib{i}.typ"));
            std::fs::write(
                &file,
                annotated_bibliography_typst(
                    "Annotated bibliography",
                    ["Key ideas", "Evidence", "Connections", "Problems"],
                    &entries(),
                    with_marks,
                ),
            )
            .unwrap();
            let out = std::process::Command::new(&typst)
                .arg("compile")
                .arg(&file)
                .arg(dir.join(format!("bib{i}.pdf")))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
