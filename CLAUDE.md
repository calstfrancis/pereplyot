# Pereplyot — Claude Instructions

Pereplyot follows the same house style and release conventions as the wider suite (see
`~/Projects/CLAUDE.md`, the repo root above this one, for dev-build/release workflow,
versioning, and flatpak publishing) — it is not a Fond suite member (Fond = zerkalo,
kartoteka, skrizhal, Gost), the same relationship Iskra and Rubric have to the house style.
This file only covers what's Pereplyot-specific.

## What this app is

A standalone PDF/EPUB reader built around `crates/fond-read-gtk`, the reader widget also
embedded in Kartoteka and Sputnik (git dependency on this repo, pinned by tag). Pereplyot
has no library/vault — it opens whatever file it's given and stores annotations/progress
locally, keyed by content hash, under `~/.local/share/pereplyot/` (`src/reader_host.rs`,
`LocalReaderHost`). See `README.md` for the fuller relationship to Kartoteka/Sputnik and
Kartoteka's own `docs/READER-EXTRACTION.md` for the `ReaderHost` boundary this all rests on.

## Build and release rules

- **Never build or release unless Cal explicitly says to.** Code changes alone do not
  trigger a build.
- Saying "prep a dev build for pereplyot" triggers a build. Saying "release pereplyot"
  triggers a release. Nothing else does.
- `pereplyot-ui-gtk/Cargo.toml`'s `version` is the sole source of truth (single crate, no
  separate CLI crate — see root `CLAUDE.md`'s Version files table).
- Whenever `Cargo.lock` changes — most likely a bump of the `fond-bib`/`fond-doc` pinned
  Kartoteka tag, since `fond-read-gtk` itself is a local workspace member — regenerate
  `packaging/cargo-sources.json` per the root `CLAUDE.md`'s vendored-dependency rule (same
  recipe as Zerkalo/Kartoteka/Sputnik).
- CI-published flatpak flow (`publish-flatpak.sh` just pushes; `release-flatpak.yml` builds
  and publishes) — needs `RELEASE-CI-SETUP.md`'s one-time secrets before it can actually
  publish anything.

## Code style

- No comments unless the WHY is non-obvious. No multi-line docstrings or comment blocks.
- No trailing summaries at the end of responses — the user can read the diff.

## Architecture

- Raw gtk4-rs/libadwaita (0.11 / 0.9, `v4_10`/`v1_4` features), matching `fond-read-gtk`'s
  own pins exactly — this has to stay version-aligned with the reader crate, not with
  whatever the rest of the suite happens to be on.
- Deliberately minimal chrome: a plain `gio::Menu` hamburger (`src/ui/menu.rs`), not the
  wider house style's hand-built popover — justified by scale (four menu items total), not
  a house-style opt-out in general. Still keeps the System/Light/Dark theme toggle and the
  `CHANGELOG.md`-backed changelog viewer the house standard expects everywhere.
- The launcher window (`src/ui/window.rs`) never toggles into "reader mode" itself —
  `fond_read_gtk::pdf::show_pdf_reader`/`epub::show_epub_reader` open their own shared
  tab-host window (`fond_read_gtk::reader_host`), so the launcher just stays open behind it
  as a start page / recents list.
- Every entry point that can hand Pereplyot a file (Open button, drag-and-drop, a CLI/"Open
  With" argument) funnels through the single `ui::window::open_path` — don't duplicate the
  hash/title-sniffing/reader-dispatch logic at a new call site.

## Not yet built (fast-follows, not oversights)

Welcome/What's New window, command palette, `capture-screenshots.sh`, a real app icon (the
current `packaging/pereplyot.svg` is a placeholder), CI publish secrets, real EPUB cover
thumbnails in the Library (needs a new `fond-doc` function — see `src/thumbnail.rs` — which
needs a Kartoteka release to reach Pereplyot's pinned tag; EPUBs show a placeholder icon
until then).
