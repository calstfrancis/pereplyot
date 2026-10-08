use super::*;

/// Return this reader's per-chapter plain-text search index, building and caching it on
/// first use by reading each `spine` chapter straight from `cache_dir` (already extracted
/// when the reader opened) and stripping tags via `fond_doc::strip_epub_tags` — the same
/// tag-stripping `fond_doc::extract_epub_text` uses for the library-wide search index, just
/// kept per-chapter here instead of joined into one string, so a match can be attributed to
/// a chapter to jump to.
pub(super) fn epub_chapter_texts(state: &Rc<RefCell<EpubReaderState>>) -> Vec<String> {
    if let Some(texts) = &state.borrow().chapter_texts {
        return texts.clone();
    }
    let (cache_dir, spine) = {
        let r = state.borrow();
        (r.cache_dir.clone(), r.spine.clone())
    };
    let texts: Vec<String> = spine
        .iter()
        .map(|chapter| {
            std::fs::read_to_string(cache_dir.join(chapter))
                .map(|xml| fond_doc::strip_epub_tags(&xml))
                .unwrap_or_default()
        })
        .collect();
    state.borrow_mut().chapter_texts = Some(texts.clone());
    texts
}

/// One whole-book search hit: which chapter (`spine` index) it's in, and a short excerpt of
/// surrounding context with the match itself in the returned range (`match_start`/`match_end`,
/// byte offsets into `snippet`) so the results list can bold it.
pub(super) struct EpubWholeBookMatch {
    pub(super) chapter: usize,
    pub(super) snippet: String,
    pub(super) match_start: usize,
    pub(super) match_end: usize,
}

/// Search every chapter's plain-text index for `query` (case-insensitive substring), capped
/// at `EPUB_WHOLE_BOOK_MATCH_LIMIT` total hits so a common word in a long book doesn't build
/// an unbounded results list. Each hit carries ~50 characters of context on each side of the
/// match, trimmed to whitespace boundaries where possible so excerpts don't start/end mid-word.
pub(super) const EPUB_WHOLE_BOOK_MATCH_LIMIT: usize = 200;

pub(super) fn epub_search_whole_book(
    state: &Rc<RefCell<EpubReaderState>>,
    query: &str,
) -> Vec<EpubWholeBookMatch> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let texts = epub_chapter_texts(state);
    let needle = query.to_lowercase();
    let mut results = Vec::new();
    'chapters: for (chapter, text) in texts.iter().enumerate() {
        let haystack = text.to_lowercase();
        let mut search_from = 0;
        while let Some(rel_idx) = haystack[search_from..].find(&needle) {
            let idx = search_from + rel_idx;
            let end = idx + needle.len();
            let ctx_start = text[..idx]
                .char_indices()
                .rev()
                .take(50)
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            let ctx_end = text[end..]
                .char_indices()
                .nth(50)
                .map(|(i, _)| end + i)
                .unwrap_or(text.len());
            results.push(EpubWholeBookMatch {
                chapter,
                snippet: text[ctx_start..ctx_end].to_string(),
                match_start: idx - ctx_start,
                match_end: end - ctx_start,
            });
            if results.len() >= EPUB_WHOLE_BOOK_MATCH_LIMIT {
                break 'chapters;
            }
            search_from = end;
        }
    }
    results
}
