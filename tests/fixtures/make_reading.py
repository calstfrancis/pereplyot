#!/usr/bin/env python3
"""Fixtures for Reading mode: a footnoted monograph and a two-column paper (Courier, so line
widths are exact). Deterministic; run from anywhere: make_reading.py [OUTDIR]."""
import pathlib, sys

OUT = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path(__file__).parent
WORDS = ("the of and to in that is was for it with as his on be at by this had not are but from or have "
         "an they which one you were her all she there would their we him been has when who will more no "
         "if out so said what up its about into than them can only other new some could time these two may "
         "then do first any my now such like our over man me even most made after also did many before "
         "must through back years where much your way well down should because each just those people "
         "how too little state good very make world still own see men work long get here between both "
         "life being under never day same another know while last might great old year off come since "
         "against go came right used take three scripture covenant grace interpretation tradition "
         "community liturgy narrative authority testimony meaning context history").split()
CW = 0.6  # Courier advance in em


def esc(s):
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


class Page:
    def __init__(self):
        self.ops = []
        self.links = []  # (rect, target page index, x, y)

    def text(self, x, y, s, size=10, bold=False, rise=0):
        font = "/F2" if bold else "/F1"
        self.ops.append(f"BT {font} {size} Tf {rise} Ts 1 0 0 1 {x:.2f} {y:.2f} Tm ({esc(s)}) Tj ET")

    def stream(self):
        return "\n".join(self.ops)


