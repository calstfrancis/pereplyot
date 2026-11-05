//! Renders a document's [`Record`] as a footnote or a reference-list entry in a CSL style.

use fond_bib::render::{resolve_style, style_choices, StyleChoice};
use hayagriva::archive::locales;
use hayagriva::citationberg::taxonomy::Locator;
use hayagriva::{
    BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest, Entry,
    LocatorPayload, SpecificLocator,
};
use pereplyot_core::record::Record;

pub const DEFAULT_STYLE: &str = "sbl";

pub fn styles() -> Vec<StyleChoice> {
    style_choices()
}

pub fn entry(record: &Record) -> Result<Entry, String> {
    let library = hayagriva::io::from_yaml_str(&record.to_yaml()).map_err(|e| e.to_string())?;
    let first = library.iter().next().cloned();
    first.ok_or_else(|| "empty record".to_string())
}

/// One note per `(record, page)`, in order; a style that shortens repeat citations does so.
pub fn notes(items: &[(Record, Option<String>)], style: &str) -> Result<Vec<String>, String> {
    let mut style = resolve_style(style).map_err(|e| e.to_string())?;
    // Each note holds one item, and hayagriva puts the layout's multi-cite delimiter between the
    // pieces of a single cite instead.
    style.citation.layout.delimiter = None;
    let locales = locales();
    // One Entry per distinct key, shared by reference: the driver tells a first note from a later one
    // by entry identity.
    let mut unique: Vec<(&str, Entry)> = Vec::new();
    let mut slot = Vec::with_capacity(items.len());
    for (record, _) in items {
        let at = match unique.iter().position(|(k, _)| *k == record.key) {
            Some(at) => at,
            None => {
                unique.push((record.key.as_str(), entry(record)?));
                unique.len() - 1
            }
        };
        slot.push(at);
    }
    let mut driver: BibliographyDriver<Entry> = BibliographyDriver::new();
    for (i, ((_, page), at)) in items.iter().zip(&slot).enumerate() {
        let entry = &unique[*at].1;
        let locator = page
            .as_deref()
            .map(|p| SpecificLocator(Locator::Page, LocatorPayload::Str(p)));
        driver.citation(CitationRequest::new(
            vec![CitationItem::with_locator(entry, locator)],
            &style,
            None,
            &locales,
            Some(i + 1),
        ));
    }
    let rendered = driver.finish(BibliographyRequest::new(&style, None, &locales));
    rendered
        .citations
        .iter()
        .map(|c| {
            let mut s = String::new();
            c.citation
                .write_buf(&mut s, BufWriteFormat::Plain)
                .map_err(|e| e.to_string())?;
            Ok(s.trim().to_string())
        })
        .collect()
}

pub fn footnote(record: &Record, style: &str, page: Option<&str>) -> Result<String, String> {
    notes(&[(record.clone(), page.map(str::to_string))], style)?
        .pop()
        .ok_or_else(|| "no citation produced".to_string())
}

pub fn reference(record: &Record, style: &str) -> Result<String, String> {
    references(std::slice::from_ref(record), style)?
        .pop()
        .ok_or_else(|| "no reference produced".to_string())
}

/// Reference-list entries in the style's own order.
pub fn references(records: &[Record], style: &str) -> Result<Vec<String>, String> {
    let style = resolve_style(style).map_err(|e| e.to_string())?;
    let entries: Vec<Entry> = records.iter().map(entry).collect::<Result<_, _>>()?;
    let refs: Vec<&Entry> = entries.iter().collect();
    fond_bib::render::render_bibliography(&refs, &style, BufWriteFormat::Plain)
        .map(|v| v.into_iter().map(|r| r.text).collect())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pereplyot_core::record::Kind;

    fn book() -> Record {
        Record {
            key: "harrington1981interpreting".into(),
            kind: Kind::Book,
            title: "Interpreting the Old Testament".into(),
            authors: vec!["Harrington, Daniel J.".into()],
            year: Some(1981),
            publisher: "Glazier".into(),
            place: "Wilmington, DE".into(),
            ..Default::default()
        }
    }

    fn article() -> Record {
        Record {
            key: "kloppenborg1987".into(),
            kind: Kind::Article,
            title: "The Formation of Q".into(),
            authors: vec!["Kloppenborg, John S.".into()],
            year: Some(1987),
            container: "Harvard Theological Review".into(),
            volume: "80".into(),
            issue: "3".into(),
            pages: "241-67".into(),
            ..Default::default()
        }
    }

    #[test]
    fn sbl_footnote_for_a_book() {
        let note = footnote(&book(), "sbl", Some("42")).unwrap();
        println!("{note}");
        assert!(note.contains("Daniel J. Harrington"), "{note}");
        assert!(note.contains("Interpreting the Old Testament"), "{note}");
        assert!(note.contains("1981"), "{note}");
        assert!(note.contains("42"), "{note}");
    }

    #[test]
    fn a_repeat_note_is_shortened() {
        let notes = notes(
            &[(book(), Some("42".into())), (book(), Some("57".into()))],
            "sbl",
        )
        .unwrap();
        println!("{notes:?}");
        assert!(notes[1].len() < notes[0].len(), "{notes:?}");
        assert!(notes[1].contains("57"));
    }

    #[test]
    fn reference_entries_in_apa_and_chicago() {
        for style in ["apa", "chicago-notes", "mla", "turabian"] {
            let r = reference(&article(), style).unwrap();
            println!("{style}: {r}");
            assert!(r.contains("Kloppenborg"), "{style}: {r}");
            assert!(r.contains("Formation of Q"), "{style}: {r}");
        }
    }

    #[test]
    fn the_generated_bibliography_compiles_in_typst() {
        let Some(typst) = std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("typst"))
                .find(|f| f.is_file())
        }) else {
            eprintln!("no typst on PATH; skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("pereplyot-cite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut chapter = book();
        chapter.key = "perkins1990chapter".into();
        chapter.kind = Kind::Chapter;
        chapter.title = "Q: a \"hard\" #case".into();
        chapter.container = "The Gospels".into();
        chapter.editors = vec!["Doe, Jane".into()];
        chapter.pages = "10-20".into();
        let yaml = [book(), article(), chapter]
            .iter()
            .map(|r| r.to_yaml())
            .collect::<String>();
        std::fs::write(dir.join("refs.yml"), yaml).unwrap();
        std::fs::write(
            dir.join("essay.typ"),
            "As @harrington1981interpreting[p. 42] and @kloppenborg1987 and @perkins1990chapter[p. 12] say.\n#bibliography(\"refs.yml\")\n",
        )
        .unwrap();
        let out = std::process::Command::new(typst)
            .arg("compile")
            .arg(dir.join("essay.typ"))
            .arg(dir.join("essay.pdf"))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "typst failed:\n{stderr}");
    }

    #[test]
    fn a_record_with_awkward_text_still_parses() {
        let mut r = article();
        r.title = "On \"Q\": a #hashtag, and a colon".into();
        assert!(reference(&r, "apa").unwrap().contains("hashtag"));
    }
}
