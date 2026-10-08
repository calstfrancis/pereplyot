use super::*;

/// The widgets and shared handles the reader's wiring functions work on. Every field is a
/// cheap reference-counted handle, so a function destructures what it needs and clones from it.
pub(super) struct EpubUi {
    pub(super) reader: Rc<RefCell<EpubReaderState>>,
    pub(super) web_view: webkit6::WebView,
    pub(super) prev: gtk4::Button,
    pub(super) next: gtk4::Button,
    pub(super) chapter_label: gtk4::Label,
    pub(super) bookmark_button: gtk4::Button,
    pub(super) contents_scroll: gtk4::ScrolledWindow,
    pub(super) notes_scroll: gtk4::ScrolledWindow,
    pub(super) notes_toggle: gtk4::ToggleButton,
    pub(super) notes_paned: gtk4::Paned,
    pub(super) paned: gtk4::Paned,
    pub(super) sidebar_toggle: gtk4::ToggleButton,
    pub(super) pending_search: Rc<RefCell<Option<String>>>,
    pub(super) reader_tab: crate::reader_host::ReaderTab,
    pub(super) rebuild_notes: Rc<dyn Fn()>,
    pub(super) results_list: gtk4::ListBox,
    pub(super) results_revealer: gtk4::Revealer,
    pub(super) search_bar_entry: gtk4::SearchEntry,
    pub(super) search_count: gtk4::Label,
    pub(super) search_next: gtk4::Button,
    pub(super) search_prev: gtk4::Button,
    pub(super) search_revealer: gtk4::Revealer,
    pub(super) search_toggle: gtk4::ToggleButton,
    pub(super) whole_book_toggle: gtk4::ToggleButton,
    pub(super) zoom_in_button: gtk4::Button,
    pub(super) zoom_out_button: gtk4::Button,
    pub(super) epub_undo: Rc<dyn Fn()>,
    pub(super) epub_redo: Rc<dyn Fn()>,
}
