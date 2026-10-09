//! The Notebook as it appears in the app: one editor shared by every window, shown beside
//! whichever document asked for it or in a window of its own, with annotations dragged or sent
//! into it from the readers and the launcher.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::connections::End;
use crate::notebook::Quote;

mod card;
pub mod connect;
mod export;
mod picker;
mod view;

pub use view::NotebookView;

pub type OpenSource = Rc<dyn Fn(&str, &str, u32)>;
pub type CiteKey = Rc<dyn Fn(&str) -> Option<String>>;
pub type Shelves = Rc<dyn Fn() -> Vec<String>>;

/// What the notebook needs to know that only the embedding app does.
#[derive(Default, Clone)]
pub struct Hooks {
    /// Open a document at an annotation when no reader is open on it.
    pub open_source: Option<OpenSource>,
    /// The bibliography key a document is cited by.
    pub cite_key: Option<CiteKey>,
    /// The Library's shelves, which a notebook can belong to.
    pub shelves: Option<Shelves>,
}

/// A document, as far as a quote needs to name it.
#[derive(Clone)]
pub struct DocRef {
    pub hash: String,
    pub title: String,
}

enum Place {
    Nowhere,
    Pane {
        paned: gtk4::Paned,
        hidden: Rc<dyn Fn()>,
    },
    Window(adw::Window),
}

struct Session {
    view: Rc<NotebookView>,
    place: RefCell<Place>,
    hooks: RefCell<Hooks>,
}

thread_local! {
    static SESSION: RefCell<Option<Rc<Session>>> = const { RefCell::new(None) };
}

fn session() -> Rc<Session> {
    if let Some(s) = SESSION.with(|s| s.borrow().clone()) {
        return s;
    }
    crate::style::ensure();
    let s = Rc::new(Session {
        view: NotebookView::new(crate::notebook::dir()),
        place: RefCell::new(Place::Nowhere),
        hooks: RefCell::new(Hooks::default()),
    });
    SESSION.with(|slot| *slot.borrow_mut() = Some(s.clone()));
    s
}

pub fn set_hooks(hooks: Hooks) {
    *session().hooks.borrow_mut() = hooks;
}

pub(crate) fn hooks() -> Hooks {
    session().hooks.borrow().clone()
}

pub fn cite_key_for(hash: &str) -> Option<String> {
    hooks().cite_key.and_then(|f| f(hash))
}

/// The document's path, from History, for drawing a clipped area of it.
pub(crate) fn document_path(hash: &str) -> Option<PathBuf> {
    crate::history::load()
        .into_iter()
        .find(|e| e.hash == hash)
        .map(|e| e.path)
}

/// The annotation as it stands in this computer's own sidecar, if the document's notes are kept
/// there (not for a document read through Kartoteka or Sputnik).
pub(crate) fn live_annotation(hash: &str, id: &str) -> Option<fond_annot::Annotation> {
    let path = glib::user_data_dir()
        .join("pereplyot")
        .join("annotations")
        .join(format!("{hash}.json"));
    let text = std::fs::read_to_string(&path).ok()?;
    let sidecar = fond_annot::AnnotationSidecar::parse(&text, &path).ok()?;
    sidecar.annotations.into_iter().find(|a| a.id == id)
}

/// Bring a quote's cached words up to date with the annotation it came from.
pub(crate) fn refresh_from_source(q: &mut Quote) {
    if let Some(a) = live_annotation(&q.hash, &q.id) {
        q.text = words(&a);
        q.note = a.note.clone().unwrap_or_default();
        q.colour = a.color.clone().unwrap_or_default();
        q.page = a.page.unwrap_or(q.page);
    }
}

fn words(a: &fond_annot::Annotation) -> String {
    a.snippet.clone().unwrap_or_default()
}