def write_pdf(path, pages, media="[0 0 612 792]"):
    objs = []

    def add(body):
        objs.append(body)
        return len(objs)

    cat = add(None)
    pages_obj = add(None)
    f1 = add("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>")
    f2 = add("<< /Type /Font /Subtype /Type1 /BaseFont /Courier-Bold >>")
    page_ids = [4 + 2 * i + 2 for i in range(len(pages))]
    next_annot = 4 + 2 * len(pages) + 1
    annots = []
    kids = []
    for i, p in enumerate(pages):
        s = p.stream()
        content = add(f"<< /Length {len(s)} >>\nstream\n{s}\nendstream")
        refs = []
        for link in p.links:
            refs.append(next_annot)
            annots.append(link)
            next_annot += 1
        annot_key = f"/Annots [{' '.join(f'{r} 0 R' for r in refs)}]" if refs else ""
        kids.append(add(f"<< /Type /Page /Parent {pages_obj} 0 R /MediaBox {media} {annot_key} "
                        f"/Resources << /Font << /F1 {f1} 0 R /F2 {f2} 0 R >> >> /Contents {content} 0 R >>"))
    for (l, b, r, t), target, x, y in annots:
        add(f"<< /Type /Annot /Subtype /Link /Rect [{l} {b} {r} {t}] /Border [0 0 0] "
            f"/Dest [{page_ids[target]} 0 R /XYZ {x} {y} null] >>")
    objs[cat - 1] = f"<< /Type /Catalog /Pages {pages_obj} 0 R >>"
    objs[pages_obj - 1] = f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {len(kids)} >>"
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for i, body in enumerate(objs, 1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n{body}\nendobj\n".encode("latin-1")
    xref = len(out)
    out += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    for o in offsets:
        out += f"{o:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objs) + 1} /Root {cat} 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    pathlib.Path(path).write_bytes(out)


def words(n, start):
    return [WORDS[(start + i * 7) % len(WORDS)] for i in range(n)]


def wrap(tokens, width_chars):
    """tokens: words, or ('^', label) markers. Returns lines of tokens."""
    lines, cur, n = [], [], 0
    for t in tokens:
        w = 0 if isinstance(t, tuple) else len(t)
        if cur and n + w + 1 > width_chars and not isinstance(t, tuple):
            lines.append(cur)
            cur, n = [], 0
        cur.append(t)
        n += (0 if isinstance(t, tuple) else w + 1)
    if cur:
        lines.append(cur)
    return lines


def put_line(page, x, y, tokens, size):
    cx = x
    for t in tokens:
        if isinstance(t, tuple):
            page.text(cx - CW * size * 0.3, y, t[1], size * 0.62, rise=size * 0.38)
            cx += CW * size * 0.62 * len(t[1]) - CW * size * 0.3
        else:
            page.text(cx, y, t, size)
            cx += CW * size * (len(t) + 1)


def monograph():
    pages = []
    # paragraph tokens per page: (tokens, indent?)
    plan = [
        [("heading", "Chapter One"), (words(70, 1) + [("^", "1")] + words(12, 4), True),
         (words(80, 9), True), (words(60, 20) + [("^", "2")] + words(4, 3), True)],
        [(words(90, 31), False), (words(75, 12) + [("^", "3")] + words(10, 2), True), (words(60, 40), True)],
        [(words(85, 50), True), (words(70, 8), True), (words(64, 17), True)],
        [(words(80, 22), True), (words(72, 5), True), (words(30, 11), True)],
    ]
    notes = {
        0: [("1", "See Smith (2019), p. 4, and the discussion there."),
            ("2", "On this point compare Jones, a long note that does not fit and so runs on over the "
                  "page break into the next one")],
        1: [("", "to its very end, which is here."), ("3", "Cf. the argument above.")],
    }
    for n, blocks in enumerate(plan):
        p = Page()
        header = "THE ART OF EXAMPLES" if n % 2 == 0 else "CHAPTER ONE"
        p.text(250 if n % 2 == 0 else 72, 750, header, 8)
        p.text(300, 40, str(11 + n), 9)
        y = 700
        for toks, indent in blocks:
            if toks == "heading":
                p.text(72, y, indent, 16, bold=True)
                y -= 34
                continue
            lines = wrap(toks, 60 if indent else 62)
            for i, line in enumerate(lines):
                put_line(p, 72 + (14 if indent and i == 0 else 0), y, line, 10)
                y -= 12.5
        ny = 130
        for label, text in notes.get(n, []):
            lines = wrap(text.split(), 75)
            for i, line in enumerate(lines):
                prefix = (label + " ") if i == 0 and label else ""
                p.text(72, ny, prefix + " ".join(line), 7.5)
                ny -= 9.5
        pages.append(p)
    write_pdf(OUT / "monograph.pdf", pages)


def twocol():
    pages = []
    for n in range(3):
        p = Page()
        p.text(72, 750, "Journal of Layout Studies  Vol. 4", 8)
        p.text(300, 40, str(101 + n), 9)
        y_top = 700
        if n == 0:
            p.text(130, y_top, "A Study of Two Columns", 18, bold=True)
            y_top -= 40
        for col, x in enumerate((72, 322)):
            y = y_top
            toks = words(190, 3 + 17 * n + 31 * col)
            toks[0] = f"start{n}{'lr'[col]}"
            for i, line in enumerate(wrap(toks, 38)):
                if y < 90:
                    break
                put_line(p, x, y, line, 10)
                y -= 12.5
        pages.append(p)
    write_pdf(OUT / "twocol.pdf", pages)


def blank():
    p = Page()
    p.ops.append("0.9 g 72 600 300 100 re f")
    write_pdf(OUT / "blank.pdf", [p])


BODY = ("the matter has been argued at length and the evidence is mixed so that readers should "
        "weigh each claim with care before drawing any firm conclusion about it").split()


def body_page(sentences):
    """sentences: list of token lists (words, or ('^', label) markers); wrapped into paragraphs."""
    p = Page()
    y = 700
    for toks in sentences:
        for i, line in enumerate(wrap(toks, 62)):
            put_line(p, 72 + (14 if i == 0 else 0), y, line, 10)
            y -= 12.5
        y -= 6
    return p


def refs_page(entries, heading="References", hanging=True, numbered=None):
    p = Page()
    p.text(72, 700, heading, 14, bold=True)
    y = 670
    for i, text in enumerate(entries):
        lines = wrap(text.split(), 62)
        for j, line in enumerate(lines):
            x = 72 + (14 if hanging and j > 0 else 0)
            put_line(p, x, y, line, 10)
            y -= 12.5
        y -= 0 if hanging else 8
    return p


def citations():
    authoryear = [
        body_page([BODY + ["(Smith", "2019;", "Jones", "and", "Lee", "2015a)", "and"] + BODY[:6]]),
        body_page([BODY]),
        refs_page([
            "Jones, A. and Lee, B. (2015a). On narrative form. Journal of Tests, 3, 1-20.",
            "Jones, A. and Lee, B. (2015b). Another piece on form. Journal of Tests, 4, 5-9.",
            "Smith, J. (2019). The Book of Examples. Example Press, London.",
            "Smith, J. (2012). An older work. Example Press, London.",
        ]),
    ]
    write_pdf(OUT / "cite-authoryear.pdf", authoryear)
    numeric = [
        body_page([BODY + ["as", "shown", "before", "[2]", "and", "see", "also", "[4,", "5]."] + BODY[:5]]),
        body_page([BODY]),
        refs_page([
            "[1] Adams, C. A first work on the subject. 2001.",
            "[2] Brown, D. A second work whose title is long enough to wrap onto another line "
            "of the page. 2005.",
            "[3] Clark, E. A third work. 2008.",
            "[4] Davis, F. The fourth and most relevant work here. 2011.",
            "[5] Evans, G. A fifth work. 2013.",
        ], hanging=False),
    ]
    write_pdf(OUT / "cite-numeric.pdf", numeric)
    narrative = [
        body_page([BODY + ["Later", "Smith", "(2019)", "argues", "that"] + BODY[:7]
                   + ["while", "Jones", "and", "Lee", "(2015a)", "reply"] + BODY[:3]]),
        body_page([BODY]),
        refs_page([
            "Jones, A. and Lee, B. (2015a). On narrative form. Journal of Tests, 3, 1-20.",
            "Smith, J. (2019). The Book of Examples. Example Press, London.",
            "Zhang, W. (2020). A work nobody cites. Example Press.",
        ], hanging=False),
    ]
    write_pdf(OUT / "cite-narrative.pdf", narrative)
    superscript = [
        body_page([BODY + ["a", "claim", ("^", "3"), "follows", "here", "and", "another", ("^", "1"), "too."] + BODY[:6]]),
        body_page([BODY]),
        refs_page([
            "1. Adams, C. A first work on the subject. 2001.",
            "2. Brown, D. A second work. 2005.",
            "3. Clark, E. The third work that supports the claim made earlier, at length. 2008.",
        ], hanging=False),
    ]
    write_pdf(OUT / "cite-superscript.pdf", superscript)


def linked():
    first = body_page([BODY + ["see", "Figure", "3", "for", "the", "plot", "and", "go", "on"] + BODY[:5]])
    # "Figure 3" starts 11+ words in; the link box is placed over the words by measuring them.
    x = 72
    tokens = BODY + ["see"]
    n = sum(len(t) + 1 for t in tokens)
    col = n % 62
    # first line holds 62 chars incl. indent handling is by wrap(); find the line of the link text
    lines = wrap(BODY + ["see", "Figure", "3", "for", "the", "plot", "and", "go", "on"] + BODY[:5], 62)
    y = 700
    for i, line in enumerate(lines):
        cx = 72 + (14 if i == 0 else 0)
        for t in line:
            w = CW * 10 * (len(t) + 1)
            if t == "Figure":
                first.links.append(((cx - 1, y - 3, cx + CW * 10 * 9 + 2, y + 10), 2, 72, 560))
            cx += w
        y -= 12.5
    second = body_page([BODY])
    third = Page()
    third.text(72, 700, "Some earlier matter on this page.", 10)
    third.text(72, 560, "Figure 3. The plot of the evidence, drawn as a bar chart.", 10, bold=True)
    third.text(72, 546, "Source: the example data set, 2019.", 10)
    write_pdf(OUT / "linked.pdf", [first, second, third])


monograph()
twocol()
blank()
citations()
linked()
