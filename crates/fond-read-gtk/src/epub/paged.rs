//! Paginated mode: the chapter set in columns, one screen or one spread at a time, turned with
//! the keys or the wheel — the alternative to scrolling a chapter as one long page.

use super::*;

/// Helpers the reader's other scripts rely on, installed on every chapter load: reading and
/// restoring the position (a percentage that means the same thing scrolling or paginated), and
/// turning pages when paginated.
pub(super) const RUNTIME_JS: &str = r#"(function() {
  function root() { return document.scrollingElement || document.documentElement; }
  window.__paged = false;
  window.__pgCount = function() { var d = root(); return Math.max(1, Math.round(d.scrollWidth / d.clientWidth)); };
  window.__pgIndex = function() { var d = root(); return Math.round(d.scrollLeft / d.clientWidth); };
  window.__snap = function() {
    if (!window.__paged) return;
    var d = root();
    d.scrollLeft = Math.round(d.scrollLeft / d.clientWidth) * d.clientWidth;
  };
  window.__pgTurn = function(dir) {
    var d = root(), i = window.__pgIndex(), n = window.__pgCount(), j = i + dir;
    if (j < 0 || j >= n) return 0;
    d.scrollLeft = j * d.clientWidth;
    return 1;
  };
  window.__percent = function() {
    var d = root();
    if (window.__paged) {
      var n = window.__pgCount();
      return n > 1 ? Math.round(100 * window.__pgIndex() / (n - 1)) : 0;
    }
    var range = d.scrollHeight - d.clientHeight;
    return range > 0 ? Math.round(d.scrollTop / range * 100) : 0;
  };
  window.__setPercent = function(p) {
    var d = root();
    if (window.__paged) {
      d.scrollLeft = Math.round((window.__pgCount() - 1) * p / 100) * d.clientWidth;
    } else {
      d.scrollTo(0, Math.round((d.scrollHeight - d.clientHeight) * p / 100));
    }
  };
  window.__pgInfo = function() {
    return window.__paged ? (window.__pgIndex() + 1) + ' / ' + window.__pgCount() : '';
  };
  window.__layout = function(cols) {
    var gap = 72;
    var d = document.documentElement, b = document.body;
    if (!b) return;
    window.__paged = true;
    d.style.cssText = 'height:100vh;overflow:hidden;margin:0;padding:0;';
    b.style.cssText = 'margin:0;padding:28px ' + (gap / 2) + 'px;box-sizing:border-box;' +
      'height:100vh;width:100vw;max-width:none;column-count:' + cols + ';column-gap:' + gap +
      'px;column-fill:auto;';
    if (!document.getElementById('__pp_paged')) {
      var st = document.createElement('style');
      st.id = '__pp_paged';
      st.textContent = 'img,svg,video{max-width:100%;max-height:calc(100vh - 64px);}' +
        'h1,h2,h3,h4{break-after:avoid;}p{orphans:2;widows:2;}';
      (document.head || d).appendChild(st);
    }
    // The last page must be scrollable to as far as a whole page, or it would sit a margin
    // off to the left; an invisible box as wide as every page together makes it so.
    var tail = document.getElementById('__pp_tail');
    if (!tail) {
      tail = document.createElement('div');
      tail.id = '__pp_tail';
      tail.style.cssText = 'position:absolute;left:0;top:0;height:1px;width:0;pointer-events:none;visibility:hidden;';
      d.appendChild(tail);
    }
    tail.style.width = '0';
    var pages = Math.max(1, Math.ceil((d.scrollWidth - 2) / d.clientWidth));
    tail.style.width = (pages * d.clientWidth) + 'px';
    window.__snap();
  };
  window.__unpaged = function() {
    var tail = document.getElementById('__pp_tail');
    if (tail) tail.remove();
    window.__paged = false;
    document.documentElement.removeAttribute('style');
    if (document.body) document.body.removeAttribute('style');
    var st = document.getElementById('__pp_paged');
    if (st) st.remove();
  };
})();"#;

const FLAG: &str = "pereplyot-paginated";

/// Whether `view` is showing its chapters paginated (read by the stylesheet builder).
pub(super) fn is_paged(view: &webkit6::WebView) -> bool {
    // SAFETY: the flag is only ever stored as a `bool` under this key.
    unsafe { view.data::<bool>(FLAG).is_some_and(|p| *p.as_ref()) }
}

fn set_paged(view: &webkit6::WebView, on: bool) {
    // SAFETY: see `is_paged`.
    unsafe { view.set_data(FLAG, on) };
    super::chrome::apply_epub_style(view, &crate::typography::shared().get());
}

/// How many columns fit a view this wide: two (a spread) on a wide window, otherwise one.
fn columns(width: i32) -> u32 {
    if width >= 1100 {
        2
    } else {
        1
    }
}

