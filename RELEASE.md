# Pereplyot v0.20.1 "Clean Gutter"

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

**Selection respects columns.** On a two-column page, dragging from the bottom of one column into
the next used to select a block straight across the gutter, mixing both columns' lines. Pereplyot
now reads the page's columns: a drag takes the rest of the first column and then the top of the
next, and clipping a rectangle that spans columns gives each column its own text and highlight.
Single-column pages behave as before.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
