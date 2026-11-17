//! Pereplyot's status file for the Porog launcher: the last three documents read and where
//! each was left, in `~/.local/share/fond/status/pereplyot.json` (the `fond-status` crate).
//!
//! Porog hands an entry's `args` back as `flatpak run io.github.calstfrancis.Pereplyot
//! <args…>`, so they are exactly the command line that reopens the document the way it was
//! opened: a bare file for a standalone open, or the file plus the `--vault=`/`--key=` or
//! `--annotations-file=` options when Kartoteka or Sputnik launched it.
//!
//! Every read and write happens on one worker thread, in the order the reader reported
//! them, so nothing here touches the disk from the GTK thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use fond_annot::Progress;
use fond_read_gtk::history::DocKind;
use fond_status::{ResumeEntry, Status};

use crate::reader_host::HostOverride;

const APP: &str = "pereplyot";

enum Msg {
    Opened {
        hash: String,
        kind: DocKind,
        title: String,
        args: Vec<String>,
        progress: Option<Progress>,
    },
    Progress {
        hash: String,
        progress: Progress,
    },
    Context {
        hash: String,
        context: Option<String>,
    },
    Flush(Sender<()>),
}

/// The worker's channel, started on first use. `None` when there is no home directory to
/// write under.
fn worker() -> Option<&'static Sender<Msg>> {
    static WORKER: OnceLock<Option<Sender<Msg>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let dir = fond_status::status_dir()?;
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("porog-status".into())
                .spawn(move || run(&dir, rx))
                .ok()?;
            Some(tx)
        })
        .as_ref()
}

/// A document was just opened in the reader. `args` come from [`resume_args`].
pub fn opened(
    kind: DocKind,
    hash: &str,
    title: &str,
    args: Vec<String>,
    progress: Option<Progress>,
) {
    if let Some(tx) = worker() {
        let _ = tx.send(Msg::Opened {
            hash: hash.to_string(),
            kind,
            title: title.to_string(),
            args,
            progress,
        });
    }
}

/// The reading position of an open document changed.
pub fn progress(hash: &str, progress: Progress) {
    if let Some(tx) = worker() {
        let _ = tx.send(Msg::Progress {
            hash: hash.to_string(),
            progress,
        });
    }
}

/// The line that says where in an open document the reader was: its newest highlight.
pub fn context(hash: &str, context: Option<String>) {
    if let Some(tx) = worker() {
        let _ = tx.send(Msg::Context {
            hash: hash.to_string(),
            context,
        });
    }
}

/// The text of the newest highlight in `sidecar` as a context line, or `None` when nothing is
/// highlighted. "Newest" is the latest `created` time; annotations with none count as older
/// than any with one, and among equals the one saved last wins.
pub fn newest_highlight(sidecar: &fond_annot::AnnotationSidecar) -> Option<String> {
    sidecar
        .annotations
        .iter()
        .enumerate()
        .filter(|(_, a)| a.snippet.as_deref().is_some_and(|s| !s.trim().is_empty()))
        .max_by(|(i, a), (j, b)| (&a.created, i).cmp(&(&b.created, j)))
        .and_then(|(_, a)| fond_status::context_line(a.snippet.as_deref()?))
}

/// Wait, briefly, for everything reported so far to reach the disk. Called at shutdown, so
/// the page a reader was closed on is not lost with the process.
pub fn flush() {
    if let Some(tx) = worker() {
        let (done, wait) = mpsc::channel();
        if tx.send(Msg::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_millis(500));
        }
    }
}

fn run(dir: &Path, rx: mpsc::Receiver<Msg>) {
    // Start from what the last run left, so the list survives a restart. A file that cannot
    // be read is replaced rather than kept.
    let mut status = Status::read(dir, APP)
        .ok()
        .flatten()
        .unwrap_or_else(|| Status::new(APP, ""));
    status.app = APP.to_string();
    let mut open: HashMap<String, (DocKind, ResumeEntry)> = HashMap::new();

    for msg in rx {
        let entry = match msg {
            Msg::Opened {
                hash,
                kind,
                title,
                args,
                progress,
            } => {
                // Reopening keeps the line saved last time: it is replaced when the reader marks
                // something new, not lost because the document was closed and opened.
                let context = status
                    .resume
                    .iter()
                    .find(|e| e.args == args)
                    .and_then(|e| e.context.clone());
                let entry = ResumeEntry {
                    title,
                    detail: progress.map(|p| position_text(kind, p)),
                    at: None,
                    context,
                    args,
                };
                open.insert(hash, (kind, entry.clone()));
                entry
            }
            Msg::Progress { hash, progress } => {
                let Some((kind, entry)) = open.get_mut(&hash) else {
                    continue;
                };
                let detail = Some(position_text(*kind, progress));
                if entry.detail == detail {
                    continue;
                }
                entry.detail = detail;
                entry.clone()
            }
            Msg::Context { hash, context } => {
                let Some((_, entry)) = open.get_mut(&hash) else {
                    continue;
                };
                if entry.context == context {
                    continue;
                }
                entry.context = context;
                entry.clone()
            }
            Msg::Flush(done) => {
                let _ = done.send(());
                continue;
            }
        };
        let now = fond_status::now_utc();
        status.push_resume(ResumeEntry {
            at: Some(now.clone()),
            ..entry
        });
        status.written = now;
        if let Err(e) = status.write(dir) {
            eprintln!("pereplyot: could not write the Porog status file: {e}");
        }
    }
}

