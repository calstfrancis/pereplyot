# Pereplyot — open items

Written 2026-10-12, after v0.17.1 "Steady Page"; updated for 0.17.2. None of these is scheduled; each is parked until it bites in real use.

## 1. Far forward jumps in Reading mode (~2.5 s)

**Symptom:** on a very long book (the 600-page `book-600` perf doc), jumping far forward before the text has been laid out (page box, Ctrl+End) freezes the window for about 2.5 s, spread over 15 slices. Jumps back, and short jumps, are fine.

**Cause:** the whole reflowed book is one GtkTextView. To scroll to a target GTK must lay out and shape every line above it, on the main thread. 0.17.1 removed the redundant layout (Clamp/Viewport wrapper, x=0 margin lookups, `changed`→relayout loop); the remaining cost is GTK's own.

**Fix:** windowed text. Keep only the pages near the viewport in the buffer, with spacers for the rest. Everything that assumes one continuous buffer must move to page offsets: scroll position, selection, search hits, margin page labels, note bubbles, the page counter.

**Measure with:** `/tmp/jump.sh` (Xvfb harness, `tests/perf/make_docs.py` docs in `~/.cache/pereplyot-perf`), `PEREPLYOT_PERF=1` stall log, gdb `break pango_layout_set_text` with `ignore` counts. Budget notes are in `tests/perf/BUDGETS.md`.

**Priority:** low. Only matters on books of several hundred pages. Do it as its own release.

## 2. Gdk frame-clock critical

`gdk_frame_clock_get_frame_time: assertion 'GDK_IS_FRAME_CLOCK (frame_clock)' failed`, seen once by the monkey test (seed 13, `dense.pdf`). It comes from inside GTK; no fix on our side. Re-check after GTK updates.

## 3. Narrow-window layout

Reader chrome has minimum widths of about 705 / 964 / 1050 px depending on mode. Cal chose to keep the defaults. The default window is 1000 px and the smoke-test coordinates depend on it, so any narrow-layout work must keep chrome narrow at the default size.

## Done in 0.17.2 "Mended Spine"

The smaller follow-ups: the unlocked-PDF copy is removed on tab close and carries the outline and page labels (kept in a `.json` beside it); dialogs resolve the tab's window when opened (`HostWindow`); Reading mode's typography button replaces Rotate so the window stays at 1000 px; `capture-screenshots.sh` targets the website's real layout (the site has no Pereplyot section yet, so nothing to publish there).

## CI smoke test: two hover checks fail

Found 2026-10-14 while releasing 0.18.0. `tests/smoke/run.sh` in CI has failed every run since at
least 0.16.1 on the same two checks — "hover: resting on an unlinked citation previews its
bibliography entry" and "hover: resting on a link previews where it leads" (the screen's ink count
doesn't change, so no preview appeared). Every other smoke check passes, and `release-flatpak.yml`
gates only on "build & test", so releases have gone out regardless. Not yet investigated: it may be
CI-only timing (the hover delay under a slow runner) rather than a real regression.

