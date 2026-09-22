//! The plain-file [`fond_read_gtk::ReaderHost`] implementation: no vault, no library, no
//! note frontmatter — every document is identified only by its content hash, matching the
//! hash `fond-read-gtk`'s own open-reader dedup registry already keys on. Two files per
//! document under `~/.local/share/pereplyot/`:
//!
//! - `annotations/<hash>.json` — the [`fond_bib::AnnotationSidecar`], byte-compatible with
//!   what Kartoteka/Sputnik write, so a sidecar is portable between all three as long as
//!   the file's blob hash matches.
//! - `meta/<hash>.json` — `{ progress, page_label_override, bookmarks }`, the fields
//!   Kartoteka keeps on a note's frontmatter instead — a bare local file has no note to
//!   piggyback on.

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bookmarks: Vec<u32>,
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

    fn load_bookmarks(&self) -> Vec<u32> {
        LocalMeta::load(&self.hash).bookmarks
    }

    fn save_bookmarks(&self, pages: &[u32]) {
        let mut meta = LocalMeta::load(&self.hash);
        meta.bookmarks = pages.to_vec();
        meta.save(&self.hash);
    }
}

/// The saved progress/page for a document, if any — used to pick `show_pdf_reader`'s
/// `start_page` and `show_epub_reader`'s `start_progress` before the reader (and thus its
/// own `LocalReaderHost`) has been constructed.
pub fn saved_progress(hash: &str) -> Option<fond_bib::Progress> {
    LocalMeta::load(hash).progress
}

/// How to route a document's annotations/progress when Pereplyot was launched on another
/// app's behalf (Kartoteka, or Sputnik) instead of standalone — see `main.rs`'s CLI parsing
/// and `README.md`'s "Relationship to Kartoteka and Sputnik" section for the full story.
/// `None` (the default, plain `pereplyot <file>` invocation) means the usual
/// [`LocalReaderHost`], unchanged.
pub enum HostOverride {
    /// Kartoteka, or a Sputnik "library reading" — both resolve to the exact same
    /// `notes/<key>.md` + `annots/<key>.json` a `fond_bib::Library` opened on `root`
    /// already reads/writes today; `hash` is still needed for the (vault-unaware)
    /// bookmarks feature, which stays local regardless of override.
    Vault {
        root: PathBuf,
        key: String,
        hash: String,
    },
    /// A Sputnik course material — not vault-linked, so there's no `key`; the caller
    /// already knows the exact file each field belongs at (Sputnik's own
    /// `annots/<hash>.json` / `material-progress/<hash>.json`) and hands the paths
    /// straight over. `progress` is optional since a caller that doesn't track resume
    /// position at all can simply omit it.
    ExternalPaths {
        annotations: PathBuf,
        progress: Option<PathBuf>,
        hash: String,
    },
}

/// Build the right `ReaderHost` for this open — the plain local one for an ordinary
/// standalone open, or one of the two override variants when Pereplyot was launched on
/// another app's behalf. The only place that decides which storage a document's
/// annotations land in.
pub fn build_host(
    widgets: &Rc<Widgets>,
    hash: &str,
    override_: Option<HostOverride>,
) -> Result<Rc<dyn ReaderHost>, String> {
    match override_ {
        None => Ok(LocalReaderHost::for_document(widgets, hash)),
        Some(HostOverride::Vault { root, key, hash }) => {
            VaultReaderHost::build(widgets, root, key, hash)
        }
        Some(HostOverride::ExternalPaths {
            annotations,
            progress,
            hash,
        }) => Ok(ExternalPathReaderHost::build(
            widgets,
            annotations,
            progress,
            hash,
        )),
    }
}

/// The saved progress for a document opened under a [`HostOverride`], if any — the
/// override equivalent of [`saved_progress`], read before the reader (and thus its host)
/// exists. Kartoteka/Sputnik's own vault-linked "Read" already computes this the same way
/// today (loading the note first); this just does the same lookup from a bare vault root +
/// key, or a bare progress file path, with no in-memory vault/library object to reuse.
pub fn saved_progress_for_override(override_: &HostOverride) -> Option<fond_bib::Progress> {
    match override_ {
        HostOverride::Vault { root, key, .. } => {
            let library = fond_bib::Library::open(root).ok()?;
            library.load_note(key).ok().flatten()?.frontmatter.progress
        }
        HostOverride::ExternalPaths { progress, .. } => {
            let path = progress.as_ref()?;
            let text = fs::read_to_string(path).ok()?;
            serde_json::from_str(&text).ok()
        }
    }
}

/// Routes a document's annotations/progress/page-numbering to the exact vault note a
/// `fond_bib::Library` opened directly on `root` reads/writes — Kartoteka's own
/// `KartotekaReaderHost` (`kartoteka-ui-gtk/src/ui/app_window.rs`) does the identical
/// `load_note`/`write_note`/`load_annotations`/`write_annotations` dance; this is that same
/// shape, just running from a separate process with no in-memory `AppState` to reach into,
/// only the vault root and key Kartoteka/Sputnik hand over on the command line. Bookmarks
/// have no vault slot yet (a genuinely new, Pereplyot-only concept), so they still go
/// through the same content-hash-keyed local store `LocalReaderHost` uses — consistent
/// regardless of which app was used to open the file.
struct VaultReaderHost {
    widgets: Rc<Widgets>,
    library: fond_bib::Library,
    key: String,
    hash: String,
}

