//! Recognising text on scanned pages with Tesseract, run as a subprocess one page at a time, so
//! a scan goes through the same layout as a born-digital PDF. Results are cached per page, since
//! recognising a page takes seconds.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use pdfium_render::prelude::{PdfDocument, PdfRenderConfig, Pixels};

use super::model::{RawPage, Word};

const DPI: f32 = 300.0;
/// Words Tesseract is less sure of than this (0–100) are left out rather than shown as noise.
const MIN_CONFIDENCE: f32 = 35.0;

/// The `tesseract` program, if there is one to run.
pub fn tesseract() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join("tesseract"))
                .collect()
        })
        .unwrap_or_default();
    candidates.push(PathBuf::from("/app/bin/tesseract"));
    candidates.into_iter().find(|p| p.is_file())
}

/// Where downloaded language data lives, searched after the system's own.
pub fn tessdata_dir() -> PathBuf {
    glib::home_dir().join(".local/share/pereplyot/tessdata")
}

/// Turn Tesseract's TSV output for a page rendered at `dpi` into words in page points.
///
/// A word's box is only as tall as its own letters, so "a" is shorter than "liturgy"; sizes and
/// vertical extents are taken from the whole line instead, or one line would read as several
/// type sizes.
pub fn parse_tsv(tsv: &str, dpi: f32) -> Vec<Word> {
    let points = 72.0 / dpi;
    struct Row<'a> {
        line: (&'a str, &'a str, &'a str),
        text: &'a str,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
    }
    let rows: Vec<Row> = tsv
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 12 || f[0] != "5" {
                return None;
            }
            let confidence: f32 = f[10].parse().ok()?;
            let text = f[11].trim();
            if confidence < MIN_CONFIDENCE || text.is_empty() {
                return None;
            }
            let num = |i: usize| f[i].parse::<f32>().ok();
            Some(Row {
                line: (f[2], f[3], f[4]),
                text,
                left: num(6)?,
                top: num(7)?,
                width: num(8)?,
                height: num(9)?,
            })
        })
        .collect();
    type LineKey<'a> = (&'a str, &'a str, &'a str);
    let mut lines: std::collections::HashMap<LineKey, (Vec<f32>, f32, f32)> =
        std::collections::HashMap::new();
    for r in &rows {
        let entry = lines
            .entry(r.line)
            .or_insert((Vec::new(), f32::MAX, f32::MIN));
        entry.0.push(r.height);
        entry.1 = entry.1.min(r.top);
        entry.2 = entry.2.max(r.top + r.height);
    }
    for (heights, _, _) in lines.values_mut() {
        heights.sort_by(|a, b| a.total_cmp(b));
    }
    rows.iter()
        .map(|r| {
            let (heights, top, bottom) = &lines[&r.line];
            let median = heights[heights.len() / 2];
            Word {
                text: r.text.to_string(),
                x0: r.left * points,
                y0: top * points,
                x1: (r.left + r.width) * points,
                y1: bottom * points,
                size: median * points * 0.9,
                bold: false,
                italic: false,
            }
        })
        .collect()
}

fn cache_file(cache: &Path, page: u16, lang: &str) -> PathBuf {
    cache.join(format!("{lang}-{page}.tsv"))
}

/// A cache folder name for the document at `path`, from where it is and when it last changed.
pub fn cache_key(path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(&mut h);
        meta.modified().ok().hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// The folder a document's recognised pages are kept in.
pub fn cache_dir(path: &Path) -> PathBuf {
    glib::user_cache_dir()
        .join("pereplyot")
        .join("ocr")
        .join(cache_key(path))
}

/// Page `index` as recognised earlier, if it was; never runs Tesseract.
pub fn cached_page(doc: &PdfDocument<'_>, index: u16, lang: &str, cache: &Path) -> Option<RawPage> {
    let tsv = std::fs::read_to_string(cache_file(cache, index, lang)).ok()?;
    let words = parse_tsv(&tsv, DPI);
    if words.is_empty() {
        return None;
    }
    let page = doc.pages().get(index).ok()?;
    Some(RawPage {
        page: index,
        width: page.width().value,
        height: page.height().value,
        words,
        graphics: Vec::new(),
    })
}

/// The text of page `index` as recognised earlier in any language, for the search index.
pub fn cached_text(path: &Path, index: u16) -> Option<String> {
    let dir = cache_dir(path);
    let suffix = format!("-{index}.tsv");
    let file = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with(&suffix))?
        .path();
    let words = parse_tsv(&std::fs::read_to_string(file).ok()?, DPI);
    let text = words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.trim().is_empty()).then_some(text)
}

/// How many pages of the document at `path` have been recognised.
pub fn cached_pages(path: &Path) -> usize {
    std::fs::read_dir(cache_dir(path)).map_or(0, |d| {
        d.flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tsv"))
            .count()
    })
}

