//! Connections between annotations: "this contradicts p. 12 of Smith", across documents. They
//! live in one file beside the sidecars rather than in either annotation, so they link
//! documents that are not open, leave every sidecar as other apps expect it, and show on both
//! annotations' cards. Each end keeps a little of what it said so a link reads correctly when
//! the other document is shut or gone.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::fsutil::{self, Loaded};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct End {
    pub hash: String,
    pub id: String,
    pub page: u32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub colour: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub text: String,
    /// `label` is a chapter number (an EPUB with no printed pages), not a page.
    #[serde(default)]
    pub chapter: bool,
}

impl End {
    pub fn is(&self, hash: &str, id: &str) -> bool {
        self.hash == hash && self.id == id
    }

    pub fn whence(&self) -> String {
        let page = if self.label.is_empty() {
            self.page.to_string()
        } else {
            self.label.clone()
        };
        let place = format!("{} {page}", if self.chapter { "ch." } else { "p." });
        if self.title.is_empty() {
            place
        } else {
            format!("{}, {place}", self.title)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub a: End,
    pub b: End,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connections {
    #[serde(default)]
    pub connections: Vec<Connection>,
}

pub fn path() -> PathBuf {
    glib::user_data_dir()
        .join("pereplyot")
        .join("connections.json")
}

pub fn load_from(path: &Path) -> Connections {
    let parse = |t: &str| serde_json::from_str::<Connections>(t).ok();
    match fsutil::load_checked(path, parse) {
        Loaded::Ok(c) => c,
        Loaded::Missing => Connections::default(),
        Loaded::Corrupt {
            recovered_from_backup,
            ..
        } => recovered_from_backup.unwrap_or_default(),
    }
}

pub fn save_to(path: &Path, c: &Connections) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(c).map_err(std::io::Error::other)?;
    fsutil::write_atomic(path, json.as_bytes())
}

fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("cn-{nanos:x}")
}

impl Connections {
    /// Link `a` and `b`. Linking the same pair twice, in either order, changes the note of the
    /// existing link instead of adding another. Returns false when `a` and `b` are the same.
    pub fn connect(&mut self, a: End, b: End, note: &str) -> bool {
        if a.is(&b.hash, &b.id) {
            return false;
        }
        let note = note.trim().to_string();
        if let Some(existing) = self.connections.iter_mut().find(|c| {
            (c.a.is(&a.hash, &a.id) && c.b.is(&b.hash, &b.id))
                || (c.a.is(&b.hash, &b.id) && c.b.is(&a.hash, &a.id))
        }) {
            existing.note = note;
            return true;
        }
        self.connections.push(Connection {
            id: new_id(),
            a,
            b,
            note,
        });
        true
    }

    pub fn remove(&mut self, id: &str) {
        self.connections.retain(|c| c.id != id);
    }

