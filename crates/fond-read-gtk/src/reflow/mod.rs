//! Reading mode's text: a PDF's words laid out as a flow of headings, paragraphs and notes.

pub mod extract;
pub mod layout;
pub mod margin;
pub mod model;
pub mod ocr;
pub mod thread;
pub mod view;

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

    #[test]
    fn a_page_with_no_text_goes_through_ocr_and_the_result_is_cached() {
        use std::os::unix::fs::PermissionsExt;
        let Ok(pdfium) = crate::pdfium::get() else {
            eprintln!("no PDFium library; skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("pereplyot-ocr-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A stand-in for Tesseract: reads the image, answers with a fixed result.
        let program = dir.join("tesseract");
        std::fs::write(
            &program,
            "#!/bin/sh\ncat >/dev/null\nprintf 'level\\tpage_num\\tblock_num\\tpar_num\\tline_num\\tword_num\\tleft\\ttop\\twidth\\theight\\tconf\\ttext\\n'\n\
printf '5\\t1\\t1\\t1\\t1\\t1\\t300\\t600\\t300\\t60\\t95\\tHello\\n'\n\
printf '5\\t1\\t1\\t1\\t1\\t2\\t650\\t600\\t310\\t60\\t95\\tworld\\n'\n\
printf '5\\t1\\t1\\t1\\t1\\t3\\t1000\\t600\\t300\\t60\\t95\\tagain.\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/blank.pdf");
        let doc = pdfium.load_pdf_from_file(&path, None).unwrap();
        let cache = dir.join("cache");
        let page = super::ocr::ocr_page(&doc, 0, &program, "eng", &cache).expect("recognised");
        assert_eq!(page.words.len(), 3);
        let items = flow_of(&[page]);
        assert_eq!(paragraphs(&items)[0].text, "Hello world again.");
        assert!(cache.join("eng-0.tsv").is_file());
        let again =
            super::ocr::ocr_page(&doc, 0, std::path::Path::new("/nonexistent"), "eng", &cache);
        assert_eq!(again.expect("from the cache").words.len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
