use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::identity::{find_arxiv, find_doi, find_isbn};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Article,
    Book,
    Chapter,
    Thesis,
    #[default]
    Other,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Article,
        Kind::Book,
        Kind::Chapter,
        Kind::Thesis,
        Kind::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Article => "Article",
            Kind::Book => "Book",
            Kind::Chapter => "Chapter",
            Kind::Thesis => "Thesis",
            Kind::Other => "Other",
        }
    }
}

/// One document's bibliographic record. Fields the file could not supply stay empty rather than guessed;
/// `sources` says where each filled field came from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub title: String,
    /// "Family, Given", in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub editors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub container: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub volume: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pages: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub place: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doi: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub isbn: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arxiv: String,
    /// Your own few sentences on what it argues and why it matters: the top of its précis.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, String>,
}

/// What the file says about itself.
#[derive(Debug, Clone, Default)]
pub struct Facts<'a> {
    pub title: Option<&'a str>,
    pub authors: Vec<&'a str>,
    pub date: Option<&'a str>,
    pub publisher: Option<&'a str>,
    pub isbn: Option<&'a str>,
    /// The first pages' text, for ids found on the page.
    pub front_text: &'a str,
    pub is_epub: bool,
}

impl Record {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.authors.is_empty()
    }

    pub fn extract(facts: &Facts) -> Record {
        let mut r = Record::default();
        let from = if facts.is_epub {
            "the EPUB's metadata"
        } else {
            "the PDF's metadata"
        };
        if let Some(t) = facts.title.map(clean_title).filter(|t| usable_title(t)) {
            r.title = t;
            r.sources.insert("title".into(), from.into());
        }
        let authors: Vec<String> = facts
            .authors
            .iter()
            .flat_map(|a| split_authors(a))
            .collect();
        if !authors.is_empty() {
            r.authors = authors;
            r.sources.insert("authors".into(), from.into());
        }
        if let Some(y) = facts.date.and_then(year_of) {
            r.year = Some(y);
            r.sources.insert("year".into(), from.into());
        }
        if let Some(p) = facts.publisher.map(str::trim).filter(|p| !p.is_empty()) {
            r.publisher = p.to_string();
            r.sources.insert("publisher".into(), from.into());
        }
        if let Some(doi) = find_doi(facts.front_text) {
            r.doi = doi;
            r.sources.insert("doi".into(), "found in the text".into());
        }
        if let Some(isbn) = facts
            .isbn
            .map(str::to_string)
            .or_else(|| find_isbn(facts.front_text))
        {
            r.isbn = isbn;
            r.sources.insert("isbn".into(), "found in the text".into());
        }
        if let Some(a) = find_arxiv(facts.front_text) {
            r.arxiv = a;
            r.sources.insert("arxiv".into(), "found in the text".into());
        }
        r.kind = if !r.doi.is_empty() || !r.arxiv.is_empty() {
            Kind::Article
        } else if !r.isbn.is_empty() || facts.is_epub {
            Kind::Book
        } else {
            Kind::Other
        };
        if r.year.is_none() {
            if let Some(y) = year_in_front(facts.front_text) {
                r.year = Some(y);
                r.sources.insert("year".into(), "found in the text".into());
            }
        }
        r
    }

    /// Fills this record's empty fields from `other` (a lookup result); fields already set are kept.
    pub fn fill_from(&mut self, other: &Record, source: &str) {
        macro_rules! take {
            ($f:ident) => {
                if self.$f.is_empty() && !other.$f.is_empty() {
                    self.$f = other.$f.clone();
                    self.sources.insert(stringify!($f).into(), source.into());
                }
            };
        }
        take!(title);
        take!(authors);
        take!(editors);
        take!(container);
        take!(volume);
        take!(issue);
        take!(pages);
        take!(publisher);
        take!(place);
        take!(doi);
        take!(isbn);
        take!(arxiv);
        if self.year.is_none() && other.year.is_some() {
            self.year = other.year;
            self.sources.insert("year".into(), source.into());
        }
        if self.kind == Kind::Other && other.kind != Kind::Other {
            self.kind = other.kind;
        }
    }

    pub fn first_family(&self) -> Option<String> {
        self.authors
            .first()
            .or(self.editors.first())
            .map(|a| family_of(a))
    }

    /// Hayagriva YAML for this record, keyed by `self.key`.
    pub fn to_yaml(&self) -> String {
        let mut out = String::new();
        let key = if self.key.is_empty() {
            "untitled"
        } else {
            &self.key
        };
        out.push_str(&format!("{key}:\n"));
        let ty = match self.kind {
            Kind::Article => "article",
            Kind::Book => "book",
            Kind::Chapter => "chapter",
            Kind::Thesis => "thesis",
            Kind::Other => "misc",
        };
        out.push_str(&format!("  type: {ty}\n"));
        let title = if self.title.is_empty() {
            "Untitled"
        } else {
            &self.title
        };
        out.push_str(&format!("  title: {}\n", q(title)));
        list(&mut out, "author", &self.authors);
        if self.kind != Kind::Chapter {
            list(&mut out, "editor", &self.editors);
        }
        if let Some(y) = self.year {
            out.push_str(&format!("  date: {y}\n"));
        }
        if !self.pages.is_empty() {
            out.push_str(&format!("  page-range: {}\n", q(&self.pages)));
        }
        if self.kind != Kind::Chapter && self.kind != Kind::Article {
            publisher_at(&mut out, "  ", &self.publisher, &self.place);
        }
        match self.kind {
            Kind::Article if !self.container.is_empty() => {
                out.push_str("  parent:\n    type: periodical\n");
                out.push_str(&format!("    title: {}\n", q(&self.container)));
                if !self.volume.is_empty() {
                    out.push_str(&format!("    volume: {}\n", q(&self.volume)));
                }
                if !self.issue.is_empty() {
                    out.push_str(&format!("    issue: {}\n", q(&self.issue)));
                }
            }
            Kind::Chapter if !self.container.is_empty() => {
                out.push_str("  parent:\n    type: book\n");
                out.push_str(&format!("    title: {}\n", q(&self.container)));
                list_at(&mut out, "    ", "editor", &self.editors);
                publisher_at(&mut out, "    ", &self.publisher, &self.place);
            }
            _ => {
                if !self.volume.is_empty() {
                    out.push_str(&format!("  volume: {}\n", q(&self.volume)));
                }
            }
        }
        if !self.doi.is_empty() {
            out.push_str(&format!("  doi: {}\n", q(&self.doi)));
        }
        if !self.isbn.is_empty() {
            out.push_str(&format!("  isbn: {}\n", q(&self.isbn)));
        }
        if !self.arxiv.is_empty() {
            out.push_str(&format!(
                "  serial-number:\n    arxiv: {}\n",
                q(&self.arxiv)
            ));
        }
        out
    }
}

