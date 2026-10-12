# Pereplyot v0.16.2 "Closed Door"

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

**A crash from crafted links, fixed.** Pereplyot opens `pereplyot://` links. A link with a `%` followed
by a non-ASCII character made the link reader panic and close the app, so a web page or a document
could close Pereplyot just by being clicked. The same slip was in the EPUB link reader, and a
notebook line with a setting that had no colon could crash the notebook loader. All three are fixed.

The parsers for links, EPUB markup, notebooks, search, anchors, annotation files and page layout
are now fuzzed with tens of thousands of hostile inputs as part of testing.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
