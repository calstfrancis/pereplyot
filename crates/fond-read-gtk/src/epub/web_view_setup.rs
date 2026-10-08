use super::*;

pub(super) struct WebViewSetupParts {
    pub(super) web_view: webkit6::WebView,
}

pub(super) fn build_web_view_setup(reader: &Rc<RefCell<EpubReaderState>>) -> WebViewSetupParts {
    let reader = reader.clone();
    let web_view = webkit6::WebView::new();
    web_view.set_vexpand(true);
    web_view.set_hexpand(true);
    // Scale text size only, not the page layout/images — "zoom" on a WebView otherwise
    // scales everything, which reads as zooming a picture rather than adjusting font size.
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&web_view) {
        settings.set_zoom_text_only(true);
        // Scripts inside the book never run; the reader's own highlight scripts still do
        // (they go through `evaluate_javascript`, not page markup).
        settings.set_enable_javascript_markup(false);
        settings.set_allow_file_access_from_file_urls(false);
        settings.set_allow_universal_access_from_file_urls(false);
    }
    {
        let cache_root = reader.borrow().cache_dir.clone();
        web_view.connect_decide_policy(move |_view, decision, kind| {
            use webkit6::PolicyDecisionType;
            if kind != PolicyDecisionType::NavigationAction {
                return false;
            }
            let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
                return false;
            };
            let uri = nav
                .navigation_action()
                .and_then(|a| a.request())
                .and_then(|r| r.uri())
                .map(|u| u.to_string())
                .unwrap_or_default();
            let local = gio::File::for_uri(&uri)
                .path()
                .is_some_and(|p| p.starts_with(&cache_root));
            if local {
                return false;
            }
            if uri.starts_with("http://")
                || uri.starts_with("https://")
                || uri.starts_with("mailto:")
            {
                gtk4::UriLauncher::new(&uri).launch(
                    gtk4::Window::NONE,
                    gio::Cancellable::NONE,
                    |_| {},
                );
            }
            decision.ignore();
            true
        });
    }
    WebViewSetupParts { web_view }
}
