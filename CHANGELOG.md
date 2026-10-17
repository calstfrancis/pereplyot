# Changelog

## [0.20.1] "Clean Gutter" — 2026-10-17 — Selection that respects columns
- **Selecting text on a multi-column PDF page follows the columns.** Dragging from the bottom of
  one column into the next used to select a block straight across the gutter, mixing the two
  columns' lines. It now takes the rest of the first column and then the top of the next, in
  reading order, and clipping a rectangle that spans columns gives each column its own text and
  highlight. Single-column pages are unchanged.

## [0.20.0] "Fair Copy" — 2026-10-16 — Scans you can select, selection that holds
- **Text selection in an EPUB no longer breaks off.** Dragging across a passage could stop after one
  letter or one line; the whole drag is selected again. Reading mode on a PDF had the same fault
  (a drag selected a single word) and is fixed too.
- **Scanned PDFs are recognised automatically, and their text is selectable in the normal page
  view.** Opening a scan starts recognising it in the background at once, the page in view first and
  then outward from it, so the words on the page being read are ready before anyone reaches for
  them. Selecting, copying, highlighting, underlining and clipping areas work on recognised text,
  and results are kept for next time. Reading mode recognises on its own too. Before this the first
  page of a scan counted as content, so the offer to recognise never appeared and a scan stayed a
  stack of pictures with nothing to select. Two pages are recognised at once without repeating each
  other's work, and language data added under `~/.local/share/pereplyot/tessdata` no longer gets
  rebuilt under a recognition in progress.
- **"Add to Library" and "Add to Kartoteka…" are in the More menu** (⋮) of both readers, not just
  Ctrl+K, and appear only when they apply. Kartoteka now opens an "Add this file" dialog for the
  file (needs a Kartoteka with that dialog) instead of adding it unseen.

## [0.19.0] "Kettle Stitch" — 2026-10-14 — Notes you can't lose
- **Changes made elsewhere are no longer lost.** If another window or app (Kartoteka, Sputnik, a
  sync client) added an annotation to a document's notes while it was open here, the next save
  kept it but the one after that silently dropped it. Saves now merge with what is on disk by
  annotation id, so additions, edits and deletions made elsewhere are all kept, and they show up in
  the open document straight away instead of after reopening it.
- **Earlier versions of notes** (menu → *Earlier versions of notes…*). A copy of a document's notes
  is kept when you start changing them, then at most once an hour, and before every bulk edit in
  the Notes tab — the newest 20 plus one a day for 90 days. Restore any of them; the notes it
  replaces are kept as a version too.
- **Backup** (menu → *Backup…*). Choose a folder (a synced one, or a backup drive) and Pereplyot
  copies your notes, bookmarks, reading places, notebooks, Library and earlier versions there once
  a day as plain files, keeping the last 14. *Restore from a backup…* brings back only what is
  missing here and never removes or overwrites anything.
- Bookmarks, pins and reading places are protected the same way notes are: a damaged file is set
  aside and the previous copy used, instead of being silently reset to empty.
- A notes file damaged while its document is open is set aside before the next save, rather than
  overwritten.

## [0.18.0] "Loose Leaf" — 2026-10-14 — Text extraction for other apps
- **`--extract-text <file>`** prints a PDF's or EPUB's text and exits without opening a
  window (`-` reads the document from stdin), also installed as the
  `pereplyot-extract-text` command. Sputnik uses it to read PDF syllabi.

## [0.17.2] "Mended Spine" — 2026-10-12
- **A password-protected PDF keeps its outline and printed page numbers.** The unlocked copy
  Pereplyot reads used to lose both; they are now carried over from the original.
- **The unlocked copy is removed when you close the tab**, instead of staying in the cache for a
  day. Anything left behind by a crash is still swept after a day.
- **Dialogs follow a tab that moved.** After dragging a tab into its own window, the dialogs it
  opens (note, export, page numbering, command palette) now belong to the window the tab is in,
  not the one it was opened in.
