use super::*;

pub(super) enum LinkTarget {
    Page(u16),
    Uri(String),
}

pub(super) fn link_at(r: &ReaderState, page: u16, x: f64, y: f64) -> Option<LinkTarget> {
    use pdfium_render::prelude::{PdfActionType, PdfPoints};
    let doc = r.doc.as_ref()?;
    let pdf_page = doc.pages().get(page).ok()?;
    let links = pdf_page.links();
    let link = links.link_at_point(PdfPoints::new(x as f32), PdfPoints::new(y as f32))?;
    if let Some(dest) = link.destination() {
        return dest.page_index().ok().map(LinkTarget::Page);
    }
    let action = link.action()?;
    match action.action_type() {
        PdfActionType::GoToDestinationInSameDocument => action
            .as_local_destination_action()?
            .destination()
            .ok()?
            .page_index()
            .ok()
            .map(LinkTarget::Page),
        PdfActionType::Uri => action.as_uri_action()?.uri().ok().map(LinkTarget::Uri),
        _ => None,
    }
}

/// If a click at pixel `(px, py)` of a `render_w`×`render_h` render of `page` landed on a
/// link, follow it and report true. Internal links jump (remembering where we were); web
/// links open in the default handler.
pub(super) fn follow_link(
    reader: &Rc<RefCell<ReaderState>>,
    page: u16,
    px: f64,
    py: f64,
    render_w: f64,
    render_h: f64,
) -> bool {
    let (target, goto) = {
        let r = reader.borrow();
        if r.rotation != 0 || render_w < 1.0 || render_h < 1.0 {
            return false;
        }
        let Some(geom) = r.geom(page) else {
            return false;
        };
        let (x, y) = geom.px_to_pdf(px, py, render_w, render_h);
        (link_at(&r, page, x, y), r.link_goto.clone())
    };
    match target {
        Some(LinkTarget::Page(p)) => {
            if let Some(goto) = goto {
                goto(p);
            }
            true
        }
        Some(LinkTarget::Uri(uri)) => {
            if uri.starts_with("http://")
                || uri.starts_with("https://")
                || uri.starts_with("mailto:")
            {
                gtk4::UriLauncher::new(&uri).launch(
                    gtk4::Window::NONE,
                    gtk4::gio::Cancellable::NONE,
                    |_| {},
                );
            }
            true
        }
        None => false,
    }
}
