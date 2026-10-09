//! Printed page numbers for an EPUB: the book's own page list (EPUB 3 `page-list` navigation or
//! an NCX `pageList`), or failing that the page-break markers placed in the text. This is what
//! makes a quotation from an e-book citable by the page of the print edition.

use std::path::{Component, Path, PathBuf};

/// A place in a chapter where a printed page begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PageBreak {
    /// The chapter file, as a path relative to the extraction directory (as `spine` has it).
    pub chapter: String,
    /// The id of the element in the chapter that marks the break.
    pub id: String,
    /// The printed page: "xii", "214".
    pub label: String,
}

fn tags(text: &str) -> impl Iterator<Item = (&str, usize)> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let start = text[at..].find('<')? + at;
        let end = text[start..].find('>')? + start + 1;
        at = end;
        Some((&text[start..end], start))
    })
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut from = 0;
    while let Some(found) = tag[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before_ok = at > 0 && bytes[at - 1].is_ascii_whitespace();
        let rest = tag[at + name.len()..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let rest = rest[1..].trim_start();
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let inner = &rest[1..];
        return inner.find(quote).map(|e| unescape(&inner[..e]));
    }
    None
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `href` (which may end in `#fragment`) resolved against the directory `base_dir` of the file
/// that holds it, as a zip-style path plus the fragment.
fn resolve(base_dir: &Path, href: &str) -> (String, Option<String>) {
    let (file, fragment) = href
        .split_once('#')
        .map_or((href, None), |(f, g)| (f, Some(g.to_string())));
    let mut path = PathBuf::new();
    for c in base_dir.join(percent_decode(file)).components() {
        match c {
            Component::ParentDir => {
                path.pop();
            }
            Component::Normal(p) => path.push(p),
            _ => {}
        }
    }
    (path.to_string_lossy().replace('\\', "/"), fragment)
}

fn inner_text(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    unescape(out.split_whitespace().collect::<Vec<_>>().join(" ").trim())
}

struct Item {
    href: String,
    properties: String,
    media_type: String,
}

fn manifest(opf: &str) -> Vec<Item> {
    tags(opf)
        .filter(|(t, _)| t.starts_with("<item ") || t.starts_with("<item\n"))
        .filter_map(|(t, _)| {
            Some(Item {
                href: attr(t, "href")?,
                properties: attr(t, "properties").unwrap_or_default(),
                media_type: attr(t, "media-type").unwrap_or_default(),
            })
        })
        .collect()
}

fn page_list_from_nav(
    nav: &str,
    nav_dir: &Path,
    opf_to_cache: &dyn Fn(&str) -> String,
) -> Vec<PageBreak> {
    let Some(start) = tags(nav)
        .find(|(t, _)| t.starts_with("<nav") && t.contains("page-list"))
        .map(|(_, at)| at)
    else {
        return Vec::new();
    };
    let end = nav[start..].find("</nav>").map_or(nav.len(), |e| start + e);
    let region = &nav[start..end];
    let mut out = Vec::new();
    for (tag, at) in tags(region) {
        if !tag.starts_with("<a ") && !tag.starts_with("<a\n") {
            continue;
        }
        let Some(href) = attr(tag, "href") else {
            continue;
        };
        let after = &region[at + tag.len()..];
        let label = inner_text(&after[..after.find("</a>").unwrap_or(after.len())]);
        let (file, fragment) = resolve(nav_dir, &href);
        if let (Some(id), false) = (fragment, label.is_empty()) {
            out.push(PageBreak {
                chapter: opf_to_cache(&file),
                id,
                label,
            });
        }
    }
    out
}

fn page_list_from_ncx(
    ncx: &str,
    ncx_dir: &Path,
    to_cache: &dyn Fn(&str) -> String,
) -> Vec<PageBreak> {
    let Some(start) = ncx.find("<pageList") else {
        return Vec::new();
    };
    let end = ncx[start..]
        .find("</pageList>")
        .map_or(ncx.len(), |e| start + e);
    let region = &ncx[start..end];
    let mut out = Vec::new();
    let mut pending: Option<String> = None;
    for (tag, at) in tags(region) {
        if tag.starts_with("<pageTarget") {
            pending = attr(tag, "value");
        } else if tag.starts_with("<text") {
            let after = &region[at + tag.len()..];
            let text = inner_text(&after[..after.find("</text>").unwrap_or(after.len())]);
            if !text.is_empty() {
                pending = Some(text);
            }
        } else if tag.starts_with("<content") {
            if let (Some(src), Some(label)) = (attr(tag, "src"), pending.take()) {
                let (file, fragment) = resolve(ncx_dir, &src);
                if let Some(id) = fragment {
                    out.push(PageBreak {
                        chapter: to_cache(&file),
                        id,
                        label,
                    });
                }
            }
        }
    }
    out
}

/// Page-break markers placed in the chapters' own text.
fn page_breaks_in_text(cache_dir: &Path, spine: &[String]) -> Vec<PageBreak> {
    let mut out = Vec::new();
    for chapter in spine {
        let Ok(text) = std::fs::read_to_string(cache_dir.join(chapter)) else {
            continue;
        };
        for (tag, at) in tags(&text) {
            let marked = attr(tag, "epub:type").is_some_and(|t| t.contains("pagebreak"))
                || attr(tag, "role").is_some_and(|t| t.contains("pagebreak"));
            if !marked {
                continue;
            }
            let Some(id) = attr(tag, "id") else { continue };
            let label = attr(tag, "title")
                .or_else(|| attr(tag, "aria-label"))
                .filter(|l| !l.trim().is_empty())
                .or_else(|| {
                    let after = &text[at + tag.len()..];
                    let inner = inner_text(&after[..after.find('<').unwrap_or(0)]);
                    (!inner.is_empty()).then_some(inner)
                });
            if let Some(label) = label {
                out.push(PageBreak {
                    chapter: chapter.clone(),
                    id,
                    label: label.trim().to_string(),
                });
            }
        }
    }
    out
}

/// The book's printed pages, in reading order. Empty if it has none.
pub(crate) fn read(cache_dir: &Path, spine: &[String]) -> Vec<PageBreak> {
    let from_lists = (|| {
        let container = std::fs::read_to_string(cache_dir.join("META-INF/container.xml")).ok()?;
        let opf_path = tags(&container)
            .find(|(t, _)| t.starts_with("<rootfile"))
            .and_then(|(t, _)| attr(t, "full-path"))?;
        let opf = std::fs::read_to_string(cache_dir.join(&opf_path)).ok()?;
        let opf_dir = Path::new(&opf_path)
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        let items = manifest(&opf);
        let in_spine = |file: &str| -> String { file.to_string() };
        let mut found = Vec::new();
        if let Some(nav) = items
            .iter()
            .find(|i| i.properties.split_whitespace().any(|p| p == "nav"))
        {
            let (nav_path, _) = resolve(&opf_dir, &nav.href);
            if let Ok(text) = std::fs::read_to_string(cache_dir.join(&nav_path)) {
                let dir = Path::new(&nav_path)
                    .parent()
                    .unwrap_or(Path::new(""))
                    .to_path_buf();
                found = page_list_from_nav(&text, &dir, &in_spine);
            }
        }
        if found.is_empty() {
            if let Some(ncx) = items.iter().find(|i| i.media_type.contains("dtbncx")) {
                let (ncx_path, _) = resolve(&opf_dir, &ncx.href);
                if let Ok(text) = std::fs::read_to_string(cache_dir.join(&ncx_path)) {
                    let dir = Path::new(&ncx_path)
                        .parent()
                        .unwrap_or(Path::new(""))
                        .to_path_buf();
                    found = page_list_from_ncx(&text, &dir, &in_spine);
                }
            }
        }
        Some(found)
    })()
    .unwrap_or_default();
    let found: Vec<PageBreak> = from_lists
        .into_iter()
        .filter(|p| spine.contains(&p.chapter))
        .collect();
    if found.is_empty() {
        page_breaks_in_text(cache_dir, spine)
    } else {
        found
    }
}

/// The page breaks inside one chapter, in order.
pub(crate) fn in_chapter<'a>(pages: &'a [PageBreak], chapter: &str) -> Vec<&'a PageBreak> {
    pages.iter().filter(|p| p.chapter == chapter).collect()
}

