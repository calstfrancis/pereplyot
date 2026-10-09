# Pereplyot — Academic Reader Plan

Goal: make Pereplyot the best reader on any platform for someone who reads to *write* — a
student, scholar or preacher who reads closely, marks with intent, comes back months later
to find the passage, and turns it into a citation in a paper. Not a general e-book app.

Written 2026-10-08 against v0.10.0 "Fair Copy". **Status: settled 2026-10-08. Phase 0: smoke harness, `AnnotationStore`, `pdf/` and `epub/` module splits and the tolerant `fond-annot` are done and pinned (fond-core PR #1, Kartoteka branch `bump-fond-core-phase0`, both awaiting merge). Reader functions are split (`show_pdf_reader` ~290 lines, `show_epub_reader` ~330), and the performance budgets are measured (`tests/perf/BUDGETS.md`). **Phase 0 is complete** apart from merging the two PRs. **fond-core PR #1 and Kartoteka's pin bump were merged 2026-10-09.** **Phase 1 (rendering) is done 2026-10-09:** render thread, HiDPI sharpness, texture cache, background search and scan, open-once loading, vector mark layer with hover/select/resize/delete, dark/sepia tones, scrollbar ticks, virtual scroll view, in-place smooth zoom, tiles above 3000 device px, thumbnails on the render thread (numbers in `tests/perf/BUDGETS.md`; the 16 ms open stall is not met, see there). **Phase 1b (Reading mode) and Phase 2 (navigation) are built 2026-10-09; the gaps listed then were closed the same day** — figures and tables inline in Reading mode, headings feeding the Contents of a PDF with no outline, search hits inside Reading mode, Tesseract built into the Flatpak manifest (verified natively against a real Tesseract 5.5, not as a Flatpak build), recognised scans searchable in the page view, per-pane keys in the split view, pins kept between sessions, and Set page numbering from the margin menu. **Still open:** a long Reading-mode jump stalls ~0.8 s on a 600-page book (GTK lays the whole text out; fixing it means windowing the text), selecting text on a recognised scan in the *page* view, on-demand OCR language downloads (languages placed in `~/.local/share/pereplyot/tessdata` are used), the hairline across the column, hold-to-peek the page image, and a header/sidebars for the second split pane. The layout pipeline lives in `fond-read-gtk` (`reflow/`) rather than a new `fond-doc` API, to avoid another fond-core change.** **Phase 3 (annotation depth) is built 2026-10-09 against a local fond-core commit (`phase3-schema`, unpushed): area capture and Typst/Markdown/LaTeX figure export, sticky notes, note bubbles, tags, the richer Notes list, colour/kind change and merge. Not yet: resizing an area by handles, a thumbnail of the area in its Notes card, OCR'd text for an area on a scan, the PNG crop cache (crops are drawn at export time), note bubbles inside Reading mode, one-step undo for a merge.** **Phase 4 (interoperability) is built 2026-10-09 except the Kartoteka hand-off and the synced Markdown file:** import of other readers' marks on open, *Save a copy with annotations…* (checked in poppler), `pereplyot://` deep links, `--annotation=<id>` and `file://` arguments, ↗ links in Typst/Markdown exports. The hand-off needs Kartoteka to accept a file to add (it has no such flag yet, so it is a Kartoteka change and release); the Markdown file with block ids is not started. Needs fond-core `phase4-interop` (unpushed until Kartoteka's pin can follow).** **Phase 5 (the Notebook) is built 2026-10-09:** a Typst notebook beside the document (or in a window) with quote cards dragged from the Notes list or the launcher's Notes tab, click-through to the source, connections between annotations across documents (kept in `connections.json`), shelves, and export to Typst/Markdown/LaTeX with citations and *Open in Zerkalo*. **Not yet:** EPUB annotations in the notebook (Phase 6), a thumbnail of an area on its card, dragging straight off a mark on the page (the Notes list and *Add to notebook* cover it), moving a card (take it out with ✕ and drop it again), and a block listing connections inside the notebook.** **Phase 6 (EPUB for scholars) is built 2026-10-09:** printed pages from the page list or in-text breaks (status, annotations, exports, notebook), note popovers, paginated mode with spreads, fuzzy re-anchoring (quote, context, position; PDF marks keep context too), and parity — Notes cards with notebook/connect/tags, the Notebook toggle, image clipping — plus the default look is now Zerkalo's LaTeX Look. **Not yet:** paginated mode's click zones, EPUB area crops of part of an image, notebook export of EPUB figures, and tuning pagination for right-to-left or vertical text.** Phases are ordered by dependency first and
value second. Each one ships on its own as a minor release.

---

## 1. Where Pereplyot stands

### What's already better than most readers

- **Colours mean things.** The four-colour scheme (thesis / evidence / connection /
  problem, `palette.rs`) is a real reading method, not decoration. Grouping exports by those
  meanings (`export.rs` → `fond_annot::export`) is something Zotero, Skim and PDF Expert
  don't do.
- **The output is citations.** Typst/LaTeX/Markdown export with printed page labels and a
  citation key, plus "Copy with citation". Most readers stop at "export highlights as text".
- **Plain-file, content-hash-keyed storage** that's portable across Kartoteka and Sputnik,
  with atomic saves, a quarantine for damaged files, and a merge on save (`fsutil.rs`,
  `store.rs`). That durability is rare and worth protecting.
- **You select first, then choose a mark**, with 1–4 keys and a selection popover. That's
  the right interaction model.
- **Accessibility groundwork**: the PDF Text view (`pdf_text.rs`), accessible names on
  buttons, keyboard-reachable sidebars.
- **Printed page labels**, with a manual override for PDFs that don't declare any. This is
  essential for citing and most readers get it wrong.

### What's holding it back (from reading the code)

| # | Finding | Where | Why it matters |
|---|---|---|---|
| A1 | `show_pdf_reader` is a single **~2,570-line function**; `pdf.rs` is 5k lines. `show_epub_reader` is ~1,590 lines. Every feature reaches into one `Rc<RefCell<ReaderState>>` and rebuilds widgets by hand. | `pdf.rs:1981–4548`, `epub.rs:531–2118` | Every phase below adds a view of the same annotations (Notes sidebar, area capture, notebook, split view). Without a store that announces changes, each new view adds another "remember to rebuild X" call site. This is the main obstacle. |
| A2 | **Annotations are baked into the page bitmap** (`fond_doc::blend_annotations` on the RGBA buffer). Any mark change re-rasterises the page. | `pdf.rs:627` `render_pdf_page_texture` | Highlights can't have hover states, resize handles, smooth fades, or crisp edges at any zoom, and every edit costs a full PDFium render. |
| A3 | **Rendering ignores the HiDPI scale factor**: width is `READER_BASE_WIDTH * zoom` in logical pixels. | `pdf.rs:628` (no `scale_factor` anywhere in the repo) | Text looks blurry on every 2× laptop or 4K screen. For a reading app that's the first thing people notice. |
| A4 | **Rendering, document search and the Notes tab search all run on the GTK main thread.** `search_document` re-parses the whole file from `bytes` on each search; `notes_index::search` reads and parses every annotation file on every keystroke. | `pdf.rs:4384`, `notes_index.rs:56`, `notes_page.rs:175` | This is the same class of bug the root CLAUDE.md records for Sputnik: disk work on the main thread that only shows up with real-sized data. A 600-page monograph or a library of 400 annotated PDFs will freeze the window. |
| A5 | **Embedded PDF annotations are neither imported nor written back.** `fond_doc::extract_annotations` and `embed_highlights` exist but nothing calls them. | `fond-doc/src/annotation.rs` | A PDF already marked up in Acrobat, Zotero or Preview shows those marks as pixels you can't list, search or export. Nobody can share an annotated PDF *out* of Pereplyot with a supervisor or co-author. |
| A6 | **The annotation schema is closed.** `AnnotationKind` has four variants, with no `#[serde(other)]` fallback, no tags, no links, and no area geometry. | `fond-annot` (fond-core) | A new kind is needed (area). As things stand, an older Kartoteka/Sputnik/Pereplyot reading a sidecar that contains `"kind": "area"` would fail to parse the **whole file**, and `SidecarSync::load` would then **quarantine a valid file as corrupt**. This has to be fixed before any schema addition ships. |
| A7 | **EPUB is rendered one chapter at a time as a scrolling page**, anchored by chapter path plus snippet. It has no pagination, no footnote popups, and doesn't read the book's printed-page list. | `epub.rs` | Academic EPUBs (university presses, Perlego/VitalSource exports) carry `page-list` navigation precisely so they can be cited. Ignoring it makes EPUB quotes uncitable by page. |
| A8 | **Nothing in the text shows that a highlight has a note**, and sidebar cards and highlights aren't linked both ways. | both readers | You can't see at a glance where you've commented, and finding a note's passage takes a click-through. |
| A9 | 22 tests, none covering UI flows. There's no headless smoke test, even though CLAUDE.md documents how to run one (Xvfb + xdotool). | — | The refactor in Phase 0 needs a safety net first. |

### Benchmark: what the best tools do that Pereplyot doesn't

- **Sioyek**: hover over a citation, figure or equation reference to preview its target
  without losing your place; "smart jump" to the bibliography entry; portals (pin a figure
  next to the text that discusses it).
- **Zotero 7 reader**: area/image annotations for figures and tables; annotations as
  first-class items with tags; drag an annotation into a note and it carries a live link
  back to the spot.
- **LiquidText / MarginNote**: a workspace beside the document where excerpts are arranged,
  connected and commented on. Synthesis, not just collection.
- **Skim / PDF Expert**: notes shown in the margin; editable highlight extents; a "save
  with annotations" PDF anyone can open.
- **Readwise Reader / Foliate**: paginated EPUB, footnote popovers, read-aloud with sentence
  tracking, and a good typographic reading mode.

---

## 2. North-star user experience

**The reader has three postures, and the chrome follows the posture you're in.**

1. **Read**: just the page. Chrome fades in on pointer movement and the status bar is
   minimal. Good for long uninterrupted reading. (This is the house-style *Focus Mode*.)
2. **Study** (default once a document has annotations): the page plus the **Notes
   sidebar** on the right (it already exists; your highlight notes stay there). The colour
   palette and tools are visible. The selection popover is the main way to mark.
3. **Synthesise**: the page plus a **Notebook** pane, a plain-file outline you drag
   annotations into (from this document or any other) and write around. Every quote in it
   links back to the exact spot.

Interaction principles, held across every phase:

- **Never lose the reader's place.** Following a reference previews it; it doesn't jump.
  Any jump that does happen can be undone with Back/Forward.
- **Keyboard first, pointer friendly, stylus welcome.** Every action is in the Ctrl+K
  palette and has a key.
- **The file is the truth.** Annotations stay in plain sidecars, indexes are rebuildable
  caches, and nothing is locked into a database.
- **The output is writing.** Any annotation, group or notebook can become Typst, LaTeX or
  Markdown with citations in one step.

### Target reader layout (Study posture)

```
┌ ≡ ◧  Interpreting the New Testament — Harrington        ⌕  ◐  ⋯ ┐
├──────────┬──────────────────────────────────────┬──────────────┤
│ Contents │                                      │ ● p.42       │
│ Pages    │      page                            │ "the Q source│
│ Marks ●  │      ░░░░░░░░ (ochre highlight)  ────┤  hypothesis" │
│ Search   │                                      │  my note…    │
│          │      ▒▒▒▒▒▒ (blue)              ─────┤ ● p.42 #synoptic
│          │                                      │              │
├──────────┴──────────────────────────────────────┴──────────────┤
│ ‹ 42 (xlii) of 164 ›  ★   ▮▮▯▯ marks in scrollbar   Read Study Synth   v0.x │
└────────────────────────────────────────────────────────────────┘
```

---

## 3. Phases

Each phase lists its **UX**, **engineering**, and **done when** criteria. Phases 0 and 1
are foundations; the rest can be reordered to taste once those land.

### Phase 0 — Foundations: store, schema, safety net *(no visible change)*

**Engineering**

1. **Headless smoke-test harness first**, before refactoring anything. Under Xvfb, with
   isolated `XDG_*` and `dbus-run-session` using a minimal config without
   `standard_session_servicedirs` (see the Zarya gotcha in root CLAUDE.md): open a fixture
   PDF and EPUB, highlight via xdotool, close, and assert on the sidecar. Run it in CI as a
   separate job. Add a couple of fixture PDFs with odd crop boxes and `/Rotate`, since 0.9.0
   showed those break coordinate maths.
2. **`AnnotationStore`** in `fond-read-gtk`: owns the sidecar, undo/redo (as commands, not
   whole-sidecar snapshots), `SidecarSync` persistence, and a change signal
   (`Added/Updated/Removed(id)`). Every view subscribes instead of being rebuilt by hand. A
   plain `Rc<RefCell<Vec<Box<dyn Fn(&Change)>>>>` is enough; a GObject isn't needed. Both
   readers, the Annotations dialog, and later the Notes sidebar and notebook all use it.
3. **Split `pdf.rs`** into `pdf/session.rs` (document, page geometry, labels, store),
   `pdf/view_paged.rs`, `pdf/view_continuous.rs`, `pdf/input.rs` (drag, selection,
   context menu, keys), `pdf/chrome.rs` (header, status bar) and `pdf/sidebar.rs`. Do the
   same for `epub.rs`. The goal is to make Phases 1–3 possible, not to make the code
   prettier: aim for no function over ~300 lines.
4. **Forward-compatible schema in `fond-annot`** (a fond-core change, released *before*
   anything writes new fields):
   - `AnnotationKind` gets `#[serde(other)] Unknown`. Readers skip unknown kinds when
     drawing but **keep them on save**.
   - Unknown fields on `Annotation` round-trip through a `#[serde(flatten)] extra: Map`, so
     an older writer never drops a newer reader's data.
   - Bump `schema`, and document the policy: new fields must be additive, and every
     consumer (Kartoteka, Sputnik, Pereplyot) must update to the tolerant parser before the
     first writer of a new field ships. Add a test that a sidecar containing an unknown kind
     loads cleanly and is never quarantined.
5. **Performance budgets**, measured (2026-10-08) on synthetic documents with `tests/perf/run.sh`
   and recorded in `tests/perf/BUDGETS.md`. Findings that Phase 1 must fix: search blocks the
   main thread (331 ms on a 600-page book), a scanned page at 2× pixels takes 16–24 ms on the
   main thread, and a scan costs ~3.3× its file size in memory. Targets: no main-thread block
   over 16 ms, first search hit under 200 ms, memory under 300 MB + 1.5× the file size,
   launch to first page under 300 ms.

**Done when:** behaviour is unchanged, the smoke tests pass in CI, a sidecar with an
unknown kind round-trips, and no function in the reader is over ~300 lines.

### Phase 1 — Rendering that disappears

**UX**: text is razor-sharp at every zoom and on every screen. Pinch zoom is fluid. Dark
mode works on PDFs without turning photos into negatives. Nothing ever stutters.

**Engineering**

- **HiDPI**: render at `logical width × widget.scale_factor()`, and display through a
  `gdk::Texture` sized to logical pixels. Re-render when `notify::scale-factor` fires (a
  window moved between monitors).
- **Off-main-thread rasterisation**: one render worker thread that owns its own
  `PdfDocument` (PDFium isn't safely re-entrant, so use a single worker, not a pool), fed
  through a channel of `(page, scale, generation)` jobs. Results come back via
  `glib::MainContext::channel` or `spawn_future_local`. Drop stale jobs by generation.
  Keep an LRU texture cache keyed by `(page, scale bucket)`, and prefetch ±2 pages.
- **Tiles above about 2× zoom**, so zooming into a figure doesn't allocate a 9,000-pixel-wide
  page bitmap.
- **Vector annotation overlay**: stop blending marks into the RGBA buffer. Draw highlights,
  underlines, search hits and the selection in a `snapshot`/Cairo layer above the page
  texture, from page-space quads converted through `PageGeom`. Marks then get hover states,
  focus rings, and resize handles (Phase 3), and changing a mark no longer re-rasterises
  anything. Draw highlights with multiply blending so they look like ink on paper in both
  light and inverted modes.
- **Smart dark mode**: replace the plain RGB invert with an invert that keeps hue (invert
  lightness only), plus a "Sepia paper" option, matching the EPUB themes so both formats
  offer the same three looks.
- **Async search** on the render worker, reusing its open document. Stream results into the
  sidebar as they're found and show **match ticks in the scrollbar**. Do the same for
  annotation positions (one tick per mark, in its colour).
- Smooth zoom: while pinching, scale the existing texture and re-render once the gesture
  settles.

**Done when:** text is crisp on a 2× display, nothing in the reader blocks the main thread
for more than 16 ms (check with `GTK_DEBUG=interactive`'s frame timings), highlighting a
passage doesn't re-rasterise the page, and the Phase 0 budgets are met.

### Phase 1b — Reading mode: a PDF that reads like an EPUB

Decided 2026-10-08. Goal is **reading comfort**: switch any PDF to a clean, reflowed page you
can restyle (font, size, spacing, width, theme) exactly as you would an EPUB. Builds on the
v0.10.0 PDF Text view (`pdf_text.rs`), which becomes the base of this mode instead of an
accessibility side-door. **Read-aloud is explicitly out of scope** for now (see Phase 8).

**UX**

- A named **Reading mode** toggle in the status bar (house style: the label is the name,
  bold when on), with a key (`T`) and a palette entry. The page image and the reflowed text
  are two views of one document: switching keeps your position, selection, search term and
  marks.
- **The same typography panel as EPUB**, shared code and shared saved settings: font family,
  size, line spacing, column width, margins, justification, paragraph style (indent or
  space), and the Light / Sepia / Dark reading themes. Settings are global, but the mode
  itself is remembered per document.
- **Clean text**: running headers and footers, page numbers inside the text flow, and stray
  scan debris are removed from the reading column.
- **A two-sided margin** (Cal, 2026-10-08), the feature to make this mode stand out:
  - **Left margin: page numbers.** Wherever a source page ends, a quiet marker shows the
    *printed* page label ("214", "xiv") level with the first line of the new page, with a
    hairline across the column. It uses the PDF's `/PageLabels` or the manual override, and
    it is what you cite from. Click it to copy a citation for that page, set the numbering
    if the document has none, or hold to peek at the original page image.
  - **Right margin: footnotes and endnotes**, set *inside the text area* beside the line
    that cites them, as sidenotes (the layout Typst and Tufte-style books use). They are
    separate from the Notes sidebar, which keeps holding your own highlight notes. Design
    goals, working from the reference layout Cal supplied:
    - the note column is about a quarter of the text width, with a hairline or generous gap
      separating it from the body, and the note's top aligned to the citing line's baseline;
    - **ragged-right, not justified**: a narrow justified column produces the wide gaps and
      broken words visible in the reference ("Jef-ferson", "ci-tations"). Hyphenation is
      limited to long words, and the line height is a little tighter than the body;
    - smaller, a touch lighter, with a hanging superscript number that matches the marker
      in the text; hovering either one highlights the other, and clicking the marker
      focuses its note;
    - **long notes don't push the next note a long way from its anchor**: past about six
      lines a note collapses to a clamped preview with "more", and when stacking does push
      a note below its anchor, a short leader tick connects it back;
    - notes that cite the same source ("ibid.", repeated short forms) are shown as normal
      notes, not merged, since merging would change what the author wrote;
    - a note that can't be matched to a marker is still shown, at the top of the margin for
      its page and marked "unmatched", never dropped. Endnotes (a notes section at the back)
      are pulled in the same way, using the PDF's internal links from superscript to note
      where they exist.
  - **Narrow windows** collapse the margins (an `adw::Breakpoint`): page labels become small
    inline tags and footnotes move to the end of their paragraph, so nothing is lost on a
    small screen or in a split view.
  - Toggles for each side ("Page numbers", "Footnotes": margin / end of paragraph / hidden).
- **Your highlight notes stay in the Notes sidebar.** Highlights that carry a note get a
  small glyph in the text, and clicking a highlight (or its glyph) focuses its sidebar
  card. The footnote margin and the Notes sidebar never share space.
- **Headings are real headings** (from the outline and font-size evidence), styled
  consistently, and they feed the Contents sidebar.
- **Figures and tables stay in the flow** as inline image crops with their captions, so
  reflowing never silently drops content. A one-click "show original page" on any
  paragraph or figure jumps to the source page in the page view.
- **Marking works as it does today** (select, then 1–4 or the popover), and marks made here
  appear on the page and the other way round. Exports are unchanged: they still cite
  printed page labels, with page boundaries tracked invisibly through the text.
- **Scanned PDFs** (no text layer): Reading mode offers "Recognise text", runs OCR in the
  background page by page, shows the pages as they complete, and then behaves like any
  other PDF. The recognised text is also used for search and selection in the page view,
  so one OCR run makes the scan searchable without writing a new file.

**Engineering**

1. **A new `fond-doc` structured-text API** (a fond-core change, pinned per Phase 0's
   rules): instead of the plain string from `extract_text`, return per-page *blocks, lines
   and runs* with bounding boxes, font size, font name and weight, from PDFium's text
   page. Everything below depends on this; plain text loses exactly the information needed.
2. **Layout pipeline** (pure functions in `fond-read-gtk`, heavily unit-tested against
   fixture PDFs, run on the Phase 1 worker thread):
   - *Reading order*: XY-cut on block boxes to find columns, rather than trusting the
     PDF's internal order.
   - *Running furniture*: normalise each page's top and bottom lines (strip digits),
     count repeats across the document at similar positions, and drop the repeats.
     Alternate-page (recto/verso) headers are handled by comparing odd and even pages
     separately.
   - *Paragraphs*: from indentation and vertical gaps measured in the boxes, replacing
     today's line-length heuristic in `reflow()`. Hyphen rejoining stays, with a
     dictionary-free rule.
   - *Footnotes*: small-font blocks at the bottom of a page, matched to superscript
     markers in the body by number and, for hyperlinked PDFs, by link target. Each note
     records its anchor run. Anything unmatched is kept and shown as "unmatched" rather
     than dropped. Losing text silently is the worst failure here. Marker matching now
     decides placement, so it needs its own fixture set (numeric, symbol * † ‡, per-chapter
     restarts, notes spanning a page break).
   - *Page boundaries*: each page's first run is tagged so the left margin can place the
     printed label exactly where the page changes, including mid-paragraph.
   - *Headings*: font-size and weight above body, cross-checked with the outline.
   - *Figures*: image objects and non-text regions are rendered to cached crops under the
     cache dir, with the nearest "Figure/Table N" text as caption.
3. **Source map.** Every reflowed run keeps `(page, quad-set)` of its source. This replaces
   today's "find the quote on the page by searching" fallback (`pdf.rs` around the
   Text-view selection path), so a mark made in Reading mode gets exact quadpoints, and
   position sync and "show original" are exact.
4. **Margin column component** (`MarginColumn`): a widget that takes items with an anchor
   (a text iter) and a height, lays them out aligned to their anchor line with collision
   stacking, and re-lays out on scroll and resize. Left and right columns are two instances.
   Try `TextView::set_gutter` first (GTK 4 supports left/right gutters; verify they scroll
   with the text), and fall back to an overlay positioned from `iter_location` plus
   `buffer_to_window_coords` on scroll. Built generically (anchored items, collision stacking, leader
   ticks) so the left page-label column and the right footnote column are the same code.
5. **Renderer**: keep `gtk4::TextView` with text tags for the typography (it already handles
   selection, accessibility and marks), use `TextChildAnchor` for figures, Pango for
   hyphenation and justification, and a `libadwaita::Clamp` for column width. Load pages
   incrementally as `ReflowView` does now, and keep the page-to-offset index for sync.
   The typography settings object is shared with `epub.rs`, which already has theme and
   font choice, so both readers use one panel and one config.
6. **OCR**:
   - Bundle Tesseract + Leptonica and English `fast` data in the flatpak manifest;
     additional languages download on demand to `~/.local/share/pereplyot/tessdata/`
     after an explicit prompt. A document-language picker (default from the PDF's
     metadata, then the system locale) chooses which pack to use.
   - Run Tesseract as a **subprocess** on a worker (never in-process on the GTK thread),
     one page at a time, feeding it the Phase 1 render at ~300 dpi and reading back
     word boxes (TSV/hOCR).
   - Cache results per page under `~/.cache/pereplyot/ocr/<hash>/`; they're derived data,
     so it's safe to delete. Cancel cleanly when the document closes.
   - Feed the word boxes into the same structured-text API from step 1, so scans take
     the same layout pipeline as born-digital PDFs.
   - The existing "OCR a searchable copy" action (`ocrmypdf`) stays as an optional extra
     for people who want a file to share.
7. **Quality guardrails**: layout heuristics can't be perfect (tables, equations,
   marginalia, poor scans). The "show original" affordance is the safety valve, and the
   fixture suite must include a two-column paper, a footnoted monograph, a scan, and a
   table-heavy page.

**Done when:** a footnoted monograph and a two-column journal article both read cleanly in
Reading mode with no running headers or page numbers in the flow, printed page labels in
the left margin at the right lines, footnotes level with their markers on the right (and
unmatched ones visibly flagged), a scanned book becomes readable in the same mode after one OCR pass (and
searchable in the page view), you can change font, spacing, width and theme and the choice
sticks, a highlight made in Reading mode lands on the right words on the page and exports
with the correct printed page, and switching modes never loses your place.

### Phase 2 — Navigation without losing your place

**UX**

- **Hover previews** (Sioyek's best feature): hovering over an internal link (a footnote
  marker, "Figure 3", "(Smith 2019)", an equation number) shows a popover with a rendered
  crop of the destination. Click jumps; Back returns. For PDFs whose citations *aren't*
  hyperlinked, which is most humanities PDFs, try a **text-matched reference lookup**:
  detect `(Author Year)`, `[12]` or superscript patterns near the pointer, and search the
  document's last pages (the bibliography) for the best match.
- **Back / Forward history**, browser-style, for every jump: link, outline, search result,
  annotation, page entry. Alt+Left/Right. Today there's only Back, and only for links.
- **Split view of the same document**, vertical or horizontal: read chapter 3 while keeping
  the endnotes or a figure open in the other pane. Both panes share one `AnnotationStore`
  (this is where Phase 0 pays off).
- **Pin a figure** (a "portal"): pin any region as a small floating card that stays visible
  while you scroll, for the figure or table the next five pages keep referring to.
- **Outline tracks your position**: the current section is highlighted and the outline
  scrolls to it. Show a breadcrumb ("Ch. 3 › The Two-Source Hypothesis") in the status bar.
- **The reading position is drawn**: a subtle "you were here" marker when reopening, so
  resuming on a long page isn't a hunt.

**Engineering:** link targets come from PDFium's page links (already used by `link_at`).
Preview crops come from the Phase 1 render worker. The text-matched reference lookup is a
pure function over `PdfText`, so unit-test it hard against fixtures in several citation
styles.

**Done when:** you can read a footnoted monograph without once losing your place, and the
reference preview finds the bibliography entry in at least 3 of 4 fixtures that have no
hyperlinks.

### Phase 3 — Annotation depth

**UX**

- **Notes sidebar, made richer** (it stays on the right, as now): cards show the colour
  stripe, quote, tags and note; clicking a card scrolls to and flashes its highlight, and
  clicking a highlight focuses its card; a highlight with a note shows a small glyph in the
  text. The sidebar can be filtered to the current page or section as you read. (A
  second, in-text margin for annotation notes was dropped: footnotes own the in-text
  margin in Reading mode.)
- **Area capture**: drag a rectangle with the Area tool (`A`) to clip a figure, table or
  equation. It's stored as a page-space rectangle, and the PNG crop is cached under the data
  dir as a derived file, rebuildable from the PDF. Its text is taken from the text layer when
  there is one, or OCR'd on demand when there isn't. In exports it becomes an image with a
  caption and citation (Typst `#figure(image(...), caption: [...])`).
- **Editable marks**: drag handles at either end to extend or shrink a highlight (snapping
  to word boundaries through `select_text_range`). Change colour or kind from the mark's
  popover. Merge overlapping highlights.
- **Tags**: `#tag` typed in a note becomes a tag, and there's also a tag picker in the
  popover. Tags are stored as a field and parsed from the note text, so either way works.
  Recent tags are offered first.
- **Sticky notes on the page**: a freestanding note gets a position and an icon on the page,
  not just a sidebar entry.

**Schema** (additive, behind Phase 0's tolerant parser): `kind: area`;
`rect: [f64;4]`; `tags: Vec<String>`; `position` for sticky notes. Ship the fond-core release to Kartoteka and Sputnik first.

**Done when:** you can annotate a scientific paper's figures and equations and see every
note in the sidebar, and each highlight with a note shows its glyph in the text. Area clips export to Typst that compiles.

### Phase 4 — Interoperability: annotations in, annotations out

**UX**

- **Import on open**: if a PDF carries embedded highlights or notes (from Acrobat, Zotero,
  Preview or Okular), offer once: "This PDF has 37 annotations made in another app — bring
  them in?" They then become ordinary, editable Pereplyot annotations. The deterministic ids
  `Annotation::imported` already generates make re-import idempotent.
- **"Save a copy with annotations…"**: writes a standard-annotated PDF (highlight,
  underline, strike and notes as `/Popup`s) that anyone can open in Acrobat or Preview, for
  a supervisor, a co-author or a submission. It always writes a copy and never touches the
  original, since the original's content hash keys the sidecar.
- **Deep links**: register a `pereplyot://` URI handler plus an additive `--annotation=<id>`
  CLI flag, so a link in Zerkalo, Obsidian or a Typst PDF opens the exact passage.
  Exports gain these links. (Additive flags are safe; never change an existing flag's
  meaning.)
- **Hand-off to Kartoteka**: when a standalone PDF has a DOI (`fond_doc::find_doi`) or
  ISBN, offer "Add to Kartoteka" (launch it with the file), so the citation key then comes
  from the vault instead of being typed by hand.
- **Per-document Markdown file** with stable block ids (`^annot-id`) for Obsidian/Logseq
  users, kept in sync on save when the user opts in.

**Done when:** a PDF annotated in Zotero opens with its marks editable, and a copy saved
from Pereplyot shows every mark in Okular and Acrobat.

### Phase 5 — Synthesis: the Notebook

This is the differentiator. Readers collect, but they rarely help you *think across* what
you've read.

**UX**

- A **Notebook** pane (the Synthesise posture) beside the document: a plain outline you
  write in, into which you **drag annotations** from the reader, the Notes sidebar, the
  Annotations dialog, or the launcher's Notes tab. A dropped annotation becomes a live
  quote block showing the source, page and colour, and clicking it opens the source at that
  spot.
- Notebooks belong to a **Library shelf** (one per paper or chapter you're writing), or
  stand alone.
- **Connections**: link any two annotations ("this contradicts p. 12 of Smith"), even
  across documents. The sage "connection" colour finally gets a real job. Links show in
  both annotations' margin cards.
- **Export the notebook** to Typst, LaTeX or Markdown with every quote cited. Or "Open in
  Zerkalo", so the notebook becomes the first draft of the essay. This is where Pereplyot
  meets the rest of the suite.

**Engineering**: a notebook is a plain file (**Typst**, decided; Markdown is an export only) under `~/.local/share/pereplyot/notebooks/`. Quotes are stored as references
(`pereplyot://<hash>#<id>`) and rendered from the live sidecar, with the quote text cached
inline so the file still reads correctly on its own. Editing is a `TextView` with block
widgets (anchored child widgets for quote cards), not a WebView.

**Done when:** you can read three sources, drag a dozen annotations into a notebook,
connect two of them, write a paragraph around them, and export Typst that compiles with
correct citations.

### Phase 6 — EPUB for scholars

- **Printed page numbers from `page-list`** (EPUB 3 nav / NCX `pageList`): show "p. 214"
  in the status bar, store a page label on every EPUB annotation, and use it in exports.
  This makes EPUB quotes citable, which is the single biggest gap for students on
  publisher e-books.
- **Footnote popovers** for `epub:type="noteref"` and the common unmarked patterns
  (superscript links into a notes file), with the same behaviour as Phase 2's hover
  previews.
- **Paginated mode** using CSS multi-column pagination in the WebView, with two-column
  spreads on wide windows, alongside the existing scroll mode.
- **Robust anchors**: store a W3C Web Annotation–style `TextQuoteSelector` (exact plus
  prefix and suffix, which the schema already half-has in `snippet_prefix`/`suffix`) **and**
  a `TextPositionSelector`, and re-anchor with fuzzy matching when a book file changes.
  Fill in `snippet_prefix`/`suffix` on every new mark, PDF included (check: they're often
  `None` today).
- Parity with the PDF side: Notes sidebar, area capture of images, tags, and the notebook all
  work on EPUB.

### Phase 7 — Retrieval

- **One search across everything you've read**: the full text of every Library/History
  document plus every annotation, ranked, with facets for colour meaning, tag, shelf,
  document and date. Run it on a worker thread over a **rebuildable index** under
  `~/.cache/pereplyot/`. Options: reuse Kartoteka's `fond-index` (tantivy), which needs it
  lifted into fond-core without its `fond-bib` dependency; or a small tantivy index of
  Pereplyot's own. The app crate may use `fond-bib`, but `fond-read-gtk` must not.
- The **Notes tab becomes a proper annotation browser**: grouped by document or tag, with
  multi-select, bulk retag or recolour, and bulk export.
- **"Resurface"** (optional, opt-in): a few of your highlights from past reading on the
  launcher, spaced-repetition-lite, for exam revision.

### Phase 8 — Accessibility & ergonomics

- **Read aloud is deferred** (Cal, 2026-10-08: not ready to add it). The design notes
  stay here for when it's wanted: through Speech Dispatcher (`speech-dispatcher` is on every GNOME desktop;
  check whether the flatpak can reach it, which may need a `--talk-name`/socket grant): it
  highlights the current sentence on the page and in the Text view, and you can highlight
  while listening.
- **Caret browsing on the PDF page itself** (F7): arrow keys move a text caret and
  Shift+arrows select, so keyboard-only marking works on the real page, not only in the
  Text view.
- **The Ctrl+K command palette** (house style), which doubles as go-to-section,
  go-to-page-label and go-to-annotation.
- Reading postures (Read/Study/Synthesise) as status-bar name-as-label toggles, in house
  style.
- A pass over the high-contrast theme and colour-blind safety: the numbered swatches
  already help, so add an optional pattern (hatching) per colour meaning in the overlay.

### Phase 9 — Finish & ship

The CLAUDE.md fast-follows: the Welcome/What's New window with a `RELEASE_NAME`, a real app
icon, `capture-screenshots.sh` with an obviously-fictional seeded document and annotations,
EPUB cover thumbnails, CI publish secrets, and `release-preflight.sh` ported from Zerkalo.
Also refresh README screenshots (the current `pereplyot.png` still shows v0.4.0 chrome),
and update the App capability matrix in the root CLAUDE.md as each of these lands.

---

## 4. Cross-cutting rules for every phase

- **Cross-app interface**: CLI flags are additive only. Kartoteka and Sputnik must keep
  working against any Pereplyot release, and vice versa.
- **Schema**: nothing writes a new `fond-annot` field until every consumer has the
  tolerant parser (Phase 0.4). Bump the fond-core rev in all three apps, Pereplyot last.
- **Threading**: any disk, PDFium-heavy, OCR, index or network work goes off the main
  thread before it ships. Put this in the PR checklist explicitly, not just "keyring";
  Sputnik's 2026-09-15 lesson applies here too.
- **Persistence**: always through `fsutil`/`SidecarSync`. Derived files (crops, indexes,
  caches) live under the cache dir and are safe to delete.
- **Export must compile**: every new export shape (figures, notebook, links) gets a test
  that runs the real `typst compile` against a fixture `.bib`, as `export.rs` already does.
- **Docs**: CHANGELOG and README updated in the same session as each feature (Documentation
  policy).

## 5. Decisions (Cal, 2026-10-08)

1. **Notebook format: Typst.** Markdown is an export only.
2. **Search index: shared.** Lift `fond-index` into fond-core (without its `fond-bib`
   dependency) so Kartoteka and Pereplyot use one implementation.
3. **No pen/ink input.** Dropped from Phase 3 and from the schema.
4. **No AI features.** Pereplyot stays local-first.
5. **Minimum GTK/libadwaita:** no bump unless Phase 1's tiling needs GTK 4.14; if it does,
   bump `v4_10` in both crates together.

## 6. Suggested release mapping

| Release | Phases | Theme |
|---|---|---|
| 0.11 | 0 + 1 | Foundations and crisp, never-blocking rendering |
| 0.12 | 1b | Reading mode, shared typography, OCR for scans |
| 0.13 | 2 | Navigation without losing your place |
| 0.14 | 3 | Richer Notes sidebar, area capture, editable marks, tags |
| 0.15 | 4 | Import, annotated-PDF export, deep links |
| 0.16 | 5 | The Notebook |
| 0.17 | 6 | EPUB for scholars |
| 0.18 | 7 + 8 | Retrieval, caret browsing, command palette |
| 1.0 | 9 | Finished and shipped |
