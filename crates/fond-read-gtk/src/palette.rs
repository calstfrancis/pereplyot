//! The four-colour highlighting palette shown in both readers' header: each colour carries a
//! meaning (thesis / evidence / connection / problem), not just a hue, so a page of highlights
//! reads as an argument map rather than decoration.
//!
//! The colours themselves are fixed; their labels are the embedding app's to change (see
//! [`set_highlight_labels`]). Kartoteka and Sputnik never call it and so show the defaults.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::Orientation;

pub struct HighlightColor {
    pub name: &'static str,
    pub hex: &'static str,
    pub default_label: &'static str,
    pub question: &'static str,
    pub guidance: &'static str,
}

pub const HIGHLIGHT_COLORS: [HighlightColor; 4] = [
    HighlightColor {
        name: "Warm ochre",
        hex: "#D6B86A",
        default_label: "Key idea / thesis",
        question: "What is the author saying?",
        guidance: "Main arguments, definitions, central claims",
    },
    HighlightColor {
        name: "Dusty blue",
        hex: "#91A9B8",
        default_label: "Evidence / support",
        question: "What supports it?",
        guidance: "Historical evidence, quotations, examples, data, biblical/textual evidence",
    },
    HighlightColor {
        name: "Sage",
        hex: "#9CAF88",
        default_label: "Connection / implication",
        question: "Why does it matter / what does it connect to?",
        guidance: "Connections to other readings, ideas you want to pursue, implications for \
                   theology/ministry",
    },
    HighlightColor {
        name: "Terracotta",
        hex: "#C98F78",
        default_label: "Problem / question",
        question: "What needs scrutiny?",
        guidance: "Claims you doubt, contradictions, weaknesses, questions, things requiring \
                   further investigation",
    },
];

thread_local! {
    static LABELS: RefCell<[Option<String>; 4]> = const { RefCell::new([None, None, None, None]) };
}

/// Override the colours' labels, in [`HIGHLIGHT_COLORS`] order. A blank entry falls back to
/// that colour's default. Tooltips read this at hover time, so already-open readers pick up a
/// change without being rebuilt.
pub fn set_highlight_labels(labels: &[String]) {
    LABELS.with(|l| {
        let mut l = l.borrow_mut();
        for (i, slot) in l.iter_mut().enumerate() {
            *slot = labels
                .get(i)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
    });
}

pub fn highlight_label(index: usize) -> String {
    LABELS
        .with(|l| l.borrow().get(index).cloned().flatten())
        .unwrap_or_else(|| HIGHLIGHT_COLORS[index].default_label.to_string())
}

/// The label for a stored annotation colour, if it's one of the palette's.
pub fn label_for_hex(hex: &str) -> Option<String> {
    HIGHLIGHT_COLORS
        .iter()
        .position(|c| c.hex.eq_ignore_ascii_case(hex))
        .map(highlight_label)
}

fn tooltip_markup(index: usize) -> String {
    let color = &HIGHLIGHT_COLORS[index];
    let label = highlight_label(index);
    let mut markup = format!("<b>{}</b>", gtk4::glib::markup_escape_text(&label));
    // The guidance describes the default meaning; once a user renames a colour it may no
    // longer apply, so only the label is shown.
    if label == color.default_label {
        markup.push_str(&format!(
            "\n<i>{}</i>\n{}",
            gtk4::glib::markup_escape_text(color.question),
            gtk4::glib::markup_escape_text(color.guidance)
        ));
    }
    markup
}

pub fn swatch(hex: &'static str) -> gtk4::DrawingArea {
    let area = gtk4::DrawingArea::new();
    area.set_content_width(16);
    area.set_content_height(16);
    area.set_valign(gtk4::Align::Center);
    area.set_halign(gtk4::Align::Center);
    area.set_draw_func(move |_, cr, w, h| {
        let [r, g, b, _] = crate::pdf::annotation_rgba(Some(hex));
        let radius = (w.min(h) as f64) / 2.0;
        cr.arc(
            w as f64 / 2.0,
            h as f64 / 2.0,
            radius,
            0.0,
            std::f64::consts::TAU,
        );
        cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        let _ = cr.fill();
    });
    area
}

/// A linked row of radio-style toggles: an optional "Select text" button followed by the four
/// colours. `on_change` gets `None` for Select text, `Some(i)` for colour `i`; it isn't
/// called for the initial state.
pub fn palette_widget(
    with_select: bool,
    initial: Option<usize>,
    on_change: impl Fn(Option<usize>) + 'static,
) -> gtk4::Box {
    let row = gtk4::Box::new(Orientation::Horizontal, 0);
    row.add_css_class("linked");
    let on_change: Rc<dyn Fn(Option<usize>)> = Rc::new(on_change);

    let mut first: Option<gtk4::ToggleButton> = None;
    if with_select {
        let select = gtk4::ToggleButton::new();
        crate::set_icon_with_fallback(
            &select,
            &["format-text-plaintext-symbolic", "edit-select-symbolic"],
        );
        select.set_tooltip_text(Some("Select text (drag to copy)"));
        select.set_active(initial.is_none());
        let on_change = on_change.clone();
        select.connect_toggled(move |b| {
            if b.is_active() {
                on_change(None);
            }
        });
        row.append(&select);
        first = Some(select);
    }

    for (i, color) in HIGHLIGHT_COLORS.iter().enumerate() {
        let button = gtk4::ToggleButton::new();
        button.set_child(Some(&swatch(color.hex)));
        button.set_has_tooltip(true);
        button.connect_query_tooltip(move |_, _, _, _, tooltip| {
            tooltip.set_markup(Some(&tooltip_markup(i)));
            true
        });
        button.update_property(&[gtk4::accessible::Property::Label(color.name)]);
        match &first {
            Some(f) => button.set_group(Some(f)),
            None => first = Some(button.clone()),
        }
        button.set_active(initial == Some(i));
        let on_change = on_change.clone();
        button.connect_toggled(move |b| {
            if b.is_active() {
                on_change(Some(i));
            }
        });
        row.append(&button);
    }
    row
}
