use super::*;

fn w(text: &str, x0: f32, y: f32) -> Word {
    Word {
        text: text.into(),
        x0,
        y0: y,
        x1: x0 + text.chars().count() as f32 * 5.0,
        y1: y + 10.0,
    }
}

/// Three columns at x = 0, 120, 240; four rows 20 apart.
fn sample() -> Vec<Word> {
    vec![
        w("Source", 0.0, 0.0),
        w("Method", 120.0, 0.0),
        w("Year", 240.0, 0.0),
        w("Kloppenborg", 0.0, 20.0),
        w("literary", 120.0, 20.0),
        w("stages", 160.0, 20.0),
        w("1987", 240.0, 20.5),
        w("Farrer", 0.0, 40.0),
        w("redactional", 120.0, 40.0),
        w("1955", 240.0, 39.5),
        w("Goodacre", 0.0, 60.0),
        w("source", 120.0, 60.0),
        w("criticism", 160.0, 60.0),
        w("2002", 240.0, 60.0),
    ]
}

#[test]
fn words_become_rows_and_columns() {
    let g = grid(&sample()).unwrap();
    assert_eq!(g.len(), 4);
    assert_eq!(g[0], ["Source", "Method", "Year"]);
    assert_eq!(
        g[1],
        ["Kloppenborg", "literary stages", "1987"],
        "words in one cell are joined"
    );
    assert_eq!(g[3][1], "source criticism");
    assert_eq!(
        g[2][2], "1955",
        "a baseline a little off still sits in its row"
    );
}

#[test]
fn ordinary_text_is_not_a_table() {
    // Words set one after another at their natural widths, so their edges fall differently on each line.
    let lines = [
        "the matter has been argued at length and the evidence is",
        "mixed so that readers should weigh each claim with care",
        "before drawing any firm conclusion about it as shown before",
        "and see also the discussion of the earlier sources above",
    ];
    let text: Vec<Word> = lines
        .iter()
        .enumerate()
        .flat_map(|(r, l)| {
            let mut x = 0.0;
            l.split(' ')
                .map(|t| {
                    let word = w(t, x, r as f32 * 14.0);
                    x = word.x1 + 4.0;
                    word
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        grid(&text),
        None,
        "a paragraph has no channels to cut it by"
    );
    assert_eq!(grid(&sample()[..3]), None);
}

#[test]
fn an_empty_column_is_dropped() {
    let mut words = sample();
    words.push(w("Note", 360.0, 0.0));
    words.retain(|x| x.text != "Note");
    assert_eq!(grid(&words).unwrap()[0].len(), 3);
}

#[test]
fn csv_tsv_and_typst_exports() {
    let mut g = grid(&sample()).unwrap();
    g[1][1] = "has, comma and \"quote\"".into();
    assert!(to_csv(&g).contains("\"has, comma and \"\"quote\"\"\""));
    assert!(to_tsv(&g).lines().next().unwrap().split('\t').count() == 3);
    let t = to_typst(&g);
    assert!(t.starts_with("#table(\n  columns: 3,"));
    assert!(t.contains("[*Source*]"));
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
    let dir = std::env::temp_dir().join(format!("pereplyot-table-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut g = grid(&sample()).unwrap();
    g[1][0] = "A #hash, $dollar$ & *star* _under_".into();
    std::fs::write(dir.join("t.typ"), to_typst(&g)).unwrap();
    let out = std::process::Command::new(typst)
        .arg("compile")
        .arg(dir.join("t.typ"))
        .arg(dir.join("t.pdf"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