/// An annotation as a notebook quote. `label` is the printed page.
pub fn quote_of(doc: &DocRef, label: &str, a: &fond_annot::Annotation) -> Quote {
    Quote {
        hash: doc.hash.clone(),
        id: a.id.clone(),
        page: a.page.unwrap_or(0),
        label: label.to_string(),
        colour: a.color.clone().unwrap_or_default(),
        title: doc.title.clone(),
        key: cite_key_for(&doc.hash).unwrap_or_default(),
        text: words(a),
        note: a.note.clone().unwrap_or_default(),
        area: a.kind == fond_annot::AnnotationKind::Area,
    }
}

/// The same annotation as one end of a connection.
pub fn end_of(doc: &DocRef, label: &str, a: &fond_annot::Annotation) -> End {
    End {
        hash: doc.hash.clone(),
        id: a.id.clone(),
        page: a.page.unwrap_or(0),
        label: label.to_string(),
        colour: a.color.clone().unwrap_or_default(),
        title: doc.title.clone(),
        text: words(a),
    }
}

/// Take the reader (or the app) to the passage a quote or connection names.
pub fn open_source(hash: &str, id: &str, page: u32) {
    if crate::jump_in_open_reader(hash, page, Some(id)) {
        return;
    }
    if let Some(open) = hooks().open_source {
        open(hash, id, page);
    }
}

/// A drag source that carries `quote()` for the notebook to drop.
pub fn drag_source(quote: Rc<dyn Fn() -> Option<Quote>>) -> gtk4::DragSource {
    let source = gtk4::DragSource::new();
    source.set_actions(gtk4::gdk::DragAction::COPY);
    source.connect_prepare(move |_, _, _| {
        let q = quote()?;
        Some(gtk4::gdk::ContentProvider::for_value(
            &glib::BoxedAnyObject::new(q).to_value(),
        ))
    });
    source
}

/// Add `quote` to the end of the open notebook (opening the last one, or starting one).
pub fn add_to_notebook(quote: Quote, notify: &dyn Fn(&str)) {
    let s = session();
    if s.view.ensure_open() {
        let title = s.view.title();
        s.view.add_quote(quote);
        notify(&format!("Added to “{title}”"));
    } else {
        notify("Couldn't open a notebook");
    }
}

/// Show the notebook in `paned`'s end child, taking it from wherever it was. `hidden` runs when
/// it is later taken away, so the toggle that showed it can let go.
pub fn show_in_pane(paned: &gtk4::Paned, hidden: Rc<dyn Fn()>) {
    let s = session();
    detach(&s);
    s.view.ensure_open();
    s.view.widget().set_size_request(280, -1);
    let total = paned.width();
    paned.set_end_child(Some(s.view.widget()));
    paned.set_resize_end_child(false);
    paned.set_shrink_end_child(false);
    let position = if total > 0 {
        (total - 380).max(total / 2)
    } else {
        700
    };
    paned.set_position(position);
    *s.place.borrow_mut() = Place::Pane {
        paned: paned.clone(),
        hidden,
    };
}

/// Take the notebook out of `paned`, if that is where it is.
pub fn hide_in_pane(paned: &gtk4::Paned) {
    let s = session();
    let here = matches!(&*s.place.borrow(), Place::Pane { paned: p, .. } if p == paned);
    if here {
        s.view.flush();
        paned.set_end_child(gtk4::Widget::NONE);
        *s.place.borrow_mut() = Place::Nowhere;
    }
}

fn detach(s: &Rc<Session>) {
    s.view.flush();
    let old = std::mem::replace(&mut *s.place.borrow_mut(), Place::Nowhere);
    match old {
        Place::Pane { paned, hidden } => {
            paned.set_end_child(gtk4::Widget::NONE);
            hidden();
        }
        Place::Window(w) => {
            w.set_content(gtk4::Widget::NONE);
            w.destroy();
        }
        Place::Nowhere => {}
    }
}

