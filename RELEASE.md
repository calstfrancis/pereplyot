# Pereplyot v0.17.2 "Mended Spine"

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

**Password-protected PDFs keep their outline and page numbers.** The unlocked copy Pereplyot reads
used to lose the contents list and the printed page numbers; both now come along. The copy is also
deleted when you close the tab rather than lingering in the cache.

**Dialogs follow a tab that moved.** After a tab is dragged into its own window, its dialogs open
over that window.

**Reading mode no longer widens the window.** Its typography button replaces the Rotate button
(which does nothing there), so the bar stays inside the default width.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
