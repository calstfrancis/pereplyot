#!/usr/bin/env python3
"""Locate dark text in a screenshot of the reader window (1000x820 PDF window / 1360x820 EPUB).

text_bands.py IMG            -> "x0 y0 x1 y1" of all dark text inside the PDF page area
text_bands.py IMG --has-highlight -> number of amber highlight pixels in the PDF page area
text_bands.py IMG --last-line -> "x0 y0 x1 y1" of the lowest text line in the EPUB content area
"""
import subprocess, sys

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
X0, Y0, X1, Y1 = (5, 85, 1355, 700) if last_line else (100, 95, 900, 740)
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
