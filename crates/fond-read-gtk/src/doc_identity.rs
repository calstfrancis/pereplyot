//! Computes and stores a document's [`Identity`]: the fingerprints that let Pereplyot recognise the
//! same work in another file. Derived from the file, so losing the store only costs a rebuild.

use std::path::{Path, PathBuf};

use pereplyot_core::identity::{pdf_trailer_id, Identity, RawFacts};

use crate::fsutil;
use crate::history::DocKind;

const FRONT_PAGES: usize = 4;
const BACK_PAGES: usize = 2;

pub fn store_path(dir: &Path, hash: &str) -> PathBuf {
    dir.join("documents").join(format!("{hash}.json"))
}

pub fn load(dir: &Path, hash: &str) -> Option<Identity> {
    match fsutil::load_checked(&store_path(dir, hash), |t| {
        serde_json::from_str::<Identity>(t).ok()
    }) {
        fsutil::Loaded::Ok(id) => Some(id),
        _ => None,
    }
}

pub fn save(dir: &Path, id: &Identity) {
    if let Ok(json) = serde_json::to_string_pretty(id) {
        let _ = fsutil::write_atomic(&store_path(dir, &id.hash), json.as_bytes());
    }
}

/// Every stored record, for matching a newly opened document against what's known.
pub fn load_all(dir: &Path) -> Vec<Identity> {
    let Ok(entries) = std::fs::read_dir(dir.join("documents")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let stem = e.path().file_stem()?.to_string_lossy().into_owned();
            load(dir, &stem)
        })
        .collect()
}

/// What reading a file once yields: the fingerprints, and what the file says about itself.
#[derive(Debug, Clone)]
pub struct Probe {
    pub identity: Identity,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub date: Option<String>,
    pub publisher: Option<String>,
    pub isbn: Option<String>,
    pub front_text: String,
    pub is_epub: bool,
}

/// The last 64 KB of a file, where a PDF keeps its trailer.
fn tail_of(path: &Path) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(64 * 1024)))
        .ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Blocking: reads a few pages. Run it off the GTK thread.
pub fn probe(path: &Path, kind: DocKind, hash: &str) -> Option<Probe> {
    match kind {
        DocKind::Pdf => {
            // Only the end of the file (for its trailer id) and a few pages of text: a book of six hundred
            // pages, or a scan of a hundred megabytes, must open as quickly as one of ten.
            let pdfium = crate::pdfium::get().ok()?;
            let doc = pdfium.load_pdf_from_file(path, None).ok()?;
            let field = |tag| {
                doc.metadata()
                    .get(tag)
                    .map(|t| t.value().trim().to_string())
                    .filter(|s| !s.is_empty())
            };
            let title = field(pdfium_render::prelude::PdfDocumentMetadataTagType::Title);
            let author = field(pdfium_render::prelude::PdfDocumentMetadataTagType::Author);
            let count = doc.pages().len() as usize;
            let text_of = |i: usize| -> String {
                doc.pages()
                    .get(i as u16)
                    .ok()
                    .and_then(|p| p.text().ok().map(|t| t.all()))
                    .unwrap_or_default()
            };
            let mut front = String::new();
            for i in (0..count.min(FRONT_PAGES))
                .chain(count.saturating_sub(BACK_PAGES).max(FRONT_PAGES)..count)
            {
                front.push_str(&text_of(i));
                front.push('\n');
            }
            let pdf_id = tail_of(path).and_then(|t| pdf_trailer_id(&t));
            let identity = Identity::build(
                hash,
                &RawFacts {
                    title: title.as_deref(),
                    first_author: author.as_deref(),
                    year: None,
                    front_text: &front,
                    pdf_id: pdf_id.as_deref(),
                    epub_id: None,
                },
            );
            Some(Probe {
                identity,
                title,
                authors: author.into_iter().collect(),
                date: None,
                publisher: None,
                isbn: None,
                front_text: front,
                is_epub: false,
            })
        }
        DocKind::Epub => {
            let meta = fond_doc::epub::extract_metadata(path).ok()?;
            let text = fond_doc::epub::extract_text(path).ok().unwrap_or_default();
            let front_text: String = text.chars().take(20_000).collect();
            let year = meta
                .date
                .as_deref()
                .map(|d| d.chars().take(4).collect::<String>());
            let mut identity = Identity::build(
                hash,
                &RawFacts {
                    title: meta.title.as_deref(),
                    first_author: meta.authors.first().map(String::as_str),
                    year: year.as_deref(),
                    front_text: &front_text,
                    pdf_id: None,
                    epub_id: None,
                },
            );
            if identity.isbn.is_none() {
                identity.isbn = meta.isbn.clone();
            }
            Some(Probe {
                identity,
                title: meta.title,
                authors: meta.authors,
                date: meta.date,
                publisher: meta.publisher,
                isbn: meta.isbn,
                front_text,
                is_epub: true,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pdf_is_fingerprinted_from_a_few_pages() {
        if crate::pdfium::get().is_err() {
            eprintln!("no PDFium library; skipping");
            return;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mixed.pdf");
        let p = probe(&path, DocKind::Pdf, "h").unwrap();
        assert_eq!(p.identity.hash, "h");
        assert!(p.front_text.contains("Page 1 of the mixed document"));
        assert!(!p.identity.shingle.is_empty());
    }

    /// `PP_PROBE=/path/to/file.pdf cargo test -p fond-read-gtk probe_timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_timing() {
        let Ok(path) = std::env::var("PP_PROBE") else {
            return;
        };
        let started = std::time::Instant::now();
        let p = probe(Path::new(&path), DocKind::Pdf, "h").expect("probe");
        eprintln!(
            "probe took {:?}; {} chars of front text",
            started.elapsed(),
            p.front_text.len()
        );
    }
}
