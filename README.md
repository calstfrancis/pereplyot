# Pereplyot (Переплёт)

A standalone GTK4/libadwaita PDF and EPUB reader with highlights, underlines, strikes, and
freestanding notes — built around `fond-read-gtk`, the same reader widget embedded in
[Kartoteka](https://github.com/calstfrancis/kartoteka) and
[Sputnik](https://github.com/calstfrancis/sputnik). "Pereplyot" (переплёт) is Russian for
binding, in the bookbinding sense.

Unlike those two apps, Pereplyot has no git-backed vault behind it — it just opens a file.
Annotations and reading progress are stored locally, keyed by the file's content hash, under
`~/.local/share/pereplyot/`. A document's annotation sidecar is byte-compatible with what
Kartoteka and Sputnik write, so it's portable between all three as long as the file's hash
matches.

## Status

**v0.1.0 "Quiet Spine", released 2026-09-15.** Opening a PDF or EPUB (via Open,
drag-and-drop, or a file manager's "Open With"); a Library (cover-grid, opt-in) and History
(every document opened, automatic) shelf; and the reader itself — highlighting/underline/
strike/notes with export to Markdown, in-document search, continuous scroll and facing-pages
view, zoom-to-fit-width/page, keyboard page and chapter navigation, a Contents outline and a
Notes list, and tabs for reading more than one document at once. No Welcome window, no
screenshot automation, and EPUB cover art (Library shows a placeholder icon for EPUBs) yet.

## Building

```bash
cargo build --release -p pereplyot-ui-gtk
```

Requires the GTK4/libadwaita/WebKitGTK development stack the rest of the Fond-adjacent
suite builds against — see `packaging/io.github.calstfrancis.Pereplyot.yml` for the exact
runtime.

## Relationship to Kartoteka and Sputnik

`crates/fond-read-gtk` is the same crate both apps depend on (as a pinned git dependency on
this repo) — the reader is never written twice. It was originally built inside Kartoteka;
see that repo's `docs/READER-EXTRACTION.md` for the boundary survey (the `ReaderHost` trait,
six methods, no citation keys, no vault) that made pulling it out into its own app possible.
`fond-bib`/`fond-doc` (the document/annotation primitives `fond-read-gtk` itself depends on)
stay in Kartoteka, MIT-licensed, consumed here as an ordinary git dependency.

## License

Proprietary — see `LICENSE`.
