// Scanned pages have no PDFium text layer, so they are recognised in the background as soon as
// the document opens (page in view first) and drag selection answers from those words.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use super::*;
use crate::reflow::model::Word;
use crate::reflow::ocr;

// Tesseract is single-threaded per page; two keeps the page in view and the next one going
// without taking the whole machine.
const WORKERS: usize = 2;

#[derive(Default)]
pub(super) struct ScanText {
    // Present only for pages with no text layer of their own, so presence means "use these".
    pages: HashMap<u16, Rc<Vec<Word>>>,
    active: bool,
    remaining: usize,
    // Text-layer pages laid out in side-by-side columns, as words in reading order; `None` for
    // pages that aren't, so a plain page keeps PDFium's own selection.
    columns: RefCell<HashMap<u16, Option<Rc<Vec<Word>>>>>,
}

impl ScanText {
    pub(super) fn words(&self, page: u16) -> Option<Rc<Vec<Word>>> {
        self.pages.get(&page).cloned()
    }

    fn column_words(&self, r: &ReaderState, page: u16) -> Option<Rc<Vec<Word>>> {
        if let Some(hit) = self.columns.borrow().get(&page) {
            return hit.clone();
        }
        let found = r
            .doc
            .as_ref()
            .and_then(|doc| crate::reflow::extract::extract_page(doc, page))
            .and_then(|raw| column_order(&raw.words))
            .map(Rc::new);
        self.columns.borrow_mut().insert(page, found.clone());
        found
    }

    pub(super) fn is_pending(&self, page: u16) -> bool {
        self.active && self.remaining > 0 && !self.pages.contains_key(&page)
    }
}

struct Shared {
    todo: Vec<u16>,
}

enum Found {
    Words(Vec<Word>),
    HasTextLayer,
}

