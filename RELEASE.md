# Pereplyot v0.3.0 "Folded Corner"

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

**Bookmarks.** A star toggle beside page/chapter navigation marks the current page (PDF) or
chapter (EPUB) — a lightweight "come back to this" marker, distinct from a highlight or
note. They show in the Notes sidebar above your annotations, and the 'B' key toggles the
current one from anywhere.

**More ways to read a PDF.** Rotate a sideways-scanned page 90° at a time (single-page mode
only); invert colours for reading scanned/white-background pages at night; click the left or
right edge of the page (no dragging) to turn it; jump around a long document from a
page-thumbnail grid; and Up/Down/Space/Backspace now turn pages too, alongside the existing
Left/Right/Page Up/Page Down.

**EPUB reading themes.** A Light/Sepia/Dark theme and a Default/Serif/Sans-serif font choice
for the page itself — independent of the app's own theme, since an EPUB's content doesn't
follow `adw::StyleManager` the way the rest of the UI does.

**Export notes & highlights to Markdown**, for both PDF and EPUB — every bookmark and
annotation, in document order, to a file you choose.

**Fullscreen and maximize buttons** on both the launcher and reader windows, F11 for
fullscreen, and a reading-progress percentage next to the page/chapter count.

**Fixed:** the Contents/outline sidebar toggle was completely absent (not just disabled) for
any document with no embedded outline or table of contents — most of them. It's now always
there, greyed out with an explanation when the document genuinely has none.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
