//! Hostile-input fuzzing for the text parsers: none of them may panic whatever they are given.
//! Run with `cargo test -p fond-read-gtk fuzz_ -- --ignored`; `FUZZ_SEEDS` sets how many.

pub(crate) struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A string built from fragments that tend to break parsers.
    pub fn text(&mut self, max_pieces: usize) -> String {
        const PIECES: &[&str] = &[
            "",
            " ",
            "\n",
            "\t",
            "\r\n",
            "<",
            ">",
            "</",
            "/>",
            "<a href=\"",
            "\"",
            "'",
            "=",
            "&",
            "&amp;",
            "&#",
            "&#x41;",
            "&lt",
            "#",
            "%",
            "%2",
            "%zz",
            "\\",
            "{",
            "}",
            "[",
            "]",
            "(",
            ")",
            "*",
            "_",
            "`",
            "$",
            "^",
            "|",
            "?",
            "!",
            ":",
            ";",
            ",",
            ".",
            "é",
            "ß",
            "İ",
            "ǅ",
            "日本語",
            "🙂",
            "\u{0}",
            "\u{200b}",
            "\u{202e}",
            "\u{feff}",
            "a\u{0301}",
            "ﬁ",
            "ⅷ",
            "Ⅻ",
            "alpha",
            "Chapter",
            "note",
            "id",
            "epub:type",
            "noteref",
            "pereplyot://",
            "open?hash=",
            "&annotation=",
            "&page=",
            "#pquote(",
            "text: \"",
            "label: ",
            "--- ",
            "// pereplyot-notebook ",
            "\"",
            "\\\"",
            "9999999999999999999",
        ];
        let n = (self.next() % (max_pieces as u64 + 1)) as usize;
        (0..n)
            .map(|_| PIECES[(self.next() % PIECES.len() as u64) as usize])
            .collect()
    }
}

fn seeds() -> u64 {
    std::env::var("FUZZ_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20000)
}

fn each(name: &str, body: impl Fn(&mut Rng) + std::panic::RefUnwindSafe) {
    for seed in 1..=seeds() {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let result = std::panic::catch_unwind(|| {
            let mut local = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            body(&mut local)
        });
        assert!(result.is_ok(), "{name} panicked for seed {seed}");
        let _ = rng.next();
    }
}

#[test]
#[ignore]
fn fuzz_deep_links() {
    each("deeplink", |r| {
        let s = format!("pereplyot://open?{}", r.text(12));
        let _ = crate::deeplink::parse(&s);
        let _ = crate::deeplink::parse(&r.text(12));
        let _ = crate::deeplink::file_uri_path(&format!("file://{}", r.text(8)));
        let link = crate::deeplink::DeepLink {
            hash: r.text(6),
            annotation: Some(r.text(6)),
            page: Some(r.next() as u32),
        };
        let built = crate::deeplink::build(&link);
        let _ = crate::deeplink::parse(&built);
    });
}

#[test]
#[ignore]
fn fuzz_anchors() {
    each("anchor", |r| {
        let text = r.text(40);
        let snippet = r.text(8);
        let (p, s) = (r.text(4), r.text(4));
        let hint = Some((r.next() as usize % 200, r.next() as usize % 200));
        let _ = crate::anchor::locate(&text, &snippet, Some(&p), Some(&s), hint);
        let _ = crate::anchor::locate(&text, &snippet, None, None, None);
        let _ = crate::anchor::normalise(&text);
        let offsets: Vec<usize> = (0..5).map(|_| r.next() as usize % 50).collect();
        let _ = crate::anchor::to_utf16(&text, &offsets);
    });
}

#[test]
#[ignore]
fn fuzz_search_queries() {
    each("index", |r| {
        let q = crate::index::Query::parse(&r.text(20));
        let doc = crate::index::Loaded::from_units(
            crate::index::DocEntry {
                hash: "h".into(),
                title: r.text(3),
                path: std::path::PathBuf::from("/x.pdf"),
                kind: crate::history::DocKind::Pdf,
                labels: vec!["1".into(), "2".into()],
                chapters: false,
                ocr_pages: 0,
            },
            vec![r.text(30), r.text(30)],
        );
        let _ = crate::index::search(&[doc], &q, &|_| true, 50);
    });
}

#[test]
#[ignore]
fn fuzz_notebooks() {
    each("notebook", |r| {
        let body = format!(
            "// pereplyot-notebook {{\"title\":\"{}\"}}\n#pquote(hash: \"{}\", id: \"{}\", page: \"{}\", label: \"{}\", text: \"{}\", note: \"{}\")\n{}",
            r.text(3),
            r.text(3),
            r.text(3),
            r.text(2),
            r.text(2),
            r.text(6),
            r.text(6),
            r.text(20)
        );
        let nb = crate::notebook::Notebook::parse("stem", &body);
        for format in [
            fond_annot::export::Format::Typst,
            fond_annot::export::Format::Markdown,
            fond_annot::export::Format::Latex,
        ] {
            let opts = crate::notebook::ExportOptions {
                format,
                bibliography: Some("refs.bib"),
                key_for: &|_| Some("key".into()),
                image_for: &|_| None,
                connections_of: &|_| Vec::new(),
                links: true,
            };
            let _ = crate::notebook::export(&nb, &opts);
        }
        let _ = nb.to_file();
        let _ = nb.body();
    });
}

#[test]
#[ignore]
fn fuzz_sidecars() {
    each("sidecar", |r| {
        let val = |r: &mut Rng| -> String {
            match r.next() % 9 {
                0 => "null".into(),
                1 => format!("{}", r.next() as i64),
                2 => "1e999".into(),
                3 => "-0.0".into(),
                4 => format!(
                    "\"{}\"",
                    r.text(3)
                        .replace(['"', '\\', '\n', '\r', '\t', '\u{0}'], "")
                ),
                5 => "[]".into(),
                6 => format!("[{}, {}]", r.next() % 100, r.next() % 100),
                7 => "{\"a\":1}".into(),
                _ => "true".into(),
            }
        };
        let kinds = [
            "highlight",
            "underline",
            "strikeout",
            "note",
            "area",
            "weird",
            "\u{0}",
        ];
        let mut items = Vec::new();
        for _ in 0..(r.next() % 4) {
            let kind = kinds[(r.next() % kinds.len() as u64) as usize];
            let mut fields = vec![
                format!("\"id\":\"i{}\"", r.next() % 5),
                format!("\"kind\":\"{kind}\""),
            ];
            for key in [
                "page",
                "chapter",
                "snippet",
                "color",
                "note",
                "created",
                "quadpoints",
                "rect",
                "tags",
                "position",
                "page_label",
                "pos",
                "image",
            ] {
                if r.next() % 2 == 0 {
                    fields.push(format!("\"{key}\":{}", val(r)));
                }
            }
            items.push(format!("{{{}}}", fields.join(",")));
        }
        let json = format!(
            "{{\"schema\":{},\"key\":\"k\",\"annotations\":[{}]}}",
            1 + r.next() % 3,
            items.join(",")
        );
        if let Ok(sidecar) =
            fond_annot::AnnotationSidecar::parse(&json, std::path::Path::new("t.json"))
        {
            for a in &sidecar.annotations {
                let _ = a.rect();
                let _ = a.tags();
                let _ = crate::page_label_of(a);
                let _ = crate::image_of(a);
                let _ = crate::text_position_of(a);
            }
            let _ = sidecar.to_json();
        }
    });
}
