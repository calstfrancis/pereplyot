//! Clipping a picture: right-click an image in the book to keep it as an area annotation, which
//! outlines it on the page, shows it in the Notes list and exports it as a figure.

use super::*;

pub(super) fn install(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
) {
    let host = host.clone();
    let reader = reader.clone();
    web_view.connect_context_menu(move |view, menu, hit| {
        if !hit.context_is_image() {
            return false;
        }
        let Some(uri) = hit.image_uri().map(|u| u.to_string()) else {
            return false;
        };
        let action = gio::SimpleAction::new("pereplyot-clip-image", None);
        {
            let host = host.clone();
            let reader = reader.clone();
            let view = view.clone();
            action.connect_activate(move |_, _| {
                clip(&host, &reader, &view, &uri);
            });
        }
        let item = webkit6::ContextMenuItem::from_gaction(&action, "Clip this image", None);
        menu.append(&item);
        false
    });
}

fn clip(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<EpubReaderState>>,
    view: &webkit6::WebView,
    uri: &str,
) {
    let (cache_dir, chapter) = {
        let r = reader.borrow();
        (r.cache_dir.clone(), r.spine.get(r.index).cloned())
    };
    let Some(chapter) = chapter else { return };
    let Some(path) = gio::File::for_uri(uri)
        .path()
        .and_then(|p| p.strip_prefix(&cache_dir).ok().map(|p| p.to_path_buf()))
    else {
        host.notify("That picture is not part of the book");
        return;
    };
    let relative = path.to_string_lossy().replace('\\', "/");
    let script = format!(
        "(function(uri) {{ var i = Array.prototype.find.call(document.images, function(x) {{ \
         return x.currentSrc === uri || x.src === uri; }}); \
         if (!i) return '{{}}'; var page = null; \
         (window.__pp || []).forEach(function(p) {{ var el = document.getElementById(p.id); \
           if (el && (el.compareDocumentPosition(i) & Node.DOCUMENT_POSITION_FOLLOWING)) page = p.label; }}); \
         return JSON.stringify({{ alt: i.alt || i.title || '', page: page }}); }})({})",
        serde_json::to_string(uri).unwrap_or_default()
    );
    let host = host.clone();
    let reader = reader.clone();
    view.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, move |result| {
        #[derive(serde::Deserialize, Default)]
        struct Found {
            #[serde(default)]
            alt: String,
            #[serde(default)]
            page: Option<String>,
        }
        let found: Found = result
            .ok()
            .and_then(|v| serde_json::from_str(&v.to_str()).ok())
            .unwrap_or_default();
        let alt = found.alt;
        let name = std::path::Path::new(&relative)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let snippet = if alt.trim().is_empty() {
            format!("Figure ({name})")
        } else {
            alt
        };
        let mut annotation = fond_annot::Annotation::drawn_epub(
            fond_annot::AnnotationKind::Area,
            chapter.clone(),
            snippet,
            None,
            None,
            None,
        );
        annotation.color = Some(crate::palette::HIGHLIGHT_COLORS[0].hex.to_string());
        crate::set_image(&mut annotation, Some(&relative));
        {
            let r = reader.borrow();
            let label = page_label::label_for(&r, &chapter, found.page.clone());
            crate::set_page_label(&mut annotation, label.as_deref());
        }
        let store = reader.borrow().store.clone();
        match store.add(annotation) {
            Ok(()) => host.notify("Image clipped — it is in the Notes list"),
            Err(e) => host.notify(&e),
        }
    });
}
