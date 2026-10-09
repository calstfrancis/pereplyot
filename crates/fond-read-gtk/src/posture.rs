//! Reading postures: Read (just the page), Study (the page and your notes) and Synthesise (the
//! page, your notes and the notebook), chosen from the command palette.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Posture {
    Read,
    Study,
    Synthesise,
}

impl Posture {
    pub const ALL: [Posture; 3] = [Posture::Read, Posture::Study, Posture::Synthesise];

    pub fn name(self) -> &'static str {
        match self {
            Posture::Read => "Read",
            Posture::Study => "Study",
            Posture::Synthesise => "Synthesise",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Posture::Read => "Just the page — no sidebars, no notebook",
            Posture::Study => "The page and your notes",
            Posture::Synthesise => "The page, your notes and the notebook beside it",
        }
    }

    /// Which of the notes list and the notebook are on in this posture.
    pub fn panes(self) -> (bool, bool) {
        match self {
            Posture::Read => (false, false),
            Posture::Study => (true, false),
            Posture::Synthesise => (true, true),
        }
    }
}

/// The posture of the panes as they stand: the notebook means Synthesise, the notes list alone
/// Study, neither Read.
pub fn of(notes_open: bool, notebook_open: bool) -> Posture {
    if notebook_open {
        Posture::Synthesise
    } else if notes_open {
        Posture::Study
    } else {
        Posture::Read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panes_that_are_open_name_the_posture_and_back() {
        for p in Posture::ALL {
            let (notes, notebook) = p.panes();
            assert_eq!(of(notes, notebook), p);
        }
        assert_eq!(of(false, true), Posture::Synthesise);
    }
}
