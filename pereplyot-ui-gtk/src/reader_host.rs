//! The plain-file [`fond_read_gtk::ReaderHost`] implementation: no vault, no library, no
//! note frontmatter — every document is identified only by its content hash, matching the
//! hash `fond-read-gtk`'s own open-reader dedup registry already keys on. Two files per
//! document under `~/.local/share/pereplyot/`:
//!
//! - `annotations/<hash>.json` — the [`fond_bib::AnnotationSidecar`], byte-compatible with
//!   what Kartoteka/Sputnik write, so a sidecar is portable between all three as long as
//!   the file's blob hash matches.
//! - `meta/<hash>.json` — `{ progress, page_label_override }`, the two fields Kartoteka
//!   keeps on a note's frontmatter instead — a bare local file has no note to piggyback on.

use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use fond_read_gtk::ReaderHost;
use serde::{Deserialize, Serialize};

use crate::ui::{toast, Widgets};

fn data_dir() -> PathBuf {
    glib::user_data_dir().join("pereplyot")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LocalMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    progress: Option<fond_bib::Progress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    page_label_override: Option<fond_bib::PageLabelOverride>,
}

impl LocalMeta {
    fn path(hash: &str) -> PathBuf {
        data_dir().join("meta").join(format!("{hash}.json"))
    }

    fn load(hash: &str) -> LocalMeta {
        fs::read_to_string(Self::path(hash))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, hash: &str) {
        let path = Self::path(hash);
        if let Some(dir) = path.parent() {
            if fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }
}

pub struct LocalReaderHost {
    widgets: Rc<Widgets>,
    hash: String,
}

impl LocalReaderHost {
    pub fn for_document(widgets: &Rc<Widgets>, hash: &str) -> Rc<dyn ReaderHost> {
        Rc::new(LocalReaderHost {
            widgets: widgets.clone(),
            hash: hash.to_string(),
        })
    }

    fn annotations_path(&self) -> PathBuf {
        data_dir()
            .join("annotations")
            .join(format!("{}.json", self.hash))
    }
}

impl ReaderHost for LocalReaderHost {
    fn load_annotations(&self) -> fond_bib::AnnotationSidecar {
        let path = self.annotations_path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|text| fond_bib::AnnotationSidecar::parse(&text, &path).ok())
            .unwrap_or_else(|| fond_bib::AnnotationSidecar::new(&self.hash))
    }

    fn save_annotations(&self, sidecar: &fond_bib::AnnotationSidecar) -> Result<(), String> {
        let path = self.annotations_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = sidecar.to_json().map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())
    }

    fn save_progress(&self, progress: fond_bib::Progress) {
        let mut meta = LocalMeta::load(&self.hash);
        meta.progress = Some(progress);
        meta.save(&self.hash);
    }

    fn page_label_override(&self) -> Option<fond_bib::PageLabelOverride> {
        LocalMeta::load(&self.hash).page_label_override
    }

    fn set_page_label_override(&self, value: Option<fond_bib::PageLabelOverride>) {
        let mut meta = LocalMeta::load(&self.hash);
        meta.page_label_override = value;
        meta.save(&self.hash);
    }

    fn notify(&self, message: &str) {
        toast(&self.widgets, message);
    }
}

/// The saved progress/page for a document, if any — used to pick `show_pdf_reader`'s
/// `start_page` and `show_epub_reader`'s `start_progress` before the reader (and thus its
/// own `LocalReaderHost`) has been constructed.
pub fn saved_progress(hash: &str) -> Option<fond_bib::Progress> {
    LocalMeta::load(hash).progress
}
