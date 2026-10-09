//! Reading mode's text: a PDF's words laid out as a flow of headings, paragraphs and notes.

pub mod extract;
pub mod layout;
pub mod model;
pub mod thread;

#[cfg(test)]
mod tests {
    use super::extract::extract_page;
    use super::layout::flow_of;
    use super::model::{Item, Paragraph};

    fn flow(name: &str) -> Option<Vec<Item>> {
        let Ok(pdfium) = crate::pdfium::get() else {
            eprintln!("no PDFium library; skipping");
            return None;
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        let doc = pdfium.load_pdf_from_file(&path, None).ok()?;
        let pages: Vec<_> = (0..doc.pages().len())
            .filter_map(|i| extract_page(&doc, i))
            .collect();
        Some(flow_of(&pages))
    }

    fn paragraphs(items: &[Item]) -> Vec<&Paragraph> {
        items
            .iter()
            .filter_map(|i| match i {
                Item::Paragraph(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_footnoted_monograph_reads_cleanly() {
        let Some(items) = flow("monograph.pdf") else {
            return;
        };
        for item in &items {
            match item {
                Item::Heading { level, text, page } => eprintln!("H{level} p{page}: {text}"),
                Item::Paragraph(p) => eprintln!(
                    "P {}.. ({} chars, markers {:?}, notes {:?}, breaks {:?})",
                    p.text.chars().take(40).collect::<String>(),
                    p.text.chars().count(),
                    p.markers.iter().map(|m| &m.label).collect::<Vec<_>>(),
                    p.notes
                        .iter()
                        .map(|n| (
                            &n.label,
                            n.anchor.is_some(),
                            n.text.chars().take(30).collect::<String>()
                        ))
                        .collect::<Vec<_>>(),
                    p.breaks.iter().map(|b| (b.page, b.at)).collect::<Vec<_>>()
                ),
            }
        }
        let all: String = paragraphs(&items)
            .iter()
            .map(|p| p.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!all.contains("EXAMPLES"), "running header leaked");
        assert!(!all.contains("CHAPTER ONE"), "running header leaked");
        assert!(items
            .iter()
            .any(|i| matches!(i, Item::Heading { text, .. } if text == "Chapter One")));
        let notes: Vec<_> = paragraphs(&items)
            .iter()
            .flat_map(|p| p.notes.clone())
            .collect();
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(notes.iter().all(|n| n.anchor.is_some()), "{notes:?}");
        let long = notes.iter().find(|n| n.label == "2").unwrap();
        assert!(
            long.text
                .contains("break into the next one to its very end, which is here."),
            "{}",
            long.text
        );
        let markers: usize = paragraphs(&items).iter().map(|p| p.markers.len()).sum();
        assert_eq!(markers, 3);
        assert!(
            !all.contains("11") && !all.contains("12"),
            "page number leaked"
        );
    }

    #[test]
    fn a_two_column_paper_reads_one_column_after_the_other() {
        let Some(items) = flow("twocol.pdf") else {
            return;
        };
        assert!(items
            .iter()
            .any(|i| matches!(i, Item::Heading { text, .. } if text.contains("Two Columns"))));
        let all: String = paragraphs(&items)
            .iter()
            .map(|p| p.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!all.contains("Journal of Layout"), "running header leaked");
        assert!(!all.contains("101"), "page number leaked");
        for n in 0..3 {
            let left = all.find(&format!("start{n}l")).expect("left column");
            let right = all.find(&format!("start{n}r")).expect("right column");
            assert!(left < right, "page {n}: right column came first");
            if n > 0 {
                let previous_right = all.find(&format!("start{}r", n - 1)).unwrap();
                assert!(
                    previous_right < left,
                    "page {n} started before the last one ended"
                );
            }
        }
    }
}
