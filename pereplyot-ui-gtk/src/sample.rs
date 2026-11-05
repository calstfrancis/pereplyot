//! The sample document the Welcome window offers: a short, fictional essay about reading with the four colours,
//! already marked in them, so the method is shown instead of described. Written to the data folder on demand.

use std::path::PathBuf;

use fond_annot::{Annotation, AnnotationKind, AnnotationSidecar};
use fond_read_gtk::palette::HIGHLIGHT_COLORS;

const PAGES: [&str; 3] = [
    "READING WITH FOUR COLOURS\n\
     A short sample essay, written for this tutorial. Every name and work in it is invented.\n\n\
     A reader who highlights everything has highlighted nothing. The remedy is not to mark less but to mark \
     with a question in mind, and Pereplyot gives that question four colours.\n\n\
     The first colour is for the key idea: what is the author saying? Mark the claim the chapter exists to \
     make, and nothing around it. The argument of an essay can usually be recovered from these marks alone, \
     which is a good test of whether they were chosen well.\n\n\
     The second colour is for evidence: what supports it? Quotations, examples, data and the passages \
     an author leans on belong here. When the evidence is thin, you will see it at once, because the blue \
     marks are few.\n\n\
     Try it: select any sentence on this page, then press 1, 2, 3 or 4. Press Ctrl+K and type 'notebook' \
     to see how marks become quotes you can cite.",
    "The third colour is for connection: why does it matter, and what does it join up with? This is where a \
     reader notes that a claim in one book answers a worry in another, or that an argument has a consequence \
     the author did not draw. Smith (2019) calls these the marks that survive into the essay.\n\n\
     The fourth colour is for the problem: what needs scrutiny? A claim you doubt, a contradiction, a step \
     that is missing. Mark it in terracotta and write a short note saying why. A note that begins with a \
     hashtag, such as #method, can be gathered with every other mark carrying it, across every document \
     you have read.\n\n\
     Notes can also ask you questions later. Write 'Q: What are the four colours for? A: A question each.' \
     with #remember in a note, and Review will ask it back to you at the pace a memory model sets.\n\n\
     This is the whole method. Marks are plain files beside the document, so nothing here depends on the \
     program, and a Notebook (press N) gathers quotes from any number of documents into one outline that \
     exports to Typst, Markdown or LaTeX with every quote cited (Jones and Lee 2015, 41).",
    "References\n\n\
     Jones, A., & Lee, B. (2015). Marking as a Way of Thinking. Example Press, Nowhere.\n\n\
     Smith, J. (2019). The Art of Reading Slowly. Example Press, Nowhere.\n\n\
     Kovacs, M. (2012). Notes Toward an Essay. Imaginary Journal of Reading, 4(2), 11-29.",
];

/// (colour index, words to mark, note) for each demo mark.
const MARKS: [(usize, &str, &str); 5] = [
    (
        0,
        "A reader who highlights everything has highlighted nothing.",
        "",
    ),
    (
        1,
        "Quotations, examples, data and the passages an author leans on belong here.",
        "",
    ),
    (
        2,
        "a claim in one book answers a worry in another",
        "Try tagging this #connection",
    ),
    (
        3,
        "A claim you doubt, a contradiction, a step that is missing.",
        "Why does this matter? #method",
    ),
    (
        0,
        "Marks are plain files beside the document, so nothing here depends on the program",
        "",
    ),
];

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        if para.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in para.split_whitespace() {
            if !line.is_empty() && line.len() + 1 + word.len() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

/// A plain PDF (Helvetica, US Letter) of the sample pages.
pub fn pdf() -> Vec<u8> {
    let mut objs: Vec<String> = vec![String::new(), String::new()];
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    let mut kids = Vec::new();
    for page in PAGES {
        let mut stream = String::from("BT /F1 11 Tf 14 TL 72 720 Td\n");
        for line in wrap(page, 84) {
            stream.push_str(&format!("({}) Tj T*\n", escape(&line)));
        }
        stream.push_str("ET");
        objs.push(format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ));
        let content = objs.len();
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {content} 0 R >>"
        ));
        kids.push(objs.len());
    }
    objs[0] = "<< /Type /Catalog /Pages 2 0 R >>".into();
    objs[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.iter()
            .map(|k| format!("{k} 0 R"))
            .collect::<Vec<_>>()
            .join(" "),
        kids.len()
    );
    let mut data = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(data.len());
        data.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
    }
    let xref = data.len();
    data.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for off in offsets {
        data.extend(format!("{off:010} 00000 n \n").bytes());
    }
    data.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    data
}

