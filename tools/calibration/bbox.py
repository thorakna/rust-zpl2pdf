import sys
from PIL import Image
def comps(path, y0=0, y1=None, x0=0, x1=None):
    im = Image.open(path).convert('L')
    w, h = im.size
    px = im.load()
    y1 = y1 or h; x1 = x1 or w
    # column-wise segments of black within region
    cols = [any(px[x, y] < 128 for y in range(y0, y1)) for x in range(x0, x1)]
    rows = [any(px[x, y] < 128 for x in range(x0, x1)) for y in range(y0, y1)]
    def runs(a, off):
        out=[]; s=None
        for i,v in enumerate(a+[False]):
            if v and s is None: s=i
            if not v and s is not None: out.append((s+off, i-1+off)); s=None
        return out
    return runs(cols, x0), runs(rows, y0)
if __name__ == '__main__':
    p = sys.argv[1]; args = list(map(int, sys.argv[2:]))
    c, r = comps(p, *args)
    print('cols', c); print('rows', r)
