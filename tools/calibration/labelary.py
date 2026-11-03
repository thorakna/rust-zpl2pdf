import hashlib, os, time, urllib.request
CACHE = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'labelary_cache')
os.makedirs(CACHE, exist_ok=True)
_last = [0.0]
def render(zpl, dpmm=8, size='4x6', index=0, accept='image/png'):
    if isinstance(zpl, str): zpl = zpl.encode('utf-8')
    key = hashlib.sha256(zpl + f'|{dpmm}|{size}|{index}|{accept}'.encode()).hexdigest()[:24]
    ext = 'png' if 'png' in accept else 'pdf'
    path = os.path.join(CACHE, key + '.' + ext)
    if os.path.exists(path): return path
    wait = 0.45 - (time.time() - _last[0])
    if wait > 0: time.sleep(wait)
    req = urllib.request.Request(f'http://api.labelary.com/v1/printers/{dpmm}dpmm/labels/{size}/{index}/', data=zpl,
                                 headers={'Accept': accept, 'Content-Type': 'application/x-www-form-urlencoded'})
    for attempt in range(5):
        try:
            with urllib.request.urlopen(req, timeout=60) as r: data = r.read()
            break
        except urllib.error.HTTPError as e:
            if e.code == 429: time.sleep(2 + attempt * 2); continue
            raise RuntimeError(f'{e.code}: {e.read()[:300]}')
    _last[0] = time.time()
    with open(path, 'wb') as f: f.write(data)
    return path