fn demo_sidecar(key: &str) -> AnnotationSidecar {
    let mut s = AnnotationSidecar::new(key);
    s.annotations = MARKS
        .iter()
        .enumerate()
        .map(|(i, (colour, words, note))| Annotation {
            id: format!("sample-{i}"),
            kind: AnnotationKind::Highlight,
            page: Some(1),
            chapter: None,
            quadpoints: vec![[0.0; 8]],
            snippet: Some((*words).to_string()),
            snippet_prefix: None,
            snippet_suffix: None,
            color: Some(HIGHLIGHT_COLORS[*colour].hex.to_string()),
            note: (!note.is_empty()).then(|| (*note).to_string()),
            created: Some("2026-01-01".into()),
            modified: None,
            extra: Default::default(),
        })
        .collect();
    s
}

/// Writes the sample PDF and its demo marks, if they are not there yet; the path of the PDF. Blocking.
pub fn ensure() -> Result<PathBuf, String> {
    let dir = crate::paths::data_dir().join("sample");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("Reading with four colours.pdf");
    if !path.is_file() {
        fond_read_gtk::fsutil::write_atomic(&path, &pdf()).map_err(|e| e.to_string())?;
    }
    let hash = crate::paths::hash_file(&path).map_err(|e| e.to_string())?;
    if !crate::reader_host::annotations_path(&hash).exists() {
        // The marks name their words; finding the words in the file gives them their places.
        let template = demo_sidecar(&hash);
        let (sidecar, _) = fond_read_gtk::carry::carry(&template, 3, &path, &hash)
            .ok_or("the sample couldn't be read")?;
        let json = sidecar.to_json().map_err(|e| e.to_string())?;
        fond_read_gtk::fsutil::write_atomic(
            &crate::reader_host::annotations_path(&hash),
            json.as_bytes(),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_is_a_real_pdf_whose_marks_are_all_found() {
        let Ok(pdfium) = fond_read_gtk::pdfium::get() else {
            eprintln!("no PDFium library; skipping");
            return;
        };
        let bytes = pdf();
        let text = fond_doc::extract_text(pdfium, &bytes).unwrap();
        assert_eq!(text.pages.len(), 3);
        assert!(text.pages[0].contains("READING WITH FOUR COLOURS"));
        assert!(text.pages[2].contains("Kovacs, M. (2012)"));

        let dir = std::env::temp_dir().join(format!("pereplyot-sample-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.pdf");
        std::fs::write(&path, &bytes).unwrap();
        let (sidecar, summary) =
            fond_read_gtk::carry::carry(&demo_sidecar("h"), 3, &path, "h").unwrap();
        assert_eq!(
            summary.orphaned + summary.uncertain,
            0,
            "every demo mark finds its words: {summary:?}"
        );
        assert!(sidecar
            .annotations
            .iter()
            .all(|a| a.quadpoints.iter().all(|q| q != &[0.0; 8])));
        let pages: Vec<u32> = sidecar.annotations.iter().filter_map(|a| a.page).collect();
        assert!(pages.iter().all(|p| (1..=3).contains(p)), "{pages:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sample_has_a_reference_list_the_references_tab_can_read() {
        let r = pereplyot_core::refs::parse_reference(
            "Smith, J. (2019). The Art of Reading Slowly. Example Press, Nowhere.",
        );
        assert_eq!(r.families, ["smith"]);
        assert_eq!(r.year, Some(2019));
        let found = pereplyot_core::refs::citations_in(
            "calls these the marks that survive. Smith (2019) calls them",
        );
        assert!(!found.is_empty());
    }
}
