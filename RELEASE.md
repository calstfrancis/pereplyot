# Pereplyot v0.1.0 "Quiet Spine"

Install via Flatpak:

```bash
flatpak remote-add --user calstfrancis \
  https://calstfrancis.github.io/flatpak/calstfrancis.flatpakrepo
flatpak install calstfrancis io.github.calstfrancis.Pereplyot
```

Already installed? Update with:

```bash
flatpak update io.github.calstfrancis.Pereplyot
```

---

### What's new

**Pereplyot is a real app now.** It started life as `fond-read-gtk`, the PDF/EPUB reader
widget shared by Kartoteka and Sputnik — this release pulls it into its own repository and
wraps it in a small standalone launcher: open a file by picker, drag-and-drop, or a file
manager's "Open With," and it remembers where you left off. No library or vault to set up
first.

**Two shelves: Library and History.** History is every document you've opened, kept
automatically. Library is opt-in — a cover-grid of documents you've specifically chosen to
keep, one click away via an "Add to Library" button on any History entry. PDFs get a real
page-1 thumbnail; EPUB covers are a placeholder icon for now (that needs a small addition
to a crate shared with Kartoteka, coming later).

**Read faster, navigate by keyboard.** Zoom now has fit-to-width and fit-to-page presets
alongside plain zoom in/out, and Left/Right (plus Page Up/Down and Home/End for PDFs) page
and turn chapters without reaching for the mouse. Zoom changes are also smoother — they're
debounced, and no longer silently re-render the entire document in the background if
continuous-scroll view isn't even the one you're looking at.

**Everything else the reader already does:** highlight, underline, strike, and freestanding
notes, with export to Markdown; in-document search; continuous scroll and facing-pages view
for PDFs; a Contents outline on the left and a Notes list on the right; tabs, so opening a
second document doesn't open a second window.

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