    /// The links of one annotation, each with the end that is not it.
    pub fn of(&self, hash: &str, id: &str) -> Vec<(&Connection, &End)> {
        self.connections
            .iter()
            .filter_map(|c| {
                if c.a.is(hash, id) {
                    Some((c, &c.b))
                } else if c.b.is(hash, id) {
                    Some((c, &c.a))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Drop the links of an annotation that was deleted.
    pub fn forget(&mut self, hash: &str, id: &str) {
        self.connections
            .retain(|c| !c.a.is(hash, id) && !c.b.is(hash, id));
    }

    /// Bring the cached words of both ends up to date with the annotation they describe.
    pub fn refresh(&mut self, hash: &str, id: &str, text: &str, colour: &str) {
        for c in &mut self.connections {
            for end in [&mut c.a, &mut c.b] {
                if end.is(hash, id) {
                    end.text = text.to_string();
                    end.colour = colour.to_string();
                }
            }
        }
    }
}

type Subscriber = (u64, Rc<dyn Fn()>);

thread_local! {
    static STORE: RefCell<Option<Connections>> = const { RefCell::new(None) };
    static SUBSCRIBERS: RefCell<Vec<Subscriber>> = const { RefCell::new(Vec::new()) };
    static NEXT_SUBSCRIBER: Cell<u64> = const { Cell::new(1) };
    static PENDING: RefCell<Option<End>> = const { RefCell::new(None) };
}

fn notify() {
    let subscribers: Vec<Rc<dyn Fn()>> =
        SUBSCRIBERS.with(|s| s.borrow().iter().map(|(_, f)| f.clone()).collect());
    for f in subscribers {
        f();
    }
}

/// Read the connections this process shares, loading them on first use.
pub fn read<R>(f: impl FnOnce(&Connections) -> R) -> R {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        let c = s.get_or_insert_with(|| load_from(&path()));
        f(c)
    })
}

/// Change the connections, save them and tell everything showing them.
pub fn edit(f: impl FnOnce(&mut Connections)) {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        let c = s.get_or_insert_with(|| load_from(&path()));
        f(c);
        let _ = save_to(&path(), c);
    });
    notify();
}

/// Call `f` whenever connections or the pending half-made one change; the id ends it.
pub fn subscribe(f: Rc<dyn Fn()>) -> u64 {
    let id = NEXT_SUBSCRIBER.with(|n| n.replace(n.get() + 1));
    SUBSCRIBERS.with(|s| s.borrow_mut().push((id, f)));
    id
}

pub fn unsubscribe(id: u64) {
    SUBSCRIBERS.with(|s| s.borrow_mut().retain(|(i, _)| *i != id));
}

/// The annotation "Connect" was pressed on, waiting for the one to connect it to.
pub fn pending() -> Option<End> {
    PENDING.with(|p| p.borrow().clone())
}

pub fn set_pending(end: Option<End>) {
    PENDING.with(|p| *p.borrow_mut() = end);
    notify();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end(hash: &str, id: &str, page: u32) -> End {
        End {
            hash: hash.into(),
            id: id.into(),
            page,
            label: String::new(),
            colour: String::new(),
            title: "Smith".into(),
            text: "words".into(),
            chapter: false,
        }
    }

    #[test]
    fn a_link_shows_from_both_ends_and_across_documents() {
        let mut c = Connections::default();
        assert!(c.connect(end("d1", "a", 3), end("d2", "b", 12), " contradicts "));
        let from_a = c.of("d1", "a");
        assert_eq!(from_a.len(), 1);
        assert_eq!(from_a[0].1.id, "b");
        assert_eq!(from_a[0].0.note, "contradicts");
        let from_b = c.of("d2", "b");
        assert_eq!(from_b[0].1.id, "a");
        assert!(c.of("d1", "other").is_empty());
    }

    #[test]
    fn linking_a_pair_again_edits_the_link_and_nothing_links_to_itself() {
        let mut c = Connections::default();
        c.connect(end("d1", "a", 1), end("d2", "b", 2), "one");
        c.connect(end("d2", "b", 2), end("d1", "a", 1), "two");
        assert_eq!(c.connections.len(), 1);
        assert_eq!(c.connections[0].note, "two");
        assert!(!c.connect(end("d1", "a", 1), end("d1", "a", 1), ""));
    }

    #[test]
    fn removing_and_forgetting() {
        let mut c = Connections::default();
        c.connect(end("d1", "a", 1), end("d2", "b", 2), "");
        c.connect(end("d1", "a", 1), end("d3", "c", 2), "");
        let first = c.connections[0].id.clone();
        c.remove(&first);
        assert_eq!(c.connections.len(), 1);
        c.forget("d3", "c");
        assert!(c.connections.is_empty());
    }

    #[test]
    fn the_file_round_trips_and_a_missing_one_is_empty() {
        let path = std::env::temp_dir().join(format!("pereplyot-cn-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(load_from(&path).connections.is_empty());
        let mut c = Connections::default();
        c.connect(end("d1", "a", 1), end("d2", "b", 2), "n");
        save_to(&path, &c).unwrap();
        assert_eq!(load_from(&path), c);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn refresh_updates_both_sides_of_every_link() {
        let mut c = Connections::default();
        c.connect(end("d1", "a", 1), end("d2", "b", 2), "");
        c.refresh("d1", "a", "new words", "#fff");
        assert_eq!(c.connections[0].a.text, "new words");
        assert_eq!(c.connections[0].b.text, "words");
    }
}
