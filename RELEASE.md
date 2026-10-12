# Pereplyot v0.16.0 "Narrow Margin"

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

The second half of the hardening work started in 0.15.1.

**A narrower EPUB window.** The EPUB reader refused to be narrower than about 1,320 px, wider than a
small laptop screen. Text size, export and "open in a new window" now sit in a ⋮ menu, as in the
PDF reader, and a long chapter title shortens instead of widening the window. It needs about
1,050 px now.

**Add to Library from the reader.** Press Ctrl+K and choose Add to Library, instead of finding the
book in History first.

**Settings that survive a bad value.** A wrong value for one setting used to discard every setting;
now it costs only that one, and an unreadable settings file is set aside rather than overwritten.
History moves into the data folder with everything else, carrying over on first run.

**Smoother opening.** A very large file is hashed in the background, so the window doesn't freeze
while it is read.

**Smaller things.** A PDF with no pages says so, an OCR language file too small to be real is
refused (it would have silently broken recognition), and "Add to Kartoteka" says it only adds the
file if Kartoteka has a library open.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
