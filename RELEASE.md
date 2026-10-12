# Pereplyot v0.16.3 "Quiet Hands"

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

**Markdown notes save in the background.** The Markdown notes folder is now written on a worker
thread, newest version only, so an edit never waits for the disk or a synced folder. When a
document's title changes, the notes file under the old title is removed instead of being left
behind; only files that carry that document's own id are ever removed.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