impl VaultReaderHost {
    fn build(
        widgets: &Rc<Widgets>,
        root: PathBuf,
        key: String,
        hash: String,
    ) -> Result<Rc<dyn ReaderHost>, String> {
        let library = fond_bib::Library::open(root).map_err(|e| e.to_string())?;
        Ok(Rc::new(VaultReaderHost {
            widgets: widgets.clone(),
            library,
            key,
            hash,
        }))
    }

    /// Read-modify-write the note's frontmatter — the only two fields the reader touches,
    /// same as Kartoteka's own `KartotekaReaderHost::edit_note`. A missing note is created
    /// fresh (`Note::default()`) rather than silently dropping the write, matching
    /// Sputnik's `LibraryHandle::save_progress`'s `unwrap_or_default()`.
    fn edit_note(&self, f: impl FnOnce(&mut fond_bib::Note)) {
        if let Ok(mut note) = self
            .library
            .load_note(&self.key)
            .map(|n| n.unwrap_or_default())
        {
            f(&mut note);
            let _ = self.library.write_note(&self.key, &note);
        }
    }
}

impl ReaderHost for VaultReaderHost {
    fn load_annotations(&self) -> fond_bib::AnnotationSidecar {
        self.library
            .load_annotations(&self.key)
            .ok()
            .flatten()
            .unwrap_or_else(|| fond_bib::AnnotationSidecar::new(&self.key))
    }

    fn save_annotations(&self, sidecar: &fond_bib::AnnotationSidecar) -> Result<(), String> {
        self.library
            .write_annotations(sidecar)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn save_progress(&self, progress: fond_bib::Progress) {
        self.edit_note(|note| note.frontmatter.progress = Some(progress));
    }

    fn page_label_override(&self) -> Option<fond_bib::PageLabelOverride> {
        self.library
            .load_note(&self.key)
            .ok()
            .flatten()
            .and_then(|note| note.frontmatter.page_label_override)
    }

    fn set_page_label_override(&self, value: Option<fond_bib::PageLabelOverride>) {
        self.edit_note(|note| note.frontmatter.page_label_override = value);
    }

    fn notify(&self, message: &str) {
        toast(&self.widgets, message);
    }

    fn load_bookmarks(&self) -> Vec<u32> {
        LocalMeta::load(&self.hash).bookmarks
    }

    fn save_bookmarks(&self, pages: &[u32]) {
        let mut meta = LocalMeta::load(&self.hash);
        meta.bookmarks = pages.to_vec();
        meta.save(&self.hash);
    }
}

/// Routes annotations/progress to exact file paths the caller (Sputnik, for a course
/// material) hands over directly, with no vault/key concept at all — a plain JSON
/// read/write at whatever path Sputnik's own `sputnik_core::Vault` already uses for that
/// hash (`annots/<hash>.json`, `material-progress/<hash>.json`), so a save shows up exactly
/// where Sputnik itself would have put it. Deliberately doesn't link `sputnik_core` to do
/// this — Sputnik's vault wrapper also git-commits each write
/// (`Vault::write_and_commit`), which a plain file write here can't replicate; the commit
/// catches up next time Sputnik itself opens that vault and finds the change. Not a
/// data-loss risk, just a commit-timing gap accepted as part of this design (see the repo's
/// handoff plan/CHANGELOG entry). Page-numbering override has no on-disk home in Sputnik's
/// material data model either way — `MaterialReaderHost` leaves it a no-op today, so this
/// does too.
struct ExternalPathReaderHost {
    widgets: Rc<Widgets>,
    annotations_path: PathBuf,
    progress_path: Option<PathBuf>,
    hash: String,
}

impl ExternalPathReaderHost {
    fn build(
        widgets: &Rc<Widgets>,
        annotations_path: PathBuf,
        progress_path: Option<PathBuf>,
        hash: String,
    ) -> Rc<dyn ReaderHost> {
        Rc::new(ExternalPathReaderHost {
            widgets: widgets.clone(),
            annotations_path,
            progress_path,
            hash,
        })
    }
}

impl ReaderHost for ExternalPathReaderHost {
    fn load_annotations(&self) -> fond_bib::AnnotationSidecar {
        fs::read_to_string(&self.annotations_path)
            .ok()
            .and_then(|text| fond_bib::AnnotationSidecar::parse(&text, &self.annotations_path).ok())
            .unwrap_or_else(|| fond_bib::AnnotationSidecar::new(&self.hash))
    }

    fn save_annotations(&self, sidecar: &fond_bib::AnnotationSidecar) -> Result<(), String> {
        if let Some(dir) = self.annotations_path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = sidecar.to_json().map_err(|e| e.to_string())?;
        fs::write(&self.annotations_path, json).map_err(|e| e.to_string())
    }

    fn save_progress(&self, progress: fond_bib::Progress) {
        let Some(path) = &self.progress_path else {
            return;
        };
        if let Some(dir) = path.parent() {
            if fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        if let Ok(json) = serde_json::to_string_pretty(&progress) {
            let _ = fs::write(path, json);
        }
    }

    fn page_label_override(&self) -> Option<fond_bib::PageLabelOverride> {
        None
    }

    fn set_page_label_override(&self, _value: Option<fond_bib::PageLabelOverride>) {}

    fn notify(&self, message: &str) {
        toast(&self.widgets, message);
    }

    fn load_bookmarks(&self) -> Vec<u32> {
        LocalMeta::load(&self.hash).bookmarks
    }

    fn save_bookmarks(&self, pages: &[u32]) {
        let mut meta = LocalMeta::load(&self.hash);
        meta.bookmarks = pages.to_vec();
        meta.save(&self.hash);
    }
}
