//! What a book already contains and a reader can make use of: its back-of-book index, its figures and
//! tables with their captions, and the data of a table. All blocking; run them off the GTK thread.

use std::path::Path;

use pereplyot_core::book_index::{self, Entry};
use pereplyot_core::table::{self, Word};

use crate::reflow::extract::extract_page;
use crate::reflow::figures::find_figures;
use crate::reflow::layout::lines_of;

fn page_texts(path: &Path) -> Option<Vec<String>> {
    let pdfium = crate::pdfium::get().ok()?;
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    Some(
        doc.pages()
            .iter()
            .map(|p| p.text().map(|t| t.all()).unwrap_or_default())
            .collect(),
    )
}

/// The back-of-book index, parsed; empty when the book has none.
pub fn back_of_book_index(path: &Path) -> Option<Vec<Entry>> {
    let pages = page_texts(path)?;
    let Some((first, last)) = book_index::index_pages(&pages) else {
        return Some(Vec::new());
    };
    Some(book_index::parse_index(&pages[first..=last].join("\n")))
}

#[derive(Debug, Clone, PartialEq)]
pub struct FigureItem {
    /// 0-based.
    pub page: u16,
    /// In the page's displayed points.
    pub region: [f32; 4],
    pub caption: String,
    pub is_table: bool,
}

/// Every figure and table that has a caption beside it.
pub fn figures(path: &Path) -> Option<Vec<FigureItem>> {
    let pdfium = crate::pdfium::get().ok()?;
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    let mut out = Vec::new();
    for i in 0..doc.pages().len() {
        let Some(page) = extract_page(&doc, i) else {
            continue;
        };
        let found = find_figures(&page, &page.words);
        if found.regions.is_empty() {
            continue;
        }
        let lines = lines_of(&page.words);
        for (region, caption) in found.regions.iter().zip(&found.captions) {
            let text = lines
                .iter()
                .find(|l| (l.x0 - caption[0]).abs() < 1.0 && (l.y0 - caption[1]).abs() < 1.0)
                .map(|l| l.text())
                .unwrap_or_default();
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push(FigureItem {
                page: i,
                region: *region,
                is_table: text.to_lowercase().starts_with("table"),
                caption: text,
            });
        }
    }
    Some(out)
}

/// The cells of the table in `region` of page `page` (0-based), if its words form one.
pub fn table_at(path: &Path, page: u16, region: [f32; 4]) -> Option<Vec<Vec<String>>> {
    let pdfium = crate::pdfium::get().ok()?;
    let doc = pdfium.load_pdf_from_file(path, None).ok()?;
    let raw = extract_page(&doc, page)?;
    let words: Vec<Word> = raw
        .words
        .iter()
        .filter(|w| {
            let (cx, cy) = ((w.x0 + w.x1) / 2.0, (w.y0 + w.y1) / 2.0);
            cx >= region[0] && cx <= region[2] && cy >= region[1] && cy <= region[3]
        })
        .map(|w| Word {
            text: w.text.clone(),
            x0: w.x0,
            y0: w.y0,
            x1: w.x1,
            y1: w.y1,
        })
        .collect();
    table::grid(&words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    #[test]
    fn captioned_figures_and_tables_are_listed() {
        if crate::pdfium::get().is_err() {
            eprintln!("no PDFium library; skipping");
            return;
        }
        let items = figures(&fixture("figures.pdf")).unwrap();
        eprintln!("{items:#?}");
        assert!(!items.is_empty(), "the fixture has captioned figures");
        assert!(items.iter().all(|f| !f.caption.is_empty()));
    }

    #[test]
    fn a_captioned_table_reads_out_as_cells() {
        if crate::pdfium::get().is_err() {
            eprintln!("no PDFium library; skipping");
            return;
        }
        let path = fixture("figures.pdf");
        let items = figures(&path).unwrap();
        let table = items.iter().find(|f| f.is_table).expect("a table");
        let cells = table_at(&path, table.page, table.region).expect("its words form a grid");
        eprintln!("{cells:?}");
        assert!(cells.len() >= 3, "{cells:?}");
        assert!(cells
            .iter()
            .all(|r| r.len() == cells[0].len() && r.len() >= 2));
        let figure = items.iter().find(|f| !f.is_table).unwrap();
        let _ = figure;
    }

    #[test]
    fn a_book_with_no_index_gives_an_empty_one() {
        if crate::pdfium::get().is_err() {
            eprintln!("no PDFium library; skipping");
            return;
        }
        assert_eq!(back_of_book_index(&fixture("plain.pdf")), Some(Vec::new()));
    }
}
