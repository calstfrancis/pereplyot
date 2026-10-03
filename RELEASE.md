# Pereplyot v0.9.0 "True Register"

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

**Selecting and highlighting PDF text now lands on the text you dragged over.** On many
publisher and scanned PDFs, a drag could select a different line from the one under the
pointer, draw the highlight in the wrong place, or select nothing at all. The cause was page
layouts and page rotations the reader didn't account for; it now handles both, so selections,
highlights, search matches and right-clicks all line up with the page.

**Copying text works properly.** Text copied from a PDF keeps its spaces instead of running
every word together, and right-clicking now offers "Copy selected text" after a selection and
"Copy highlighted text" on any existing highlight, underline or note.

**Page-turn keys work from anywhere in the reader window**, not only when the page itself has
focus. Menus and text fields keep their usual keys, and the numeric keypad's arrow and Page
Up/Down keys work too.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
