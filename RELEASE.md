# Pereplyot v0.20.0 "Fair Copy"

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

This release is about getting text out of what you read, wherever it came from.

**Scans are recognised as soon as you open them.** A scanned PDF used to be a stack of pictures:
nothing to select, copy or highlight until you found and used Reading mode's recognise option,
which in practice never appeared. Now Pereplyot starts recognising a scan the moment it opens, the
page you are on first and then outward from it, two pages at a time, so the words on the page in
front of you are ready before you reach for them. Select, copy, highlight, underline and clip an
area on recognised text in the normal page view, and Reading mode recognises on its own too.
Results are kept, so a scan you have opened before has its text at once. Selection snaps to whole
words, since that is what recognition gives.

**Selection that holds.** Dragging across a passage in an EPUB, or in a PDF's Reading mode, could
stop after a single letter or line. The whole drag is selected again.

**Add to Library and Add to Kartoteka are in the ⋮ menu** of both readers, not only on Ctrl+K,
and show only when they apply. With Kartoteka 0.23 or later, adding a file from Pereplyot opens
an Add this file dialog there to check the details, look it up, or type them in.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
