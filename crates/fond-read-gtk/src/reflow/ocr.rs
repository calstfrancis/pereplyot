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
pub fn parse_tsv(tsv: &str, dpi: f32) -> Vec<Word> {
    let points = 72.0 / dpi;
    tsv.lines()
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
            let (left, top, width, height) = (num(6)?, num(7)?, num(8)?, num(9)?);
            Some(Word {
                text: text.to_string(),
                x0: left * points,
                y0: top * points,
                x1: (left + width) * points,
                y1: (top + height) * points,
                size: height * points * 0.9,
                bold: false,
                italic: false,
            })
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

/// The folder Tesseract looks for language data in: ours if it has any, otherwise whatever the
/// environment already says (so a system install keeps working).
fn tessdata_prefix() -> std::ffi::OsString {
    let ours = tessdata_dir();
    if ours.is_dir() {
        ours.into_os_string()
    } else {
        std::env::var_os("TESSDATA_PREFIX").unwrap_or_default()
    }
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
    fn a_header_only_or_empty_result_is_no_words() {
        assert!(parse_tsv("", 300.0).is_empty());
        assert!(parse_tsv("level\tpage_num\n", 300.0).is_empty());
    }
}
