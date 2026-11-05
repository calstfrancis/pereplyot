//! "Look up online": fills a [`Record`] from CrossRef, arXiv or Open Library. Only ever runs when the
//! person presses the button (and never when Preferences say not to go online).

use hayagriva::Entry;
use pereplyot_core::record::{flip_name, Kind, Record};
use serde_json::Value;

pub struct Found {
    pub record: Record,
    pub service: &'static str,
}

/// Blocking: does network I/O. Tries the DOI, then the arXiv id, then the ISBN.
pub fn lookup(have: &Record) -> Result<Found, String> {
    if !have.doi.is_empty() {
        let bibtex = fond_bib::acquire::fetch_doi_bibtex(&have.doi).map_err(|e| e.to_string())?;
        return Ok(Found {
            record: from_bibtex(&bibtex)?,
            service: "CrossRef",
        });
    }
    if !have.arxiv.is_empty() {
        let bibtex =
            fond_bib::acquire::fetch_arxiv_bibtex(&have.arxiv).map_err(|e| e.to_string())?;
        return Ok(Found {
            record: from_bibtex(&bibtex)?,
            service: "arXiv",
        });
    }
    if !have.isbn.is_empty() {
        let yaml = fond_bib::acquire::fetch_isbn_yaml(&have.isbn).map_err(|e| e.to_string())?;
        return Ok(Found {
            record: from_yaml(&yaml)?,
            service: "Open Library",
        });
    }
    Err("Enter a DOI, arXiv id or ISBN first".to_string())
}

pub fn from_bibtex(source: &str) -> Result<Record, String> {
    let library = hayagriva::io::from_biblatex_str(source).map_err(|e| {
        e.first()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unreadable BibTeX".to_string())
    })?;
    let first = library.iter().next().map(from_entry);
    first.ok_or_else(|| "the service returned no record".to_string())
}

pub fn from_yaml(source: &str) -> Result<Record, String> {
    let library = hayagriva::io::from_yaml_str(source).map_err(|e| e.to_string())?;
    let first = library.iter().next().map(from_entry);
    first.ok_or_else(|| "the service returned no record".to_string())
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Object(o)) => text(o.get("value").or_else(|| o.get("name"))),
        _ => String::new(),
    }
}

fn people(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(items)) => items.iter().map(person).filter(|p| !p.is_empty()).collect(),
        Some(one) => vec![person(one)]
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect(),
        None => Vec::new(),
    }
}

fn person(v: &Value) -> String {
    match v {
        Value::String(s) => flip_name(s),
        Value::Object(o) => {
            let family = text(o.get("name"));
            let given = text(o.get("given-name"));
            if given.is_empty() {
                family
            } else {
                format!("{family}, {given}")
            }
        }
        _ => String::new(),
    }
}

fn year_of(v: Option<&Value>) -> Option<i32> {
    match v {
        Some(Value::Number(n)) => n.as_i64().map(|n| n as i32),
        Some(Value::String(s)) => pereplyot_core::record::year_of(s),
        _ => None,
    }
}

/// An identifier, whether the entry holds it at the top or under `serial-number`.
fn serial(v: &Value, name: &str) -> String {
    let top = text(v.get(name));
    if top.is_empty() {
        text(v.get("serial-number").and_then(|s| s.get(name)))
    } else {
        top
    }
}

pub fn from_entry(entry: &Entry) -> Record {
    let v = serde_json::to_value(entry).unwrap_or(Value::Null);
    let parent = v
        .get("parent")
        .and_then(|p| p.as_array().and_then(|a| a.first()).or(Some(p)));
    let mut r = Record {
        title: text(v.get("title")),
        authors: people(v.get("author")),
        editors: people(v.get("editor")),
        year: year_of(v.get("date")),
        pages: text(v.get("page-range")),
        doi: serial(&v, "doi"),
        isbn: serial(&v, "isbn"),
        ..Default::default()
    };
    let publisher = v.get("publisher");
    match publisher {
        Some(Value::Object(o)) => {
            r.publisher = text(o.get("name"));
            r.place = text(o.get("location"));
        }
        other => r.publisher = text(other),
    }
    if r.place.is_empty() {
        r.place = text(v.get("location"));
    }
    if let Some(parent) = parent {
        r.container = text(parent.get("title"));
        r.volume = text(parent.get("volume"));
        r.issue = text(parent.get("issue"));
        if r.editors.is_empty() {
            r.editors = people(parent.get("editor"));
        }
        if r.publisher.is_empty() {
            r.publisher = text(parent.get("publisher").map(|p| p.get("name").unwrap_or(p)));
        }
        if r.volume.is_empty() {
            r.volume = text(v.get("volume"));
        }
    } else {
        r.volume = text(v.get("volume"));
    }
    let ty = text(v.get("type"));
    r.kind = match ty.as_str() {
        "article" => Kind::Article,
        "book" => Kind::Book,
        "chapter" => Kind::Chapter,
        "thesis" => Kind::Thesis,
        _ => Kind::Other,
    };
    r.arxiv = serial(&v, "arxiv");
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    const CROSSREF: &str = "@article{Kloppenborg_1987,\n\
        title={The Formation of Q},\n\
        volume={80}, ISSN={0017-8160}, url={http://dx.doi.org/10.1017/s0017816000030234},\n\
        DOI={10.1017/s0017816000030234}, number={3},\n\
        journal={Harvard Theological Review}, publisher={Cambridge University Press (CUP)},\n\
        author={Kloppenborg, John S.}, year={1987}, month=jul, pages={241–267}\n}\n";

    #[test]
    fn a_crossref_article_becomes_a_record() {
        let r = from_bibtex(CROSSREF).unwrap();
        assert_eq!(r.title, "The Formation of Q");
        assert_eq!(r.authors, ["Kloppenborg, John S."]);
        assert_eq!(r.year, Some(1987));
        assert_eq!(r.container, "Harvard Theological Review");
        assert_eq!(r.volume, "80");
        assert_eq!(r.issue, "3");
        assert!(r.pages.starts_with("241"));
        assert_eq!(r.doi, "10.1017/s0017816000030234");
        assert_eq!(r.kind, Kind::Article);
    }

    #[test]
    fn an_open_library_book_becomes_a_record() {
        let yaml = "doe2001x:\n  type: book\n  title: A Book\n  author:\n    - \"Doe, Jane\"\n  date: 2001\n  publisher:\n    name: Press\n    location: London\n  isbn: \"9780140449136\"\n";
        let r = from_yaml(yaml).unwrap();
        assert_eq!(r.title, "A Book");
        assert_eq!(r.authors, ["Doe, Jane"]);
        assert_eq!(r.publisher, "Press");
        assert_eq!(r.place, "London");
        assert_eq!(r.kind, Kind::Book);
    }

    #[test]
    fn nothing_to_look_up_says_so() {
        assert!(lookup(&Record::default()).is_err());
    }
}