- **Reading mode no longer widens the window.** Its typography button took the bar past the 1000 px
  default width; it now replaces the Rotate button, which does nothing in Reading mode.
- `capture-screenshots.sh` copies to the website only where the site already shows Pereplyot,
  and at the path the site really uses.

## [0.17.1] "Steady Page" — 2026-10-12
- **Reading mode no longer freezes a long book.** On a 600-page book the window froze for up to
  0.8 s while the text loaded and again at the first click or key press. The text view was wrapped
  in a way that made GTK lay out the whole book at every redraw, and the margin column re-measured
  every page label each time; both are fixed. A far jump forward over text not yet laid out is
  smoother but still takes a couple of seconds on a very long book.
- **The page counter and chapter follow your scrolling in Reading mode.** They stayed on page 1
  (and "Chapter 1") however far you scrolled.
- The README screenshots are redrawn for the single tabbed window, and `capture-screenshots.sh`
  works with it again.

## [0.17.0] "Turned Key" — 2026-10-12
- **Password-protected PDFs open.** Pereplyot asks for the password (and again if it was wrong),
  then reads an unlocked copy it keeps in its cache for a day; your file is not changed. Notes
  stay under the original file's identity. The copy carries the pages but not the outline or the
  printed page numbers of the original.
- **Dragging a tab out of the tab bar opens it in its own window.** It used to snap back with an
  Adwaita critical in the log. A tab now always knows which window it is in, so Back/pop-out/
  re-opening a document that was dragged out still reaches the right window; the pinned Library
  tab stays put.

## [0.16.3] "Quiet Hands" — 2026-10-12
- **Markdown notes are written in the background**, newest version of each document only, so an
  edit never waits for the disk (or a synced folder) to save; and when a document's title changes
  the notes file under the old title is removed instead of being left behind.

## [0.16.2] "Closed Door" — 2026-10-12
- **A crafted link could crash Pereplyot.** Pereplyot opens `pereplyot://` links, and a link
  containing a `%` followed by a non-ASCII character made the link reader panic — closing the app
  from a web page or a document. The same slip was in the EPUB link reader, and a notebook line with a
  setting that had no colon could crash the notebook loader. All three are fixed, and the parsers
  (links, EPUB markup, notebooks, search, anchors, annotation files, page layout) are now fuzzed
  with tens of thousands of hostile inputs.

## [0.16.1] "Light Load" — 2026-10-12
- **Big EPUBs unpack in the background.** A book that has to be unpacked first no longer freezes the
  window while it is; it says it is unpacking, and a second click does not start a second reader.
- **Notes sidebars build 300 cards at a time** with a "Show more" row, so a book with thousands of
  marks opens at once (30,000 annotations: about 21 s down to under 1 s in a debug build).

## [0.16.0] "Narrow Margin" — 2026-10-12
- **The EPUB reader fits on a laptop screen.** It refused to be narrower than about 1,320 px;
  text size, export and "open in a new window" now live in a ⋮ menu (as in the PDF reader) and a long
  chapter title shortens instead of widening the window, so it now needs about 1,050 px.
- **Add to Library** is a command in each reader's Ctrl+K palette, so a book no longer has to be
  found in History first.
- **Settings survive a bad value.** A wrong value for one setting (in `config.json` or the
  typography file) now costs only that setting instead of resetting them all, and an unreadable
  settings file is set aside instead of overwritten. Saves use a unique temporary name and sync the
  folder, and a read that fails for a passing reason (a sync client holding the file) is retried
  before a file is called damaged.
- **History lives with the rest of the data** (the XDG data folder), carrying over from the old
  location on first run.
- **Opening a file no longer hashes it on the UI thread.** A very large book opens without freezing
  the window while it is read.
- A zero-page PDF says so instead of showing a blank page; a language file for OCR that is too
  small to be real is refused instead of silently breaking text recognition; and "Add to
  Kartoteka" says it only adds the file if Kartoteka has a library open.

