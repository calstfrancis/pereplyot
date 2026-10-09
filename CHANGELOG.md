# Changelog

## [Unreleased]

- **Zooming is smooth and stays where you are.** Pinch, Ctrl+wheel and the zoom buttons used to
  wait 150 ms, then throw the whole page layout away and rebuild it, which flashed blank and
  put you back at the top of the page. Pages now resize as the zoom changes, at most once a
  frame, keeping the point in the middle of the window fixed; each page keeps showing its last
  picture, stretched, until the sharp one for the new size arrives a moment after you stop.
- **Long documents scroll with only the pages you can see.** The continuous view used to build a
  widget for every page of the book up front (600 for a long one) and keep them all. It now
  builds widgets only for the pages near the viewport, with blank spacers standing in for the
  rest, so opening is cheaper and a very long document costs no more to scroll than a short
  one. Scroll position, page tracking and jumping to a page work as before.
- **Thumbnails are drawn by the render thread.** The sidebar's thumbnails used to be rasterised
  one per idle tick on the window thread, so opening the Thumbnails tab on a long book made
  scrolling stutter while they filled in. They now queue behind page renders on the render
  thread, nearest the current page first, and never delay the page you are reading.
- **Dark mode that keeps pictures looking right, plus Sepia.** The night button used to invert
  every colour, which turned photographs and coloured figures into negatives and left
  highlights the wrong colour. It now cycles Normal, Dark and Sepia: Dark flips lightness
  but keeps hue (and stops short of pure black and white to ease glare), Sepia warms the
  page like old paper. Highlights keep their own colours in every mode.
- **Scrollbar ticks in continuous view.** A thin strip along the scrollbar edge marks the pages
  that hold your notes and highlights (in their colours) and every search match, with the
  current match standing out, so the shape of a whole book is visible at a glance.
- **Highlights are drawn over the page instead of into it.** Adding, undoing or recolouring a
  highlight, moving to the next search match, and extending a selection used to re-render
  the whole page with the marks baked into its pixels. The marks are now a separate layer of
  rectangles over the page picture, so those changes repaint in a frame and the page itself is
  never re-rendered or evicted from the texture cache. The facing page in Two-page view gets
  its marks too.
- **Pages are drawn on their own thread, at the screen's real resolution.** Page rendering used
  to run on the same thread as the window, so on a scanned book zooming or paging could
  freeze the whole interface for a frame or more, and pages were drawn at logical size, so
  text looked soft on a 2× or 4K screen. Pages are now rasterised by a background thread at
  the display's scale factor (and redrawn if the window moves to a screen with a different
  one), finished pages are kept in a texture cache so scrolling back is instant, and the
  page you are on is always drawn first. Measured on a 600-page scan: no main-thread stall
  while paging or zooming (it was 24 ms per page at 2×).
- **Large PDFs open faster and use far less memory.** Opening used to parse the whole file
  four times and hold two copies of it in memory, and read every page's label and size on the
  window thread. The file is now opened once and read on demand, and page sizes and printed
  page labels are read by a background thread (the scroll view is laid out assuming pages
  match the first one, and rebuilt only if some page really differs). A 125 MB scan now
  shows its first page in 160 ms (was 284) and sits at 340 MB (was 640).
- **Fixed: a PDF without an outline drew every page as a thumbnail behind the hidden
  sidebar.** Scanned books have no outline, so the Thumbnails tab was "open" and rendered all
  600 pages on the window thread for nothing — seconds of work and ~150 MB. Thumbnails are
  now drawn when the sidebar is shown.
- **Search no longer freezes the window.** Searching a PDF scanned every page on the same thread
  as the window — 330 ms of frozen interface on a 600-page book, growing with length. The
  search now runs on its own thread: the first match is shown within a few milliseconds
  (the counter reads "1 of 12+" while the rest are found), a new search cancels the old one,
  and closing the reader stops it.
