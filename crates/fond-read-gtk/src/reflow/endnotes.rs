//! Notes gathered at the back of a book, in a list per chapter, and the pages that hold them.

use std::collections::HashMap;

use super::layout::{is_page_number, lines_of, Line};
use super::model::RawPage;

/// One chapter's (or section's) notes, by label.
#[derive(Debug, Default, Clone)]
pub struct Group {
    pub title: String,
    pub entries: HashMap<String, String>,
}

#[derive(Debug, Default, Clone)]
pub struct EndNotes {
    /// The pages of the notes section, which are left out of the flow.
    pub pages: std::ops::Range<u16>,
    pub groups: Vec<Group>,
}

/// A title as plain lower-case words, so a heading and a notes list can be compared.
pub fn plain_title(text: &str) -> String {
    let mut words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if words.first().is_some_and(|w| w == "chapter") {
        words.remove(0);
        if words
            .first()
            .is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()) || is_numeral(w))
        {
            words.remove(0);
        }
    }
    words.join(" ")
}

fn is_numeral(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| "ivxlcdm".contains(c))
}

impl EndNotes {
    pub fn group_for(&self, heading: &str) -> Option<usize> {
        let title = plain_title(heading);
        if title.is_empty() {
            return None;
        }
        self.groups.iter().position(|g| {
            !g.title.is_empty()
                && (g.title == title
                    || (title.len() >= 12
                        && (g.title.contains(&title) || title.contains(&g.title))))
        })
    }

    pub fn entry_count(&self) -> usize {
        self.groups.iter().map(|g| g.entries.len()).sum()
    }
}

/// The first few lines near the top of a page, as plain titles.
fn headings_of(page: &RawPage) -> Vec<String> {
    lines_of(&page.words)
        .into_iter()
        .filter(|l| l.y1 < 0.35 * page.height && !is_page_number(&l.text()))
        .take(3)
        .map(|l| plain_title(&l.text()))
        .collect()
}

const BACK_MATTER: [&str; 8] = [
    "bibliography",
    "selected bibliography",
    "works cited",
    "references",
    "index",
    "acknowledgments",
    "about the author",
    "glossary",
];

/// Find the notes section among `pages` (given as `(page index, page)`) and read it: it starts at
/// a page headed "Notes" in the second half of the book and runs to the next back-matter heading.
pub fn find(total: u16, page_at: &dyn Fn(u16) -> Option<RawPage>) -> Option<EndNotes> {
    let mut start = None;
    for i in (total / 2)..total {
        let Some(page) = page_at(i) else { continue };
        if headings_of(&page)
            .iter()
            .any(|h| h == "notes" || h == "endnotes")
        {
            start = Some(i);
            break;
        }
    }
    let start = start?;
    let mut raw: Vec<RawPage> = Vec::new();
    let mut end = total;
    for i in start..total {
        let Some(page) = page_at(i) else { continue };
        if i > start
            && headings_of(&page)
                .iter()
                .any(|h| BACK_MATTER.contains(&h.as_str()))
        {
            end = i;
            break;
        }
        raw.push(page);
    }
    let groups = parse(&raw);
    let found = EndNotes {
        pages: start..end,
        groups,
    };
    (found.entry_count() >= 5).then_some(found)
}

fn leading_number(line: &Line) -> Option<(String, usize)> {
    let first = line.words.first()?;
    let t = first.text.trim_end_matches(['.', ')']);
    (line.words.len() >= 2
        && !t.is_empty()
        && t.len() <= 3
        && t.chars().all(|c| c.is_ascii_digit()))
    .then(|| (t.to_string(), 1))
}

fn join(into: &mut String, more: &str) {
    if into.ends_with('-') && more.chars().next().is_some_and(|c| c.is_lowercase()) {
        into.pop();
    } else if !into.is_empty() {
        into.push(' ');
    }
    into.push_str(more);
}

fn parse(pages: &[RawPage]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut current: Option<String> = None;
    let mut last = 0u32;
    for page in pages {
        let lines: Vec<Line> = lines_of(&page.words)
            .into_iter()
            .filter(|l| l.y1 >= 0.1 * page.height && l.y0 <= 0.92 * page.height)
            .collect();
        let mut lefts: Vec<f32> = lines.iter().map(|l| l.x0).collect();
        lefts.sort_by(f32::total_cmp);
        let left = lefts.get(lefts.len() / 10).copied().unwrap_or(0.0);
        for line in lines {
            let text = line.text();
            let lower = text.trim().to_lowercase();
            if lower == "notes" || lower == "endnotes" {
                continue;
            }
            if lower.starts_with("chapter ") && line.words.len() <= 14 {
                groups.push(Group {
                    title: plain_title(&text),
                    entries: HashMap::new(),
                });
                current = None;
                last = 0;
                continue;
            }
            if groups.is_empty() {
                groups.push(Group::default());
            }
            let group = groups.last_mut().expect("a group");
            if let Some((label, skip)) = leading_number(&line) {
                let n: u32 = label.parse().unwrap_or(0);
                if n > last && n <= last + 40 && line.x0 <= left + 0.03 * page.width {
                    last = n;
                    let body: Vec<&str> = line
                        .words
                        .iter()
                        .skip(skip)
                        .map(|w| w.text.as_str())
                        .collect();
                    group.entries.insert(label.clone(), body.join(" "));
                    current = Some(label);
                    continue;
                }
            }
            if let Some(label) = &current {
                if let Some(entry) = group.entries.get_mut(label) {
                    join(entry, &text);
                }
            }
        }
    }
    groups.retain(|g| !g.entries.is_empty());
    groups
}
