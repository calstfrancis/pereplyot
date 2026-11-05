use super::*;

fn source(hash: &str, title: &str, key: Option<&str>) -> Source {
    Source {
        hash: hash.into(),
        title: title.into(),
        key: key.map(str::to_string),
    }
}

fn mark(hash: &str, id: &str, label: &str, text: &str, tags: &[&str]) -> Mark {
    Mark {
        hash: hash.into(),
        id: id.into(),
        label: label.into(),
        text: text.into(),
        note: String::new(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
    }
}

fn def() -> Def {
    Def {
        title: "Q and the sources".into(),
        shelf: None,
        columns: vec![
            Column {
                name: "Method".into(),
                tags: vec!["method".into()],
            },
            Column {
                name: "Date of Q".into(),
                tags: vec!["q-date".into(), "dating".into()],
            },
        ],
        notes: BTreeMap::new(),
    }
}

fn fixture() -> Matrix {
    let sources = [
        source("a", "Kloppenborg", Some("kloppenborg1987")),
        source("b", "Farrer", None),
    ];
    let marks = [
        mark("a", "1", "41", "Q was composed in stages.", &["#Method"]),
        mark("a", "2", "12", "The first layer is wisdom.", &["q-date"]),
        mark(
            "a",
            "3",
            "50",
            "An aside with a \"quote\", comma, and | pipe.",
            &["method"],
        ),
        mark("b", "4", "7", "Dated to the 50s.", &["dating"]),
        mark("b", "5", "9", "Untagged remark.", &[]),
    ];
    build(&def(), &sources, &marks)
}

#[test]
fn marks_land_in_their_cells_and_empty_cells_stay_empty() {
    let m = fixture();
    assert_eq!(m.columns, ["Method", "Date of Q"]);
    assert_eq!(m.rows[0].cells[0].marks.len(), 2);
    assert_eq!(m.rows[0].cells[1].marks.len(), 1);
    assert!(
        m.rows[1].cells[0].is_empty(),
        "Farrer says nothing about method"
    );
    assert_eq!(m.rows[1].cells[1].marks[0].id, "4");
}

#[test]
fn marks_in_a_cell_are_in_page_order_and_tags_ignore_case_and_hash() {
    let m = fixture();
    let labels: Vec<&str> = m.rows[0].cells[0]
        .marks
        .iter()
        .map(|k| k.label.as_str())
        .collect();
    assert_eq!(labels, ["41", "50"]);
}

#[test]
fn your_notes_are_kept_per_cell() {
    let mut d = def();
    d.notes
        .insert(note_key("b", "Method"), "No method stated".into());
    let m = build(&d, &[source("b", "Farrer", None)], &[]);
    assert_eq!(m.rows[0].cells[0].note, "No method stated");
    assert!(!m.rows[0].cells[0].is_empty());
}

#[test]
fn csv_quotes_what_needs_it() {
    let csv = to_csv(&fixture());
    let mut lines = csv.lines();
    assert_eq!(lines.next(), Some("Source,Method,Date of Q"));
    let first = lines.next().unwrap();
    assert!(first.starts_with("Kloppenborg,"));
    assert!(first.contains("\"\"quote\"\""), "{first}");
    assert!(csv.lines().nth(2).unwrap().starts_with("Farrer,,"), "{csv}");
}

#[test]
fn markdown_escapes_pipes() {
    let md = to_markdown(&fixture());
    assert!(md.contains("| Source | Method | Date of Q |"));
    assert!(md.contains("\\| pipe"), "{md}");
}

#[test]
fn typst_marks_empty_cells_and_cites_keyed_sources() {
    let t = to_typst(&fixture(), true);
    assert!(t.contains("[@kloppenborg1987]"));
    assert!(t.contains("[Farrer]"));
    assert!(t.contains("table.cell(fill: luma(240))[]"));
}

fn on_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(name))
            .find(|f| f.is_file())
    })
}

#[test]
fn the_typst_table_compiles() {
    let Some(typst) = on_path("typst") else {
        eprintln!("no typst on PATH; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("pereplyot-matrix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("refs.yml"),
        "kloppenborg1987:\n  type: book\n  title: \"The Formation of Q\"\n  author: \"Kloppenborg, John S.\"\n  date: 1987\n",
    )
    .unwrap();
    for (name, cite) in [("plain.typ", false), ("cited.typ", true)] {
        let mut text = to_typst(&fixture(), cite);
        if cite {
            text.push_str("\n#bibliography(\"refs.yml\")\n");
        }
        std::fs::write(dir.join(name), text).unwrap();
        let out = std::process::Command::new(&typst)
            .arg("compile")
            .arg(dir.join(name))
            .arg(dir.join(format!("{name}.pdf")))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
