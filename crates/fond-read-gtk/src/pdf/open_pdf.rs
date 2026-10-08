use super::*;

pub(super) struct OpenedPdf {
    pub(super) reader: Rc<RefCell<ReaderState>>,
    pub(super) outline_entries: Vec<fond_doc::PdfOutlineEntry>,
    pub(super) has_native_page_labels: bool,
    pub(super) start_page: u16,
}

/// Read and parse the PDF, work out its page labels and outline, and build the reader state.
/// Shows an error dialog and returns `None` if the file can't be opened.
pub(super) fn open_pdf(
    host: &Rc<dyn ReaderHost>,
    window: &adw::ApplicationWindow,
    pdf_hash: &str,
    blob: &std::path::Path,
    start_page: u32,
) -> Option<OpenedPdf> {
    let bytes = match std::fs::read(blob) {
        Ok(b) => b,
        Err(e) => {
            gtk4::AlertDialog::builder()
                .message("Could not open PDF")
                .detail(e.to_string())
                .build()
                .show(Some(window));
            return None;
        }
    };
    let pdfium = match crate::pdfium::get() {
        Ok(p) => p,
        Err(e) => {
            gtk4::AlertDialog::builder()
                .message("PDF reader unavailable")
                .detail(format!("PDFium could not be loaded: {e}"))
                .build()
                .show(Some(window));
            return None;
        }
    };
    let count = fond_doc::page_count(pdfium, &bytes).unwrap_or(1).max(1);
    // Empty for most PDFs — outlines are the exception, not the rule — so the Contents
    // button below only appears when there's actually something to jump to.
    let outline_entries = fond_doc::outline(pdfium, &bytes).unwrap_or_default();
    // Likewise empty for most PDFs (no custom /PageLabels) — falls back to the raw page
    // number wherever it's displayed. When the PDF declares nothing of its own, fall back to
    // a manually-set `page_label_override` on the entry's note (see "Set page numbering…"
    // below) — this is the only way to get printed-page-number navigation on the common case
    // of a scanned or older PDF with no `/PageLabels` dictionary at all.
    let native_page_labels = fond_doc::page_labels(pdfium, &bytes).unwrap_or_default();
    let has_native_page_labels = native_page_labels.iter().any(|l| l.is_some());
    let page_label_override = host.page_label_override();
    let page_labels = if has_native_page_labels {
        native_page_labels
    } else {
        page_label_override
            .map(|ov| ov.apply(count))
            .unwrap_or(native_page_labels)
    };

    let store = AnnotationStore::new(host, Some(pdf_hash.to_string()));
    let mut bookmarks = host.load_bookmarks();
    bookmarks.sort_unstable();

    let start_page = start_page
        .saturating_sub(1)
        .min(count.saturating_sub(1) as u32) as u16;
    let doc = pdfium.load_pdf_from_byte_vec(bytes.clone(), None).ok();
    let reader = Rc::new(RefCell::new(ReaderState {
        pdfium,
        bytes,
        doc,
        page: start_page,
        count,
        zoom: 1.0,
        store,
        render_px: (0, 0),
        page_geoms: RefCell::new(std::collections::HashMap::new()),
        draw_kind: None,
        last_selection: None,
        search_matches: Vec::new(),
        search_current: 0,
        draw_color: crate::palette::HIGHLIGHT_COLORS[0].hex.to_string(),
        continuous_pictures: Vec::new(),
        continuous_offsets: Vec::new(),
        continuous_rendered: Vec::new(),
        continuous_window: (0, 0),
        link_goto: None,
        nav_back: Vec::new(),
        text_goto: None,
        text_zoom: None,
        page_labels,
        rotation: 0,
        invert_colors: false,
        textures: TextureCache::new(96 * 1024 * 1024),
        worker: Some(RenderWorker::spawn(blob.to_path_buf())),
        bookmarks,
    }));
    Some(OpenedPdf {
        reader,
        outline_entries,
        has_native_page_labels,
        start_page,
    })
}
