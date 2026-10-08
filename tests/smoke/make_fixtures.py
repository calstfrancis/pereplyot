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
print("fixtures in", out)
