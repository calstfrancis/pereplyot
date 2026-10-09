use super::*;

/// Why the reader jumped, so a run of search-result steps counts as one trip in the history.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum JumpKind {
    Link,
    Outline,
    Search,
    Annotation,
    Page,
    Thumbnail,
    Key,
}

/// Where the reader has been, browser style: pages to go back to and pages to go forward to.
#[derive(Default)]
pub(super) struct NavHistory {
    back: Vec<u16>,
    forward: Vec<u16>,
    last: Option<JumpKind>,
}

impl NavHistory {
    /// Note a jump from `from` to `to`. A jump to the page you are on is not one, and stepping
    /// through search results keeps the page the search began from rather than every hit.
    pub(super) fn record(&mut self, from: u16, to: u16, kind: JumpKind) {
        if from == to {
            return;
        }
        if kind == JumpKind::Search && self.last == Some(JumpKind::Search) {
            self.forward.clear();
            return;
        }
        if self.back.last() != Some(&from) {
            self.back.push(from);
        }
        self.forward.clear();
        self.last = Some(kind);
    }

    pub(super) fn go_back(&mut self, current: u16) -> Option<u16> {
        let page = self.back.pop()?;
        if self.forward.last() != Some(&current) {
            self.forward.push(current);
        }
        self.last = None;
        Some(page)
    }

    pub(super) fn go_forward(&mut self, current: u16) -> Option<u16> {
        let page = self.forward.pop()?;
        if self.back.last() != Some(&current) {
            self.back.push(current);
        }
        self.last = None;
        Some(page)
    }

    pub(super) fn back_target(&self) -> Option<u16> {
        self.back.last().copied()
    }

    pub(super) fn forward_target(&self) -> Option<u16> {
        self.forward.last().copied()
    }
}

/// What the history needs from the window: how to move there, and how to refresh the buttons.
pub(super) struct NavUi {
    pub(super) goto: Rc<dyn Fn(u16)>,
    pub(super) changed: Rc<dyn Fn()>,
}

/// Jump to `page`, remembering where you were so Back (Alt+Left) returns.
pub(super) fn jump(reader: &Rc<RefCell<ReaderState>>, page: u16, kind: JumpKind) {
    let ui = {
        let mut r = reader.borrow_mut();
        let from = r.page;
        r.history.record(from, page, kind);
        r.nav.as_ref().map(|n| (n.goto.clone(), n.changed.clone()))
    };
    if let Some((goto, changed)) = ui {
        goto(page);
        changed();
    }
}

/// Refresh the Back and Forward buttons after the history changed.
pub(super) fn notify_nav_changed(reader: &Rc<RefCell<ReaderState>>) {
    let changed = reader.borrow().nav.as_ref().map(|n| n.changed.clone());
    if let Some(changed) = changed {
        changed();
    }
}

pub(super) fn install_link_nav(ui: &PdfUi) {
    let PdfUi {
        reader,
        render,
        continuous_toggle,
        link_back,
        link_forward,
        continuous_scroll,
        ..
    } = ui.clone();
    let goto: Rc<dyn Fn(u16)> = {
        let reader = reader.clone();
        let render = render.clone();
        let continuous_toggle = continuous_toggle.clone();
        let continuous_scroll = continuous_scroll.clone();
        Rc::new(move |page: u16| {
            if continuous_toggle.is_active() {
                scroll_continuous_to_page(&reader, &continuous_scroll, page);
            } else {
                reader.borrow_mut().page = page;
                render();
            }
        })
    };
    let changed: Rc<dyn Fn()> = {
        let reader = reader.clone();
        let link_back = link_back.clone();
        let link_forward = link_forward.clone();
        Rc::new(move || {
            let r = reader.borrow();
            match r.history.back_target() {
                Some(p) => {
                    link_back.set_label(&format!("← Back to p. {}", p + 1));
                    link_back.set_visible(true);
                }
                None => link_back.set_visible(false),
            }
            match r.history.forward_target() {
                Some(p) => {
                    link_forward.set_label(&format!("Forward to p. {} →", p + 1));
                    link_forward.set_visible(true);
                }
                None => link_forward.set_visible(false),
            }
        })
    };
    reader.borrow_mut().nav = Some(NavUi {
        goto: goto.clone(),
        changed: changed.clone(),
    });
    {
        let reader = reader.clone();
        let goto = goto.clone();
        let changed = changed.clone();
        link_back.connect_clicked(move |_| {
            let dest = {
                let mut r = reader.borrow_mut();
                let current = r.page;
                r.history.go_back(current)
            };
            if let Some(page) = dest {
                goto(page);
            }
            changed();
        });
    }
    {
        let reader = reader.clone();
        link_forward.connect_clicked(move |_| {
            let dest = {
                let mut r = reader.borrow_mut();
                let current = r.page;
                r.history.go_forward(current)
            };
            if let Some(page) = dest {
                goto(page);
            }
            changed();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_and_forward_walk_the_trail() {
        let mut h = NavHistory::default();
        h.record(0, 10, JumpKind::Link);
        h.record(10, 40, JumpKind::Outline);
        assert_eq!(h.go_back(40), Some(10));
        assert_eq!(h.go_back(10), Some(0));
        assert_eq!(h.go_back(0), None);
        assert_eq!(h.go_forward(0), Some(10));
        assert_eq!(h.go_forward(10), Some(40));
        assert_eq!(h.go_forward(40), None);
    }

    #[test]
    fn a_new_jump_after_going_back_drops_the_forward_trail() {
        let mut h = NavHistory::default();
        h.record(0, 10, JumpKind::Link);
        h.go_back(10);
        assert_eq!(h.forward_target(), Some(10));
        h.record(0, 25, JumpKind::Page);
        assert_eq!(h.forward_target(), None);
    }

    #[test]
    fn stepping_through_search_results_is_one_trip() {
        let mut h = NavHistory::default();
        h.record(3, 12, JumpKind::Search);
        h.record(12, 30, JumpKind::Search);
        h.record(30, 77, JumpKind::Search);
        assert_eq!(h.go_back(77), Some(3));
        assert_eq!(h.go_back(3), None);
    }

    #[test]
    fn jumping_to_the_page_you_are_on_is_not_a_jump() {
        let mut h = NavHistory::default();
        h.record(5, 5, JumpKind::Link);
        assert_eq!(h.back_target(), None);
    }
}
