//! Each document's bibliographic [`Record`] (`bib/<hash>.json`), filled from the file on first open.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fond_read_gtk::doc_identity;
use fond_read_gtk::fsutil;
use fond_read_gtk::history::DocKind;
use pereplyot_core::record::{Facts, Record};
use pereplyot_core::refs::{is_same_work, Known, ParsedRef};

fn dir() -> PathBuf {
    crate::paths::data_dir().join("bib")
}

fn path_of(hash: &str) -> PathBuf {
    dir().join(format!("{hash}.json"))
}

pub fn load(hash: &str) -> Option<Record> {
    match fsutil::load_checked(&path_of(hash), |t| serde_json::from_str::<Record>(t).ok()) {
        fsutil::Loaded::Ok(r) => Some(r),
        fsutil::Loaded::Corrupt {
            recovered_from_backup,
            ..
        } => recovered_from_backup,
        fsutil::Loaded::Missing => None,
    }
}

pub fn save(hash: &str, record: &Record) {
    if let Ok(json) = serde_json::to_string_pretty(record) {
        let _ = fsutil::write_atomic(&path_of(hash), json.as_bytes());
    }
}

/// Keys already in use by other documents, so a new one never repeats one.
pub fn keys_in_use(except: &str) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let stem = e.path().file_stem()?.to_string_lossy().into_owned();
            (stem != except).then(|| load(&stem))?
        })
        .map(|r| r.key)
        .filter(|k| !k.is_empty())
        .collect()
}

/// A citation key for `record` in Kartoteka's convention (family, year, first title word), made
/// unique among the records already stored.
pub fn generate_key(record: &Record, hash: &str) -> String {
    let base = fond_bib::key::generate_base_key(
        record.first_family().as_deref(),
        record.year,
        Some(record.title.as_str()).filter(|t| !t.is_empty()),
    )
    .unwrap_or_else(|_| "untitled".to_string());
    fond_bib::key::assign_key(&base, &keys_in_use(hash))
}

/// Reads the file (blocking) and stores its identity and, if it has none yet, a record. A record
/// the user already edited is never touched.
pub fn record_document(path: &Path, kind: DocKind, hash: &str, legacy_key: Option<String>) {
    let data = crate::paths::data_dir();
    let have_identity = doc_identity::load(&data, hash).is_some();
    if have_identity && load(hash).is_some() {
        return;
    }
    let Some(probe) = doc_identity::probe(path, kind, hash) else {
        return;
    };
    if !have_identity {
        doc_identity::save(&data, &probe.identity);
    }
    if load(hash).is_some() {
        return;
    }
    let mut record = Record::extract(&Facts {
        title: probe.title.as_deref(),
        authors: probe.authors.iter().map(String::as_str).collect(),
        date: probe.date.as_deref(),
        publisher: probe.publisher.as_deref(),
        isbn: probe.isbn.as_deref(),
        front_text: &probe.front_text,
        is_epub: probe.is_epub,
    });
    // A document that says nothing about itself gets no key until the person gives it details.
    record.key = match legacy_key.filter(|k| !k.is_empty()) {
        Some(k) => k,
        None if record.title.is_empty() => String::new(),
        None => generate_key(&record, hash),
    };
    save(hash, &record);
}

/// Hayagriva YAML for the documents `hashes` that have a keyed record, one entry per key.
pub fn yaml_for(hashes: &[String]) -> Option<String> {
    let mut seen = HashSet::new();
    let mut out = String::new();
    for hash in hashes {
        let Some(r) = load(hash).filter(|r| !r.key.is_empty() && !r.title.is_empty()) else {
            continue;
        };
        if seen.insert(r.key.clone()) {
            out.push_str(&r.to_yaml());
        }
    }
    (!out.is_empty()).then_some(out)
}

type KnownList = Vec<(Known, String)>;

thread_local! {
    static KNOWN: std::cell::RefCell<Option<(std::time::Instant, KnownList)>> =
        const { std::cell::RefCell::new(None) };
}

/// The documents with a record, as far as matching a reference needs, each with its title for display. Kept
/// for a few seconds so a reference list of hundreds of entries doesn't re-read every record for each.
fn known_documents() -> KnownList {
    let fresh = KNOWN.with(|k| {
        k.borrow()
            .as_ref()
            .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(5))
            .map(|(_, v)| v.clone())
    });
    if let Some(v) = fresh {
        return v;
    }
    let present: HashSet<String> = fond_read_gtk::history::load()
        .into_iter()
        .filter(|e| e.path.is_file())
        .map(|e| e.hash)
        .collect();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir()) {
        for e in entries.flatten() {
            let Some(hash) = e
                .path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
            else {
                continue;
            };
            if !present.contains(&hash) {
                continue;
            }
            let Some(r) = load(&hash).filter(|r| !r.title.is_empty()) else {
                continue;
            };
            let families = r
                .authors
                .iter()
                .map(|a| pereplyot_core::identity::surname_of(a))
                .collect();
            out.push((
                Known {
                    hash,
                    families,
                    year: r.year,
                    title: r.title.clone(),
                    doi: r.doi.clone(),
                },
                r.title,
            ));
        }
    }
    KNOWN.with(|k| *k.borrow_mut() = Some((std::time::Instant::now(), out.clone())));
    out
}

/// The owned document (other than `except`) that `entry` refers to.
pub fn find_owned(entry: &ParsedRef, except: &str) -> Option<(String, String)> {
    known_documents()
        .into_iter()
        .filter(|(k, _)| k.hash != except)
        .find(|(k, _)| is_same_work(entry, k))
        .map(|(k, title)| (k.hash, title))
}