/// Put the chapter in view into the mode the reader is in, keeping the place in it.
pub(super) fn sync_layout(view: &webkit6::WebView, reader: &Rc<RefCell<EpubReaderState>>) {
    let script = if reader.borrow().paginated {
        format!(
            "(function(){{var p=window.__percent();window.__layout({});window.__setPercent(p);}})()",
            columns(view.width())
        )
    } else {
        "(function(){var p=window.__percent();window.__unpaged();window.__setPercent(p);})()"
            .to_string()
    };
    view.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});
}

/// Script run on a chapter that has just loaded.
pub(super) fn load_script(paginated: bool, width: i32) -> String {
    if paginated {
        format!("{RUNTIME_JS}\nwindow.__layout({});", columns(width))
    } else {
        RUNTIME_JS.to_string()
    }
}

pub(super) struct PagedParts {
    pub(super) toggle: gtk4::ToggleButton,
    pub(super) spread: gtk4::Label,
    /// Turn the page by 1 or -1; at the end of the chapter, go to the next or previous one.
    pub(super) turn: Rc<dyn Fn(i32)>,
}

pub(super) fn build(
    host: &Rc<dyn ReaderHost>,
    reader: &Rc<RefCell<EpubReaderState>>,
    web_view: &webkit6::WebView,
    prev: &gtk4::Button,
    next: &gtk4::Button,
    pending_percent: &Rc<RefCell<Option<u8>>>,
) -> PagedParts {
    let toggle = gtk4::ToggleButton::new();
    toggle.add_css_class("flat");
    toggle.set_label("Pages");
    toggle.set_tooltip_text(Some(
        "Paginated — turn pages instead of scrolling (arrow keys, Space, the wheel); two columns \
         on a wide window",
    ));
    let spread = gtk4::Label::new(None);
    spread.add_css_class("dim-label");
    spread.set_visible(false);

    let turn: Rc<dyn Fn(i32)> = {
        let view = web_view.clone();
        let prev = prev.clone();
        let next = next.clone();
        let pending_percent = pending_percent.clone();
        Rc::new(move |dir: i32| {
            let prev = prev.clone();
            let next = next.clone();
            let pending_percent = pending_percent.clone();
            view.evaluate_javascript(
                &format!("window.__pgTurn({dir})"),
                None,
                None,
                gio::Cancellable::NONE,
                move |result| {
                    if result.map_or(1, |v| v.to_int32()) != 0 {
                        return;
                    }
                    if dir > 0 {
                        next.emit_clicked();
                    } else {
                        *pending_percent.borrow_mut() = Some(100);
                        prev.emit_clicked();
                    }
                },
            );
        })
    };

    {
        let host = host.clone();
        let reader = reader.clone();
        let view = web_view.clone();
        let spread = spread.clone();
        toggle.connect_toggled(move |t| {
            reader.borrow_mut().paginated = t.is_active();
            set_paged(&view, t.is_active());
            host.set_epub_paginated(t.is_active());
            spread.set_visible(t.is_active());
            sync_layout(&view, &reader);
        });
    }
    if reader.borrow().paginated {
        set_paged(web_view, true);
        toggle.set_active(true);
        spread.set_visible(true);
    }

    // The wheel turns pages (with a pause between turns, so a spinning wheel does not skip a
    // chapter), and the layout follows the window's width and the text size.
    {
        let wheel = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        wheel.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let reader = reader.clone();
        let turn = turn.clone();
        let last = Rc::new(Cell::new(
            std::time::Instant::now() - std::time::Duration::from_secs(1),
        ));
        wheel.connect_scroll(move |_, _, dy| {
            if !reader.borrow().paginated {
                return glib::Propagation::Proceed;
            }
            if last.get().elapsed() > std::time::Duration::from_millis(220) && dy != 0.0 {
                last.set(std::time::Instant::now());
                turn(if dy > 0.0 { 1 } else { -1 });
            }
            glib::Propagation::Stop
        });
        web_view.add_controller(wheel);
    }
    {
        let reader = Rc::downgrade(reader);
        let view = web_view.downgrade();
        let spread = spread.clone();
        let width = Cell::new(0);
        let size = Cell::new(0.0f64);
        glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
            let (Some(reader), Some(view)) = (reader.upgrade(), view.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if !reader.borrow().paginated {
                return glib::ControlFlow::Continue;
            }
            let (w, z) = (view.width(), view.zoom_level());
            if w != width.get() || (z - size.get()).abs() > f64::EPSILON {
                width.set(w);
                size.set(z);
                sync_layout(&view, &reader);
            }
            let spread = spread.clone();
            view.evaluate_javascript(
                "window.__snap && window.__snap(); window.__pgInfo ? window.__pgInfo() : ''",
                None,
                None,
                gio::Cancellable::NONE,
                move |result| {
                    if let Ok(v) = result {
                        spread.set_text(&v.to_str());
                    }
                },
            );
            glib::ControlFlow::Continue
        });
    }
    PagedParts {
        toggle,
        spread,
        turn,
    }
}