## [0.15.1] "Tight Binding" — 2026-10-12
Found by an adversarial pass (hostile files, random-input testing, fuzzing, code review):
- **Fixed a hard crash in Reading mode.** A scan with degenerate coordinates (NaN or infinite
  boxes) could send the page layout into unbounded recursion and take the whole app down.
- **EPUBs can no longer reach the network or start your browser.** A book's remote images,
  stylesheets and frames used to be fetched (a tracking vector), and an iframe or redirect inside a
  book could launch the default browser by itself. Web traffic is now blocked, and only a link you
  click opens outside. An EPUB that would unpack to more than 4 GB is refused.
- **Closing a book no longer quits the app while you are using the Library.** Opened from a file
  manager, Pereplyot used to exit when the book closed even if you had been browsing the Library tab.
- **Several files at once.** Selecting a few files in the file manager and choosing Open With opens
  all of them (it used to open none), and a relative path given to an already-running Pereplyot is
  resolved from where you typed the command.
- **History keeps 1,000 documents instead of 50**, so annotations, search and links to older
  books you never added to the Library stay reachable, and a damaged history file is set aside
  instead of silently reset.
- **The Markdown notes folder never overwrites a file that is not its own** — your own note with
  the same name, or another document with the same title (it adds a short id to the name instead).
- **Library thumbnails are made in the background**, one at a time, so a large library no longer
  freezes the app; opening a very large PDF no longer reads it all into memory to hash it.
