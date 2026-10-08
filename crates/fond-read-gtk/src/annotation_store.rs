use std::cell::{Ref, RefCell};
use std::rc::Rc;

use fond_annot::{Annotation, AnnotationSidecar};

use crate::ReaderHost;

pub const UNDO_HISTORY_LIMIT: usize = 50;

/// What a store change did, with the annotations involved so a view can redraw just what moved.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Added(Annotation),
    Updated {
        before: Box<Annotation>,
        after: Box<Annotation>,
    },
    Removed(Annotation),
}

impl Change {
    pub fn id(&self) -> &str {
        match self {
            Change::Added(a) | Change::Removed(a) => &a.id,
            Change::Updated { after, .. } => &after.id,
        }
    }

    /// The PDF pages (1-based) whose appearance this change affects.
    pub fn pages(&self) -> Vec<u32> {
        let mut pages: Vec<u32> = match self {
            Change::Added(a) | Change::Removed(a) => a.page.into_iter().collect(),
            Change::Updated { before, after } => {
                before.page.into_iter().chain(after.page).collect()
            }
        };
        pages.dedup();
        pages
    }
}

enum Op {
    Add(Annotation, Option<usize>),
    Remove {
        annotation: Annotation,
        index: usize,
    },
    Update {
        before: Box<Annotation>,
        after: Box<Annotation>,
    },
}

struct State {
    sidecar: AnnotationSidecar,
    undo: Vec<Op>,
    redo: Vec<Op>,
}

type Listener = Rc<dyn Fn(&AnnotationStore, &Change)>;

/// The single owner of one open document's annotations: every add, edit, delete, undo and redo
/// goes through here, is saved through the [`ReaderHost`], and is announced to subscribers, so
/// views (page overlay, notes sidebar, undo buttons) follow the data instead of each mutation
/// site remembering to refresh them. A failed save rolls the change back, keeping memory and
/// disk in step.
pub struct AnnotationStore {
    host: Rc<dyn ReaderHost>,
    source_hash: Option<String>,
    state: RefCell<State>,
    listeners: RefCell<Vec<Listener>>,
}

impl AnnotationStore {
    /// `source_hash` is stamped onto the sidecar's `pdf_hash` when an annotation is added
    /// (PDF documents; EPUB passes `None`).
    pub fn new(host: &Rc<dyn ReaderHost>, source_hash: Option<String>) -> Rc<AnnotationStore> {
        Rc::new(AnnotationStore {
            host: host.clone(),
            source_hash,
            state: RefCell::new(State {
                sidecar: host.load_annotations(),
                undo: Vec::new(),
                redo: Vec::new(),
            }),
            listeners: RefCell::new(Vec::new()),
        })
    }

