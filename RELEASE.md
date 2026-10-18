# Pereplyot v0.20.2 "Facing Leaf"

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

**Two-page view works on both pages.** The right-hand page of a spread used to be view-only: a drag
there did nothing, so selecting, highlighting and clipping meant switching to single-page view.
Now the facing page takes everything the left one does: drag to select, highlight, underline,
strike out or clip an area, right-click a mark to edit it or add a note, and rest the pointer on a
link or citation for its preview.

**Selection respects columns** (from 0.20.1). On a two-column page a drag follows one column and
then the next instead of cutting across the gutter.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
