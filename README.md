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

**v0.3.0 "Folded Corner", released 2026-09-22.** Opening a PDF or EPUB (via Open,
drag-and-drop, or a file manager's "Open With"); a Library (cover-grid, opt-in) and History
(every document opened, automatic, **shared across Kartoteka and Sputnik too** — see below)
shelf; and the reader itself — highlighting/underline/strike/notes with export to Markdown,
bookmarks, in-document search, continuous scroll and facing-pages view, zoom-to-fit-width/
page, rotate and invert-colours for PDF, reading themes (Light/Sepia/Dark) and font choice
for EPUB, a page-thumbnail grid, click-to-turn page zones, keyboard/mouse page and chapter
navigation, a Contents outline and a Notes list, and tabs for reading more than one document
at once. No Welcome window, no screenshot automation, and EPUB cover art (Library shows a
placeholder icon for EPUBs) yet.

## Building

```bash
cargo build --release -p pereplyot-ui-gtk
```

Requires the GTK4/libadwaita/WebKitGTK development stack the rest of the Fond-adjacent
suite builds against — see `packaging/io.github.calstfrancis.Pereplyot.yml` for the exact
runtime.

## Relationship to Kartoteka and Sputnik

As of the CLI work below, Kartoteka and Sputnik launch this app directly to open a
document, rather than embedding the reader UI in-process — one reader implementation, not
three copies that only stay in sync when someone remembers to bump a version pin (this used
to happen; Sputnik's embedded copy visibly lagged Pereplyot's own releases before the
switch). If Pereplyot isn't installed, they fall back to the system's registered default
handler for the file instead of keeping an embedded reader around as a backup.

`pereplyot` accepts three invocation shapes:

```bash
pereplyot <file>                                            # standalone: Pereplyot's own local storage
pereplyot --vault=<root> --key=<key> <file>                 # Kartoteka; a Sputnik "library reading"
pereplyot --annotations-file=<path> [--progress-file=<path>] <file>   # a Sputnik course material
```

The `--vault`/`--key` form writes straight into that vault's `notes/<key>.md` /
`annots/<key>.json` — the exact files Kartoteka's own `KartotekaReaderHost` reads and
writes — via a `fond_bib::Library` opened directly on `root`, so annotations, resume
position, and the manual page-numbering override all round-trip precisely as if Kartoteka
had opened the reader itself. This works because Pereplyot already depends on `fond-bib`
directly (see below), and `fond_bib::Library::open` is just a vault-root path with no lock
file or daemon — trivially constructible from a separate process. The `--annotations-file`
form is deliberately dumber: a plain JSON read/write at whatever path the caller hands
over, with no vault/key concept at all, since Sputnik's course-material store isn't a
`fond_bib::Library`. Bookmarks (a Pereplyot-only concept, no vault schema slot yet) always
stay in Pereplyot's own local, content-hash-keyed store regardless of invocation shape.

`crates/fond-read-gtk` is the same crate both apps depend on (as a pinned git dependency on
this repo) — the reader is never written twice. It was originally built inside Kartoteka;
see that repo's `docs/READER-EXTRACTION.md` for the boundary survey (the `ReaderHost` trait,
now eight methods including bookmarks, no citation keys, no vault) that made pulling it out
into its own app possible. `fond-bib`/`fond-doc` (the document/annotation primitives
`fond-read-gtk` itself depends on) stay in Kartoteka, MIT-licensed, consumed here as an
ordinary git dependency.

`fond_read_gtk::history` is a small shared reading-history log both apps also write to
(alongside their own annotation storage, unaffected) whenever they open a document — so
Pereplyot's History shelf reflects what was opened anywhere, not just through Pereplyot
itself. See that module's own doc comment for why it resolves a real home-relative path
rather than the usual sandbox-relative one, and the one flatpak permission that requires.

## License

Proprietary — see `LICENSE`.