- Plainer errors: a damaged `.pdf`/`.epub` says so (instead of "only PDF and EPUB files are
  supported"), and a password-protected PDF says why it cannot be opened instead of showing an
  internal error name. Dropping more than 8 files at once opens the first 8.
- **Ctrl+W** closes the current tab and **Ctrl+1** goes to the Library; the window title names the
  book on screen.

## [0.15.0] "Common Room" — 2026-10-10
- **One window.** The library is no longer a window of its own that gets lost behind the reader:
  it is the first tab of the reader window, pinned and always visible, holding Library, History,
  Notes, Notebooks and Search. Opening a book adds a tab beside it; click Library to go back, and
  closing the last book lands you there. A book can still be popped out into a window of its own.
  Opening a file from another app (or "Open With") goes straight to the book, as before.
- A book's scroll position is kept when you switch to another tab and back.
- **Fixed a crash** when adding a document to the Library with the + button in History: the
  library was borrowed twice while the new entry was built.

## [0.14.0] "Quiet Page" — 2026-10-10
- **Reading mode on scanned books.** Footnote markers that the scan turned into stray symbols
  (`novel,*?`, `tales.**`) are matched to their notes by count and order, so notes land beside the
  right line instead of reading "unmatched note"; small body lines above the notes are no longer
  taken for notes; running heads in capitals (or carrying a page number) are dropped even when
  they change every few pages.
- **Endnotes.** A book whose notes are gathered at the back (a section headed "Notes", a list per
  chapter) now gets them beside the markers that cite them: the notes pages are read, matched to
  the body by chapter and order, and left out of the reading text.
- **Cover and title pages stay as pictures.** The sparse pages at the front of a book (and an
  unrecognised cover) are shown whole instead of as garbled text, and Select All (Ctrl+A) leaves
  them out. A scanned first page is no longer sent to OCR.
- **The page-colours button works in Reading mode** and now sets one shared reading theme (normal,
  dark, sepia) for PDFs and EPUBs; the whole Reading surface, margins included, takes the colours.
  EPUBs get the same button in their status bar.
- **Single line spacing.** The spacing slider is now a multiple of the font's own line height, so
  1.0 is single spacing (it used to bottom out near 1.3×); the default is 1.1.

## [0.13.0] "Loose Leaf" — 2026-10-10
- **Markdown notes folder** (menu): keep one Markdown file per document in a folder you choose —
  an Obsidian or Logseq vault, say. Each highlight is a quote ending in a stable block id
  (`^annot-…`) with a link that opens the passage in Pereplyot; the file is rewritten whenever the
  notes change. Off until you choose a folder.
- **Add to Kartoteka** (Ctrl+K, in a PDF or EPUB that is not yet in a library): asks the installed
  Kartoteka to add the open file to its library, so the citation key can come from there. Needs a
  Kartoteka that accepts files on its command line (0.22.0 or later).
- **Search opens on the hit.** Clicking a result in the Search tab or the Notes browser opens the
  document with that word highlighted (`--search=`), and recognised (OCR) text is indexed too.
- **OCR languages you add yourself** (menu → *OCR languages…*): English stays bundled; other
  languages are `.traineddata` files you pick, never downloaded.
- **Resize an area** by dragging its corner handles; the quoted text follows. Areas show a
  **thumbnail on the Notes card**.
- **Note bubbles in Reading mode:** a marked passage with a note gets a small ✎ after it (the note
  is its tooltip).
- **Merging marks is one undo step.**
- **Paginated EPUBs turn pages with a click** in the outer tenth of either side.
- **Notebook export includes clipped EPUB pictures** (copied from the book, keeping their format).

## [0.12.0] "Clear Margin" — 2026-10-09
- **Ctrl+K command palette**, in the PDF reader, the EPUB reader and the launcher: type to run any
  command, jump to a heading or contents entry, to a note (it flashes), or to a page — by number
  or, in a PDF or EPUB with printed pages, by its printed label — and in the launcher to any tab,
  recent document or notebook. *Posture: Read / Study / Synthesise* are palette commands that set
  the sidebars and the notebook in one go.
- **Caret browsing on the page (F7).** Arrow keys move a caret over the page's own text,
  Ctrl+arrows by word, Home/End by line, Up/Down by line keeping the column, carrying on across
  pages; Shift extends a selection, and 1–4 mark it — so a PDF can be read and marked from the
  keyboard alone, on the real page.
- **Welcome / What's New window** the first time and after each update (also in the menu), with the
  release's name; **EPUB covers** on Library cards; a **real app icon**; `capture-screenshots.sh`
  for the README and website pictures; `release-preflight.sh`.
- **Fixed:** a book's own page list (EPUB 3 `page-list`) was never read when the text had no page
  markers of its own — the container file was matched wrongly — so printed page numbers only
  appeared for books that mark them in the text. They now work from the navigation list too.
- **Textures on highlights** (menu, off by default): each of the four reading colours gets its own
  hatching or dots — in the PDF page and in EPUBs — so they can be told apart without colour.

## [0.11.0] "Gilt Spine" — 2026-10-09

- **Search everything you've read.** The new *Search* tab (Ctrl+F in the launcher) searches the
  text of every document in History and the Library — each PDF page and EPUB chapter — together
  with every note and highlight, ranked, with the matched words in bold. “Quotes” find a phrase,
  `#tag` finds notes with a tag, and filters narrow by shelf, colour and date. Documents are
  indexed in the background into a cache that is safe to delete (*Rebuild* reads everything
  again); results open at the page.
- **The Notes tab is an annotation browser.** Group by document or tag, tick notes to select
  several, then recolour, add or remove a tag, export (Typst, Markdown or LaTeX, one cited
  section per document) or delete them together. Documents open in a reader are left alone and
  counted, so a bulk change never fights a live copy.
- **Resurface** (menu, off by default): a few highlights from past reading on the Library page, a
  different few each day, for revision.

- **The default text look is Zerkalo's "LaTeX Look".** Reading mode and EPUBs are now set in New
  Computer Modern, justified, with indented paragraphs and Zerkalo's tighter rhythm, together with
  the printed page numbers in the left margin and footnotes beside their lines. The face is bundled
  in the Flatpak; the *Aa* panel's first font choice is *LaTeX*, and the older choices are still
  there (anyone who had picked one keeps it).
- **EPUBs for scholars.** A book's own page list (EPUB 3 `page-list` or NCX `pageList`, or the
  page breaks in its text) gives the **printed page** you are on, shown beside the chapter; every
  mark remembers it, and exports, *Copy with citation* and notebook quotes cite `p. 214` instead of
  a chapter. **Note references open in a popover** beside the text, whether marked as notes or just
  a superscript number linking to an endnote. **Pages** (status bar) sets the book in columns —
  a spread on a wide window — turned with Space, the arrow keys or the wheel and carrying on into
  the next chapter; it is remembered per book. Marks are found again by their words, the text
  around them and where they were, approximately if the book has changed, and the Notes list says
  when one cannot be found; new PDF marks keep the text around them too. The EPUB reader now has
  the Notebook (**N**), *Connect…*, tags and *Add to notebook* on its Notes cards, and **Clip this
  image** (right-click a picture) keeps a figure that is outlined on the page, shown in Notes and
  exported as a Typst/Markdown/LaTeX figure.
