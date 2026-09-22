# Changelog

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
