# Pereplyot v0.19.0 "Kettle Stitch"

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

This release is about keeping your notes and highlights safe.

**Changes made elsewhere are kept.** If Kartoteka, Sputnik, a sync client or another window
changed a document's notes while it was open here, an earlier Pereplyot could quietly overwrite
those changes a save or two later. Each save now merges with what is on disk, annotation by
annotation, so additions, edits and deletions made elsewhere all survive, and they appear in the
open document straight away.

**Earlier versions.** Pereplyot now keeps a copy of a document's notes when you start changing
them, then at most once an hour, and before every bulk edit in the Notes tab: the newest 20, plus
one a day for 90 days. *Earlier versions of notes…* in the menu lists them with how many notes
each held, and restores any of them. Restoring keeps the notes it replaces as a version too, so
nothing is lost by trying.

**Backup.** Choose a folder under *Backup…* (a synced folder or a backup drive is best) and
Pereplyot copies your notes, bookmarks, reading places, notebooks, Library and earlier versions
there once a day, as plain files you can open yourself, keeping the last 14. *Restore from a
backup…* brings back only what is missing here and never removes or overwrites anything.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
