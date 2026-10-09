use super::*;

pub(super) fn install_mark_mode(ui: &PdfUi) {
    let PdfUi {
        reader,
        mode_change,
        palette_choice,
        style_drop,
        hint,
        picture,
        ..
    } = ui.clone();
    {
        let reader = reader.clone();
        let hint = hint.clone();
        let picture = picture.clone();
        let style_drop_for_mode = style_drop.clone();
        let palette_choice = palette_choice.clone();
        let apply_mode: Rc<dyn Fn()> = Rc::new(move || {
            let style_drop = &style_drop_for_mode;
            let choice = palette_choice.get();
            let kind = choice.map(|_| {
                MARK_KIND_OPTIONS
                    .get(style_drop.selected() as usize)
                    .map(|(_, k)| *k)
                    .unwrap_or(fond_annot::AnnotationKind::Highlight)
            });
            style_drop.set_sensitive(choice.is_some());
            {
                let mut r = reader.borrow_mut();
                r.draw_kind = kind;
                if let Some(color) = choice.and_then(|i| crate::palette::HIGHLIGHT_COLORS.get(i)) {
                    r.draw_color = color.hex.to_string();
                }
            }
            let cursor = cursor_for_select_mode(kind.is_none());
            picture.set_cursor(cursor.as_ref());
            if let Some(pages) = &reader.borrow().continuous {
                for (_, picture) in pages.live.values() {
                    picture.set_cursor(cursor.as_ref());
                }
            }
            let text = match kind {
                None => "Drag over text to select it, then choose a colour (or press 1–4)",
                Some(fond_annot::AnnotationKind::Highlight) => "Drag over text to highlight it",
                Some(fond_annot::AnnotationKind::Underline) => "Drag over text to underline it",
                Some(fond_annot::AnnotationKind::Strikeout) => "Drag over text to strike it out",
                Some(fond_annot::AnnotationKind::Area) => {
                    "Drag a box round a figure, table or equation to clip it"
                }
                Some(_) => "Drag over the page",
            };
            hint.set_text(text);
        });
        {
            let apply_mode = apply_mode.clone();
            style_drop.connect_selected_notify(move |_| apply_mode());
        }
        *mode_change.borrow_mut() = Some(apply_mode);
    }
}
