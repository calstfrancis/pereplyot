#!/usr/bin/env python3
"""Locate dark text in a screenshot of the reader window (1000x820 PDF window / 1360x820 EPUB).

text_bands.py IMG            -> "x0 y0 x1 y1" of all dark text inside the PDF page area
text_bands.py IMG --has-search-match -> number of blue search-match pixels in the PDF page area
text_bands.py IMG --has-highlight -> number of amber highlight pixels in the PDF page area
text_bands.py IMG --highlight-box -> "x0 y0 x1 y1" of the amber highlight pixels in the PDF page area
text_bands.py IMG --ink-in X0 Y0 X1 Y1 -> number of darker-than-page pixels in that rectangle of the window
text_bands.py IMG --sidebar-ink -> number of text pixels on the first thumbnail card
text_bands.py IMG --last-line -> "x0 y0 x1 y1" of the lowest text line in the EPUB content area
"""
import subprocess, sys

if "--has-search-match" in sys.argv:
    W0, H0, X0, Y0 = 800, 645, 100, 95
    raw = subprocess.run(
        ["convert", sys.argv[1], "-crop", f"{W0}x{H0}+{X0}+{Y0}", "-depth", "8", "rgb:-"],
        capture_output=True,
    ).stdout
    print(sum(1 for i in range(0, len(raw) - 2, 3) if raw[i + 2] > 200 and raw[i] < 190 and raw[i + 1] < 215 and raw[i + 2] - raw[i] > 40))
    sys.exit(0)

if "--ink-box" in sys.argv:
    i = sys.argv.index("--ink-box")
    x0, y0, x1, y1 = (int(v) for v in sys.argv[i + 1 : i + 5])
    W, H = x1 - x0, y1 - y0
    raw = subprocess.run(
        ["convert", sys.argv[1], "-crop", f"{W}x{H}+{x0}+{y0}", "-colorspace", "Gray", "-depth", "8", "gray:-"],
        capture_output=True,
    ).stdout
    rows = [y for y in range(H) if any(raw[y * W + x] < 90 for x in range(W))]
    if rows:
        cols = [x for x in range(W) if any(raw[y * W + x] < 90 for y in range(rows[0], rows[-1] + 1))]
        print(x0 + cols[0], y0 + rows[0], x0 + cols[-1], y0 + rows[-1])
    sys.exit(0)

if "--amber-in" in sys.argv:
    i = sys.argv.index("--amber-in")
    x0, y0, x1, y1 = (int(v) for v in sys.argv[i + 1 : i + 5])
    raw = subprocess.run(
        ["convert", sys.argv[1], "-crop", f"{x1 - x0}x{y1 - y0}+{x0}+{y0}", "-depth", "8", "rgb:-"],
        capture_output=True,
    ).stdout
    print(sum(1 for k in range(0, len(raw) - 2, 3) if raw[k] > 200 and 150 < raw[k + 1] < 235 and raw[k + 2] < 170))
    sys.exit(0)

if "--ink-in" in sys.argv:
    i = sys.argv.index("--ink-in")
    x0, y0, x1, y1 = (int(v) for v in sys.argv[i + 1 : i + 5])
    raw = subprocess.run(
        ["convert", sys.argv[1], "-crop", f"{x1 - x0}x{y1 - y0}+{x0}+{y0}", "-colorspace", "Gray", "-depth", "8", "gray:-"],
        capture_output=True,
    ).stdout
    print(sum(1 for v in raw if v < 200))
    sys.exit(0)

if "--sidebar-ink" in sys.argv:
    W0, H0, X0, Y0 = 190, 400, 20, 100
    raw = subprocess.run(
        ["convert", sys.argv[1], "-crop", f"{W0}x{H0}+{X0}+{Y0}", "-colorspace", "Gray", "-depth", "8", "gray:-"],
        capture_output=True,
    ).stdout
    first_card = raw[8 * W0 : 196 * W0]
    print(sum(1 for v in first_card if v < 215))
    sys.exit(0)

if "--highlight-box" in sys.argv:
    W0, H0, X0, Y0 = 800, 645, 100, 95
    raw = subprocess.run(
        ["convert", sys.argv[1], "-crop", f"{W0}x{H0}+{X0}+{Y0}", "-depth", "8", "rgb:-"],
        capture_output=True,
    ).stdout
    hits = [
        divmod(i // 3, W0)
        for i in range(0, len(raw) - 2, 3)
        if raw[i] > 200 and 150 < raw[i + 1] < 235 and raw[i + 2] < 170
    ]
    if hits:
        ys = [y for y, _ in hits]
        xs = [x for _, x in hits]
        print(X0 + min(xs), Y0 + min(ys), X0 + max(xs), Y0 + max(ys))
    sys.exit(0)

if "--has-highlight" in sys.argv:
    import re
    W0, H0, X0, Y0 = 800, 645, 100, 95
    raw = subprocess.run(
        ["convert", sys.argv[1], "-crop", f"{W0}x{H0}+{X0}+{Y0}", "-depth", "8", "rgb:-"],
        capture_output=True,
    ).stdout
    n = sum(
        1
        for i in range(0, len(raw) - 2, 3)
        if raw[i] > 200 and 150 < raw[i + 1] < 235 and raw[i + 2] < 170
    )
    print(n)
    sys.exit(0)

last_line = "--last-line" in sys.argv
X0, Y0, X1, Y1 = (5, 85, 1200, 700) if last_line else (100, 95, 900, 740)
W, H = X1 - X0, Y1 - Y0

raw = subprocess.run(
    ["convert", sys.argv[1], "-crop", f"{W}x{H}+{X0}+{Y0}", "-colorspace", "Gray", "-depth", "8", "gray:-"],
    capture_output=True,
).stdout
if len(raw) != W * H:
    sys.exit(0)


def dark_rows(x_lo=0, x_hi=W):
    return [y for y in range(H) if any(raw[y * W + x] < 90 for x in range(x_lo, x_hi))]


rows = dark_rows()
if not rows:
    sys.exit(0)

if last_line:
    start = rows[-1]
    for y in reversed(rows):
        if start - y > 6:
            break
        start = y
    lo, hi = start, rows[-1]
    rows = [y for y in rows if lo <= y <= hi]

y0, y1 = rows[0], rows[-1]
cols = [x for x in range(W) if any(raw[y * W + x] < 90 for y in range(y0, y1 + 1))]
print(X0 + cols[0], Y0 + y0, X0 + cols[-1], Y0 + y1)
