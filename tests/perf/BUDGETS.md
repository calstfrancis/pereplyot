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
