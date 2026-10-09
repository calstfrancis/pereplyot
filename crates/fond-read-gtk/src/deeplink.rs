//! `pereplyot://open?hash=<content hash>&annotation=<id>&page=<n>` — a link to a document (found
//! by the content hash its sidecar is keyed by) and, optionally, to one annotation or page in
//! it. Exports carry these so a note in Zerkalo, Obsidian or a Typst PDF opens the passage.

pub const SCHEME: &str = "pereplyot://";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeepLink {
    pub hash: String,
    pub annotation: Option<String>,
    pub page: Option<u32>,
}

fn escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn build(link: &DeepLink) -> String {
    let mut url = format!("{SCHEME}open?hash={}", escape(&link.hash));
    if let Some(a) = &link.annotation {
        url.push_str(&format!("&annotation={}", escape(a)));
    }
    if let Some(p) = link.page {
        url.push_str(&format!("&page={p}"));
    }
    url
}

/// `None` if `s` isn't a Pereplyot link or names no document.
pub fn parse(s: &str) -> Option<DeepLink> {
    let rest = s.strip_prefix(SCHEME)?;
    let query = rest.split_once('?').map(|(_, q)| q)?;
    let mut link = DeepLink::default();
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        match k {
            "hash" => link.hash = unescape(v),
            "annotation" => link.annotation = Some(unescape(v)).filter(|v| !v.is_empty()),
            "page" => link.page = v.parse().ok(),
            _ => {}
        }
    }
    (!link.hash.is_empty()).then_some(link)
}

/// The path a `file://` URI names, as when a file manager hands the app a URI.
pub fn file_uri_path(s: &str) -> Option<std::path::PathBuf> {
    let rest = s.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    Some(std::path::PathBuf::from(unescape(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_survives_a_round_trip_with_awkward_ids() {
        let link = DeepLink {
            hash: "abc123".into(),
            annotation: Some("hl-9f&x=1 é".into()),
            page: Some(42),
        };
        let url = build(&link);
        assert!(url.starts_with("pereplyot://open?hash=abc123&annotation=hl-9f%26x%3D1%20%C3%A9"));
        assert_eq!(parse(&url), Some(link));
    }

    #[test]
    fn things_that_are_not_links_are_not_parsed() {
        assert_eq!(parse("https://x"), None);
        assert_eq!(parse("pereplyot://open"), None);
        assert_eq!(parse("pereplyot://open?page=3"), None);
        assert_eq!(
            parse("pereplyot://open?hash=h&page=7"),
            Some(DeepLink {
                hash: "h".into(),
                annotation: None,
                page: Some(7)
            })
        );
    }

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            file_uri_path("file:///home/a%20b/c.pdf"),
            Some(std::path::PathBuf::from("/home/a b/c.pdf"))
        );
        assert_eq!(file_uri_path("/plain"), None);
    }
}