    pub fn sidecar(&self) -> Ref<'_, AnnotationSidecar> {
        Ref::map(self.state.borrow(), |s| &s.sidecar)
    }

    pub fn get(&self, id: &str) -> Option<Annotation> {
        self.state
            .borrow()
            .sidecar
            .annotations
            .iter()
            .find(|a| a.id == id)
            .cloned()
    }

    pub fn can_undo(&self) -> bool {
        !self.state.borrow().undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.state.borrow().redo.is_empty()
    }

    /// Run `f` after every change, with the store it came from. Called once the change has been
    /// saved and the store's own borrows released, so `f` may read the store freely.
    pub fn subscribe(&self, f: impl Fn(&AnnotationStore, &Change) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    /// Drop every subscriber. Listeners typically capture the views they redraw, and those
    /// views own the store, so a closing reader calls this to break the cycle.
    pub fn clear_listeners(&self) {
        self.listeners.borrow_mut().clear();
    }

    pub fn add(&self, annotation: Annotation) -> Result<(), String> {
        self.apply_new(Op::Add(annotation, None))
    }

    /// Edit the annotation `id` in place. `Ok(false)` if there is none.
    pub fn update(&self, id: &str, edit: impl FnOnce(&mut Annotation)) -> Result<bool, String> {
        let Some(before) = self.get(id) else {
            return Ok(false);
        };
        let mut after = before.clone();
        edit(&mut after);
        if after == before {
            return Ok(true);
        }
        self.apply_new(Op::Update {
            before: Box::new(before),
            after: Box::new(after),
        })?;
        Ok(true)
    }

    /// Delete the annotation `id`. `Ok(false)` if there is none.
    pub fn remove(&self, id: &str) -> Result<bool, String> {
        let found = {
            let s = self.state.borrow();
            s.sidecar
                .annotations
                .iter()
                .position(|a| a.id == id)
                .map(|index| (index, s.sidecar.annotations[index].clone()))
        };
        let Some((index, annotation)) = found else {
            return Ok(false);
        };
        self.apply_new(Op::Remove { annotation, index })?;
        Ok(true)
    }

    /// `Ok(false)` if there was nothing to undo.
    pub fn undo(&self) -> Result<bool, String> {
        let Some(op) = self.state.borrow_mut().undo.pop() else {
            return Ok(false);
        };
        let inverse = invert(&op);
        match self.run(&inverse) {
            Ok(change) => {
                self.state.borrow_mut().redo.push(op);
                self.notify(&change);
                Ok(true)
            }
            Err(e) => {
                self.state.borrow_mut().undo.push(op);
                Err(e)
            }
        }
    }

    /// `Ok(false)` if there was nothing to redo.
    pub fn redo(&self) -> Result<bool, String> {
        let Some(op) = self.state.borrow_mut().redo.pop() else {
            return Ok(false);
        };
        match self.run(&op) {
            Ok(change) => {
                self.state.borrow_mut().undo.push(op);
                self.notify(&change);
                Ok(true)
            }
            Err(e) => {
                self.state.borrow_mut().redo.push(op);
                Err(e)
            }
        }
    }

    fn apply_new(&self, op: Op) -> Result<(), String> {
        let change = self.run(&op)?;
        {
            let mut s = self.state.borrow_mut();
            s.redo.clear();
            s.undo.push(op);
            if s.undo.len() > UNDO_HISTORY_LIMIT {
                s.undo.remove(0);
            }
        }
        self.notify(&change);
        Ok(())
    }

    /// Apply `op` to the sidecar and save it; undo it again if the save fails.
    fn run(&self, op: &Op) -> Result<Change, String> {
        let (change, previous_hash) = {
            let mut s = self.state.borrow_mut();
            let previous_hash = s.sidecar.pdf_hash.clone();
            let change = match op {
                Op::Add(a, at) => {
                    if self.source_hash.is_some() {
                        s.sidecar.pdf_hash = self.source_hash.clone();
                    }
                    if s.sidecar.annotations.iter().any(|x| x.id == a.id) {
                        s.sidecar.upsert(a.clone());
                    } else {
                        let len = s.sidecar.annotations.len();
                        s.sidecar
                            .annotations
                            .insert(at.unwrap_or(len).min(len), a.clone());
                    }
                    Change::Added(a.clone())
                }
                Op::Remove { annotation, .. } => {
                    s.sidecar.annotations.retain(|a| a.id != annotation.id);
                    Change::Removed(annotation.clone())
                }
                Op::Update { before, after } => {
                    if let Some(slot) = s.sidecar.annotations.iter_mut().find(|a| a.id == after.id)
                    {
                        *slot = (**after).clone();
                    }
                    Change::Updated {
                        before: before.clone(),
                        after: after.clone(),
                    }
                }
            };
            (change, previous_hash)
        };
        let saved = self.host.save_annotations(&self.state.borrow().sidecar);
        if let Err(e) = saved {
            let mut s = self.state.borrow_mut();
            match op {
                Op::Add(a, _) => s.sidecar.annotations.retain(|x| x.id != a.id),
                Op::Remove { annotation, index } => {
                    let at = (*index).min(s.sidecar.annotations.len());
                    s.sidecar.annotations.insert(at, annotation.clone());
                }
                Op::Update { before, .. } => {
                    if let Some(slot) = s.sidecar.annotations.iter_mut().find(|a| a.id == before.id)
                    {
                        *slot = (**before).clone();
                    }
                }
            }
            s.sidecar.pdf_hash = previous_hash;
            return Err(e);
        }
        Ok(change)
    }

    fn notify(&self, change: &Change) {
        let listeners: Vec<Listener> = self.listeners.borrow().clone();
        for l in listeners {
            l(self, change);
        }
    }
}

