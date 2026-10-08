#!/usr/bin/env python3
"""Generate large synthetic documents for performance budgets (deterministic, no real data).

make_docs.py OUTDIR  ->  scan-600.pdf   600 pages of scanned-looking JPEG pages, no text layer
                         book-600.pdf   600 pages of born-digital text, outline, roman+arabic page labels
                         paper-50.pdf   50 pages of two-column text with a vector figure on each
"""
import io, random, sys, pathlib, zlib

try:
    from PIL import Image, ImageDraw, ImageFont, ImageChops
except ImportError:
    sys.exit("needs Pillow")

out = pathlib.Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)

WORDS = ("the of and to in that is was he for it with as his on be at by this had not are but from or have an "
         "they which one you were her all she there would their we him been has when who will more no if out so "
         "said what up its about into than them can only other new some could time these two may then do first any "
         "my now such like our over man me even most made after also did many before must through back years where "
         "much your way well down should because each just those people how too little state good very make world "
         "still own see men work long get here between both life being under never day same another know while last "
         "might us great old year off come since against go came right used take three scripture covenant grace "
         "interpretation tradition community liturgy narrative authority testimony meaning context history").split()


def sentence(rng, lo=6, hi=18):
    n = rng.randint(lo, hi)
    s = " ".join(rng.choice(WORDS) for _ in range(n))
    return s[0].upper() + s[1:] + "."


def para(rng, sentences=5):
    return " ".join(sentence(rng) for _ in range(sentences))


def wrap(text, width_chars):
    lines, cur = [], ""
    for w in text.split():
        if len(cur) + len(w) + 1 > width_chars:
            lines.append(cur)
            cur = w
        else:
            cur = (cur + " " + w).strip()
    if cur:
        lines.append(cur)
    return lines


def esc(s):
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


