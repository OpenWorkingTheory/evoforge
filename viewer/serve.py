#!/usr/bin/env python3
"""Serve the replay viewer together with a read-only view of `runs/`.

    python viewer/serve.py              # http://localhost:8000 and every LAN address
    python viewer/serve.py --host 127.0.0.1 --port 9000 --runs path/to/runs

Standard library only. Beyond what `python -m http.server` does this adds:

* `GET /api/runs`         every run directory that recorded at least one replay
* `GET /api/runs/<run>`   a run's manifest and the size of every replay in it
* `GET /runs/<run>/manifest.json` and `/runs/<run>/replays/<file>.json`
  with HTTP Range support, so the viewer can read just the header of each
  replay; nothing else under a run directory is served, and in particular the
  multi-gigabyte `organisms.jsonl` never crosses the wire.
* `Cache-Control: no-cache` on everything, so a phone does not draw fresh
  replays with last week's JavaScript.

The point of it is a phone on the same Wi-Fi: a browser there cannot open a
folder on this machine, so the viewer's "Browse server runs" list asks here
instead. Read-only, no authentication — run it on a network you trust, or bind
`--host 127.0.0.1` to keep it local.
"""

import argparse
import gzip
import json
import os
import re
import socket
import sys
from datetime import datetime, timezone
from email.utils import formatdate, parsedate_to_datetime
from functools import partial
from http import HTTPStatus
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

MANIFEST = 'manifest.json'
REPLAY_DIR = 'replays'
# What a run directory and a replay file are allowed to be called. Everything
# else is a 404 before any path is built, which is what keeps `..`, drive
# letters, and backslashes out of the file system entirely.
SAFE_NAME = re.compile(r'^[A-Za-z0-9][A-Za-z0-9._-]*$')
RANGE = re.compile(r'^bytes=(\d+)-(\d*)$')
GZIP_MIN = 16 * 1024


def safe_segment(name):
    return bool(SAFE_NAME.match(name)) and name not in ('.', '..')


class Handler(SimpleHTTPRequestHandler):
    # Keep-alive: opening a run reads the head of every replay in it, and a
    # fresh TCP handshake per file over Wi-Fi is most of the wait.
    protocol_version = 'HTTP/1.1'

    def __init__(self, *args, runs_dir, quiet=False, **kwargs):
        self.runs_dir = runs_dir
        self.quiet = quiet
        super().__init__(*args, **kwargs)

    # ------------------------------------------------------------ plumbing

    def end_headers(self):
        self.send_header('Cache-Control', 'no-cache')
        super().end_headers()

    def log_message(self, fmt, *args):
        if not self.quiet:
            super().log_message(fmt, *args)

    def do_HEAD(self):
        # Same routing as GET; the writers below skip the body for HEAD.
        self.do_GET()

    def do_GET(self):
        raw = self.path
        # Encoded separators are a way of smuggling `..` past the whitelist;
        # nothing here has a legitimate reason to send one.
        if '%2f' in raw.lower() or '%5c' in raw.lower() or '\\' in raw:
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        path = urlsplit(raw).path
        if path == '/api/runs':
            self.send_json(self.list_runs())
        elif path.startswith('/api/runs/'):
            self.serve_run_index(path[len('/api/runs/'):])
        elif path.startswith('/runs/'):
            self.serve_run_file(path[len('/runs/'):].split('/'))
        else:
            super().do_GET()

    def send_json(self, obj, status=HTTPStatus.OK):
        self.send_bytes(json.dumps(obj).encode(), 'application/json', status)

    def send_bytes(self, body, content_type, status=HTTPStatus.OK):
        if len(body) >= GZIP_MIN and 'gzip' in self.headers.get('Accept-Encoding', ''):
            body = gzip.compress(body, compresslevel=1)
            encoding = 'gzip'
        else:
            encoding = None
        self.send_response(status)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(body)))
        if encoding:
            self.send_header('Content-Encoding', encoding)
            self.send_header('Vary', 'Accept-Encoding')
        self.end_headers()
        if self.command != 'HEAD':
            self.wfile.write(body)

    # ------------------------------------------------------------ the index

    def run_dir(self, run):
        """The directory for `run`, or None if the name is not one we serve."""
        if not safe_segment(run):
            return None
        d = (self.runs_dir / run).resolve()
        if not d.is_relative_to(self.runs_dir) or not d.is_dir():
            return None
        return d

    @staticmethod
    def replays_in(d):
        rd = d / REPLAY_DIR
        if not rd.is_dir():
            return []
        return sorted(
            p for p in rd.iterdir() if p.suffix == '.json' and p.is_file() and safe_segment(p.name)
        )

    @staticmethod
    def read_manifest(d):
        try:
            with open(d / MANIFEST, encoding='utf-8') as f:
                m = json.load(f)
            return m if isinstance(m, dict) else None
        except (OSError, ValueError):
            return None

    def list_runs(self):
        runs = []
        try:
            dirs = [d for d in self.runs_dir.iterdir() if d.is_dir() and safe_segment(d.name)]
        except OSError:
            dirs = []
        for d in dirs:
            replays = self.replays_in(d)
            if not replays:
                continue
            m = self.read_manifest(d) or {}
            runs.append({
                'name': d.name,
                'experiment_name': m.get('experiment_name'),
                'experiment_id': m.get('experiment_id'),
                'format': m.get('format'),
                'replays': len(replays),
                'mtime': int(d.stat().st_mtime),
            })
        runs.sort(key=lambda r: (-r['mtime'], r['name']))
        return {'runs': runs}

    def serve_run_index(self, run):
        d = self.run_dir(run)
        if d is None:
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        self.send_json({
            'name': d.name,
            'manifest': self.read_manifest(d),
            'replays': [{'name': p.name, 'size': p.stat().st_size} for p in self.replays_in(d)],
        })

    # ------------------------------------------------------------ the files

    def serve_run_file(self, parts):
        # Exactly two shapes are served: <run>/manifest.json and
        # <run>/replays/<file>.json. Anything else in a run directory is
        # either huge (organisms.jsonl), or not the viewer's business.
        if len(parts) == 2 and parts[1] == MANIFEST:
            d = self.run_dir(parts[0])
            target = d / MANIFEST if d else None
        elif len(parts) == 3 and parts[1] == REPLAY_DIR and safe_segment(parts[2]) \
                and parts[2].endswith('.json'):
            d = self.run_dir(parts[0])
            target = d / REPLAY_DIR / parts[2] if d else None
        else:
            target = None
        if target is None or not target.is_file():
            self.send_error(HTTPStatus.NOT_FOUND)
            return

        st = target.stat()
        size = st.st_size
        last_modified = datetime.fromtimestamp(st.st_mtime, timezone.utc)
        since = self.headers.get('If-Modified-Since')
        if since and 'Range' not in self.headers:
            try:
                ims = parsedate_to_datetime(since)
            except (TypeError, ValueError, IndexError, OverflowError):
                ims = None
            if ims and ims.tzinfo and last_modified.replace(microsecond=0) <= ims:
                self.send_response(HTTPStatus.NOT_MODIFIED)
                self.send_header('Content-Length', '0')
                self.end_headers()
                return

        start, end = 0, size - 1
        partial_ = False
        m = RANGE.match(self.headers.get('Range', ''))
        if m and size > 0:
            start = int(m.group(1))
            # A range past the end is clamped rather than refused: the viewer
            # asks for the first 64 KB of every replay without knowing which
            # ones are shorter than that.
            end = min(int(m.group(2)) if m.group(2) else size - 1, size - 1)
            if start <= end:
                partial_ = True
            else:
                start, end = 0, size - 1
        length = end - start + 1 if size > 0 else 0

        if partial_:
            self.send_response(HTTPStatus.PARTIAL_CONTENT)
            self.send_header('Content-Range', f'bytes {start}-{end}/{size}')
        else:
            self.send_response(HTTPStatus.OK)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(length))
        self.send_header('Accept-Ranges', 'bytes')
        self.send_header('Last-Modified', formatdate(st.st_mtime, usegmt=True))
        self.end_headers()
        if self.command == 'HEAD':
            return
        with open(target, 'rb') as f:
            f.seek(start)
            remaining = length
            while remaining > 0:
                chunk = f.read(min(remaining, 1 << 16))
                if not chunk:
                    break
                self.wfile.write(chunk)
                remaining -= len(chunk)


class ViewerServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True
    # The stock backlog is 5. A browser opening a run fires a burst of header
    # reads, and on Windows a connection the backlog cannot hold is reset
    # rather than queued, which the viewer sees as a failed fetch.
    request_queue_size = 128

    def handle_error(self, request, client_address):
        # A client dropping a keep-alive connection is not an error worth a
        # traceback on the console; anything else still is.
        exc = sys.exception()
        if isinstance(exc, (ConnectionResetError, ConnectionAbortedError, BrokenPipeError)):
            return
        super().handle_error(request, client_address)


def make_server(viewer_dir, runs_dir, host='0.0.0.0', port=8000, quiet=False):
    """A server ready for `serve_forever()`; port 0 picks a free one."""
    handler = partial(
        Handler, directory=str(viewer_dir), runs_dir=Path(runs_dir).resolve(), quiet=quiet,
    )
    return ViewerServer((host, port), handler)


def lan_addresses():
    """IPv4 addresses a phone on the same network could use, best guess first."""
    found = []
    # The address the default route uses. UDP connect sends nothing.
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.connect(('10.255.255.255', 1))
            found.append(s.getsockname()[0])
    except OSError:
        pass
    try:
        for info in socket.getaddrinfo(socket.gethostname(), None, socket.AF_INET):
            addr = info[4][0]
            if not addr.startswith('127.') and addr not in found:
                found.append(addr)
    except OSError:
        pass
    return found


def main(argv=None):
    here = Path(__file__).resolve().parent
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--host', default='0.0.0.0', help='interface to bind (default: all)')
    ap.add_argument('--port', type=int, default=8000)
    ap.add_argument('--runs', default=str(here.parent / 'runs'), help='run directories to list')
    ap.add_argument('--quiet', action='store_true', help='do not log each request')
    args = ap.parse_args(argv)

    runs_dir = Path(args.runs).resolve()
    if not runs_dir.is_dir():
        print(f'note: {runs_dir} does not exist yet; the run list will be empty', file=sys.stderr)

    srv = make_server(here, runs_dir, args.host, args.port, args.quiet)
    port = srv.server_address[1]
    # The banner is the whole point when stdout is a file or a pipe; do not let
    # block buffering hold it back until the server exits.
    sys.stdout.reconfigure(line_buffering=True)
    print(f'EvoForge viewer, serving {here}')
    print(f'runs from       {runs_dir}')
    print(f'on this machine http://localhost:{port}')
    if args.host in ('0.0.0.0', ''):
        for addr in lan_addresses():
            print(f'from a phone    http://{addr}:{port}')
        print('If Windows asks, allow Python through the firewall on private networks.')
        print('A phone that cannot connect usually means the Wi-Fi profile is "Public",')
        print('or an earlier prompt was cancelled; see README.md, "Viewer".')
    print('Ctrl+C stops it.')
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        srv.server_close()


if __name__ == '__main__':
    main()
