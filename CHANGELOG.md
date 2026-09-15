# Changelog

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