- **The Notebook.** Press **N** (or the *Notebook* toggle in the status bar) for a page of your own
  writing beside the document. Drag a card from the Notes list into it — or press **+** on the
  card, or *Add to notebook* in a mark's right-click menu — and it becomes a quote block showing
  its source, page and colour; click the source line and the document opens at that passage
  (in the tab that already has it, or a new one). Quotes can come from any document, and the
  Notes tab of the launcher drags into it too. A notebook is a plain Typst file under
  `~/.local/share/pereplyot/notebooks/` that compiles by itself and carries each quote's text
  inline, so it reads correctly with Pereplyot closed. The *Notebooks* tab lists them by Library
  shelf; each notebook can belong to a shelf, which follows the shelf when it is renamed.
- **Connections.** *Connect…* on one annotation, then on another — in any two documents — links
  them with a note of how they relate ("contradicts p. 12"). The link shows on both cards, in the
  Notes list and in the notebook, and clicking it opens the other passage. Connections live in
  their own file (`connections.json`), so nothing in your annotation files changes.
- **Export the notebook** to Typst, Markdown or LaTeX with every quote cited — `@key[p. 12]`,
  `[@key, p. 12]`, `\autocite[p.~12]{key}` when the document has a citation key, its title and
  page in words when it does not — plus your own text, notes, connections and the clipped figures.
  Typst gets an optional `#bibliography(…)` line; the exported file and the stored notebook are both
  compiled by the real `typst` in a test. *Open in Zerkalo* saves the Typst and opens it there.
- **Bring in marks made in other readers.** Open a PDF carrying highlights, underlines,
  strike-outs or sticky notes from Acrobat, Zotero, Preview or Okular and Pereplyot offers, once,
  to bring them in as ordinary annotations of its own (colours go to the nearest of the four
  reading colours). Marks it already has are not duplicated.
- **Save a copy with annotations…** (*More* menu) writes a PDF with every mark you have made —
  highlights, underlines, strike-outs, sticky notes and clipped areas, with their colours and
  notes — as standard annotations any reader shows. It always writes a new file, never the
  original. Checked against poppler, which Okular uses.
- **Links that open the passage.** Exports carry a ↗ link on each quotation
  (`pereplyot://open?hash=…&annotation=…`); Pereplyot registers the `pereplyot://` scheme, finds the
  document by its hash in its History, and opens at that page. `--annotation=<id>` does the same
  from the command line, and `file://` URIs are accepted as files.
- **Area capture.** Press **A** (or choose *Area* in the mark-style menu) and drag a box round a
  figure, table or equation: it is kept as a dashed, tinted rectangle on the page with the text
  the page has inside it. Right-click it to add a note, a tag or a colour. When you export
  notes, each clipped area becomes a picture at print sharpness in a `…-figures` folder beside
  the file and a figure in the output — `#figure(image(…), caption: […])` in Typst, an image in
  Markdown, a `figure` in LaTeX; the Typst is compiled by the real `typst` in a test.