fn publisher_at(out: &mut String, indent: &str, name: &str, place: &str) {
    if name.is_empty() && place.is_empty() {
        return;
    }
    out.push_str(&format!("{indent}publisher:\n"));
    if !name.is_empty() {
        out.push_str(&format!("{indent}  name: {}\n", q(name)));
    }
    if !place.is_empty() {
        out.push_str(&format!("{indent}  location: {}\n", q(place)));
    }
}

fn list(out: &mut String, name: &str, items: &[String]) {
    list_at(out, "  ", name, items);
}

fn list_at(out: &mut String, indent: &str, name: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("{indent}{name}:\n"));
    for i in items {
        out.push_str(&format!("{indent}  - {}\n", q(i)));
    }
}

/// A YAML double-quoted scalar.
fn q(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn family_of(name: &str) -> String {
    match name.split_once(',') {
        Some((f, _)) => f.trim().to_string(),
        None => name.split_whitespace().last().unwrap_or("").to_string(),
    }
}

pub fn clean_title(t: &str) -> String {
    let t = t.trim().trim_end_matches(".pdf").trim_end_matches(".PDF");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Rejects the placeholder titles word processors write ("Microsoft Word - draft.docx", "untitled", a bare file name).
pub fn usable_title(t: &str) -> bool {
    let l = t.to_lowercase();
    t.chars().filter(|c| c.is_alphabetic()).count() >= 4
        && !l.starts_with("microsoft word")
        && !l.starts_with("untitled")
        && !l.ends_with(".docx")
        && !l.ends_with(".doc")
        && !l.ends_with(".tex")
        && !l.contains("pdfscreen")
}

/// "Daniel J. Harrington and Pheme Perkins; Smith, J." → ["Harrington, Daniel J.", "Perkins, Pheme", "Smith, J."].
pub fn split_authors(s: &str) -> Vec<String> {
    let s = s.replace(" & ", " and ");
    let mut out = Vec::new();
    for chunk in s.split(';') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        if chunk.contains(',') && !chunk.contains(" and ") && chunk.matches(',').count() == 1 {
            out.push(chunk.to_string());
            continue;
        }
        for part in chunk.split(" and ").flat_map(|p| p.split(',')) {
            let part = part.trim();
            if !part.is_empty() {
                out.push(flip_name(part));
            }
        }
    }
    out
}

/// "Daniel J. Harrington" → "Harrington, Daniel J."
pub fn flip_name(name: &str) -> String {
    if name.contains(',') {
        return name.to_string();
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    if words.len() < 2 {
        return name.to_string();
    }
    let mut family = vec![words[words.len() - 1]];
    let mut i = words.len() - 1;
    while i > 1
        && matches!(
            words[i - 1].to_lowercase().as_str(),
            "van" | "von" | "de" | "der" | "del" | "da" | "di" | "la" | "le"
        )
    {
        i -= 1;
        family.insert(0, words[i]);
    }
    format!("{}, {}", family.join(" "), words[..i].join(" "))
}

pub fn year_of(s: &str) -> Option<i32> {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take(4)
        .collect();
    let y: i32 = digits.parse().ok()?;
    (1000..=2100).contains(&y).then_some(y)
}

/// A year in parentheses or after "©"/"Copyright" near the start of the text.
pub fn year_in_front(text: &str) -> Option<i32> {
    let head: String = text.chars().take(6000).collect();
    for marker in ["©", "copyright", "Copyright", "published"] {
        if let Some(at) = head.find(marker) {
            if let Some(y) = year_of(
                &head[at + marker.len()..]
                    .chars()
                    .take(30)
                    .collect::<String>(),
            ) {
                return Some(y);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_flip_and_split() {
        assert_eq!(flip_name("Daniel J. Harrington"), "Harrington, Daniel J.");
        assert_eq!(flip_name("Ludwig van Beethoven"), "van Beethoven, Ludwig");
        assert_eq!(
            split_authors("Daniel J. Harrington and Pheme Perkins"),
            ["Harrington, Daniel J.", "Perkins, Pheme"]
        );
        assert_eq!(
            split_authors("Smith, J.; Jones, K."),
            ["Smith, J.", "Jones, K."]
        );
        assert_eq!(
            split_authors("Harrington, Daniel J."),
            ["Harrington, Daniel J."]
        );
    }

    #[test]
    fn placeholder_titles_are_refused() {
        assert!(!usable_title("Microsoft Word - essay draft.docx"));
        assert!(!usable_title("Untitled"));
        assert!(usable_title("Interpreting the Old Testament"));
    }

    #[test]
    fn extract_reports_where_each_field_came_from() {
        let r = Record::extract(&Facts {
            title: Some("Q and the History of Early Christianity"),
            authors: vec!["John S. Kloppenborg"],
            date: Some("1987-05-01"),
            front_text: "Copyright 1987 ... doi:10.1017/abc123 ...",
            ..Default::default()
        });
        assert_eq!(r.authors, ["Kloppenborg, John S."]);
        assert_eq!(r.year, Some(1987));
        assert_eq!(r.doi, "10.1017/abc123");
        assert_eq!(r.kind, Kind::Article);
        assert_eq!(r.sources["doi"], "found in the text");
        assert_eq!(r.sources["title"], "the PDF's metadata");
    }

    #[test]
    fn year_comes_from_the_copyright_line_when_metadata_has_none() {
        let r = Record::extract(&Facts {
            front_text: "Some Book\n© 1981 by Glazier",
            ..Default::default()
        });
        assert_eq!(r.year, Some(1981));
    }

    #[test]
    fn fill_keeps_what_is_set() {
        let mut a = Record {
            title: "Mine".into(),
            ..Default::default()
        };
        let b = Record {
            title: "Theirs".into(),
            container: "JBL".into(),
            year: Some(2001),
            ..Default::default()
        };
        a.fill_from(&b, "CrossRef");
        assert_eq!(a.title, "Mine");
        assert_eq!(a.container, "JBL");
        assert_eq!(a.year, Some(2001));
        assert_eq!(a.sources["container"], "CrossRef");
    }

    #[test]
    fn yaml_quotes_awkward_text() {
        let r = Record {
            key: "k".into(),
            kind: Kind::Article,
            title: "A \"quoted\": title # not a comment".into(),
            authors: vec!["Smith, J.".into()],
            container: "J. of Things".into(),
            year: Some(2001),
            ..Default::default()
        };
        let y = r.to_yaml();
        assert!(y.contains(r#"title: "A \"quoted\": title # not a comment""#));
        assert!(y.contains("type: periodical"));
    }
}
