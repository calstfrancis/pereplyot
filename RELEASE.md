# Pereplyot v0.2.0 "Shared Shelf"

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

**History is now shared across every app that embeds the reader.** Open a PDF or EPUB from
Kartoteka or Sputnik and it now shows up in Pereplyot's own History shelf too — not just
documents opened through Pereplyot itself. (Kartoteka and Sputnik need their own small
update to actually record into it, released alongside this one.) One honest gap: clicking
a history entry another app recorded can still fail if Pereplyot was never granted access
to that specific file — you'll get a "couldn't read file" toast rather than it opening.

**Fixed: Pereplyot could fail to fully close.** When opened purely to view one file —
handed off from Kartoteka, Sputnik, or a file manager's "Open With" — an empty Library/
History launcher window was quietly opening right alongside the reader. Closing the reader
you actually came for didn't quit the app, because that invisible launcher was still there.
It no longer appears in that case, and the app now closes itself once nothing's left open
to read.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
