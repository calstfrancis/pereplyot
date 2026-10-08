use super::*;

pub(super) struct MarkPaletteParts {
    pub(super) mode_change: crate::RebuildCell,
    pub(super) style_drop: gtk4::DropDown,
    pub(super) palette_choice: Rc<Cell<Option<usize>>>,
    pub(super) palette: gtk4::Box,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_mark_palette() -> MarkPaletteParts {
    // What a drag on the page does: the palette picks Select text or a colour, and the style
    // drop-down picks which kind of mark that colour draws. Both feed one `apply_mode` closure,
    // wired below once `hint`/`picture` exist.
    let mode_change: crate::RebuildCell = Rc::new(RefCell::new(None));
    let style_labels: Vec<&str> = MARK_KIND_OPTIONS.iter().map(|(l, _)| *l).collect();
    let style_drop = gtk4::DropDown::from_strings(&style_labels);
    style_drop.set_tooltip_text(Some("What kind of mark a drag draws"));
    style_drop.set_sensitive(false);
    let palette_choice: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    let palette = {
        let palette_choice = palette_choice.clone();
        let mode_change = mode_change.clone();
        crate::palette::palette_widget(true, None, move |choice| {
            palette_choice.set(choice);
            if let Some(f) = mode_change.borrow().as_ref() {
                f();
            }
        })
    };

    MarkPaletteParts {
        mode_change,
        style_drop,
        palette_choice,
        palette,
    }
}
