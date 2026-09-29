# Pereplyot v0.8.0 "Open Clasp"

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

**Kartoteka and Sputnik now hand everything reading-related to Pereplyot.** Choosing
"Annotations…" on a Kartoteka entry now opens that entry's highlights and notes in Pereplyot
— the same dialog as before, with "Go to page" jumping straight into the reader. Documents
opened from Kartoteka or Sputnik also show their proper library title rather than whatever
the file's own metadata says. Because neither app builds in any part of Pereplyot any more,
they'll always use whichever version of Pereplyot you have installed.

**Fixed: Pereplyot quietly kept running after you closed the reader**, whenever it had been
opened from Kartoteka, Sputnik, or a file manager's "Open With". It now exits as soon as the
last reader or Annotations window closes.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