pub(super) fn start(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<ReaderState>>,
    path: PathBuf,
    program: PathBuf,
) {
    let (count, first) = {
        let mut r = reader.borrow_mut();
        if r.scan_text.active {
            return;
        }
        r.scan_text.active = true;
        r.scan_text.remaining = r.count as usize;
        (r.count, r.page)
    };
    host.notify(
        "This scan has no text layer — recognising it in the background. Text becomes \
         selectable page by page, starting with this one.",
    );

    let focus = Arc::new(AtomicU16::new(first));
    let shared = Arc::new(Mutex::new(Shared {
        todo: (0..count).collect(),
    }));
    let (tx, rx) = mpsc::channel::<(u16, Found)>();
    let language = ocr::language();
    let cache_dir = ocr::cache_dir(&path);
    for _ in 0..WORKERS {
        let (shared, focus, tx) = (shared.clone(), focus.clone(), tx.clone());
        let (path, program, language, cache_dir) = (
            path.clone(),
            program.clone(),
            language.clone(),
            cache_dir.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("pdf-recognise".into())
            .spawn(move || worker(&shared, &focus, &tx, &path, &program, &language, &cache_dir));
        if let Err(e) = spawned {
            eprintln!("could not start a recognition thread: {e}");
        }
    }
    drop(tx);

    let weak = Rc::downgrade(reader);
    glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
        let Some(reader) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        focus.store(reader.borrow().page, Ordering::Relaxed);
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok((page, found)) => {
                    let mut r = reader.borrow_mut();
                    r.scan_text.remaining = r.scan_text.remaining.saturating_sub(1);
                    if let Found::Words(words) = found {
                        r.scan_text.pages.insert(page, Rc::new(words));
                        changed = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    reader.borrow_mut().scan_text.remaining = 0;
                    break;
                }
            }
        }
        if changed {
            // Marks and the search overlay may have been waiting on this page's text.
            mark_layer::redraw_all(&reader);
        }
        if reader.borrow().scan_text.remaining == 0 {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

// Nearest to the page being read, looking forward more eagerly than back.
fn take_next(shared: &Mutex<Shared>, focus: u16) -> Option<u16> {
    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
    let at = s
        .todo
        .iter()
        .enumerate()
        .min_by_key(|(_, &p)| {
            let d = p as i32 - focus as i32;
            if d >= 0 {
                d
            } else {
                -d * 3
            }
        })
        .map(|(i, _)| i)?;
    Some(s.todo.swap_remove(at))
}

fn worker(
    shared: &Mutex<Shared>,
    focus: &AtomicU16,
    tx: &mpsc::Sender<(u16, Found)>,
    path: &std::path::Path,
    program: &std::path::Path,
    language: &str,
    cache_dir: &std::path::Path,
) {
    let Ok(pdfium) = crate::pdfium::get() else {
        return;
    };
    let Ok(doc) = pdfium.load_pdf_from_file(path, None) else {
        return;
    };
    while let Some(index) = take_next(shared, focus.load(Ordering::Relaxed)) {
        let has_text =
            doc.pages().get(index).ok().is_some_and(|page| {
                crate::page_text(&page).split_whitespace().take(3).count() >= 3
            });
        let found = if has_text {
            Found::HasTextLayer
        } else {
            Found::Words(
                ocr::ocr_page(&doc, index, program, language, cache_dir)
                    .map(|p| p.words)
                    .unwrap_or_default(),
            )
        };
        if tx.send((index, found)).is_err() {
            return;
        }
    }
}

fn column_order(words: &[Word]) -> Option<Vec<Word>> {
    use crate::reflow::layout::reading_order;
    let regions = reading_order(words);
    let boxes: Vec<[f32; 4]> = regions
        .iter()
        .map(|lines| {
            lines.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, l| {
                [b[0].min(l.x0), b[1].min(l.y0), b[2].max(l.x1), b[3].max(l.y1)]
            })
        })
        .collect();
    let side_by_side = boxes.iter().enumerate().any(|(i, a)| {
        boxes[i + 1..]
            .iter()
            .any(|b| a[1].max(b[1]) < a[3].min(b[3]) && (a[2] <= b[0] || b[2] <= a[0]))
    });
    side_by_side.then(|| {
        regions
            .into_iter()
            .flatten()
            .flat_map(|l| l.words)
            .collect()
    })
}

fn between(words: &[Word], from: (f32, f32), to: (f32, f32)) -> Vec<&Word> {
    let nearest = |(x, y): (f32, f32)| -> Option<usize> {
        // Distance from the point to a word's box; zero inside it. A word on the pointer's line
        // wins over a nearer one on another line.
        let dist = |w: &Word| {
            let dx = (w.x0 - x).max(0.0).max(x - w.x1);
            let dy = (w.y0 - y).max(0.0).max(y - w.y1);
            (dy * 4.0 + dx, dy)
        };
        words
            .iter()
            .enumerate()
            .min_by(|a, b| {
                let (da, db) = (dist(a.1), dist(b.1));
                da.0.total_cmp(&db.0).then(da.1.total_cmp(&db.1))
            })
            .map(|(i, _)| i)
    };
    let (Some(a), Some(b)) = (nearest(from), nearest(to)) else {
        return Vec::new();
    };
    words[a.min(b)..=a.max(b)].iter().collect()
}

fn inside(words: &[Word], rect: [f32; 4]) -> Vec<&Word> {
    words
        .iter()
        .filter(|w| {
            let (cx, cy) = ((w.x0 + w.x1) / 2.0, w.centre_y());
            cx >= rect[0] && cx <= rect[2] && cy >= rect[1] && cy <= rect[3]
        })
        .collect()
}

fn gather(chosen: &[&Word]) -> Option<(String, Vec<[f32; 4]>)> {
    if chosen.is_empty() {
        return None;
    }
    let text = chosen
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut lines: Vec<[f32; 4]> = Vec::new();
    let mut last: Option<&Word> = None;
    for w in chosen {
        let same_line = last.is_some_and(|p| (w.y0 - p.y0).abs() < (p.y1 - p.y0) * 0.5);
        match (same_line, lines.last_mut()) {
            (true, Some(line)) => {
                line[0] = line[0].min(w.x0);
                line[1] = line[1].min(w.y0);
                line[2] = line[2].max(w.x1);
                line[3] = line[3].max(w.y1);
            }
            _ => lines.push([w.x0, w.y0, w.x1, w.y1]),
        }
        last = Some(w);
    }
    Some((text, lines))
}

fn to_quads(geom: PageGeom, rects: &[[f32; 4]]) -> Vec<[f64; 8]> {
    let (w, h) = geom.display_size();
    let (w, h) = (w as f64, h as f64);
    rects
        .iter()
        .map(|b| {
            let corner = |x: f32, y: f32| geom.px_to_pdf(x as f64, y as f64, w, h);
            let (tl, tr) = (corner(b[0], b[1]), corner(b[2], b[1]));
            let (bl, br) = (corner(b[0], b[3]), corner(b[2], b[3]));
            [tl.0, tl.1, tr.0, tr.1, bl.0, bl.1, br.0, br.1]
        })
        .collect()
}

pub(super) fn select_range(
    r: &ReaderState,
    page: u16,
    from: (f64, f64),
    to: (f64, f64),
) -> Option<fond_doc::TextSelection> {
    let Some(words) = r
        .scan_text
        .words(page)
        .or_else(|| r.scan_text.column_words(r, page))
    else {
        return fond_doc::select_text_range(
            r.pdfium,
            r.bytes(),
            page,
            from.0 as f32,
            from.1 as f32,
            to.0 as f32,
            to.1 as f32,
        )
        .ok()
        .flatten();
    };
    let geom = r.geom(page)?;
    let (w, h) = geom.display_size();
    let at = |p: (f64, f64)| {
        let (x, y) = geom.pdf_to_px(p.0, p.1, w as f64, h as f64);
        (x as f32, y as f32)
    };
    let (text, lines) = gather(&between(&words, at(from), at(to)))?;
    Some(fond_doc::TextSelection {
        text,
        quads: to_quads(geom, &lines),
    })
}

pub(super) fn select_in_rect(
    r: &ReaderState,
    page: u16,
    rect: [f64; 4],
) -> Option<fond_doc::TextSelection> {
    let Some(words) = r
        .scan_text
        .words(page)
        .or_else(|| r.scan_text.column_words(r, page))
    else {
        return fond_doc::select_text_in_rect(
            r.pdfium,
            r.bytes(),
            page,
            rect[0] as f32,
            rect[1] as f32,
            rect[2] as f32,
            rect[3] as f32,
        )
        .ok()
        .flatten();
    };
    let geom = r.geom(page)?;
    let (w, h) = geom.display_size();
    let a = geom.pdf_to_px(rect[0], rect[1], w as f64, h as f64);
    let b = geom.pdf_to_px(rect[2], rect[3], w as f64, h as f64);
    let display = [
        a.0.min(b.0) as f32,
        a.1.min(b.1) as f32,
        a.0.max(b.0) as f32,
        a.1.max(b.1) as f32,
    ];
    let (text, lines) = gather(&inside(&words, display))?;
    Some(fond_doc::TextSelection {
        text,
        quads: to_quads(geom, &lines),
    })
}

pub(super) fn page_text(r: &ReaderState, page: u16) -> String {
    if let Some(words) = r.scan_text.words(page) {
        return words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
    }
    r.doc
        .as_ref()
        .and_then(|doc| doc.pages().get(page).ok())
        .map(|p| crate::page_text(&p))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x0: f32, y0: f32) -> Word {
        Word {
            text: text.into(),
            x0,
            y0,
            x1: x0 + 40.0,
            y1: y0 + 10.0,
            size: 10.0,
            bold: false,
            italic: false,
        }
    }

    fn page() -> Vec<Word> {
        vec![
            word("The", 10.0, 10.0),
            word("quick", 60.0, 10.0),
            word("brown", 110.0, 10.0),
            word("fox", 10.0, 30.0),
            word("jumps", 60.0, 30.0),
            word("over", 110.0, 30.0),
        ]
    }

    #[test]
    fn a_drag_along_a_line_takes_the_words_it_touches() {
        let words = page();
        let (text, lines) = gather(&between(&words, (65.0, 15.0), (125.0, 15.0))).unwrap();
        assert_eq!(text, "quick brown");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], [60.0, 10.0, 150.0, 20.0]);
    }

    #[test]
    fn a_drag_across_lines_takes_everything_between_in_reading_order() {
        let words = page();
        let (text, lines) = gather(&between(&words, (115.0, 15.0), (65.0, 35.0))).unwrap();
        assert_eq!(text, "brown fox jumps");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn dragging_backwards_selects_the_same_words() {
        let words = page();
        let forward = gather(&between(&words, (15.0, 15.0), (65.0, 35.0)))
            .unwrap()
            .0;
        let backward = gather(&between(&words, (65.0, 35.0), (15.0, 15.0)))
            .unwrap()
            .0;
        assert_eq!(forward, backward);
        assert_eq!(forward, "The quick brown fox jumps");
    }

    #[test]
    fn a_point_in_the_margin_snaps_to_the_word_on_its_line() {
        let words = page();
        let (text, _) = gather(&between(&words, (0.0, 33.0), (300.0, 33.0))).unwrap();
        assert_eq!(text, "fox jumps over");
    }

    #[test]
    fn a_rectangle_takes_the_words_centred_in_it() {
        let words = page();
        let (text, _) = gather(&inside(&words, [0.0, 25.0, 200.0, 45.0])).unwrap();
        assert_eq!(text, "fox jumps over");
        assert!(gather(&inside(&words, [300.0, 300.0, 400.0, 400.0])).is_none());
    }

    #[test]
    fn nothing_recognised_selects_nothing() {
        assert!(gather(&between(&[], (0.0, 0.0), (10.0, 10.0))).is_none());
    }

    #[test]
    fn two_columns_are_ordered_one_after_the_other_and_one_column_is_not_special() {
        let mut words = Vec::new();
        for row in 0..8 {
            let y = 10.0 + row as f32 * 14.0;
            words.push(word(&format!("l{row}a"), 10.0, y));
            words.push(word(&format!("l{row}b"), 55.0, y));
            words.push(word(&format!("r{row}a"), 300.0, y));
            words.push(word(&format!("r{row}b"), 345.0, y));
        }
        let ordered = column_order(&words).expect("two columns");
        let texts: Vec<&str> = ordered.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts[0], "l0a");
        assert_eq!(texts[15], "l7b");
        assert_eq!(texts[16], "r0a");
        let single: Vec<Word> = words.iter().filter(|w| w.x0 < 100.0).cloned().collect();
        assert!(column_order(&single).is_none());
    }
}