/// Where a document was left, in the words Porog shows beside its title. A PDF's number is
/// its place in the file, not the number printed on the page, so it is given with the total.
fn position_text(kind: DocKind, progress: Progress) -> String {
    let unit = match kind {
        DocKind::Pdf => "p.",
        DocKind::Epub => "chapter",
    };
    if progress.of > 0 {
        format!("{unit} {} of {}", progress.page, progress.of)
    } else {
        format!("{unit} {}", progress.page)
    }
}

/// The command line that reopens `path` the way it was opened. The file comes first, so a
/// reader of the status file can check it still exists. `None` for a path that is not
/// valid UTF-8, which a JSON status file cannot carry faithfully.
pub fn resume_args(
    path: &Path,
    override_: Option<&HostOverride>,
    title: Option<&str>,
) -> Option<Vec<String>> {
    let text = |p: &PathBuf| p.to_str().map(str::to_string);
    let mut args = vec![path.to_str()?.to_string()];
    match override_ {
        None => {}
        Some(HostOverride::Vault { root, key, .. }) => {
            args.push(format!("--vault={}", text(root)?));
            args.push(format!("--key={key}"));
        }
        Some(HostOverride::ExternalPaths {
            annotations,
            progress,
            ..
        }) => {
            args.push(format!("--annotations-file={}", text(annotations)?));
            if let Some(progress) = progress {
                args.push(format!("--progress-file={}", text(progress)?));
            }
        }
    }
    if let Some(title) = title.filter(|t| !t.trim().is_empty()) {
        args.push(format!("--title={title}"));
    }
    Some(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sidecar(annotations: &str) -> fond_annot::AnnotationSidecar {
        serde_json::from_str(&format!(
            r#"{{"schema":1,"key":"k","annotations":[{annotations}]}}"#
        ))
        .unwrap()
    }

    #[test]
    fn the_context_is_the_newest_highlights_text() {
        let one = |id: &str, snippet: &str, created: &str| {
            format!(
                r#"{{"id":"{id}","kind":"highlight","page":1,"quadpoints":[],"snippet":"{snippet}","created":"{created}"}}"#
            )
        };
        let sc = sidecar(&format!(
            "{},{},{}",
            one("a", "older passage", "2026-11-01T10:00:00Z"),
            one("b", "the newest   passage", "2026-11-09T10:00:00Z"),
            one("c", "   ", "2026-11-10T10:00:00Z"),
        ));
        assert_eq!(newest_highlight(&sc).as_deref(), Some("the newest passage"));
        assert_eq!(newest_highlight(&sidecar("")), None);
    }

    #[test]
    fn a_standalone_open_resumes_with_the_bare_file() {
        let args = resume_args(Path::new("/tmp/example/reader.pdf"), None, None);
        assert_eq!(args.unwrap(), ["/tmp/example/reader.pdf"]);
    }

    #[test]
    fn a_launch_for_another_app_resumes_into_that_apps_storage() {
        let vault = HostOverride::Vault {
            root: PathBuf::from("/tmp/example/library"),
            key: "example2026".to_string(),
            hash: "abc".to_string(),
        };
        assert_eq!(
            resume_args(
                Path::new("/tmp/example/reader.pdf"),
                Some(&vault),
                Some("An Example Reader")
            )
            .unwrap(),
            [
                "/tmp/example/reader.pdf",
                "--vault=/tmp/example/library",
                "--key=example2026",
                "--title=An Example Reader"
            ]
        );

        let external = HostOverride::ExternalPaths {
            annotations: PathBuf::from("/tmp/example/annots/abc.json"),
            progress: Some(PathBuf::from("/tmp/example/progress/abc.json")),
            hash: "abc".to_string(),
        };
        assert_eq!(
            resume_args(Path::new("/tmp/example/reader.epub"), Some(&external), None).unwrap(),
            [
                "/tmp/example/reader.epub",
                "--annotations-file=/tmp/example/annots/abc.json",
                "--progress-file=/tmp/example/progress/abc.json"
            ]
        );
    }

    #[test]
    fn positions_are_exact_and_say_what_they_count() {
        let at = |page, of| Progress {
            page,
            of,
            chapter_percent: None,
        };
        assert_eq!(position_text(DocKind::Pdf, at(47, 312)), "p. 47 of 312");
        assert_eq!(position_text(DocKind::Epub, at(3, 12)), "chapter 3 of 12");
        assert_eq!(position_text(DocKind::Pdf, at(47, 0)), "p. 47");
    }

    #[test]
    fn the_worker_keeps_the_newest_first_and_updates_the_page() {
        let dir = std::env::temp_dir().join(format!("pereplyot-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (tx, rx) = mpsc::channel();
        let opened = |hash: &str, title: &str| Msg::Opened {
            hash: hash.to_string(),
            kind: DocKind::Pdf,
            title: title.to_string(),
            args: vec![format!("/tmp/example/{hash}.pdf")],
            progress: None,
        };
        tx.send(opened("a", "First")).unwrap();
        tx.send(opened("b", "Second")).unwrap();
        tx.send(Msg::Progress {
            hash: "a".to_string(),
            progress: Progress {
                page: 9,
                of: 40,
                chapter_percent: None,
            },
        })
        .unwrap();
        drop(tx);
        run(&dir, rx);

        let status = Status::read(&dir, APP).unwrap().unwrap();
        let shown: Vec<_> = status
            .resume
            .iter()
            .map(|e| (e.title.as_str(), e.detail.as_deref()))
            .collect();
        assert_eq!(shown, [("First", Some("p. 9 of 40")), ("Second", None)]);
        assert!(status.resume[0].at.is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
