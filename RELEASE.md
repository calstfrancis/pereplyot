# Pereplyot v0.10.0 "Fair Copy"

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

**Export your notes as Typst, LaTeX or Markdown, grouped by what your colours mean** — key
ideas, evidence, connections and problems under their own headings — with citations against a
bibliography key (`@key[p. 42]` in Typst). "Copy with citation" does the same for a single quote.

**Select first, then mark.** Releasing a text selection offers the four colours, underline,
strike-out, a note, copy and copy-with-citation, in both PDF and EPUB. The keys 1–4 mark the
current selection.

**Organise your Library:** named shelves, adjustable card size, sorting, reading progress on
each card — and a new Notes tab that searches every highlight and note across all your
documents, optionally one colour at a time.

**A Text view for PDFs** that screen readers can read and that you can select and mark from
the keyboard alone, with resizable text. PDF links now work, with a Back button.

**Safer, lighter.** Your annotations, progress and settings are saved atomically with a
backup, and damaged files are set aside instead of overwritten. A 300-page PDF now stays
around 360 MB while you scroll it, down from over 1.7 GB. Scanned PDFs get a warning and a
one-click OCR (with ocrmypdf installed), and if a file is re-saved or OCR'd Pereplyot offers
to carry its notes over.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
