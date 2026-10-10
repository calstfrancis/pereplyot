# Pereplyot v0.13.0 "Loose Leaf"

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

**Notes that leave the app.** Choose a folder (an Obsidian or Logseq vault, say) from the menu and
Pereplyot keeps one Markdown file per document there. Each highlight is a quote ending in a stable
`^annot-…` block id, with a link that opens the passage in Pereplyot. Off until you choose a folder.

**Add to Kartoteka.** From Ctrl+K, hand the open file to Kartoteka so its citation key comes from
your library. Needs Kartoteka 0.22.

**Search that lands on the word.** A result in the Search tab or the Notes browser opens the
document with the hit marked, and pages you have recognised from scans are searched too. OCR
languages are ones you add yourself from the menu; English is built in and nothing is downloaded.

**Smaller things.** Resize an area by its corners and see it on its Notes card; a ✎ beside noted
passages in Reading mode; paginated EPUBs turn with a click at either edge; notebook export includes
clipped EPUB pictures; merging marks undoes in one step.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