- **Sticky notes.** A note that isn't tied to text now has an icon on the page where you put it
  (*right-click ▸ Add a note here*), and shows in the Notes list like any other.
- **A bubble on every mark that has a note**, beside the highlight, so you can see at a glance
  which ones you commented on.
- **A richer Notes list.** Each card has a colour stripe, the kind and page, the quote, your note
  and its tags. Click a card to jump to its mark, which flashes; click a mark on the page and its
  card lights up and scrolls into view. *This page* narrows the list to the page you are
  reading (and follows you), and a tag narrows it to that tag.
- **Tags.** Type `#method` in a note and it is a tag; or pick from the tags already in use in
  the mark's right-click menu. Tags are kept in the annotation file in a way older versions
  of Kartoteka and Sputnik carry along untouched.
- **Change a mark after the fact.** Right-click a mark to change its colour or switch it between
  highlight, underline and strikeout, and *Merge with N overlapping marks* folds marks that
  cross into one (their notes and tags joined).
- **Figures and tables in Reading mode.** A chart, a diagram or a ruled table with a "Figure 3." or
  "Table 2." caption beside it now appears in the text as a picture of that part of the page,
  with its caption underneath or above as an ordinary paragraph; the axis labels and table cells
  that used to be scrambled into the prose are gone from it. Only a drawing with a caption next
  to it is taken for a figure, so underlines and boxed words are left alone.
- **Search in Reading mode.** Hits are marked in the text, the current one strongly, and Next and
  Previous scroll to it.
- **A Contents for PDFs that have none.** A PDF with no outline of its own gets one from the
  headings Reading mode finds, so the Outline tab, the highlighted section and the breadcrumb
  work for it too once its text has been read.
- **Scans you have recognised are searchable.** Search in the page view now looks through the
  text recognised from a scanned page (once you have chosen *Recognise text*), and a recognised
  scan opens straight into Reading mode the next time without asking again. Recognition
  itself reads a scanned line as one type size, so a line of mixed tall and short letters no
  longer turns into several paragraphs or a false heading; tested against the real Tesseract.
- **Tesseract in the Flatpak.** The manifest now builds Tesseract and Leptonica with English
  language data, so *Recognise text* works out of the box in the Flatpak. Other languages put in
  `~/.local/share/pereplyot/tessdata` are used alongside it.
- **Split view keeps its own keys.** Arrow keys, Space, Home/End, `+`/`-`, Ctrl+F, `T`, the 1–4
  marking keys and Esc act on the pane you last clicked in, not always the first.
- **Pinned figures come back.** Pins are kept with the document, with where each card sat, and
  return when it is reopened.
- **Set page numbering** is in the menu on a printed page number in Reading mode's margin, and
  the margin numbers follow the numbering you set.
- **Split view.** *More ▸ Split view: side by side* (or *top and bottom*) opens a second pane of
  the same document beside or below the first, for reading chapter 3 with the endnotes or a
  figure open in the other. Each pane has its own page, zoom, search and selection; they share
  one set of annotations, so a highlight made in either shows in both (and Undo covers it).
  The same menu row closes it.
