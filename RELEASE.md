# Pereplyot v0.22.1 "Full Measure"

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

**Zoom to fit width fits the width.** In the continuous view the button measured the wrong pane,
and on a rotated page it ignored that the page's width is its height, so the page could shrink to
fit the window rather than fill its width. It now measures the pane you're looking at and the
rotated page.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
