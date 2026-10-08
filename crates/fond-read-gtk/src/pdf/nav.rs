use super::*;

/// Refresh the page-number entry/label/prev-next sensitivity for `page` (0-based) — shared
/// by the paged view's `render()` and continuous mode's scroll-position tracker, so the two
/// can't disagree about how the current page is displayed. Shows the document's own printed
/// label when the PDF defines one (`page_labels[page]`), falling back to the raw 1-based
/// file position otherwise — identical to the pre-`/PageLabels`-aware behaviour for the
/// common case of a PDF with no custom numbering.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_page_display(
    page_entry: &gtk4::Entry,
    page_of_label: &gtk4::Label,
    prev: &gtk4::Button,
    next: &gtk4::Button,
    bookmark_button: &gtk4::Button,
    page: u16,
    count: u16,
    page_labels: &[Option<String>],
    bookmarks: &[u32],
) {
    let raw = (page + 1).to_string();
    let label = page_labels
        .get(page as usize)
        .and_then(|l| l.clone())
        .unwrap_or_else(|| raw.clone());
    page_entry.set_text(&label);
    // Reading-progress indicator: the raw file position already says "how far in", but the
    // percentage reads at a glance without doing the division yourself.
    let percent = if count > 0 {
        ((page as u32 + 1) * 100 / count as u32).min(100)
    } else {
        0
    };
    page_of_label.set_text(&format!("of {count} · {percent}%"));
    // The tooltip always gives the raw file position too — a PDF's `/PageLabels` isn't
    // required to be unique or even present on every page, but the raw number is the one
    // every internal API here (`Annotation.page`, `PdfSearchMatch.page`, `Contents` targets)
    // always means, so it's worth surfacing even when the printed label differs.
    page_entry.set_tooltip_text(Some(&format!("Page {raw} of {count} in the file")));
    prev.set_sensitive(page > 0);
    next.set_sensitive(page + 1 < count);
    update_bookmark_button(bookmark_button, bookmarks.contains(&(page as u32 + 1)));
}

/// Resolve typed text in the page-number entry to a 0-based page index: an exact match
/// against the document's own printed labels first (case-insensitive, since a roman numeral
/// typed in the "wrong" case should still work), falling back to parsing it as a raw 1-based
/// file page number — so typing still works exactly as before on a PDF with no
/// `/PageLabels`, and a user who prefers raw numbers can always use them even on one that
/// has them.
pub(super) fn find_page_by_label(page_labels: &[Option<String>], text: &str) -> Option<u16> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(idx) = page_labels.iter().position(|l| l.as_deref() == Some(text)) {
        return Some(idx as u16);
    }
    let lower = text.to_lowercase();
    if let Some(idx) = page_labels
        .iter()
        .position(|l| l.as_deref().map(|s| s.to_lowercase()) == Some(lower.clone()))
    {
        return Some(idx as u16);
    }
    text.parse::<u16>().ok().and_then(|n| n.checked_sub(1))
}