- **Reopening puts you back on the line, and shows you where.** The reader used to remember only
  the page; it now also remembers how far down it you were (kept with your progress, in this
  app's own settings), scrolls back to that spot, and draws a hairline with **Continue here**
  across the top of the view that fades after a few seconds or as soon as you scroll.
- **The Contents follows you.** The section you are in is highlighted in the Contents list
  (which scrolls to keep it in view), and its path through the book, "Part I › Ch. 3 › The
  Two-Source Hypothesis", sits above the page.
- **Pin a figure.** Right-click a page, choose **Pin a region…** and drag a box round a figure or
  table: it stays on screen as a small floating card while you read on through the pages that
  keep referring to it. Drag a card by its title bar to move it, ✕ to unpin; up to four at
  once.
- **Hover previews.** Rest the pointer on a link to somewhere in the document — a footnote
  marker, "Figure 3", a table of contents entry — and a small popover shows what is there, a
  crop of the destination page, so you can check it without leaving your place; click still
  jumps, and Back returns. Most humanities PDFs have no links on their citations, so it also
  **reads the citation under the pointer** — `[12]`, `(Smith 2019; Jones and Lee 2015a)`,
  `Smith and Jones (2019)` or a superscript number — and shows the matching entry from the
  bibliography at the end of the document. The popover goes away when the pointer moves on.
  (Reading the bibliography takes a moment the first time, in the background.)
- **Back and Forward through everywhere you have jumped.** Back used to exist only for links.
  Now every jump is remembered, whether it came from a link, the Contents, a thumbnail, a note
  or bookmark in the sidebar, typing a page number, Home/End, or a search result: **Alt+Left**
  returns to where you were and **Alt+Right** goes forward again, with the status bar saying
  which page each leads to. Stepping through search results counts as one trip, not one entry
  per match.
- **Reading mode: a PDF that reads like an EPUB.** The old Text view is now **Reading** (the
  named toggle in the status bar, or `T`): the document re-set as clean, restylable text.
  Running headers, footers and page numbers are removed from the flow; two-column pages read
  one column after the other; paragraphs and headings are recognised from indents, gaps and
  type size, and a paragraph that runs over a column or page break stays one paragraph. A
  quiet **printed page number** sits in a left margin at the line where each source page
  begins (click it to copy a citation or jump to the original page), and **footnotes sit in
  a right margin, level with the marker that cites them**, ragged-right and set smaller;
  point at a marker and its note lights up, click either to open a long note. A note with
  no marker is kept and flagged as unmatched rather than dropped. Selecting text and
  pressing 1–4 saves a mark with the exact place of those words on the page, so it shows
  there too, and marks made on the page show in the text. The mode is remembered per
  document. The text is laid out on a background thread while you start reading: 600 pages
  in under a second.
- **Scanned PDFs can be read in Reading mode.** For a document with no text layer, Reading mode
  offers **Recognise text**: Tesseract is run on each page in the background (at 300 dpi, one
  page at a time, never on the window thread), the pages go through the same layout as any
  other PDF and appear as they finish, and each page's result is cached under
  `~/.cache/pereplyot/ocr/` so it is only ever done once. This needs the `tesseract` program to
  be installed; the Flatpak does not bundle it yet. (The text is used by Reading mode only; page
  view search and selection do not use it.)
- **One typography panel for both formats.** The **Aa** button (EPUB header, and the status
  bar in Reading mode) sets font, size, line spacing, column width, margins, justification,
  paragraph style (indented or spaced) and the Light / Sepia / Dark reading theme, once, for
  both the EPUB reader and PDF Reading mode, and remembers it (`~/.config/pereplyot/reading.json`).
  The EPUB reader's separate theme and font menus are replaced by it.
- **Fixed: a mark made in the Text view never appeared until you reopened it.** Marks now
  repaint as soon as they change, like they do on the page.
- **Edit a highlight in place.** Pointing at a highlight, underline or strikeout outlines it;
  clicking selects it and shows a handle at each end. Drag a handle to cover more or less
  text (the new extent follows the pointer, and Ctrl+Z puts it back), press Delete to remove
  the selected mark, Escape to let go. Selecting text and drawing new marks over an existing
  one work as before.
- **Zooming far in on a sharp screen no longer balloons memory.** At high zoom on a 2× display one
  page used to be rendered as a single bitmap of 6500 × 8500 pixels. Pages wider than 3000
  device pixels are now drawn in 1024-pixel tiles, only for the part of the page on screen
  (plus half a screen of margin), over a low-resolution picture of the whole page that shows
  stretched until the tiles arrive. Zoomed to 3.7× on a 2× screen, peak memory is 420 MB
  where it was 1.1 GB.
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