fn invert(op: &Op) -> Op {
    match op {
        Op::Add(a, _) => Op::Remove {
            annotation: a.clone(),
            index: usize::MAX,
        },
        Op::Remove { annotation, index } => Op::Add(annotation.clone(), Some(*index)),
        Op::Update { before, after } => Op::Update {
            before: after.clone(),
            after: before.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fond_annot::AnnotationKind;
    use std::cell::{Cell, RefCell};

    struct MockHost {
        saved: RefCell<Vec<usize>>,
        fail_next: Cell<bool>,
    }

    impl MockHost {
        fn new() -> Rc<MockHost> {
            Rc::new(MockHost {
                saved: RefCell::new(Vec::new()),
                fail_next: Cell::new(false),
            })
        }
    }

    impl ReaderHost for MockHost {
        fn load_annotations(&self) -> AnnotationSidecar {
            AnnotationSidecar::new("k")
        }
        fn save_annotations(&self, sidecar: &AnnotationSidecar) -> Result<(), String> {
            if self.fail_next.replace(false) {
                return Err("disk full".into());
            }
            self.saved.borrow_mut().push(sidecar.annotations.len());
            Ok(())
        }
        fn save_progress(&self, _: fond_annot::Progress) {}
        fn page_label_override(&self) -> Option<fond_annot::PageLabelOverride> {
            None
        }
        fn set_page_label_override(&self, _: Option<fond_annot::PageLabelOverride>) {}
        fn notify(&self, _: &str) {}
    }

    fn ann(id: &str, text: &str) -> Annotation {
        let mut a = Annotation::drawn(
            AnnotationKind::Highlight,
            1,
            vec![],
            Some(text.into()),
            None,
            None,
        );
        a.id = id.into();
        a
    }

    fn store(host: &Rc<MockHost>) -> Rc<AnnotationStore> {
        let dynamic: Rc<dyn ReaderHost> = host.clone();
        AnnotationStore::new(&dynamic, Some("hash".into()))
    }

    fn ids(s: &AnnotationStore) -> Vec<String> {
        s.sidecar()
            .annotations
            .iter()
            .map(|a| a.id.clone())
            .collect()
    }

    #[test]
    fn add_saves_stamps_the_hash_and_announces() {
        let host = MockHost::new();
        let s = store(&host);
        let seen = Rc::new(RefCell::new(Vec::new()));
        {
            let seen = seen.clone();
            s.subscribe(move |_, c| seen.borrow_mut().push(c.clone()));
        }
        s.add(ann("a", "one")).unwrap();
        assert_eq!(ids(&s), ["a"]);
        assert_eq!(s.sidecar().pdf_hash.as_deref(), Some("hash"));
        assert_eq!(*host.saved.borrow(), [1]);
        assert_eq!(
            seen.borrow()
                .iter()
                .map(|c| c.id().to_string())
                .collect::<Vec<_>>(),
            ["a"]
        );
        assert!(matches!(seen.borrow()[0], Change::Added(_)));
    }

    #[test]
    fn undo_and_redo_walk_add_update_remove() {
        let host = MockHost::new();
        let s = store(&host);
        s.add(ann("a", "one")).unwrap();
        s.add(ann("b", "two")).unwrap();
        s.update("a", |a| a.note = Some("n".into())).unwrap();
        s.remove("b").unwrap();
        assert_eq!(ids(&s), ["a"]);

        assert!(s.undo().unwrap());
        assert_eq!(ids(&s), ["a", "b"]);
        assert!(s.undo().unwrap());
        assert_eq!(s.get("a").unwrap().note, None);
        assert!(s.undo().unwrap());
        assert!(s.undo().unwrap());
        assert!(ids(&s).is_empty());
        assert!(!s.undo().unwrap());

        assert!(s.redo().unwrap());
        assert!(s.redo().unwrap());
        assert!(s.redo().unwrap());
        assert_eq!(s.get("a").unwrap().note.as_deref(), Some("n"));
        assert!(s.redo().unwrap());
        assert_eq!(ids(&s), ["a"]);
        assert!(!s.redo().unwrap());
    }

    #[test]
    fn undoing_a_remove_puts_the_annotation_back_in_its_place() {
        let host = MockHost::new();
        let s = store(&host);
        for id in ["a", "b", "c"] {
            s.add(ann(id, id)).unwrap();
        }
        s.remove("b").unwrap();
        s.undo().unwrap();
        assert_eq!(ids(&s), ["a", "b", "c"]);
    }

    #[test]
    fn a_new_edit_clears_redo() {
        let host = MockHost::new();
        let s = store(&host);
        s.add(ann("a", "one")).unwrap();
        s.undo().unwrap();
        assert!(s.can_redo());
        s.add(ann("b", "two")).unwrap();
        assert!(!s.can_redo());
    }

    #[test]
    fn a_failed_save_rolls_the_change_back_and_records_nothing() {
        let host = MockHost::new();
        let s = store(&host);
        s.add(ann("a", "one")).unwrap();
        host.fail_next.set(true);
        assert_eq!(s.add(ann("b", "two")), Err("disk full".to_string()));
        assert_eq!(ids(&s), ["a"]);
        host.fail_next.set(true);
        assert!(s.remove("a").is_err());
        assert_eq!(ids(&s), ["a"]);
        host.fail_next.set(true);
        assert!(s.update("a", |a| a.note = Some("x".into())).is_err());
        assert_eq!(s.get("a").unwrap().note, None);
        s.undo().unwrap();
        assert!(!s.can_undo(), "only the successful add was recorded");
    }

    #[test]
    fn a_failed_undo_keeps_the_step_available() {
        let host = MockHost::new();
        let s = store(&host);
        s.add(ann("a", "one")).unwrap();
        host.fail_next.set(true);
        assert!(s.undo().is_err());
        assert_eq!(ids(&s), ["a"]);
        assert!(s.can_undo());
        assert!(s.undo().unwrap());
    }

    #[test]
    fn history_is_capped() {
        let host = MockHost::new();
        let s = store(&host);
        for i in 0..UNDO_HISTORY_LIMIT + 10 {
            s.add(ann(&format!("a{i}"), "x")).unwrap();
        }
        let mut undone = 0;
        while s.undo().unwrap() {
            undone += 1;
        }
        assert_eq!(undone, UNDO_HISTORY_LIMIT);
    }

    #[test]
    fn listeners_may_read_the_store() {
        let host = MockHost::new();
        let s = store(&host);
        let count = Rc::new(Cell::new(0));
        {
            let count = count.clone();
            s.subscribe(move |st, _| count.set(st.sidecar().annotations.len()));
        }
        s.add(ann("a", "one")).unwrap();
        s.add(ann("b", "two")).unwrap();
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn missing_ids_are_reported_not_errors() {
        let host = MockHost::new();
        let s = store(&host);
        assert_eq!(s.remove("nope"), Ok(false));
        assert_eq!(s.update("nope", |_| {}), Ok(false));
    }
}
