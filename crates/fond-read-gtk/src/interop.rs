//! Marks in and out of other PDF readers: what a PDF's own annotations become here, and what a
//! copy written for Acrobat, Preview or Okular is made of. The PDF work is `fond_doc::interop`;
//! this is the mapping to and from Pereplyot's annotations.

use fond_annot::{Annotation, AnnotationKind, AnnotationSidecar};
use fond_doc::interop::{MarkKind, ReadMark, WriteMark};

use crate::palette::HIGHLIGHT_COLORS;

fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ])
}

/// The palette colour closest to `rgb`, so a yellow from another reader becomes the reading
/// colour that is nearest it.
fn nearest_palette_hex(rgb: [u8; 3]) -> String {
    HIGHLIGHT_COLORS
        .iter()
        .min_by_key(|c| {
            let p = parse_hex(c.hex).unwrap_or([0, 0, 0]);
            (0..3)
                .map(|i| (p[i] as i32 - rgb[i] as i32).pow(2))
                .sum::<i32>()
        })
        .map(|c| c.hex.to_string())
        .unwrap_or_else(|| HIGHLIGHT_COLORS[0].hex.to_string())
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Pereplyot annotations for the marks read from a PDF.
pub fn annotations_from_marks(marks: &[ReadMark]) -> Vec<Annotation> {
    marks
        .iter()
        .filter_map(|m| {
            let kind = match m.kind {
                MarkKind::Highlight => AnnotationKind::Highlight,
                MarkKind::Underline => AnnotationKind::Underline,
                MarkKind::Strikeout => AnnotationKind::Strikeout,
                MarkKind::Note => AnnotationKind::Note,
                MarkKind::Area => return None,
            };
            let quads: Vec<[f64; 8]> = m
                .quads
                .iter()
                .map(|q| {
                    let mut out = [0.0; 8];
                    for (o, v) in out.iter_mut().zip(q) {
                        *o = *v as f64;
                    }
                    out
                })
                .collect();
            let mut a = Annotation::imported(
                kind,
                m.page as u32,
                quads,
                m.snippet.clone(),
                m.contents.clone(),
            );
            a.color = Some(nearest_palette_hex(m.color.unwrap_or([246, 195, 68])));
            if let Some([x, y]) = m.position {
                a.set_position(Some([x as f64, y as f64]));
            }
            Some(a)
        })
        .collect()
}

/// Of `incoming`, the ones not already in `sidecar`: a mark is already there if it has the same
/// id, or the same kind and words on the same page (as when the PDF is a copy this app wrote).
pub fn not_yet_present(sidecar: &AnnotationSidecar, incoming: Vec<Annotation>) -> Vec<Annotation> {
    let mut seen: Vec<(Option<u32>, AnnotationKind, String)> = sidecar
        .annotations
        .iter()
        .filter_map(|a| {
            Some((
                a.page,
                a.kind,
                collapse(a.snippet.as_deref().or(a.note.as_deref())?),
            ))
        })
        .collect();
    let mut out = Vec::new();
    for a in incoming {
        if sidecar.annotations.iter().any(|e| e.id == a.id) {
            continue;
        }
        let words = a
            .snippet
            .as_deref()
            .or(a.note.as_deref())
            .map(collapse)
            .unwrap_or_default();
        let key = (a.page, a.kind, words);
        if !key.2.is_empty() && seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(a);
    }
    out
}

/// What goes in a mark's contents: its note, then any tags set on it that the note doesn't
/// already carry as `#tag`.
fn contents_of(a: &Annotation) -> Option<String> {
    let mut text = a.note.clone().unwrap_or_default();
    let typed = a.tags();
    let extra: Vec<String> = a
        .explicit_tags()
        .into_iter()
        .filter(|t| {
            !fond_annot::annotation::parse_note_tags(&text).contains(t) && typed.contains(t)
        })
        .map(|t| format!("#{t}"))
        .collect();
    if !extra.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&extra.join(" "));
    }
    (!text.trim().is_empty()).then_some(text)
}

