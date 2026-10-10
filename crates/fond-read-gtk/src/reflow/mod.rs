//! Reading mode's text: a PDF's words laid out as a flow of headings, paragraphs and notes.

pub mod citations;
pub mod endnotes;
pub mod extract;
pub mod figures;
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
                Item::Figure { page, bbox } => eprintln!("figure p{page}: {bbox:?}"),
                Item::TitlePage { page, .. } => eprintln!("title page p{page}"),
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
        let mut page = super::ocr::ocr_page(&doc, 0, &program, "eng", &cache).expect("recognised");
        assert_eq!(page.words.len(), 3);
        page.page = 20;
        let items = flow_of(&[page]);
        assert_eq!(paragraphs(&items)[0].text, "Hello world again.");
        assert!(cache.join("eng-0.tsv").is_file());
        let again =
            super::ocr::ocr_page(&doc, 0, std::path::Path::new("/nonexistent"), "eng", &cache);
        assert_eq!(again.expect("from the cache").words.len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real Tesseract, when this machine has one with English data (it is skipped otherwise).
    #[test]
    fn a_real_scan_is_read_by_the_real_tesseract() {
        let Some(program) = super::ocr::tesseract() else {
            eprintln!("no tesseract; skipping");
            return;
        };
        let Ok(pdfium) = crate::pdfium::get() else {
            eprintln!("no PDFium library; skipping");
            return;
        };
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scan.pdf");
        let doc = pdfium.load_pdf_from_file(&path, None).unwrap();
        assert!(
            extract_page(&doc, 0).map_or(true, |p| p.words.is_empty()),
            "the fixture has a text layer"
        );
        let cache = std::env::temp_dir().join(format!("pereplyot-real-ocr-{}", std::process::id()));
        let Some(page) = super::ocr::ocr_page(&doc, 0, &program, "eng", &cache) else {
            eprintln!("tesseract has no English data; skipping");
            return;
        };
        let items = flow_of(&[page]);
        let all: String = paragraphs(&items)
            .iter()
            .map(|p| p.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        for word in ["hidden", "community", "liturgy", "testimony", "authority"] {
            assert!(
                all.to_lowercase().contains(word),
                "{word} missing from {all:?}"
            );
        }
        assert_eq!(paragraphs(&items).len(), 2, "{all:?}");
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn a_chart_and_a_table_become_pictures_beside_their_captions() {
        let Some(items) = flow("figures.pdf") else {
            return;
        };
        let kinds: Vec<String> = items
            .iter()
            .map(|i| match i {
                Item::Figure { .. } | Item::TitlePage { .. } => "figure".to_string(),
                Item::Heading { text, .. } => format!("heading {text}"),
                Item::Paragraph(p) => p.text.chars().take(12).collect(),
            })
            .collect();
        let figures: Vec<usize> = (0..items.len())
            .filter(|&i| matches!(items[i], Item::Figure { .. }))
            .collect();
        assert_eq!(figures.len(), 2, "{kinds:?}");
        let caption = |prefix: &str| {
            items
                .iter()
                .position(|i| matches!(i, Item::Paragraph(p) if p.text.starts_with(prefix)))
                .unwrap_or_else(|| panic!("no {prefix} caption in {kinds:?}"))
        };
        assert_eq!(figures[0] + 1, caption("Figure 1."), "{kinds:?}");
        assert_eq!(figures[1], caption("Table 1.") + 1, "{kinds:?}");
        let all: String = paragraphs(&items)
            .iter()
            .map(|p| p.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        for label in ["Readers per", "90", "2019", "2020"] {
            let leaked =
                all.lines().any(|l| l.split(' ').any(|w| w == label)) && label != "Readers per";
            assert!(!leaked, "{label} leaked into the text: {all}");
        }
        let Item::Figure { bbox, .. } = items[figures[0]] else {
            unreachable!()
        };
        assert!(
            bbox[2] - bbox[0] > 200.0 && bbox[3] - bbox[1] > 80.0,
            "{bbox:?}"
        );
    }

    fn raw_pages(name: &str) -> Option<Vec<super::model::RawPage>> {
        let Ok(pdfium) = crate::pdfium::get() else {
            eprintln!("no PDFium library; skipping");
            return None;
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        let doc = pdfium.load_pdf_from_file(&path, None).ok()?;
        Some(
            (0..doc.pages().len())
                .filter_map(|i| extract_page(&doc, i))
                .collect(),
        )
    }

    /// What pointing at `word` on page 1 of `name` leads to in the bibliography.
    fn looked_up(name: &str, word: &str) -> Option<String> {
        use super::citations::{citation_at, find_entry};
        let pages = raw_pages(name)?;
        let w = pages[0]
            .words
            .iter()
            .find(|w| w.text == word)
            .unwrap_or_else(|| panic!("{word} not on page 1 of {name}"));
        let c = citation_at(&pages[0], (w.x0 + w.x1) / 2.0, (w.y0 + w.y1) / 2.0)
            .unwrap_or_else(|| panic!("{word} in {name} was not seen as a citation"));
        Some(
            find_entry(&pages, &c)
                .unwrap_or_else(|| panic!("{c:?} from {name} found no entry"))
                .text,
        )
    }

    #[test]
    fn citations_without_links_lead_to_their_bibliography_entries() {
        let cases = [
            ("cite-authoryear.pdf", "2019;", "Book of Examples"),
            ("cite-authoryear.pdf", "Lee", "On narrative form"),
            (
                "cite-numeric.pdf",
                "[2]",
                "second work whose title is long enough to wrap onto another line",
            ),
            ("cite-numeric.pdf", "[4,", "fourth and most relevant"),
            ("cite-narrative.pdf", "Smith", "Book of Examples"),
            ("cite-narrative.pdf", "(2015a)", "On narrative form"),
            (
                "cite-superscript.pdf",
                "3",
                "third work that supports the claim",
            ),
        ];
        for (file, word, expect) in cases {
            let Some(entry) = looked_up(file, word) else {
                return;
            };
            assert!(entry.contains(expect), "{file} {word}: got {entry:?}");
        }
    }

    #[test]
    #[ignore]
    fn dump_sample() {
        let Ok(path) = std::env::var("PP_SAMPLE") else {
            return;
        };
        let (from, to): (u16, u16) = (
            std::env::var("PP_FROM")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            std::env::var("PP_TO")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
        );
        let pdfium = crate::pdfium::get().expect("pdfium");
        let doc = pdfium
            .load_pdf_from_file(std::path::Path::new(&path), None)
            .expect("open");
        let pages: Vec<_> = (0..doc.pages().len())
            .filter_map(|i| extract_page(&doc, i))
            .collect();
        if let Ok(p) = std::env::var("PP_WORDS") {
            let p: usize = p.parse().unwrap();
            if let Some(page) = pages.iter().find(|x| x.page as usize == p) {
                eprintln!("page {p}: {}x{}", page.width, page.height);
                for w in &page.words {
                    eprintln!(
                        "{:7.1} {:7.1} {:7.1} {:7.1} s{:5.1} {}",
                        w.x0, w.y0, w.x1, w.y1, w.size, w.text
                    );
                }
            }
        }
        let by_page: std::collections::HashMap<u16, super::model::RawPage> =
            pages.iter().map(|p| (p.page, p.clone())).collect();
        let endnotes = super::endnotes::find(doc.pages().len(), &|i| by_page.get(&i).cloned());
        if let Some(e) = &endnotes {
            eprintln!(
                "endnotes: pages {:?}, {} groups: {:?}",
                e.pages,
                e.groups.len(),
                e.groups
                    .iter()
                    .map(|g| (&g.title, g.entries.len()))
                    .collect::<Vec<_>>()
            );
        }
        let items = super::layout::flow_with(&pages, endnotes);
        for item in &items {
            match item {
                Item::Heading { level, text, page } if *page >= from && *page <= to => {
                    eprintln!("H{level} p{page}: {text}")
                }
                Item::Figure { page, bbox } if *page >= from && *page <= to => {
                    eprintln!("figure p{page}: {bbox:?}")
                }
                Item::TitlePage { page, .. } if *page >= from && *page <= to => {
                    eprintln!("title page p{page}")
                }
                Item::Paragraph(p)
                    if p.breaks
                        .first()
                        .is_some_and(|b| b.page >= from && b.page <= to) =>
                {
                    eprintln!(
                        "P p{} {}.. ({} chars, markers {:?}, notes {:?})",
                        p.breaks[0].page,
                        p.text.chars().take(60).collect::<String>(),
                        p.text.chars().count(),
                        p.markers.iter().map(|m| &m.label).collect::<Vec<_>>(),
                        p.notes
                            .iter()
                            .map(|n| (
                                &n.label,
                                n.anchor.is_some(),
                                n.text.chars().take(30).collect::<String>()
                            ))
                            .collect::<Vec<_>>()
                    )
                }
                _ => {}
            }
        }
    }
}
