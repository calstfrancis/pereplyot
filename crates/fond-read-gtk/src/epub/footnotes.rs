//! Note references in the text open the note in a popover beside them, so reading is not
//! interrupted by a jump to the back of the book and the way back.

use super::*;

/// Listens for a click on a link that is a note reference — marked `epub:type="noteref"` or
/// `role="doc-noteref"`, or an unmarked superscript or bare number linking to an anchor — and
/// reports it instead of following it.
const FOOTNOTE_JS: &str = r#"(function() {
  if (window.__pereplyotNotes) return;
  window.__pereplyotNotes = true;
  function noteText(el) {
    var copy = el.cloneNode(true);
    copy.querySelectorAll('a[href]').forEach(function(a) {
      if (/^[\s↩↵⏎^↑]*$/.test(a.textContent)) a.remove();
    });
    return (copy.textContent || '').replace(/\s+/g, ' ').trim();
  }
  document.addEventListener('click', function(e) {
    var a = e.target && e.target.closest ? e.target.closest('a[href]') : null;
    if (!a || a.closest('nav')) return;
    var href = a.getAttribute('href') || '';
    if (!href || /^[a-z][a-z0-9+.-]*:/i.test(href) || href.indexOf('#') < 0) return;
    var kind = (a.getAttribute('epub:type') || '') + ' ' + (a.getAttribute('role') || '');
    var label = (a.textContent || '').trim();
    var isNote = /noteref/.test(kind) ||
      (!!a.closest('sup') || /^[\[(]?(\d{1,3}|[*†‡§]+)[\])]?$/.test(label));
    if (!isNote) return;
    var r = a.getBoundingClientRect();
    var payload = { href: href, x: r.left + r.width / 2, y: r.bottom, label: label };
    if (href.charAt(0) === '#') {
      var target = document.getElementById(href.slice(1));
      if (target) payload.text = noteText(target);
    }
    e.preventDefault();
    e.stopPropagation();
    window.webkit.messageHandlers.pereplyot.postMessage(JSON.stringify(payload));
  }, true);
})();"#;

pub(super) fn install_script(view: &webkit6::WebView) {
    view.evaluate_javascript(FOOTNOTE_JS, None, None, gio::Cancellable::NONE, |_| {});
}

#[derive(serde::Deserialize)]
struct Click {
    href: String,
    x: f64,
    y: f64,
    #[serde(default)]
    label: String,
    #[serde(default)]
    text: Option<String>,
}

/// The text of the element with id `id` in `html`: its contents without tags, minus any link
/// back to the reference.
pub(super) fn note_text_in(html: &str, id: &str) -> Option<String> {
    let needle_double = format!("id=\"{id}\"");
    let needle_single = format!("id='{id}'");
    let at = html
        .find(&needle_double)
        .or_else(|| html.find(&needle_single))?;
    let open = html[..at].rfind('<')?;
    let name: String = html[open + 1..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if name.is_empty() {
        return None;
    }
    let open_end = html[open..].find('>')? + open + 1;
    let mut depth = 1usize;
    let mut cursor = open_end;
    let open_tag = format!("<{name}");
    let close_tag = format!("</{name}");
    let mut end = html.len();
    if html[open..open_end].ends_with("/>") {
        end = open_end;
    } else {
        while cursor < html.len() {
            let next_open = html[cursor..].find(&open_tag).map(|i| i + cursor);
            let next_close = html[cursor..].find(&close_tag).map(|i| i + cursor)?;
            match next_open {
                Some(o)
                    if o < next_close
                        && html[o + open_tag.len()..].starts_with(['>', ' ', '\n', '/']) =>
                {
                    depth += 1;
                    cursor = o + open_tag.len();
                }
                _ => {
                    depth -= 1;
                    if depth == 0 {
                        end = next_close;
                        break;
                    }
                    cursor = next_close + close_tag.len();
                }
            }
        }
    }
    let inner = &html[open_end..end];
    let mut text = String::new();
    let mut rest = inner;
    while let Some(lt) = rest.find('<') {
        text.push_str(&rest[..lt]);
        let Some(gt) = rest[lt..].find('>') else {
            break;
        };
        let tag = &rest[lt..lt + gt + 1];
        if tag.starts_with("<a ") {
            if let Some(close) = rest[lt..].find("</a>") {
                let link_text = decode_numeric(&strip(&rest[lt + gt + 1..lt + close]));
                let is_back = link_text
                    .chars()
                    .all(|c| c.is_whitespace() || "\u{21a9}\u{21b5}\u{23ce}^\u{2191}".contains(c));
                if is_back {
                    rest = &rest[lt + close + 4..];
                    continue;
                }
            }
        }
        let name: String = tag[1..]
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if matches!(
            name.as_str(),
            "p" | "div"
                | "li"
                | "ul"
                | "ol"
                | "br"
                | "aside"
                | "section"
                | "blockquote"
                | "tr"
                | "td"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
        ) {
            text.push(' ');
        }
        rest = &rest[lt + gt + 1..];
    }
    text.push_str(rest);
    let text = decode_numeric(&text);
    let text = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&nbsp;", " ");
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!flat.is_empty()).then_some(flat)
}