/// The marks of a sidecar as they are written into a copy of the PDF.
pub fn marks_to_write(sidecar: &AnnotationSidecar) -> Vec<WriteMark> {
    let f = |q: &[f64; 8]| {
        let mut out = [0.0f32; 8];
        for (o, v) in out.iter_mut().zip(q) {
            *o = *v as f32;
        }
        out
    };
    sidecar
        .annotations
        .iter()
        .filter_map(|a| {
            let page = u16::try_from(a.page?).ok()?;
            let color = a
                .color
                .as_deref()
                .and_then(parse_hex)
                .unwrap_or([246, 195, 68]);
            let contents = contents_of(a);
            let quads: Vec<[f32; 8]> = a.quadpoints.iter().map(f).collect();
            let blank = WriteMark {
                kind: MarkKind::Highlight,
                page,
                quads: Vec::new(),
                rect: None,
                position: None,
                contents,
                color,
            };
            Some(match a.kind {
                AnnotationKind::Highlight => WriteMark { quads, ..blank },
                AnnotationKind::Underline => WriteMark {
                    kind: MarkKind::Underline,
                    quads,
                    ..blank
                },
                AnnotationKind::Strikeout => WriteMark {
                    kind: MarkKind::Strikeout,
                    quads,
                    ..blank
                },
                AnnotationKind::Note if !quads.is_empty() => WriteMark { quads, ..blank },
                AnnotationKind::Note => {
                    let [x, y] = a.position()?;
                    WriteMark {
                        kind: MarkKind::Note,
                        position: Some([x as f32, y as f32]),
                        ..blank
                    }
                }
                AnnotationKind::Area => {
                    let [l, b, r, t] = a.rect()?;
                    WriteMark {
                        kind: MarkKind::Area,
                        rect: Some([l as f32, b as f32, r as f32, t as f32]),
                        contents: blank.contents.clone().or_else(|| a.snippet.clone()),
                        ..blank
                    }
                }
                _ => return None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sidecar(annotations: Vec<Annotation>) -> AnnotationSidecar {
        AnnotationSidecar {
            schema: 1,
            key: "k".into(),
            pdf_hash: None,
            annotations,
            extra: Default::default(),
        }
    }

    fn quad(x0: f64, x1: f64, top: f64) -> [f64; 8] {
        [x0, top, x1, top, x0, top - 10.0, x1, top - 10.0]
    }

    fn sample() -> AnnotationSidecar {
        let mut hl = Annotation::drawn(
            AnnotationKind::Highlight,
            1,
            vec![quad(72.0, 200.0, 700.0), quad(72.0, 150.0, 688.0)],
            Some("a marked passage".into()),
            Some("why this matters #key".into()),
            Some("#9CAF88".into()),
        );
        hl.id = "h1".into();
        hl.set_explicit_tags(&["key".into(), "extra".into()]);
        let mut ul = Annotation::drawn(
            AnnotationKind::Underline,
            1,
            vec![quad(72.0, 120.0, 660.0)],
            Some("underlined".into()),
            None,
            Some("#91A9B8".into()),
        );
        ul.id = "u1".into();
        let mut st = Annotation::drawn(
            AnnotationKind::Strikeout,
            1,
            vec![quad(72.0, 110.0, 640.0)],
            Some("struck".into()),
            None,
            Some("#C98F78".into()),
        );
        st.id = "s1".into();
        let mut sticky = Annotation::drawn(
            AnnotationKind::Note,
            1,
            vec![],
            None,
            Some("a loose thought".into()),
            None,
        );
        sticky.id = "n1".into();
        sticky.set_position(Some([400.0, 700.0]));
        let mut area = Annotation::area(
            1,
            [300.0, 400.0, 500.0, 600.0],
            Some("Fig. 1".into()),
            None,
            Some("#D6B86A".into()),
        );
        area.id = "a1".into();
        sidecar(vec![hl, ul, st, sticky, area])
    }

    #[test]
    fn marks_to_write_cover_every_kind_with_colours_and_contents() {
        let marks = marks_to_write(&sample());
        assert_eq!(marks.len(), 5);
        assert_eq!(marks[0].kind, MarkKind::Highlight);
        assert_eq!(marks[0].color, [0x9C, 0xAF, 0x88]);
        assert_eq!(
            marks[0].contents.as_deref(),
            Some("why this matters #key\n#extra")
        );
        assert_eq!(marks[3].position, Some([400.0, 700.0]));
        assert_eq!(marks[4].rect, Some([300.0, 400.0, 500.0, 600.0]));
        assert_eq!(marks[4].contents.as_deref(), Some("Fig. 1"));
    }

    #[test]
    fn a_colour_from_another_reader_becomes_the_nearest_palette_colour() {
        assert_eq!(nearest_palette_hex([255, 230, 0]), "#D6B86A");
        assert_eq!(nearest_palette_hex([140, 180, 140]), "#9CAF88");
    }

    #[test]
    fn importing_twice_adds_nothing_and_own_marks_are_not_duplicated() {
        let read = vec![ReadMark {
            kind: MarkKind::Highlight,
            page: 1,
            quads: vec![[72.0, 700.0, 200.0, 700.0, 72.0, 690.0, 200.0, 690.0]],
            contents: None,
            snippet: Some("a  marked passage".into()),
            color: Some([255, 255, 0]),
            position: None,
        }];
        let imported = annotations_from_marks(&read);
        let mut existing = sample();
        assert!(not_yet_present(&existing, imported.clone()).is_empty());
        existing.annotations.clear();
        let fresh = not_yet_present(&existing, imported.clone());
        assert_eq!(fresh.len(), 1);
        existing.annotations.extend(fresh);
        assert!(not_yet_present(&existing, imported).is_empty());
    }

    /// Marks written into a copy of a real PDF read back with their kinds, colours, notes and
    /// places (skipped without PDFium).
    #[test]
    fn a_saved_copy_reads_back_with_every_mark() {
        let Ok(pdfium) = crate::pdfium::get() else {
            eprintln!("no PDFium library; skipping");
            return;
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/figures.pdf");
        let bytes = std::fs::read(path).unwrap();
        let written = fond_doc::interop::write_marks(pdfium, &bytes, &marks_to_write(&sample()))
            .expect("written");
        if let Ok(out) = std::env::var("DUMP_ANNOTATED_COPY") {
            std::fs::write(out, &written).unwrap();
        }
        let read = fond_doc::interop::read_marks(pdfium, &written).expect("read back");
        let kinds: Vec<MarkKind> = read.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds.iter().filter(|k| **k == MarkKind::Highlight).count(),
            1,
            "{kinds:?}"
        );
        assert!(kinds.contains(&MarkKind::Underline), "{kinds:?}");
        assert!(kinds.contains(&MarkKind::Strikeout), "{kinds:?}");
        let note = read
            .iter()
            .find(|m| m.kind == MarkKind::Note)
            .expect("sticky note");
        assert_eq!(note.contents.as_deref(), Some("a loose thought"));
        let p = note.position.unwrap();
        assert!(
            (p[0] - 400.0).abs() < 1.0 && (p[1] - 700.0).abs() < 1.0,
            "{p:?}"
        );
        let hl = read.iter().find(|m| m.kind == MarkKind::Highlight).unwrap();
        assert_eq!(hl.quads.len(), 2);
        assert_eq!(
            hl.contents.as_deref(),
            Some("why this matters #key\n#extra")
        );
        let c = hl.color.expect("a colour");
        assert!(
            (c[0] as i32 - 0x9C).abs() < 4 && (c[1] as i32 - 0xAF).abs() < 4,
            "{c:?}"
        );
        let back = annotations_from_marks(&read);
        assert!(back
            .iter()
            .any(|a| a.kind == AnnotationKind::Highlight && a.color.as_deref() == Some("#9CAF88")));
        assert!(back
            .iter()
            .any(|a| a.kind == AnnotationKind::Note && a.position() == Some([400.0, 700.0])));
    }
}