- **Fixed: a crash waiting to happen with any second thread — PDFium was never thread-safe.**
  The PDF library's own "thread safe" switch only guards start-up and shut-down, not each
  call. Every PDFium call in Pereplyot now goes through one lock (`pdfium_lock.rs`, generated
  from the library's source by `tools/gen_pdfium_lock.py`), so the render thread and the
  window thread can both use it. Without it the first version of the render thread corrupted
  the heap on 11 of 12 launches.
- **Fixed: a false "This PDF has no text layer" warning, and missing lines in the Text view,
  on PDFs with a rotated page or an offset crop box.** The reader asked PDFium for the text
  inside a rectangle anchored at the page's origin, which comes back empty for a rotated
  page and drops the top lines of a cropped one. Text is now read character by character.
- **Fixed: pressing 1–4 with the selection popover open did nothing** — the popover took the
  keystroke before the reader's shortcut saw it, although the hint line says "or press 1–4".
  The keys now choose the colour from inside the popover too, in both readers.
- **Fixed: the page jumped away from the text you had just marked.** Closing the selection
  popover returned keyboard focus to the page, and GTK scrolled the window to reveal it — on
  any page taller than the window, every highlight scrolled the view down. Focus now returns
  without moving the scroll position.
- **A failed save no longer leaves a ghost annotation.** Adding, editing or deleting an
  annotation (and undo/redo) is rolled back if the file can't be written, so what you see
  matches what is on disk.
- Internal: `pdf.rs` and `epub.rs` are now `pdf/` and `epub/` modules, and the two reader-opening functions (once ~2,370 and ~1,500 lines) are ~290 and ~330 lines of builders and wiring; every annotation change in both readers now goes through one store
  (`annotation_store.rs`) that saves it, records undo/redo as small steps instead of copying
  the whole annotation list, and tells the page, the Notes sidebar and the undo buttons to
  update — replacing about twenty hand-written snapshot/save/redraw sequences.
- Internal: `PEREPLYOT_PERF=1` prints timed trace lines (open, page renders, search) to stderr, and `tests/perf/` generates large synthetic PDFs and measures them against written budgets.
- Internal: a headless smoke test (`tests/smoke/run.sh`, run in CI) drives the real app under
  Xvfb through open → select → mark for PDF (plain, cropped, rotated) and EPUB, and checks
  the saved annotation, that the highlight is drawn, and that Ctrl+Z / Ctrl+Shift+Z undo and redo it.
- **Fixed: a highlight dragged along a line no longer loses its trailing full stop, comma or
  quote mark** (it was kept only if the pointer happened to cross the glyph's ink), so quotes
  saved and exported by Typst/LaTeX/Markdown are complete.
- **Fixed: dragging over one line of an upright page that carries `/Rotate 90` selected the
  whole page.** Selection now follows the page as it is displayed (landscape scans stored on
  a portrait sheet are common).
- **Annotation files from a newer version are no longer set aside as damaged.** An
  annotation kind or field this version doesn't know is kept untouched through a load and
  save (fond-core now tolerates both).

## [0.10.0] "Fair Copy" — 2026-10-06

- **Typst, LaTeX and Markdown export, grouped by what your colours mean.** The export dialog
  (Typst is the default) lays quotes out under "Key idea / thesis", "Evidence / support",
  "Connection / implication" and "Problem / question" headings — or in document order — and
  cites them against a bibliography key: `#quote(block: true, attribution: [@key[p. 42]])` in
  Typst, `\autocite[p.~42]{key}` in LaTeX, `[@key, p. 42]` in Markdown. Launched from
  Kartoteka the key is the vault's citation key; otherwise you type it once and it's
  remembered for that document. Printed page labels are used for the page. "Copy with
  citation" in the selection popover does the same for a single quote.
- **Select first, then mark.** The PDF reader now starts in text-selection mode; releasing a
  selection offers the four colours, underline, strike-out, a note, copy, and copy with
  citation. The EPUB reader gets the same popover. Keys `1`–`4` mark the current selection
  (or switch the paint colour when nothing is selected). A note made from a selection now
  stores the quote as the quote instead of pasting it into the note text.
- **Library shelves.** Create named shelves, move documents between them from the card's
  right-click (or Menu key) menu, rename or delete a shelf (its documents are kept), sort by
  recently added or title, and resize the cards with a slider. Cards show reading progress.
- **Notes tab:** search every note and highlight across all documents, or show only one
  colour's meaning (all your "Problem / question" highlights, say); activating a result opens
  the document at that spot.
- **PDF Text view** for screen readers, low vision and keyboard-only reading: the document's
  text reflowed into a real text widget you can resize (Ctrl+plus/minus), select with
  Shift+arrows, and mark with `1`–`4`. Marks made there also appear on the page.
- **PDF links work** — footnotes, cross-references and web links — with a Back button
  (Alt+Left) to return to where you were.
- **Scanned PDFs:** a warning when a PDF has no text layer, and, if `ocrmypdf` is installed,
  a one-click OCR that writes a searchable copy next to the original.
- **Notes follow a changed file.** If a file is re-saved or OCR'd, Pereplyot offers to bring
  its annotations, bookmarks and reading position over to the new version.
- **Safer saving.** Annotations, progress, library and settings are written atomically with a
  backup; a damaged file is set aside and the backup restored instead of being silently
  replaced by an empty one; annotations added by another window or app are merged rather than
  overwritten. Reading position is saved every few seconds, and the EPUB reader saves it
  before quitting.
- **Faster, lighter PDF reading.** The document is parsed once; continuous mode keeps only
  nearby pages in memory; undo, redo and invert-colours no longer redraw the whole book;
  Library thumbnails are cached.
- **Accessibility:** icon-only buttons have names; colour swatches carry their number; page
  keys no longer steal Space and the arrow keys from focused buttons; sidebar entries and EPUB
  search results are reachable by keyboard; Ctrl+plus/minus/0, Ctrl+scroll and pinch zoom;
  Ctrl+F searches a PDF.
- **Fixed:** Ctrl+Shift+Z never redid; messages ("Highlight added") appeared in the hidden
  launcher instead of the reader; EPUB undo stayed disabled after marking; closing the
  selection popover dropped keyboard focus into the page-number box; the reader now has the
  app menu (theme, highlight labels, about) even when launched by Kartoteka or Sputnik.
- EPUB: book scripts no longer run, external links open in your browser, and an interrupted
  first open no longer leaves a half-extracted cache.

## [0.9.0] "True Register" — 2026-10-03

- **Fixed: PDF text selection and highlighting missing the text under the pointer.** On many
  publisher and scanned PDFs the page area doesn't start at the corner of the page's
  coordinate space, and some pages carry their own rotation. The reader assumed neither,
  so a drag could select a different line than the one you dragged over, draw the
  highlight somewhere else, or land on blank margin and select nothing at all. The drag
  preview, the saved highlight, right-click hit-testing, search matches, and highlights
  imported from the PDF itself now all line up with the text on these pages too.
- **Copied and saved text keeps its spaces.** Many PDFs (LaTeX and Typst output among them)
  place words without storing space characters, and selections from them came out as one
  run-together word. Text copied from a selection, and the quote saved with a new
  highlight, now keeps the word breaks.
- **Right-click to copy text (PDF):** "Copy selected text" after a Select-text drag, and
  "Copy highlighted text" (or underlined / struck-out / noted) on any existing mark.
- **Keyboard page navigation works wherever focus is in the reader window**, not only
  inside the page area: after clicking a toolbar button, the tab bar, or anywhere in the
  window. Menus and drop-downs still get their own arrow keys, and typing in the page
  number, search, or a note is unaffected. The numeric keypad's arrow, Page Up/Down, Home
  and End keys work too.

## [0.8.0] "Open Clasp" — 2026-09-29

- **New launch options for Kartoteka and Sputnik:** `--title=<title>` shows the calling app's
  own name for a document (e.g. Kartoteka's bibliographic title) instead of the file's
  metadata title, and `--annotations` opens straight to the Annotations dialog instead of
  the reader (Kartoteka's "Annotations…" now uses this rather than its own built-in copy
  of the dialog). With these, neither app builds in any part of Pereplyot any more — they
  always get whatever version of Pereplyot is installed.
- **Unknown command-line options are now ignored with a warning** instead of refusing to
  open the file, so a newer Kartoteka or Sputnik can't break an older Pereplyot.
- **Fixed: Pereplyot kept running invisibly after its reader was closed**, whenever it had
  been launched by Kartoteka, Sputnik, or a file manager's "Open With". The hidden start
  window was meant to close itself, but GTK ignores a close request on a window that was
  never shown — it's now destroyed instead.

## [0.7.0] "Ochre Thread" — 2026-09-28

- **Highlighting now uses a four-colour reading scheme, always visible in the reader's
  toolbar** instead of a colour drop-down: warm ochre (key idea / thesis — *what is the
  author saying?*), dusty blue (evidence / support — *what supports it?*), sage (connection
  / implication — *why does it matter?*), and terracotta (problem / question — *what needs
  scrutiny?*). Click a colour to highlight in it, or the "A" button beside them to switch
  back to plain text selection (PDF). Hovering a colour shows its meaning and what to use it
  for. The Highlight / Underline / Strikeout choice stays as a small drop-down next to the
  palette.
- **The colour labels are editable** — "Highlight labels…" in the menu. Renamed labels show
  in the toolbar tooltips and next to annotations in the Notes and Annotations lists.
- **Two-page and Continuous are now icon toggles** in the PDF toolbar, freeing header space.
- Highlights made before this keep the colours they were drawn in.
- Fixed a formatting-check failure that had left CI red since 0.6.0.

## [0.6.0] "Wide Margin" — 2026-09-24

- **Notes and highlights, reworked to match how other readers (Kindle, Apple Books, Foliate)
  handle them.** Margin notes were limited to a single line (a plain `Entry`, silently eating
  anything past the first Enter) everywhere a note could be edited — the right-click menu on
  a highlight, both readers' Notes sidebars, and the Annotations dialog. All four now use a
  small multi-line editor that still saves automatically when it loses focus, with no new
  button to press.
- **Every annotation list now shows its highlight color** as a small swatch next to the page/
  chapter and kind (the Notes sidebars, the right-click menu, and the Annotations dialog) —
  previously only the on-page highlight itself showed color; the lists were plain text.
- **The Annotations dialog now shows the quoted passage**, not just the location/kind and
  note — it already rendered this way in the on-page Notes sidebars and in the Markdown
  export, but the dedicated dialog had left it out.
- **The Annotations dialog gained a search field**, filtering the list by the highlighted
  text, the note, and the page/chapter as you type.

## [0.5.0] "Turned Leaf" — 2026-09-23

- **Page thumbnails moved into the Contents sidebar**, as a second tab alongside Outline —
  a linked "Outline" / "Thumbnails" switcher above the sidebar's list, sharing the same
  panel Outline already used instead of a separate popup window. Thumbnails still rasterize
  lazily (one page per idle tick, nearest-to-current-page first) and only once the tab is
  actually shown. A PDF with no outline now opens the sidebar to Thumbnails by default
  instead of disabling the sidebar toggle entirely, since thumbnails are always available
  even when there's nothing to show in Outline.
- **Fixed: reopening a document didn't actually resume at the last-read page.** The page
  counter correctly showed the saved page (e.g. "6 of 10"), but the visible content was
  always page 1 — continuous scroll mode (the default) tried to scroll to the saved page in
  the same call that built the page list, before GTK had laid out the new content, so the
  scroll silently clamped back to the top. Fixed by deferring that scroll to the next idle
  cycle, after layout has actually happened.

## [0.4.1] "Slim Gutter" — 2026-09-23

- **Reader chrome consolidated to reclaim reading space.** Both readers used to show two
  header rows (the shared reader-host window's own header, plus a second `HeaderBar` nested
  inside each tab for its sidebar/undo-redo/mode/etc. controls) and, for the PDF reader,
  three separate bottom rows (its own page-nav/zoom bar, a permanently-visible search bar
  above the page, and the host window's own footer bar below everything). All of that is now
  one header row and one status-bar row per tab: each reader hands its controls to the host
  header instead of packing its own (`fond-read-gtk`'s `reader_host::set_tab_header`, backing
  a swap keyed on which tab is selected), and the PDF reader's search bar and the host's
  footer widget both moved into its bottom status bar alongside page nav/zoom.
- **Fixed: the Contents sidebar toggle's icon could render as a generic "missing icon"
  glyph** in icon themes that don't ship `sidebar-show-symbolic` under that exact name.
  Now resolved through a themed-icon fallback chain (`fond-read-gtk::set_icon_with_fallback`)
  instead of a single hardcoded name.

## [0.4.0] "Shared Binding" — 2026-09-22

- **New CLI modes**, for Kartoteka and Sputnik to launch this binary instead of embedding
  the reader crate in-process (`fond-read-gtk` pinned copies were going stale — see the
  v0.3.0 entry below's own context): `pereplyot --vault=<root> --key=<key> <file>` routes
  annotations/progress/page-numbering straight into that vault's `notes/<key>.md` /
  `annots/<key>.json`, exactly like Kartoteka's own `KartotekaReaderHost` does today, via a
  new `VaultReaderHost` built on the `fond-bib` crate Pereplyot already depends on.
  `pereplyot --annotations-file=<path> [--progress-file=<path>] <file>` does the plain-file
  equivalent for Sputnik's non-vault-linked course materials. A bare `pereplyot <file>` is
  unchanged. Required switching `main.rs` from `HANDLES_OPEN` to `HANDLES_COMMAND_LINE` with
  its own small argv parser.
- **Fixed (`fond-read-gtk`): closing a reader via its window's own close button never saved
  reading progress or unregistered the open reader** — only an explicit tab-close (its own
  close button, or a keyboard shortcut) ever fired `TabView`'s `close-page` signal, which is
  the only thing `on_tab_closed`'s hooks were wired to. A stale doc comment claimed the
  window-close path already cascaded into it; it didn't. Found verifying the vault handoff
  above actually round-trips resume position, not just annotations. Fixes it for every app
  embedding this crate, not just Pereplyot.
- **`--filesystem=home`** replaces Pereplyot's previous narrow
  `~/.local/share/pereplyot:create` sandbox grant — a vault/material store the CLI modes
  above write into can live anywhere under the user's home, the same reason Kartoteka's and
  Sputnik's own manifests already grant this.

## [0.3.0] "Folded Corner" — 2026-09-22

- **Fixed: the Contents/outline sidebar toggle was completely absent, not just disabled,**
  for any PDF with no embedded bookmarks or EPUB with no table of contents — which is most
  of them. There was no way to tell the feature existed at all for such a document. The
  toggle now always shows in the reader's headerbar; it's greyed out with a "This PDF/EPUB
  has no table of contents" tooltip when the document genuinely has none, and clickable as
  before when it does. Shared fix in `fond-read-gtk`, so it applies to Kartoteka's and
  Sputnik's embedded readers too.
- **Fullscreen and maximize buttons**, plus an F11 shortcut for fullscreen, added to the
  launcher window and to the shared reader window's headerbar.
- **The reader window now shows its own version/changelog button**, bottom right of a new
  status bar, matching the launcher window's — previously only the launcher had one, so
  a document opened straight into its own window (no launcher visible) had no way to check
  the version or read the changelog. `fond-read-gtk` gained a `reader_host::set_host_footer`
  hook so only Pereplyot opts into this (Kartoteka and Sputnik already show their own
  version/changelog on their own main window).
- **Bookmarks.** A star toggle beside page/chapter navigation marks the current page (PDF)
  or chapter (EPUB) as a bookmark — a lightweight "come back to this" marker, distinct from
  a highlight or note. Bookmarks show in the Notes sidebar above your annotations, and the
  'B' key toggles the current page/chapter's bookmark from anywhere.
- **PDF: page navigation via Up/Down and Space/Backspace**, matching the existing
  Left/Right/Page Up/Page Down behavior — Space/Backspace follow the common reader
  convention (Preview, Acrobat).
- **PDF: click-to-turn page zones.** Clicking (without dragging) in the leftmost or
  rightmost 20% of the page turns to the previous/next page, in both paged and continuous
  view.
- **PDF: rotate page**, a view-only 90°-at-a-time rotation for sideways-scanned documents —
  single-page mode only (Continuous/Two-page reset it and disable the control, since
  neither one's layout accounts for a rotated page).
- **PDF: invert colours**, a night-reading mode for scanned/white-background pages, which
  don't otherwise respond to the app's own Light/Dark theme since they're rendered pixels,
  not themed UI.
- **PDF: a reading-progress percentage** next to the page count, and **a page-thumbnail
  grid** ("Page thumbnails…" in the headerbar) for jumping around a long document visually.
- **EPUB: a reading theme (Light/Sepia/Dark) and font-family choice (Default/Serif/
  Sans-serif)**, independent of the app's own theme — a WebView's page content doesn't
  inherit `adw::StyleManager`, and reading typography is a preference people often want
  separate from their OS theme (Sepia being the obvious example neither Light nor Dark
  covers).
- **EPUB: a reading-progress percentage** next to the chapter count.
- **Export notes & highlights to Markdown**, for both PDF and EPUB — bookmarks and every
  highlight/underline/strikeout/note, in document order, to a file you choose.

## [0.2.0] "Shared Shelf" — 2026-09-15

- **History is now shared across every app that embeds the reader.** Opening a PDF or EPUB
  from Kartoteka or Sputnik now shows up in Pereplyot's History shelf too, not just
  documents opened through Pereplyot itself — `fond_read_gtk::history` is a new small
  module in the shared reader crate that any host can call, writing to one real path under
  `~/.local/share/pereplyot/` that Kartoteka and Sputnik (which already hold broad home
  access, the same way they already share a Kartoteka library between their own sandboxes)
  can reach directly, and that Pereplyot's own otherwise-minimal sandbox now grants a
  narrow, single-directory exception for. One known gap: clicking a history entry that
  another app recorded can still fail if Pereplyot was never granted access to that
  specific file — it'll show a "couldn't read file" toast rather than opening.
- **Fixed: Pereplyot could fail to fully close** when opened purely to view one file (from
  Kartoteka, Sputnik, a file manager's "Open With," or anything else handing it a file
  argument on a fresh launch). The empty Library/History launcher window was being shown
  right alongside the reader in that case — invisible to the user most of the time, but
  still the one window keeping the app alive, so closing the reader you actually came for
  didn't quit the app. The launcher no longer appears on that first open, and now closes
  itself (which lets the app quit normally) once nothing is left open to read.

## [0.1.1] "True Margin" — 2026-09-15

Fixes found by actually using v0.1.0:

- **The PDF reader showed "Reader" twice** — the shared reader window's own headerbar and
  the PDF reader's own inner headerbar both fell back to the window's title, which is now
  a fixed "Reader" shared across every open tab rather than per-document the way it used to
  be. The inner header now shows the actual document title instead of falling back.
- **Keyboard page/chapter navigation now actually works.** It was wired up in 0.1.0 but
  never fired in practice — a focused button, dropdown, or (for EPUB) the page content
  itself could consume Left/Right/Home/End before the shortcut ever saw them. Fixed by
  intercepting those keys ahead of that, while still leaving Left/Right/Home/End alone
  when the page-number or search field has focus, so typing there still moves the text
  cursor normally.
- **The Notes/annotations sidebar toggle moved to the right** of the header, next to
  Contents/outline on the left — it had been sitting on the left in both readers despite
  the code's own comment claiming otherwise.
- **Hamburger menu and status bar now match the rest of the suite**: a hand-built popover
  (Open, Theme, About) instead of a plain menu, and the usual bottom-right `v{VERSION}`
  button that opens the changelog.

## [0.1.0] "Quiet Spine" — 2026-09-15

First cut of Pereplyot as a standalone app: `fond-read-gtk` (the shared GTK4/libadwaita
PDF/EPUB reader already used by Kartoteka and Sputnik) extracted into its own repository,
wrapped in a small launcher — Open, drag-and-drop, a System/Light/Dark theme toggle, and an
in-app changelog viewer. Annotations and reading progress are stored per document under
`~/.local/share/pereplyot/`, keyed by content hash, so a document's sidecar stays portable
between Pereplyot and the two embedding apps.

Extracted from `github.com/calstfrancis/kartoteka` — see that repo's
`docs/READER-EXTRACTION.md` for the boundary survey the crate itself came from.

**Reader (`fond-read-gtk`, shared with Kartoteka and Sputnik once they bump their pin):**
- PDF: zoom-to-fit-width and zoom-to-fit-page, next to the existing zoom in/out.
- PDF and EPUB: keyboard page/chapter navigation — Left/Right (and, for PDF, Page Up/Page
  Down, Home/End).
- PDF: zoom changes (in/out/fit) are now debounced (~150ms), and no longer eagerly
  re-render the entire document's continuous-scroll view when continuous mode isn't the
  visible one — that rebuild is deferred until the next time it's actually switched to.

**App:** replaced the single "Recent" list with two shelves — **Library** (a cover-grid of
documents you've intentionally added; PDFs get a real page-1 thumbnail, EPUBs a placeholder
icon for now — see the code's own note on why) and **History** (every document ever opened,
auto-populated as before, now with an "Add to Library" action per row).