/// `&#8617;` and `&#x21a9;` as the characters they stand for.
fn decode_numeric(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find("&#") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let decoded = after.find(';').and_then(|end| {
            let body = &after[..end];
            let code = match body.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => body.parse().ok()?,
            };
            Some((char::from_u32(code)?, end + 1))
        });
        match decoded {
            Some((c, used)) => {
                out.push(c);
                rest = &after[used..];
            }
            None => {
                out.push_str("&#");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Show note references in a popover. `go_to` follows a target like `notes.xhtml#n1`.
pub(super) fn install(
    view: &webkit6::WebView,
    reader: &Rc<RefCell<EpubReaderState>>,
    go_to: Rc<dyn Fn(&str)>,
) {
    let Some(ucm) = webkit6::prelude::WebViewExt::user_content_manager(view) else {
        return;
    };
    ucm.register_script_message_handler("pereplyot", None);
    let view = view.clone();
    let reader = reader.clone();
    ucm.connect_script_message_received(Some("pereplyot"), move |_, value| {
        let Ok(click) = serde_json::from_str::<Click>(&value.to_str()) else {
            return;
        };
        let (current, cache_dir) = {
            let r = reader.borrow();
            (
                r.spine.get(r.index).cloned().unwrap_or_default(),
                r.cache_dir.clone(),
            )
        };
        let dir = std::path::Path::new(&current)
            .parent()
            .unwrap_or(std::path::Path::new(""))
            .to_path_buf();
        let (file, fragment) = pages::resolve(&dir, &click.href);
        let target = if click.href.starts_with('#') {
            format!("{current}#{}", fragment.clone().unwrap_or_default())
        } else {
            match &fragment {
                Some(f) => format!("{file}#{f}"),
                None => file.clone(),
            }
        };
        let text = click.text.clone().filter(|t| !t.is_empty()).or_else(|| {
            let id = fragment.as_deref()?;
            let html = std::fs::read_to_string(cache_dir.join(&file)).ok()?;
            note_text_in(&html, id)
        });
        show(
            &view,
            (click.x, click.y),
            &click.label,
            text,
            &target,
            go_to.clone(),
        );
    });
}

fn show(
    view: &webkit6::WebView,
    at: (f64, f64),
    label: &str,
    text: Option<String>,
    target: &str,
    go_to: Rc<dyn Fn(&str)>,
) {
    let popover = gtk4::Popover::new();
    popover.set_parent(view);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        at.0.round() as i32,
        at.1.round() as i32,
        1,
        1,
    )));
    let body = gtk4::Box::new(Orientation::Vertical, 6);
    body.set_margin_top(8);
    body.set_margin_bottom(8);
    body.set_margin_start(10);
    body.set_margin_end(10);
    let words = gtk4::Label::new(Some(
        text.as_deref()
            .unwrap_or("The note could not be found; use “Go to note”."),
    ));
    words.set_wrap(true);
    words.set_xalign(0.0);
    words.set_max_width_chars(56);
    words.set_selectable(true);
    words.set_can_focus(false);
    words.update_property(&[gtk4::accessible::Property::Label(&format!("Note {label}"))]);
    body.append(&words);
    let go = gtk4::Button::with_label("Go to note");
    go.add_css_class("flat");
    go.set_halign(gtk4::Align::End);
    {
        let popover = popover.clone();
        let target = target.to_string();
        go.connect_clicked(move |_| {
            popover.popdown();
            go_to(&target);
        });
    }
    body.append(&go);
    popover.set_child(Some(&body));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_its_text_without_the_link_back() {
        let html = r#"<body><aside id="n1" epub:type="footnote"><p><a href="c1.xhtml">1.</a> See Smith (2019), p. 4, for the <em>original</em> argument. <a href="c1.xhtml#r1">&#8617;</a></p></aside></body>"#;
        assert_eq!(
            note_text_in(html, "n1").as_deref(),
            Some("1. See Smith (2019), p. 4, for the original argument.")
        );
    }

    #[test]
    fn a_list_item_note_stops_at_its_own_end_even_with_nested_lists() {
        let html = r#"<ol><li id="n2"><p>On this point compare Jones.</p><ol><li>inner</li></ol></li><li id="n3">Other</li></ol>"#;
        assert_eq!(
            note_text_in(html, "n2").as_deref(),
            Some("On this point compare Jones. inner")
        );
        assert_eq!(note_text_in(html, "n3").as_deref(), Some("Other"));
        assert_eq!(note_text_in(html, "missing"), None);
    }

    #[test]
    fn the_fixtures_notes_are_found_where_the_book_keeps_them() {
        let epub = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/scholar.epub");
        let dir = std::env::temp_dir().join(format!("pereplyot-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fond_doc::epub::extract_all(&epub, &dir).unwrap();
        let html = std::fs::read_to_string(dir.join("OEBPS/notes.xhtml")).unwrap();
        assert!(note_text_in(&html, "n1").unwrap().contains("Smith (2019)"));
        assert!(note_text_in(&html, "n2").unwrap().contains("Jones (2021)"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