class Pdf:
    def __init__(self):
        self.objs = []

    def add(self, body=None):
        self.objs.append(body)
        return len(self.objs)

    def set(self, n, body):
        self.objs[n - 1] = body

    def stream(self, dict_extra, data):
        head = f"<< {dict_extra} /Length {len(data)} >>\nstream\n".encode("latin-1")
        return head + data + b"\nendstream"

    def write(self, path, root):
        buf = bytearray(b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n")
        offs = []
        for i, o in enumerate(self.objs, 1):
            offs.append(len(buf))
            body = o if isinstance(o, bytes) else o.encode("latin-1")
            buf += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"
        x = len(buf)
        buf += f"xref\n0 {len(self.objs) + 1}\n0000000000 65535 f \n".encode()
        for o in offs:
            buf += f"{o:010d} 00000 n \n".encode()
        buf += f"trailer\n<< /Size {len(self.objs) + 1} /Root {root} 0 R >>\nstartxref\n{x}\n%%EOF\n".encode()
        pathlib.Path(path).write_bytes(bytes(buf))


def build(path, pages, extra_catalog=""):
    """pages: list of (content_stream_bytes, resources_str, image_objs) built by caller via closures."""
    raise NotImplementedError


def scan(path, n=600, w=1275, h=1650):
    rng = random.Random(1)
    try:
        font = ImageFont.load_default(size=26)
    except TypeError:
        font = ImageFont.load_default()
    pdf = Pdf()
    cat = pdf.add()
    pages_obj = pdf.add()
    kids = []
    for p in range(n):
        im = Image.effect_noise((w, h), 18).point(lambda v: 205 + (v - 128) // 6)
        d = ImageDraw.Draw(im)
        y = 150 + rng.randint(-6, 6)
        while y < h - 160:
            line = " ".join(rng.choice(WORDS) for _ in range(rng.randint(8, 13)))
            d.text((140 + rng.randint(-4, 4), y), line, fill=30 + rng.randint(0, 40), font=font)
            y += 38
        d.text((w // 2 - 20, h - 100), str(p + 1), fill=40, font=font)
        im = im.rotate(rng.uniform(-0.6, 0.6), fillcolor=215, resample=Image.BILINEAR)
        buf = io.BytesIO()
        im.save(buf, "JPEG", quality=72)
        data = buf.getvalue()
        img = pdf.add(pdf.stream(f"/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode", data))
        content = f"q 612 0 0 792 0 0 cm /Im0 Do Q".encode()
        c = pdf.add(pdf.stream("", content))
        pg = pdf.add(f"<< /Type /Page /Parent {pages_obj} 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /Im0 {img} 0 R >> >> /Contents {c} 0 R >>")
        kids.append(pg)
    pdf.set(pages_obj, f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {n} >>")
    pdf.set(cat, f"<< /Type /Catalog /Pages {pages_obj} 0 R >>")
    pdf.write(path, cat)


def book(path, n=600):
    rng = random.Random(2)
    pdf = Pdf()
    cat = pdf.add()
    pages_obj = pdf.add()
    font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>")
    bold = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold /Encoding /WinAnsiEncoding >>")
    kids = []
    chapter_pages = []
    for p in range(n):
        ops = ["BT", "/F1 11 Tf", "13.2 TL", "72 720 Td"]
        if p % 30 == 0:
            chapter_pages.append(p)
            ops += ["/F2 18 Tf", f"({esc('Chapter ' + str(p // 30 + 1) + ' ' + sentence(rng, 2, 4))}) Tj", "T*", "T*", "/F1 11 Tf"]
        lines = []
        while len(lines) < 46:
            lines += wrap(para(rng, rng.randint(4, 8)), 82) + [""]
        for ln in lines[:46]:
            ops.append(f"({esc(ln)}) Tj T*" if ln else "T*")
        ops += ["ET", "BT /F1 10 Tf 300 40 Td", f"({p + 1}) Tj", "ET"]
        c = pdf.add(pdf.stream("", "\n".join(ops).encode("latin-1")))
        pg = pdf.add(f"<< /Type /Page /Parent {pages_obj} 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font} 0 R /F2 {bold} 0 R >> >> /Contents {c} 0 R >>")
        kids.append(pg)
    # outline
    items = []
    out_root = pdf.add()
    for i, p in enumerate(chapter_pages):
        items.append(pdf.add())
    for i, p in enumerate(chapter_pages):
        nxt = f" /Next {items[i + 1]} 0 R" if i + 1 < len(items) else ""
        prv = f" /Prev {items[i - 1]} 0 R" if i else ""
        pdf.set(items[i], f"<< /Title ({esc('Chapter ' + str(i + 1))}) /Parent {out_root} 0 R /Dest [{kids[p]} 0 R /Fit]{prv}{nxt} >>")
    pdf.set(out_root, f"<< /Type /Outlines /First {items[0]} 0 R /Last {items[-1]} 0 R /Count {len(items)} >>")
    pdf.set(pages_obj, f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {n} >>")
    pdf.set(cat, f"<< /Type /Catalog /Pages {pages_obj} 0 R /Outlines {out_root} 0 R /PageLabels << /Nums [0 << /S /r >> 10 << /S /D /St 1 >>] >> >>")
    pdf.write(path, cat)


def paper(path, n=50):
    rng = random.Random(3)
    pdf = Pdf()
    cat = pdf.add()
    pages_obj = pdf.add()
    font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>")
    bold = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold /Encoding /WinAnsiEncoding >>")
    kids = []
    for p in range(n):
        ops = []
        for col, x in enumerate((54, 322)):
            ops += ["BT", "/F1 9.5 Tf", "11.5 TL", f"{x} 700 Td"]
            if p == 0 and col == 0:
                ops += ["/F2 14 Tf", f"({esc(sentence(rng, 5, 8))}) Tj", "T*", "T*", "/F1 9.5 Tf"]
            lines = []
            while len(lines) < 52:
                lines += wrap(para(rng, rng.randint(4, 7)), 58) + [""]
            for ln in lines[:52]:
                ops.append(f"({esc(ln)}) Tj T*" if ln else "T*")
            ops.append("ET")
        # a vector "figure": axes and a polyline in the lower right
        pts = [(340 + i * 12, 120 + rng.randint(0, 60)) for i in range(18)]
        ops += ["0.5 w", "340 110 m 556 110 l S", "340 110 m 340 190 l S", f"{pts[0][0]} {pts[0][1]} m"]
        ops += [f"{x} {y} l" for x, y in pts[1:]] + ["S"]
        ops += ["BT /F1 8 Tf 340 96 Td", f"({esc('Figure ' + str(p + 1) + '. ' + sentence(rng, 4, 7))}) Tj", "ET"]
        ops += ["BT /F1 9 Tf 300 36 Td", f"({p + 1}) Tj", "ET"]
        c = pdf.add(pdf.stream("", "\n".join(ops).encode("latin-1")))
        pg = pdf.add(f"<< /Type /Page /Parent {pages_obj} 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font} 0 R /F2 {bold} 0 R >> >> /Contents {c} 0 R >>")
        kids.append(pg)
    pdf.set(pages_obj, f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {n} >>")
    pdf.set(cat, f"<< /Type /Catalog /Pages {pages_obj} 0 R >>")
    pdf.write(path, cat)


if __name__ == "__main__":
    paper(out / "paper-50.pdf")
    book(out / "book-600.pdf")
    scan(out / "scan-600.pdf")
    for f in sorted(out.glob("*.pdf")):
        print(f"{f.name}: {f.stat().st_size / 1e6:.1f} MB")
