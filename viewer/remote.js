// Runs served by `viewer/serve.py`, presented as the File objects library.js
// already knows how to read.
//
// library.js was written against a directory picker, and it touches only
// `name`, `size`, `webkitRelativePath`, `slice(a, b).text()` and `text()` of
// each File. A phone cannot pick a folder on the machine that ran the
// experiment, so this module hands over objects with that same surface whose
// bytes come over HTTP instead — `slice` becomes a Range request, which is
// what lets the library read 64 KB headers rather than whole replays.

/** Where the run files live, relative to the page. */
const RUNS = './runs';
const API = './api/runs';

class RemoteFile {
  constructor(url, name, size, relPath) {
    this.url = url;
    this.name = name;
    this.size = size;
    this.webkitRelativePath = relPath;
  }

  /** The Blob.slice shape library.js uses: `file.slice(0, n).text()`. */
  slice(start = 0, end = this.size) {
    const s = Math.max(0, start);
    const e = Math.min(end, this.size);
    return {
      text: async () => {
        if (e <= s) return '';
        const res = await fetch(this.url, { headers: { Range: `bytes=${s}-${e - 1}` } });
        if (!res.ok) throw new Error(`HTTP ${res.status} for ${this.name}`);
        const text = await res.text();
        // A server that ignores Range sends the whole file with a 200; cut it
        // down here so the caller sees the same thing either way.
        return res.status === 200 ? text.slice(0, e - s) : text;
      },
    };
  }

  async text() {
    const res = await fetch(this.url);
    if (!res.ok) throw new Error(`HTTP ${res.status} for ${this.name}`);
    return res.text();
  }
}

async function getJson(url) {
  const res = await fetch(url, { headers: { Accept: 'application/json' } });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}

/**
 * Every run the server lists, newest first. Rejects when there is no such
 * server — a stock `python -m http.server` answers 404 — so callers can hide
 * the control rather than show an empty one.
 */
export async function listRuns() {
  const { runs } = await getJson(API);
  return runs;
}

/**
 * The manifest and every replay of `run`, as File-likes that `library.open`
 * accepts unchanged. Paths use forward slashes, as a directory picker's do on
 * every platform.
 */
export async function openRun(run) {
  const d = await getJson(`${API}/${encodeURIComponent(run)}`);
  const files = [];
  if (d.manifest) {
    const text = JSON.stringify(d.manifest);
    files.push({ name: 'manifest.json', size: text.length, webkitRelativePath: `${run}/manifest.json`, text: async () => text });
  }
  for (const r of d.replays) {
    files.push(new RemoteFile(
      `${RUNS}/${encodeURIComponent(run)}/replays/${encodeURIComponent(r.name)}`,
      r.name,
      r.size,
      `${run}/replays/${r.name}`,
    ));
  }
  return files;
}