/// The words on page `index`, recognised from a 300 dpi render of it. `None` if Tesseract could
/// not be run or found nothing.
pub fn ocr_page(
    doc: &PdfDocument<'_>,
    index: u16,
    program: &Path,
    lang: &str,
    cache: &Path,
) -> Option<RawPage> {
    let page = doc.pages().get(index).ok()?;
    let (width, height) = (page.width().value, page.height().value);
    let cached = cache_file(cache, index, lang);
    let tsv = match std::fs::read_to_string(&cached) {
        Ok(tsv) => tsv,
        Err(_) => {
            let target = (width * DPI / 72.0) as Pixels;
            let bitmap = page
                .render_with_config(&PdfRenderConfig::new().set_target_width(target))
                .ok()?;
            let rgba = bitmap.as_rgba_bytes();
            let (w, h) = (bitmap.width() as usize, bitmap.height() as usize);
            let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
            for px in rgba.chunks_exact(4) {
                ppm.extend_from_slice(&px[..3]);
            }
            let tsv = run(program, &ppm, lang)?;
            let _ = std::fs::create_dir_all(cache);
            let _ = std::fs::write(&cached, &tsv);
            tsv
        }
    };
    let words = parse_tsv(&tsv, DPI);
    (!words.is_empty()).then_some(RawPage {
        page: index,
        width,
        height,
        words,
        graphics: Vec::new(),
    })
}

fn run(program: &Path, image: &[u8], lang: &str) -> Option<String> {
    let mut child = Command::new(program)
        .args(["stdin", "stdout", "-l", lang, "--psm", "3", "tsv"])
        .env("TESSDATA_PREFIX", tessdata_prefix())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let data = image.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let output = child.wait_with_output().ok()?;
    let _ = writer.join();
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Where installed language data may be found besides our own downloads, best first.
fn system_tessdata_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(env) = std::env::var_os("TESSDATA_PREFIX") {
        dirs.push(PathBuf::from(env));
    }
    dirs.push(PathBuf::from("/app/share/tessdata"));
    dirs.push(PathBuf::from("/usr/share/tessdata"));
    if let Ok(entries) = std::fs::read_dir("/usr/share/tesseract-ocr") {
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path().join("tessdata"))
            .collect();
        versions.sort();
        versions.reverse();
        dirs.extend(versions);
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

/// The folder Tesseract looks for language data in. Tesseract reads one folder, so when languages
/// have been downloaded they are linked together with the installed ones (downloads winning) in a
/// folder of their own; with none downloaded, the installed ones are used as they are.
fn tessdata_prefix() -> std::ffi::OsString {
    let ours = tessdata_dir();
    let system = system_tessdata_dirs();
    let has_ours = std::fs::read_dir(&ours)
        .map(|d| {
            d.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "traineddata"))
        })
        .unwrap_or(false);
    if !has_ours {
        return system
            .into_iter()
            .next()
            .unwrap_or_default()
            .into_os_string();
    }
    let union = glib::user_cache_dir().join("pereplyot").join("tessdata");
    let _ = std::fs::remove_dir_all(&union);
    if std::fs::create_dir_all(&union).is_err() {
        return ours.into_os_string();
    }
    for dir in std::iter::once(ours).chain(system) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|x| x == "traineddata") {
                let _ = std::os::unix::fs::symlink(&path, union.join(entry.file_name()));
            }
        }
    }
    union.into_os_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TSV: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
1\t1\t0\t0\t0\t0\t0\t0\t2550\t3300\t-1\t\n\
5\t1\t1\t1\t1\t1\t300\t600\t300\t60\t96.5\tHello\n\
5\t1\t1\t1\t1\t2\t650\t600\t310\t60\t91\tworld.\n\
5\t1\t1\t1\t1\t3\t980\t600\t100\t60\t12\tnoise\n\
5\t1\t1\t1\t2\t1\t300\t700\t200\t60\t90\t \n";

    #[test]
    fn words_come_out_in_points_and_unsure_ones_are_dropped() {
        let words = parse_tsv(TSV, 300.0);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "Hello");
        assert!((words[0].x0 - 72.0).abs() < 0.01);
        assert!((words[0].y0 - 144.0).abs() < 0.01);
        assert!((words[0].x1 - 144.0).abs() < 0.01);
        assert!(words[0].size > 10.0 && words[0].size < 16.0);
    }

    #[test]
    fn every_word_of_a_line_has_the_lines_size_and_extent() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
5\t1\t1\t1\t1\t1\t300\t600\t300\t60\t95\tliturgy\n\
5\t1\t1\t1\t1\t2\t650\t612\t60\t30\t95\ta\n\
5\t1\t1\t1\t1\t3\t750\t600\t300\t66\t95\ttestimony\n";
        let words = parse_tsv(tsv, 300.0);
        assert_eq!(words.len(), 3);
        assert!(words.iter().all(|w| w.size == words[0].size));
        assert!(words
            .iter()
            .all(|w| w.y0 == words[0].y0 && w.y1 == words[0].y1));
        assert!((words[1].y1 - 666.0 * 72.0 / 300.0).abs() < 0.01);
    }

    #[test]
    fn a_header_only_or_empty_result_is_no_words() {
        assert!(parse_tsv("", 300.0).is_empty());
        assert!(parse_tsv("level\tpage_num\n", 300.0).is_empty());
    }

    #[test]
    fn recognised_pages_are_found_by_page_number_in_any_language() {
        let doc = std::env::temp_dir().join(format!("pereplyot-ocr-doc-{}.pdf", std::process::id()));
        std::fs::write(&doc, b"%PDF-").unwrap();
        let dir = cache_dir(&doc);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fra-3.tsv"), TSV).unwrap();
        assert_eq!(cached_pages(&doc), 1);
        let text = cached_text(&doc, 3).unwrap();
        assert!(!text.is_empty());
        assert_eq!(cached_text(&doc, 4), None);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&doc);
    }
}
