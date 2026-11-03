"""Renders the PDFs written by the golden test (target/golden/*.pdf) with
PDFium and compares them with the Labelary references, the same way the
golden test compares the raster backend. Also prints the extracted text of
each page to check that text is selectable.

Usage: python tools/check_pdf.py   (after `cargo test --test golden`)
"""
import glob
import os

import pypdfium2 as pdfium
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def black(img):
    w, h = img.size
    px = img.load()
    return [[px[x, y] < 128 for x in range(w)] for y in range(h)]


def near(m, x, y, w, h):
    for yy in range(max(0, y - 1), min(h, y + 2)):
        for xx in range(max(0, x - 1), min(w, x + 2)):
            if m[yy][xx]:
                return True
    return False


def compare(ref, ours):
    h, w = len(ref), len(ref[0])
    miss = extra = nr = no = 0
    for y in range(h):
        for x in range(w):
            r, o = ref[y][x], ours[y][x]
            nr += r
            no += o
            if r and not near(ours, x, y, w, h):
                miss += 1
            elif o and not near(ref, x, y, w, h):
                extra += 1
    return (miss + extra) / max(1, nr + no)


for pdf_path in sorted(glob.glob(os.path.join(ROOT, 'target', 'golden', '*.pdf'))):
    name = os.path.splitext(os.path.basename(pdf_path))[0]
    ref_path = os.path.join(ROOT, 'tests', 'fixtures', name + '.png')
    doc = pdfium.PdfDocument(pdf_path)
    page = doc[0]
    ref_img = Image.open(ref_path).convert('L')
    scale = ref_img.width / page.get_width()
    img = page.render(scale=scale, grayscale=True).to_pil().convert('L')
    img = img.resize(ref_img.size)
    img.save(os.path.join(ROOT, 'target', 'golden', name + '.pdfium.png'))
    ratio = compare(black(ref_img), black(img))
    text = page.get_textpage().get_text_range().replace('\r', ' ').replace('\n', ' | ')
    print(f'{name}: PDFium vs Labelary mismatch {ratio * 100:.2f}%  text: {text[:90]!r}')
