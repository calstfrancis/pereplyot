#!/usr/bin/env python3
import sys, zipfile, pathlib

out = pathlib.Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)

LINES = [
    "The quick brown fox jumps over the lazy dog.",
    "Pack my box with five dozen liquor jugs.",
    "Sphinx of black quartz, judge my vow.",
]


def pdf(pages, extra_page_keys="", media="[0 0 612 792]", text_origin=(72, 700), tm=None):
    objs = []

    def add(body):
        objs.append(body)
        return len(objs)

    cat = add(None)
    pages_obj = add(None)
    font = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    kids = []
    for n, lines in enumerate(pages, 1):
        x, y = text_origin
        ops = ["BT", "/F1 14 Tf", f"{tm} Tm" if tm else f"{x} {y} Td", "20 TL"]
        ops.append(f"(Page {n}) Tj T*")
        for line in lines:
            ops.append(f"({line}) Tj T*")
        ops.append("ET")
        stream = "\n".join(ops)
        content = add(f"<< /Length {len(stream)} >>\nstream\n{stream}\nendstream")
        page = add(
            f"<< /Type /Page /Parent {pages_obj} 0 R /MediaBox {media} {extra_page_keys} "
            f"/Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
        )
        kids.append(page)
    objs[cat - 1] = f"<< /Type /Catalog /Pages {pages_obj} 0 R >>"
    objs[pages_obj - 1] = (
        f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {len(kids)} >>"
    )
    data = bytearray(b"%PDF-1.4\n")
    offsets = []
    for i, body in enumerate(objs, 1):
        offsets.append(len(data))
        data += f"{i} 0 obj\n{body}\nendobj\n".encode("latin-1")
    xref = len(data)
    data += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    for off in offsets:
        data += f"{off:010d} 00000 n \n".encode()
    data += f"trailer\n<< /Size {len(objs) + 1} /Root {cat} 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(data)


pages = [LINES, LINES, LINES]
(out / "plain.pdf").write_bytes(pdf(pages))
(out / "cropbox.pdf").write_bytes(pdf(pages, "/CropBox [50 60 562 732]"))
# Text drawn so it reads upright once /Rotate 90 is applied, as in a real landscape scan.
(out / "rotated.pdf").write_bytes(pdf(pages, "/Rotate 90", tm="0 1 -1 0 72 72"))

chap = (
    '<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml">'
    "<head><title>{t}</title></head><body><h1>{t}</h1><p>{p}</p></body></html>"
)
with zipfile.ZipFile(out / "book.epub", "w") as z:
    z.writestr("mimetype", "application/epub+zip", compress_type=zipfile.ZIP_STORED)
    z.writestr(
        "META-INF/container.xml",
        '<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
        '<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
    )
    z.writestr(
        "OEBPS/content.opf",
        '<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">'
        '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">smoke-1</dc:identifier>'
        "<dc:title>Smoke Book</dc:title><dc:language>en</dc:language></metadata>"
        '<manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>'
        '<item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>'
        '<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest>'
        '<spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>',
    )
    z.writestr(
        "OEBPS/nav.xhtml",
        '<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">'
        '<body><nav epub:type="toc"><ol><li><a href="c1.xhtml">One</a></li><li><a href="c2.xhtml">Two</a></li></ol></nav></body></html>',
    )
    z.writestr("OEBPS/c1.xhtml", chap.format(t="One", p=LINES[0]))
    z.writestr("OEBPS/c2.xhtml", chap.format(t="Two", p=LINES[1]))


