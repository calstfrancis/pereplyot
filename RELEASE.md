# Pereplyot v0.23.0 "Woven Margin"

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

This release is about getting from reading to writing.

**Every document knows what it is.** *Document details…* holds a bibliographic record, filled from the file
first and, only if you press *Look up online*, from CrossRef, arXiv or Open Library. Copy a selection as a
footnote or a reference in SBL, Chicago, Turabian, APA or MLA, and notebook exports now carry the bibliography
their citations need. (The flatpak gains network access for that one button; *Never go online* hides it.)

**What this cites, and whether you own it.** The sidebar's *More* tab lists the references with where each is
cited, marks the ones in your library, and opens your copy at the page the citation names. It also reads the
back-of-book index and lists figures and tables, and a table can be copied as cells.

**Marks that follow a document.** Open a preprint, another edition or an OCR'd copy and your notes are found
again by their words. Any that are uncertain are flagged, never deleted.

**Writing and remembering.** A synthesis matrix and an annotated bibliography, a notebook you can reorder,
opt-in spaced review of what you tag `#remember`, a sync folder with conflict merging, shared layers, *Look up…*
on a selection, and a sample document on first run.

Not in this release: windowed Reading mode (a far jump in a very long book still stalls), scripture references,
Compare versions, and a narrow-window layout.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
