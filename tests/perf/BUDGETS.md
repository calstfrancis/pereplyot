# Performance budgets

Measured with `tests/perf/run.sh` on synthetic documents from `tests/perf/make_docs.py`
(release build, Xvfb, software rendering, 1× scale, 8-core machine). Timings come from the
opt-in tracer: run any build with `PEREPLYOT_PERF=1` and read `PERF` lines on stderr.

Documents: `paper-50` (50 pages, two columns + vector figure, 0.3 MB), `book-600` (600 pages
of text with outline and page labels, 2 MB), `scan-600` (600 JPEG pages at 1275×1650, no text
layer, 125 MB).

## Baseline — 2026-10-08, v0.10.0 + Phase 0

| | paper-50 | book-600 | scan-600 | budget |
|---|---:|---:|---:|---|
| `open_pdf` (read + parse) | 33 ms | 86 ms | 104 ms | ≤ 100 ms (**scan: marginal**) |
| launch → reader window up | 154 ms | 257 ms | 322 ms | ≤ 300 ms (**scan: over**) |
| first page render | 10 ms | 7 ms | 10 ms | ≤ 50 ms |
| page render while paging, median / max | 3.5 / 4.5 ms | 2.5 / 3.9 ms | 10 / 12 ms | ≤ 16 ms on the main thread |
| page render at ~2× zoom, median / max | 8 / 13 ms | 7 / 11 ms | 16 / **24 ms** | ≤ 16 ms (**scan: over**) |
| search "covenant", **blocks the main thread** | 48 ms | **331 ms** | 54 ms | no block > 16 ms; first hit ≤ 200 ms (**book: over**) |
| memory after open | 275 MB | 292 MB | **685 MB** | ≤ 300 MB + 1.5 × file size |
| peak memory | 381 MB | 398 MB | **875 MB** | |

First launch after boot adds about 1 s to `open_pdf` (cold `libpdfium.so`); every number
above is a warm run.

## What this says

1. **Search is the biggest stall.** `search_document` re-parses the whole file and runs on the
   GTK thread: 331 ms frozen on a 600-page book, and it grows linearly with length.
2. **Rendering is cheap at 1×, but not at HiDPI.** Four times the pixels puts a scanned page at
   16–24 ms, past one frame, and Phase 1 will render at the screen's scale factor.
3. **Memory is about 3.3× the file size for a scan** (410 MB above the ~275 MB baseline for a
   125 MB file). The reader keeps the file's bytes and hands PDFium a second copy
   (`load_pdf_from_byte_vec(bytes.clone())`). A 500 MB scan would need roughly 1.7 GB.
4. **Opening a large scan** spends its time reading and copying the file on the GTK thread.

These synthetic scans are small next to real ones (300 dpi, 500 MB+), so the memory and
open-time numbers are lower bounds.

## After Phase 1a (render thread, HiDPI) and 1b (background search) — 2026-10-09

Same machine and documents; GTK Cairo renderer (`run.sh` sets it). "Stall" = the GTK main loop
going more than 20 ms without servicing an 8 ms timer (`PEREPLYOT_PERF=1`).

| | paper-50 | book-600 | scan-600 | budget |
|---|---:|---:|---:|---|
| launch → reader window up | 131 ms | 221 ms | 284 ms | ≤ 300 ms ✔ |
| main-thread stalls while paging (40 page-downs) | none | none | none | none ✔ |
| main-thread stalls while zooming to ~2× | none | none | none | none ✔ |
| render time per page at ~2× (now on the render thread) | 5 / 10 ms | 5 / 7 ms | 14 / 23 ms | not on the GTK thread ✔ |
| search "covenant": first hit | 5 ms | 4 ms | – (no text) | ≤ 200 ms ✔ |
| search: main-thread stall | none | none | none | none ✔ |
| search: whole document (background) | 51 ms | 315 ms | 57 ms | |
| stall at open (building the window and the 600-page layout) | 67 ms | 166 ms | 179 ms | ≤ 16 ms ✘ |
| memory after open | 210 MB | 226 MB | **643 MB** | ≤ 300 MB + 1.5 × file ✘ (scan) |
| peak memory | 352 MB | 389 MB | **941 MB** | |

Still open: the stall while the window and the continuous-scroll layout are built (it measures
the per-page geometry of every page), and the scan's memory, which still holds the whole file
twice (see "What this says", point 3). Sharpness on a HiDPI display was checked by eye: the
same text at 2× is crisp where the 1× render, scaled up, is soft.

## After Phase 1c (open once, scan in the background, lazy thumbnails) — 2026-10-09

| | paper-50 | book-600 | scan-600 | budget |
|---|---:|---:|---:|---|
| launch → reader window up | 137 ms | 120 ms | 160 ms | ≤ 300 ms ✔ |
| stall at open | 69 ms | 70 ms | 53 ms | ≤ 16 ms ✘ (building the 600-page scroll view) |
| memory after open | 200 MB | 223 MB | **339 MB** | ≤ 300 MB + 1.5 × file ✔ (scan limit 487) |
| memory after paging | 296 MB | 312 MB | 437 MB | |
| peak memory | 357 MB | 385 MB | 636 MB | |

The scan's memory was dominated by two things, both fixed: the file held twice, and drawing
all 600 thumbnails behind a hidden sidebar.

Still open: the open stall is now mostly creating 600 page widgets; making the scroll view
virtual (only widgets near the viewport) would remove it.

## After the virtual scroll view — 2026-10-09

| | paper-50 | book-600 | scan-600 | budget |
|---|---:|---:|---:|---|
| launch → reader window up | 144 ms | 147 ms | 152 ms | ≤ 300 ms ✔ |
| stall at open | 75 ms | 76 ms | 31 ms | ≤ 16 ms ✘ |
| memory after open | 199 MB | 213 MB | 332 MB | ✔ |
| memory after paging | 292 MB | 302 MB | 472 MB | |
| peak memory | 368 MB | 387 MB | 700 MB | |

The continuous view now builds in 0.1 ms and holds widgets for the pages near the viewport
only. That removed ~22 ms from the scan's stall, but the stall on the small documents did not
move, so page widgets were not its main cost there. The perf log shows what is: reading the
first page's geometry on the window thread (23 ms, mostly PDFium loading the page while the
render thread loads it too) and GTK's first frame under the Cairo renderer in Xvfb (~30 ms).
Both happen before the window can be shown, so getting under 16 ms would mean showing a blank
window first and reading the geometry afterwards.
