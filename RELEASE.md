# Pereplyot v0.14.0 "Quiet Page"

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

**Reading mode copes with scanned books.** Scans turn small raised note numbers into stray symbols
(`novel,*?`); Pereplyot now matches those to their notes by count and order, so footnotes land
beside the right line instead of reading "unmatched note". Notes gathered at the back of a book,
a list per chapter, are placed beside the text that cites them and left out of the reading text.
Running heads in capitals, or carrying a page number, no longer turn up in the text, and small
body lines above the notes are no longer mistaken for notes.

**Cover and title pages stay as pictures.** The sparse pages at the front of a book are shown
whole instead of as garbled text, and Select All (Ctrl+A) skips them. A scanned first page is no
longer sent to OCR.

**Page colours that work in Reading mode.** The moon button sets normal, dark or sepia for the
whole Reading view, margins included, and the same setting for EPUBs, which now have the button in
their status bar.

**Single line spacing.** The spacing slider is now measured against the font's own line height, so
1.0 is single spacing, and the default is tighter.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