/// The notebook in a window of its own, optionally opened on `stem`.
pub fn open_window(parent: Option<&gtk4::Window>, stem: Option<&str>) {
    let s = session();
    if let Some(stem) = stem {
        s.view.open(stem);
    } else {
        s.view.ensure_open();
    }
    if let Place::Window(w) = &*s.place.borrow() {
        w.present();
        return;
    }
    detach(&s);
    let window = adw::Window::new();
    window.set_title(Some("Notebook"));
    window.set_default_size(520, 680);
    if let Some(p) = parent {
        window.set_transient_for(Some(p));
    }
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    s.view.widget().set_size_request(-1, -1);
    toolbar.set_content(Some(s.view.widget()));
    window.set_content(Some(&toolbar));
    {
        let s = s.clone();
        window.connect_close_request(move |w| {
            s.view.flush();
            if let Some(tb) = w
                .content()
                .and_then(|c| c.downcast::<adw::ToolbarView>().ok())
            {
                tb.set_content(gtk4::Widget::NONE);
            }
            *s.place.borrow_mut() = Place::Nowhere;
            glib::Propagation::Proceed
        });
    }
    *s.place.borrow_mut() = Place::Window(window.clone());
    window.present();
}

/// Save whatever is being written; call when the app is closing.
pub fn flush() {
    if let Some(s) = SESSION.with(|s| s.borrow().clone()) {
        s.view.flush();
    }
}

/// The notebooks on disk, for the launcher's list.
pub fn summaries() -> Vec<crate::notebook::Summary> {
    crate::notebook::list(&crate::notebook::dir())
}

pub fn delete_notebook(stem: &str) {
    let s = session();
    s.view.forget(stem);
    crate::notebook::delete(&crate::notebook::dir(), stem);
}

/// A Library shelf was renamed (`to` its new name) or deleted (`to` is `None`).
pub fn shelf_changed(from: &str, to: Option<&str>) {
    let s = session();
    s.view.flush();
    crate::notebook::move_shelf(&crate::notebook::dir(), from, to);
    s.view.reload();
}

pub fn create_notebook(title: &str, shelf: Option<String>) -> Option<String> {
    let nb = crate::notebook::create(&crate::notebook::dir(), title, shelf).ok()?;
    Some(nb.stem)
}

/// Name a new notebook and choose the Library shelf it belongs to, if any.
pub fn prompt_new(
    parent: Option<&gtk4::Window>,
    shelves: Vec<String>,
    then: impl Fn(String, Option<String>) + 'static,
) {
    let dialog = adw::MessageDialog::new(parent, Some("New notebook"), None);
    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some("Chapter 3 reading, Essay on Q…"));
    entry.set_activates_default(true);
    let mut names = vec!["No shelf (stand-alone)".to_string()];
    names.extend(shelves.iter().cloned());
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let drop = gtk4::DropDown::from_strings(&refs);
    drop.set_tooltip_text(Some("The Library shelf this notebook belongs to"));
    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    column.append(&entry);
    column.append(&drop);
    dialog.set_extra_child(Some(&column));
    dialog.add_responses(&[("cancel", "Cancel"), ("ok", "Create")]);
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    let field = entry.clone();
    dialog.connect_response(None, move |_, id| {
        if id == "ok" {
            let title = field.text().trim().to_string();
            let title = if title.is_empty() {
                "Notebook".to_string()
            } else {
                title
            };
            let shelf = match drop.selected() {
                0 => None,
                i => shelves.get(i as usize - 1).cloned(),
            };
            then(title, shelf);
        }
    });
    dialog.present();
    entry.grab_focus();
}

/// A one-line text prompt, for naming things.
pub(crate) fn prompt(
    parent: Option<&gtk4::Window>,
    heading: &str,
    initial: &str,
    ok: &str,
    allow_empty: bool,
    then: impl Fn(String) + 'static,
) {
    let dialog = adw::MessageDialog::new(parent, Some(heading), None);
    let entry = gtk4::Entry::new();
    entry.set_text(initial);
    entry.set_activates_default(true);
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "Cancel"), ("ok", ok)]);
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    let field = entry.clone();
    dialog.connect_response(None, move |_, id| {
        if id == "ok" {
            let text = field.text().trim().to_string();
            if allow_empty || !text.is_empty() {
                then(text);
            }
        }
    });
    dialog.present();
    entry.grab_focus();
}
