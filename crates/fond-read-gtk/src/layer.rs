//! A layer: one person's notes on a document, in a file you can hand to someone else. Read-only
//! when shown beside your own, and kept apart from the sidecar so it needs no schema change.

use fond_annot::{Annotation, AnnotationSidecar};
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerFile {
    pub schema: u32,
    /// Whose notes these are.
    pub name: String,
    /// The content hash of the file they were made on.
    pub hash: String,
    pub title: String,
    pub annotations: Vec<Annotation>,
}

impl LayerFile {
    /// `mine` as a layer called `name`: the marks with words or a place, not the bookmarks or position.
    pub fn from_sidecar(
        name: &str,
        hash: &str,
        title: &str,
        mine: &AnnotationSidecar,
    ) -> LayerFile {
        LayerFile {
            schema: SCHEMA,
            name: name.to_string(),
            hash: hash.to_string(),
            title: title.to_string(),
            annotations: mine.annotations.clone(),
        }
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn parse(text: &str) -> Result<LayerFile, String> {
        let l: LayerFile =
            serde_json::from_str(text).map_err(|e| format!("not a Pereplyot layer: {e}"))?;
        if l.schema > SCHEMA {
            return Err("this layer was made by a newer Pereplyot".to_string());
        }
        if l.name.trim().is_empty() {
            return Err("the layer has no name".to_string());
        }
        Ok(l)
    }

    /// A file name for this layer.
    pub fn stem(&self) -> String {
        let s: String = self
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let s = s
            .split('-')
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        if s.is_empty() {
            "layer".to_string()
        } else {
            s
        }
    }
}

/// A colour for a layer's outlines, the same every time for the same name and apart from the four
/// reading colours' own hues.
pub fn colour_of(name: &str) -> [u8; 3] {
    let mut h = 0xcbf29ce484222325u64;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    // Eight well-spaced, mid-lightness hues that read on white and on dark pages alike.
    const PALETTE: [[u8; 3]; 8] = [
        [0x8e, 0x44, 0xad],
        [0x16, 0xa0, 0x85],
        [0xd3, 0x54, 0x00],
        [0x29, 0x80, 0xb9],
        [0xc0, 0x39, 0x2b],
        [0x7f, 0x8c, 0x8d],
        [0xb7, 0x95, 0x0b],
        [0x27, 0xae, 0x60],
    ];
    PALETTE[(h % 8) as usize]
}

/// A fresh id for a mark adopted from a layer.
pub fn adopted_id(original: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "ad-{nanos:x}-{}",
        original.chars().take(8).collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fond_annot::AnnotationKind;

    fn mark(id: &str) -> Annotation {
        Annotation {
            id: id.into(),
            kind: AnnotationKind::Highlight,
            page: Some(3),
            chapter: None,
            quadpoints: vec![[1.0, 2.0, 3.0, 2.0, 1.0, 1.0, 3.0, 1.0]],
            snippet: Some("some words".into()),
            snippet_prefix: None,
            snippet_suffix: None,
            color: Some("#f5c211".into()),
            note: Some("check this #method".into()),
            created: None,
            modified: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn a_layer_survives_its_file() {
        let mut s = AnnotationSidecar::new("h");
        s.annotations = vec![mark("a"), mark("b")];
        let layer = LayerFile::from_sidecar("Dr. Ahmed", "h", "On Things", &s);
        let back = LayerFile::parse(&layer.to_json().unwrap()).unwrap();
        assert_eq!(back, layer);
        assert_eq!(back.stem(), "dr-ahmed");
    }

    #[test]
    fn a_stranger_file_or_a_newer_one_is_refused_with_a_reason() {
        assert!(LayerFile::parse("{}")
            .unwrap_err()
            .starts_with("not a Pereplyot layer"));
        assert!(LayerFile::parse("[1,2]").is_err());
        let newer = r#"{"schema":99,"name":"x","hash":"h","title":"t","annotations":[]}"#;
        assert!(LayerFile::parse(newer).unwrap_err().contains("newer"));
        let nameless = r#"{"schema":1,"name":" ","hash":"h","title":"t","annotations":[]}"#;
        assert!(LayerFile::parse(nameless).is_err());
    }

    #[test]
    fn a_name_always_gets_the_same_colour() {
        assert_eq!(colour_of("Dr. Ahmed"), colour_of("Dr. Ahmed"));
        let distinct: std::collections::HashSet<_> = ["a", "b", "c", "d", "e", "f", "g", "h"]
            .iter()
            .map(|n| colour_of(n))
            .collect();
        assert!(distinct.len() >= 3);
    }
}
