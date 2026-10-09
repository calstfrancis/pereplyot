//! The data the reflow pipeline passes along: words as they sit on a page, and the flow of
//! headings, paragraphs and notes made from them.

/// One word of a page, in the page as displayed: points, origin at the top left, y growing down.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
}

impl Word {
    pub fn centre_y(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
}

/// Every word of one page, in no particular order.
#[derive(Clone, Debug, Default)]
pub struct RawPage {
    pub page: u16,
    pub width: f32,
    pub height: f32,
    pub words: Vec<Word>,
    /// The regions of the page drawn with pictures or lines rather than text, as `[x0, y0, x1, y1]`
    /// in the same space as the words: touching ones are already joined into one.
    pub graphics: Vec<[f32; 4]>,
}

/// Where a stretch of a paragraph's text came from on the page: `start..end` are char offsets
/// into the paragraph's text.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceWord {
    pub start: usize,
    pub end: usize,
    pub page: u16,
    pub bbox: [f32; 4],
}

/// A note-marker in the body: `label` ("12", "*") goes at char offset `at` of the paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub at: usize,
    pub label: String,
}

/// A footnote or endnote. `anchor` is the char offset in its paragraph of the marker that cites
/// it; `None` means no marker was found, and the note is kept and shown as unmatched rather than
/// dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub label: String,
    pub text: String,
    pub page: u16,
    pub anchor: Option<usize>,
}

/// Source page `page` begins at char offset `at` of the paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct PageBreak {
    pub at: usize,
    pub page: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paragraph {
    pub text: String,
    pub markers: Vec<Marker>,
    pub notes: Vec<Note>,
    pub breaks: Vec<PageBreak>,
    pub source: Vec<SourceWord>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Heading {
        level: u8,
        text: String,
        page: u16,
    },
    /// A figure or table, kept as a picture of this part of `page` (`[x0, y0, x1, y1]`, page
    /// points as displayed) instead of text.
    Figure {
        page: u16,
        bbox: [f32; 4],
    },
    Paragraph(Paragraph),
}