/// The printed page in force at the very start of `chapter`: that of the last break in an
/// earlier chapter.
pub(crate) fn label_entering(
    pages: &[PageBreak],
    spine: &[String],
    chapter: &str,
) -> Option<String> {
    let at = spine.iter().position(|c| c == chapter)?;
    pages
        .iter()
        .rev()
        .find(|p| {
            spine
                .iter()
                .position(|c| *c == p.chapter)
                .is_some_and(|i| i < at)
        })
        .map(|p| p.label.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extracted(name: &str) -> (PathBuf, Vec<String>) {
        let dir =
            std::env::temp_dir().join(format!("pereplyot-pages-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let epub =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/{name}.epub"));
        fond_doc::epub::extract_all(&epub, &dir).unwrap();
        let book = fond_doc::epub::open_book(&epub).unwrap();
        (dir, book.spine)
    }

    #[test]
    fn the_page_list_of_a_publisher_epub_is_read_in_order() {
        let (dir, spine) = extracted("scholar");
        let pages = read(&dir, &spine);
        let labels: Vec<_> = pages.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["41", "42", "43", "44", "45"]);
        assert_eq!(pages[0].chapter, "OEBPS/c1.xhtml");
        assert_eq!(pages[0].id, "pg41");
        assert_eq!(pages[3].chapter, "OEBPS/c2.xhtml");
        assert_eq!(in_chapter(&pages, "OEBPS/c1.xhtml").len(), 3);
        assert_eq!(
            label_entering(&pages, &spine, "OEBPS/c2.xhtml").as_deref(),
            Some("43")
        );
        assert_eq!(label_entering(&pages, &spine, "OEBPS/c1.xhtml"), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_book_without_a_page_list_has_none() {
        let (dir, spine) = extracted("book");
        assert!(read(&dir, &spine).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn page_breaks_in_the_text_stand_in_when_there_is_no_list() {
        let dir = std::env::temp_dir().join(format!("pereplyot-pages-text-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.xhtml"),
            r#"<body><span epub:type="pagebreak" id="p1" title="xii"/><p>x</p><span role="doc-pagebreak" id="p2" aria-label="xiii"/></body>"#,
        )
        .unwrap();
        let pages = read(&dir, &["a.xhtml".to_string()]);
        assert_eq!(
            pages
                .iter()
                .map(|p| (p.id.as_str(), p.label.as_str()))
                .collect::<Vec<_>>(),
            [("p1", "xii"), ("p2", "xiii")]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_ncx_page_list_is_read_and_paths_resolve_against_the_file_that_holds_them() {
        let ncx = r#"<ncx><pageList><pageTarget id="a" value="7"><navLabel><text>7</text></navLabel><content src="../Text/c%201.xhtml#p7"/></pageTarget></pageList></ncx>"#;
        let pages = page_list_from_ncx(ncx, Path::new("OEBPS/Nav"), &|f| f.to_string());
        assert_eq!(
            pages,
            [PageBreak {
                chapter: "OEBPS/Text/c 1.xhtml".into(),
                id: "p7".into(),
                label: "7".into()
            }]
        );
    }
}