def scholar_epub():
    """A publisher-style EPUB: page-list navigation, in-text page breaks, note references that
    point into a notes file, and enough text to fill several screens."""
    import struct
    import zlib

    def png(w, h, rgb):
        raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))

        def chunk(tag, data):
            body = tag + data
            return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

        return (
            b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw))
            + chunk(b"IEND", b"")
        )

    words = "grace covenant liturgy scripture tradition narrative community witness".split()

    def para(n, extra=""):
        body = " ".join(words[(n + i) % len(words)] for i in range(70))
        return f"<p>Paragraph {n}. {body}.{extra}</p>"

    ns = 'xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"'

    def pb(label, ident):
        return f'<span epub:type="pagebreak" role="doc-pagebreak" id="{ident}" title="{label}"></span>'

    c1 = [pb("41", "pg41")]
    for n in range(1, 10):
        extra = ""
        if n == 2:
            extra = ' See also<a epub:type="noteref" href="notes.xhtml#n1"><sup>1</sup></a> for the argument.'
        if n == 5:
            extra = ' A second point<sup><a href="notes.xhtml#n2">2</a></sup> follows.'
        c1.append(para(n, extra))
        if n == 3:
            c1.append(pb("42", "pg42"))
        if n == 6:
            c1.append(pb("43", "pg43"))
    c1.append('<p><img src="fig.png" alt="A figure" width="120" height="80"/></p>')
    c2 = [pb("44", "pg44")]
    for n in range(10, 17):
        c2.append(para(n))
        if n == 12:
            c2.append(pb("45", "pg45"))
    page = '<?xml version="1.0" encoding="UTF-8"?><html ' + ns + "><head><title>{t}</title></head><body><h1>{t}</h1>{b}</body></html>"
    with zipfile.ZipFile(out / "scholar.epub", "w") as z:
        z.writestr("mimetype", "application/epub+zip", compress_type=zipfile.ZIP_STORED)
        z.writestr(
            "META-INF/container.xml",
            '<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
            '<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
        )
        z.writestr(
            "OEBPS/content.opf",
            '<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">'
            '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">scholar-1</dc:identifier>'
            "<dc:title>Scholar Book</dc:title><dc:language>en</dc:language></metadata>"
            '<manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>'
            '<item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>'
            '<item id="notes" href="notes.xhtml" media-type="application/xhtml+xml"/>'
            '<item id="fig" href="fig.png" media-type="image/png" properties="cover-image"/>'
            '<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest>'
            '<spine><itemref idref="c1"/><itemref idref="c2"/><itemref idref="notes"/></spine></package>',
        )
        z.writestr(
            "OEBPS/nav.xhtml",
            '<?xml version="1.0"?><html ' + ns + "><body>"
            '<nav epub:type="toc"><ol><li><a href="c1.xhtml">One</a></li><li><a href="c2.xhtml">Two</a></li><li><a href="notes.xhtml">Notes</a></li></ol></nav>'
            '<nav epub:type="page-list"><ol>'
            '<li><a href="c1.xhtml#pg41">41</a></li><li><a href="c1.xhtml#pg42">42</a></li><li><a href="c1.xhtml#pg43">43</a></li>'
            '<li><a href="c2.xhtml#pg44">44</a></li><li><a href="c2.xhtml#pg45">45</a></li>'
            "</ol></nav></body></html>",
        )
        z.writestr("OEBPS/c1.xhtml", page.format(t="One", b="".join(c1)))
        z.writestr("OEBPS/c2.xhtml", page.format(t="Two", b="".join(c2)))
        z.writestr(
            "OEBPS/notes.xhtml",
            page.format(
                t="Notes",
                b='<aside id="n1" epub:type="footnote"><p><a href="c1.xhtml">1.</a> See Smith (2019), p. 4, for the original argument.</p></aside>'
                '<ol><li id="n2"><p>On this point compare Jones (2021), chapter 3. <a href="c1.xhtml">\u21a9</a></p></li></ol>',
            ),
        )
        z.writestr("OEBPS/fig.png", png(120, 80, (200, 90, 60)))


def mixed():
    """Pages of different sizes (portrait, landscape, small) with roman then arabic page labels."""
    sizes = ["[0 0 612 792]", "[0 0 792 612]", "[0 0 400 500]", "[0 0 612 792]", "[0 0 595 842]"]
    objs = []

    def add(body=None):
        objs.append(body)
        return len(objs)

    cat = add()
    pages_obj = add()
    font = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    kids = []
    for n, media in enumerate(sizes, 1):
        stream = f"BT /F1 14 Tf 40 {int(media.split()[3].rstrip(']')) - 60} Td (Page {n} of the mixed document) Tj ET"
        c = add(f"<< /Length {len(stream)} >>\nstream\n{stream}\nendstream")
        kids.append(add(f"<< /Type /Page /Parent {pages_obj} 0 R /MediaBox {media} /Resources << /Font << /F1 {font} 0 R >> >> /Contents {c} 0 R >>"))
    objs[cat - 1] = f"<< /Type /Catalog /Pages {pages_obj} 0 R /PageLabels << /Nums [0 << /S /r >> 2 << /S /D /St 1 >>] >> >>"
    objs[pages_obj - 1] = f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {len(kids)} >>"
    data = bytearray(b"%PDF-1.4\n")
    offsets = []
    for i, body in enumerate(objs, 1):
        offsets.append(len(data))
        data += f"{i} 0 obj\n{body}\nendobj\n".encode("latin-1")
    xref = len(data)
    data += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    for off in offsets:
        data += f"{off:010d} 00000 n \n".encode()
    data += f"trailer\n<< /Size {len(objs) + 1} /Root {cat} 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(data)


(out / "mixed.pdf").write_bytes(mixed())
print("fixtures in", out)

scholar_epub()
