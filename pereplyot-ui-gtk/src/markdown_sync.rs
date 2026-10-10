use std::path::{Path, PathBuf};

use fond_annot::{Annotation, AnnotationKind, AnnotationSidecar};

pub struct Document<'a> {
    pub title: &'a str,
    pub hash: &'a str,
    pub citation_key: Option<&'a str>,
    pub source: &'a Path,
}

pub fn block_id(annotation_id: &str) -> String {
    let id: String = annotation_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("annot-{id}")
}

fn file_stem(title: &str, hash: &str) -> String {
    let clean: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']' => ' ',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let clean: String = clean.chars().take(80).collect();
    if clean.is_empty() {
        hash.chars().take(12).collect()
    } else {
        clean
    }
}

pub fn path_for(folder: &Path, title: &str, hash: &str) -> PathBuf {
    folder.join(format!("{}.md", file_stem(title, hash)))
}

fn locator(a: &Annotation) -> String {
    match (
        fond_read_gtk::page_label_of(a),
        a.page,
        a.chapter.as_deref(),
    ) {
        (Some(label), _, _) => format!("p. {label}"),
        (None, Some(p), _) => format!("p. {p}"),
        (None, None, Some(c)) => Path::new(c)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| c.to_string()),
        _ => String::new(),
    }
}

fn quote_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn yaml(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn render(doc: &Document, sidecar: &AnnotationSidecar) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("title: {}\n", yaml(doc.title)));
    if let Some(key) = doc.citation_key {
        out.push_str(&format!("citekey: {}\n", yaml(key)));
    }
    out.push_str(&format!("pereplyot-hash: {}\n", yaml(doc.hash)));
    out.push_str(&format!(
        "source: {}\n",
        yaml(&doc.source.to_string_lossy())
    ));
    out.push_str("---\n\n");
    out.push_str(&format!("# {}\n\n", doc.title));
    out.push_str(
        "<!-- Written by Pereplyot each time the notes change; edits here are overwritten. \
         The ^annot-… ids stay the same, so links to them keep working. -->\n\n",
    );

    let mut order: Vec<&Annotation> = sidecar.annotations.iter().collect();
    order.sort_by(|a, b| {
        let key = |x: &Annotation| (x.page.unwrap_or(u32::MAX), x.created.clone());
        key(a).cmp(&key(b))
    });
    for a in order {
        let link = fond_read_gtk::deeplink::build(&fond_read_gtk::deeplink::DeepLink {
            hash: doc.hash.to_string(),
            annotation: Some(a.id.clone()),
            page: a.page,
        });
        let mut lines: Vec<String> = Vec::new();
        let kind = match a.kind {
            AnnotationKind::Area => Some("figure"),
            _ => None,
        };
        if let Some(snippet) = a.snippet.as_deref().filter(|s| !s.trim().is_empty()) {
            let mut text = quote_lines(snippet);
            if let (Some(kind), Some(first)) = (kind, text.first_mut()) {
                *first = format!("({kind}) {first}");
            }
            lines.extend(text);
        }
        let place = locator(a);
        let cite = if place.is_empty() {
            format!("[↗]({link})")
        } else {
            format!("— {place} [↗]({link})")
        };
        if lines.is_empty() {
            lines.push(cite);
        } else {
            let last = lines.len() - 1;
            lines[last] = format!("{} {cite}", lines[last]);
        }
        let mut body: Vec<String> = lines.into_iter().map(|l| format!("> {l}")).collect();
        if let Some(note) = a.note.as_deref().filter(|n| !n.trim().is_empty()) {
            body.push(">".to_string());
            for l in quote_lines(note) {
                body.push(format!("> {l}"));
            }
        }
        let tags: Vec<String> = a
            .tags()
            .into_iter()
            .map(|t| format!("#{}", t.trim_start_matches('#').replace(' ', "-")))
            .collect();
        if !tags.is_empty() {
            body.push(format!("> {}", tags.join(" ")));
        }
        let last = body.len() - 1;
        body[last] = format!("{} ^{}", body[last], block_id(&a.id));
        out.push_str(&body.join("\n"));
        out.push_str("\n\n");
    }
    out
}

pub fn write(folder: &Path, doc: &Document, sidecar: &AnnotationSidecar) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let path = path_for(folder, doc.title, doc.hash);
    fond_read_gtk::fsutil::write_atomic(&path, render(doc, sidecar).as_bytes())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sidecar(json: &str) -> AnnotationSidecar {
        AnnotationSidecar::parse(json, Path::new("t.json")).unwrap()
    }

    #[test]
    fn each_annotation_is_a_quote_block_that_ends_with_its_own_block_id() {
        let sc = sidecar(
            r#"{"schema":1,"key":"k","annotations":[
              {"id":"hl-1","kind":"highlight","page":12,"snippet":"A line\nof text","note":"Check this","created":"2026-01-01"},
              {"id":"n 2","kind":"note","page":3,"note":"Remember"}
            ]}"#,
        );
        let doc = Document {
            title: "On Things",
            hash: "abc",
            citation_key: Some("smith2020"),
            source: Path::new("/books/on-things.pdf"),
        };
        let out = render(&doc, &sc);
        assert!(
            out.starts_with("---\ntitle: \"On Things\"\ncitekey: \"smith2020\"\n"),
            "{out}"
        );
        assert!(out.contains("> Remember"), "{out}");
        assert!(out.contains("^annot-n-2\n"), "{out}");
        assert!(
            out.contains("> A line\n> of text — p. 12 [↗](pereplyot://open?hash=abc&annotation=hl-1&page=12)\n>\n> Check this ^annot-hl-1\n"),
            "{out}"
        );
        assert!(
            out.find("Remember").unwrap() < out.find("A line").unwrap(),
            "in page order"
        );
    }

    #[test]
    fn the_file_name_is_safe_and_falls_back_to_the_hash() {
        assert_eq!(
            path_for(Path::new("/n"), "A: b/c?", "h"),
            PathBuf::from("/n/A b c.md")
        );
        assert_eq!(
            path_for(Path::new("/n"), "///", "0123456789abcdef"),
            PathBuf::from("/n/0123456789ab.md")
        );
    }

    #[test]
    fn writing_replaces_the_file_in_place_with_the_current_notes() {
        let dir = std::env::temp_dir().join(format!("pp-md-{}", std::process::id()));
        let doc = Document {
            title: "Book",
            hash: "h",
            citation_key: None,
            source: Path::new("/b.pdf"),
        };
        let one = sidecar(
            r#"{"schema":1,"key":"k","annotations":[{"id":"a","kind":"highlight","page":1,"snippet":"first"}]}"#,
        );
        write(&dir, &doc, &one).unwrap();
        let two = sidecar(
            r#"{"schema":1,"key":"k","annotations":[{"id":"b","kind":"highlight","page":1,"snippet":"second"}]}"#,
        );
        write(&dir, &doc, &two).unwrap();
        let text = std::fs::read_to_string(dir.join("Book.md")).unwrap();
        assert!(text.contains("second") && !text.contains("first"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
