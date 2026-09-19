#!/usr/bin/env python3
"""Offline check of serve.py: `python viewer/serve_check.py`.

Builds a throwaway runs/ directory, starts the server on a free local port, and
asserts the index, the Range handling the viewer's header reads depend on, and
that nothing outside manifest.json and replays/*.json can be fetched.
"""

import json
import os
import sys
import tempfile
import threading
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from serve import make_server  # noqa: E402

failures = 0


def check(cond, what):
    global failures
    print(('ok   ' if cond else 'FAIL ') + what)
    if not cond:
        failures += 1


def get(base, path, headers=None, method='GET'):
    req = urllib.request.Request(base + path, headers=headers or {}, method=method)
    try:
        with urllib.request.urlopen(req) as r:
            return r.status, dict(r.headers), r.read()
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers), e.read()


def main():
    viewer_dir = Path(__file__).resolve().parent
    with tempfile.TemporaryDirectory() as tmp:
        runs = Path(tmp) / 'runs'
        run = runs / 'x-1'
        (run / 'replays').mkdir(parents=True)
        (run / 'manifest.json').write_text('{"experiment_name":"x","experiment_id":"x-1"}')
        (run / 'organisms.jsonl').write_text('secret\n' * 100)
        (run / 'config.toml').write_text('[experiment]\n')
        a = '{"organism_id":1,"trace":{"frames":[]}}\n'
        b = '{"organism_id":2,' + '"pad":"' + 'p' * 100000 + '","trace":{"frames":[]}}\n'
        # Bytes, not text: on Windows write_text would turn \n into \r\n and
        # every size below would be off by one.
        (run / 'replays' / 'a.json').write_bytes(a.encode())
        (run / 'replays' / 'b.json').write_bytes(b.encode())
        # A run with no replays must not be listed; a stray file must not be a run.
        (runs / 'empty-1' / 'replays').mkdir(parents=True)
        (runs / 'stray.txt').write_text('x')

        srv = make_server(viewer_dir, runs, '127.0.0.1', 0, quiet=True)
        base = f'http://127.0.0.1:{srv.server_address[1]}'
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        try:
            st, hd, body = get(base, '/api/runs')
            idx = json.loads(body)
            check(st == 200 and [r['name'] for r in idx['runs']] == ['x-1'], '/api/runs lists x-1 only')
            check(idx['runs'][0]['replays'] == 2, '/api/runs counts replays')
            check(idx['runs'][0]['experiment_name'] == 'x', '/api/runs reads manifest')

            st, hd, body = get(base, '/api/runs/x-1')
            d = json.loads(body)
            names = [r['name'] for r in d['replays']]
            sizes = {r['name']: r['size'] for r in d['replays']}
            check(st == 200 and names == ['a.json', 'b.json'], '/api/runs/x-1 lists replays')
            check(sizes['a.json'] == len(a) and sizes['b.json'] == len(b), 'replay sizes')
            check(d['manifest']['experiment_id'] == 'x-1', 'run index carries manifest')

            st, hd, body = get(base, '/api/runs/nope')
            check(st == 404, 'unknown run is 404')

            st, hd, body = get(base, '/runs/x-1/manifest.json')
            check(st == 200 and json.loads(body)['experiment_name'] == 'x', 'manifest served')
            check(hd.get('Cache-Control') == 'no-cache', 'run files are no-cache')

            st, hd, body = get(base, '/runs/x-1/replays/b.json', {'Range': 'bytes=0-9'})
            check(st == 206 and body == b.encode()[:10], 'Range 0-9 gives 10 bytes')
            check(hd.get('Content-Range') == f'bytes 0-9/{len(b)}', 'Content-Range header')

            st, hd, body = get(base, '/runs/x-1/replays/a.json', {'Range': 'bytes=0-65535'})
            check(st == 206 and body == a.encode(), 'Range past EOF is clamped, not 416')
            check(hd.get('Content-Range') == f'bytes 0-{len(a) - 1}/{len(a)}', 'clamped Content-Range')

            st, hd, body = get(base, '/runs/x-1/replays/b.json',
                               {'Accept-Encoding': 'gzip'})
            check(st == 200 and hd.get('Content-Encoding') is None and body == b.encode(),
                  'run files are sent raw (viewer slices them by byte)')

            st, hd, body = get(base, '/runs/x-1/replays/b.json', {'Range': 'bytes=0-9', 'Accept-Encoding': 'gzip'})
            check(st == 206 and 'Content-Encoding' not in hd, 'a 206 is never gzipped')

            for path in ['/runs/x-1/organisms.jsonl', '/runs/x-1/config.toml',
                         '/runs/x-1/replays/', '/runs/x-1/', '/runs/',
                         '/runs/x-1/replays/a.txt', '/runs/x-1/checkpoints/gen_000000.json',
                         '/runs/..%2Fviewer/index.html', '/runs/x-1/replays/..%2F..%2Fmanifest.json',
                         '/runs/x-1/replays/..%5C..%5Cmanifest.json', '/runs/../serve.py/manifest.json']:
                st, hd, body = get(base, path)
                check(st == 404, f'{path} is 404')

            st, hd, body = get(base, '/index.html')
            check(st == 200 and hd.get('Cache-Control') == 'no-cache', 'viewer files are no-cache')
            st, hd, body = get(base, '/main.js')
            check(st == 200, 'main.js served')

            st, hd, body = get(base, '/runs/x-1/replays/b.json', {'Range': 'bytes=0-9'}, 'HEAD')
            check(st == 206 and body == b'' and hd.get('Content-Length') == '10', 'HEAD on a replay')
            st, hd, body = get(base, '/api/runs', method='HEAD')
            check(st == 200 and body == b'', 'HEAD on the index')

            lm = get(base, '/runs/x-1/manifest.json')[1]['Last-Modified']
            st, hd, body = get(base, '/runs/x-1/manifest.json', {'If-Modified-Since': lm})
            check(st == 304, 'If-Modified-Since gives 304')
        finally:
            srv.shutdown()
            srv.server_close()

    print(f'{failures} failure(s)')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
