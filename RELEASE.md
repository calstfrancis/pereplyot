# Pereplyot v0.1.1 "True Margin"

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

**Fixes found by actually using v0.1.0.** The PDF reader was showing "Reader" twice at the
top of the window — the shared reader window's own header and the PDF reader's own header
both fell back to the same fixed title instead of one of them showing the document you
actually opened. Fixed.

**Keyboard page and chapter navigation now actually works.** It shipped in 0.1.0 but never
fired in practice — a focused button, dropdown, or the page content itself could eat
Left/Right/Home/End before the shortcut ever saw them. Fixed, while still leaving those
keys alone for their normal job (moving the text cursor) when the page-number or search
field has focus.

**The Notes/annotations sidebar toggle moved to the right** of the header, next to
Contents/outline on the left, where it belongs.

**The hamburger menu and status bar now match the rest of the reading app suite** — a
proper popover menu and the usual bottom-right version button that opens the changelog.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
