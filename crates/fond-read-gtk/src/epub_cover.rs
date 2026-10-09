//! The cover picture of an EPUB, for Library cards: the manifest item marked `cover-image`, the
//! one the `<meta name="cover">` points at, or failing both an image called "cover".

use std::path::Path;

use crate::epub::pages::{attr, resolve, tags};

struct Item {
    id: String,
    href: String,
    properties: String,
    media_type: String,
}

fn cover_href(opf: &str) -> Option<String> {
    let items: Vec<Item> = tags(opf)
        .filter(|(t, _)| t.starts_with("<item ") || t.starts_with("<item\n"))
        .filter_map(|(t, _)| {
            Some(Item {
                id: attr(t, "id")?,
                href: attr(t, "href")?,
                properties: attr(t, "properties").unwrap_or_default(),
                media_type: attr(t, "media-type").unwrap_or_default(),
            })
        })
        .collect();
    let is_image = |i: &Item| i.media_type.starts_with("image/");
    items
        .iter()
        .find(|i| i.properties.split_whitespace().any(|p| p == "cover-image"))
        .or_else(|| {
            let id = tags(opf)
                .filter(|(t, _)| t.starts_with("<meta"))
                .find(|(t, _)| attr(t, "name").as_deref() == Some("cover"))
                .and_then(|(t, _)| attr(t, "content"))?;
            items.iter().find(|i| i.id == id && is_image(i))
        })
        .or_else(|| {
            items.iter().find(|i| {
                is_image(i)
                    && (i.id.to_lowercase().contains("cover")
                        || i.href.to_lowercase().contains("cover"))
            })
        })
        .map(|i| i.href.clone())
}

/// The cover image's bytes (PNG, JPEG, …), if the book has one. Blocking.
pub fn cover_bytes(epub: &Path) -> Option<Vec<u8>> {
    let tmp = std::env::temp_dir().join(format!(
        "pereplyot-cover-{}-{}",
        std::process::id(),
        epub.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    fond_doc::epub::extract_all(epub, &tmp).ok()?;
    let found = (|| {
        let container = std::fs::read_to_string(tmp.join("META-INF/container.xml")).ok()?;
        let opf_path = tags(&container)
            .find(|(t, _)| t.starts_with("<rootfile ") || t.starts_with("<rootfile\n"))
            .and_then(|(t, _)| attr(t, "full-path"))?;
        let opf = std::fs::read_to_string(tmp.join(&opf_path)).ok()?;
        let href = cover_href(&opf)?;
        let dir = Path::new(&opf_path)
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        let (file, _) = resolve(&dir, &href);
        std::fs::read(tmp.join(file)).ok()
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marked_cover_is_found_by_property_meta_or_name() {
        let by_property = r#"<manifest><item id="a" href="x.png" media-type="image/png"/><item id="c" href="img/c.jpg" media-type="image/jpeg" properties="cover-image"/></manifest>"#;
        assert_eq!(cover_href(by_property).as_deref(), Some("img/c.jpg"));
        let by_meta = r#"<metadata><meta name="cover" content="pic"/></metadata><manifest><item id="pic" href="p.png" media-type="image/png"/><item id="z" href="z.png" media-type="image/png"/></manifest>"#;
        assert_eq!(cover_href(by_meta).as_deref(), Some("p.png"));
        let by_name = r#"<manifest><item id="x" href="a.png" media-type="image/png"/><item id="y" href="Cover.png" media-type="image/png"/></manifest>"#;
        assert_eq!(cover_href(by_name).as_deref(), Some("Cover.png"));
        assert_eq!(
            cover_href(
                r#"<manifest><item id="x" href="a.xhtml" media-type="application/xhtml+xml"/></manifest>"#
            ),
            None
        );
    }

    #[test]
    fn a_book_with_a_cover_gives_its_bytes_and_one_without_gives_none() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        let bytes = cover_bytes(&fixtures.join("scholar.epub")).expect("a cover");
        assert!(bytes.starts_with(b"\x89PNG"));
        assert!(cover_bytes(&fixtures.join("book.epub")).is_none());
    }
}
