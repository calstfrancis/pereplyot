# Pereplyot v0.18.0 "Loose Leaf"

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

**Text for other apps.** Pereplyot can now hand a document's text to another program without
opening a window. `--extract-text <file>` prints a PDF's or EPUB's text and exits, and `-` reads
the document from standard input. The flatpak also installs it as its own
`pereplyot-extract-text` command, so an app can run
`flatpak run --command=pereplyot-extract-text io.github.calstfrancis.Pereplyot -` and pipe the
file in, with no shared folder needed between sandboxes. Sputnik 0.22 uses this to import PDF
syllabi.

Reading itself is unchanged in this release.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
