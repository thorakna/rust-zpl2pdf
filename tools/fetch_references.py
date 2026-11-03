"""Fetches Labelary reference renderings for the golden tests.

Usage: python tools/fetch_references.py [fixture.zpl ...]

Every tests/fixtures/<name>.zpl is sent to the public Labelary API
(8 dpmm, 4x6 inch unless the first line contains `^FX size=WxH dpmm=N`) and
saved as tests/fixtures/<name>.png (later labels as <name>-2.png, ...). Only send synthetic labels: the data
leaves your machine.
"""
import os
import re
import sys
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIXTURES = os.path.join(ROOT, 'tests', 'fixtures')


def render(zpl: bytes, dpmm: int, size: str, index: int = 0) -> bytes:
    url = f'http://api.labelary.com/v1/printers/{dpmm}dpmm/labels/{size}/{index}/'
    req = urllib.request.Request(url, data=zpl, headers={'Accept': 'image/png'})
    for attempt in range(5):
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            if e.code == 429:
                time.sleep(2 + 2 * attempt)
                continue
            if e.code == 404:
                return None
            raise
    raise RuntimeError('rate limited')


def main(paths):
    if not paths:
        paths = sorted(os.path.join(FIXTURES, f) for f in os.listdir(FIXTURES) if f.endswith('.zpl'))
    for path in paths:
        zpl = open(path, 'rb').read()
        m = re.search(rb'\^FX size=([0-9.]+x[0-9.]+) dpmm=(\d+)', zpl)
        size, dpmm = (m.group(1).decode(), int(m.group(2))) if m else ('4x6', 8)
        index = 0
        while True:
            png = render(zpl, dpmm, size, index)
            if png is None:
                break
            suffix = '' if index == 0 else f'-{index + 1}'
            out = os.path.splitext(path)[0] + suffix + '.png'
            open(out, 'wb').write(png)
            print(f'{os.path.basename(out)}: {len(png)} bytes')
            index += 1
            time.sleep(0.5)


if __name__ == '__main__':
    main(sys.argv[1:])
