# Changelog

## [0.1.0-dev1] — unreleased

First cut of Pereplyot as a standalone app: `fond-read-gtk` (the shared GTK4/libadwaita
PDF/EPUB reader already used by Kartoteka and Sputnik) extracted into its own repository,
wrapped in a small launcher — Open, drag-and-drop, recent files, a System/Light/Dark theme
toggle, and an in-app changelog viewer. Annotations and reading progress are stored per
document under `~/.local/share/pereplyot/`, keyed by content hash, so a document's sidecar
stays portable between Pereplyot and the two embedding apps.

Extracted from `github.com/calstfrancis/kartoteka` — see that repo's
`docs/READER-EXTRACTION.md` for the boundary survey the crate itself came from.
