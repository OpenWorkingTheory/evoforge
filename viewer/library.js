// Browsing what a run recorded.
//
// A run directory holds one replay per recorded organism — the generation's top
// few plus a random sample — and the interesting ones are not always the fast
// ones. This module turns that directory into a sortable, groupable, filterable
// list so the outliers can be found and played without knowing a filename.
//
// It knows nothing about rendering. It hands a `File` back to whoever asked.
//
// # Why headers rather than whole files
//
// A replay is mostly trajectory: a few hundred kilobytes of poses behind a few
// kilobytes of description. Everything this list needs — identity, parentage,
// fitness, metrics, and the whole genome — sits before the `trace` key, so each
// file is read only up to there. Three hundred replays cost a few megabytes of
// reads instead of thirty, and the trajectory is loaded only for the one
// actually being watched.

/** Bytes read from each replay while looking for the end of its header. */
const HEADER_SLICE = 65536;

/** How many replay headers to read at once. */
const CONCURRENCY = 16;

/**
 * Net elevation change over the measured window.
 *
 * Reads the recorded pair when a replay has it, and falls back to the endpoints
 * for anything written before format 8, where the metric did not exist yet.
 */
function netElevation(m) {
  if (typeof m.net_gain === 'number' || typeof m.net_loss === 'number') {
    return (m.net_gain || 0) - (m.net_loss || 0);
  }
  if (m.end && m.start) return m.end.y - m.start.y;
  return 0;
}

/** Sort keys offered in the dropdown, and what the presets below reach for. */
const SORTS = [
  { key: 'fitness', label: 'fitness', of: (e) => e.fitness },
  { key: 'dx', label: 'distance along +X', of: (e) => e.m.displacement_x },
  { key: 'speed', label: 'mean speed', of: (e) => e.speed },
  { key: 'path', label: 'path length', of: (e) => e.m.path_length },
  { key: 'wander', label: 'wander (path − displacement)', of: (e) => e.wander },
  { key: 'upright', label: 'seconds upright', of: (e) => e.m.upright_seconds },
  { key: 'height', label: 'mean height', of: (e) => e.m.mean_height },
  { key: 'elevation', label: 'net elevation', of: (e) => netElevation(e.m) },
  { key: 'climb', label: 'total ascent', of: (e) => e.m.climb || 0 },
  { key: 'descent', label: 'total descent', of: (e) => e.m.descent || 0 },
  { key: 'actuation', label: 'actuation (effort)', of: (e) => e.m.actuation },
  { key: 'economy', label: 'distance per effort', of: (e) => e.economy },
  { key: 'breaks', label: 'joints lost', of: (e) => e.breaks },
  { key: 'caution', label: 'caution', of: (e) => e.caution },
  { key: 'parts', label: 'part count', of: (e) => e.parts },
  { key: 'generation', label: 'generation', of: (e) => e.generation },
  { key: 'organism', label: 'organism id', of: (e) => e.id },
];

/** Ways to bucket the list. `of` returns the heading a row belongs under. */
const GROUPS = [
  { key: 'none', label: 'no grouping', of: null },
  { key: 'generation', label: 'by generation', of: (e) => `generation ${e.generation}` },
  {
    key: 'parent',
    label: 'by parent',
    of: (e) => (e.parents[0] ? `child of ${e.parents[0]}` : 'founder'),
  },
  { key: 'shapes', label: 'by shape mix', of: (e) => e.shapeLabel },
  { key: 'parts', label: 'by part count', of: (e) => `${e.parts} parts` },
  {
    key: 'broke',
    label: 'by whether a joint failed',
    of: (e) => (e.breaks > 0 ? 'lost a joint' : 'intact'),
  },
];

/**
 * One-click views onto the same list. These are shortcuts, not the only way in:
 * every sort key and grouping above stays available underneath, and the filter
 * box composes with all of them.
 */
const PRESETS = [
  { label: 'Best', sort: 'fitness', dir: -1 },
  { label: 'Worst', sort: 'fitness', dir: 1 },
  { label: 'Fastest', sort: 'speed', dir: -1 },
  { label: 'Most upright', sort: 'upright', dir: -1 },
  { label: 'Wanderers', sort: 'wander', dir: -1 },
  { label: 'Most efficient', sort: 'economy', dir: -1 },
  { label: 'Hardest working', sort: 'actuation', dir: -1 },
  { label: 'Climbers', sort: 'elevation', dir: -1 },
  { label: 'Descenders', sort: 'elevation', dir: 1 },
  { label: 'Broke a joint', sort: 'breaks', dir: -1, filter: 'broke' },
  { label: 'Most cautious', sort: 'caution', dir: -1 },
  { label: 'Latest', sort: 'generation', dir: -1 },
];

// ---------------------------------------------------------------- sequences
//
// The list above answers "show me everything, my way". These answer the three
// questions people actually arrive with — what was the best, how did it get
// better, and where did this one come from — without having to know that the
// answer is a particular file. They are built from headers already in memory,
// so they cost nothing and need no support from the recorder.

/** The single highest-scoring recorded organism. */
export function bestInRun(entries) {
  if (!entries.length) return { items: [] };
  return { items: [entries.reduce((a, b) => (b.fitness > a.fitness ? b : a))] };
}

/** The best recorded organism of each generation, oldest first. */
export function championsByGeneration(entries) {
  const best = new Map();
  for (const e of entries) {
    const held = best.get(e.generation);
    if (!held || e.fitness > held.fitness) best.set(e.generation, e);
  }
  return { items: [...best.values()].sort((a, b) => a.generation - b.generation) };
}

/**
 * One organism's recorded ancestry, oldest first.
 *
 * Only each generation's top few and a random sample are recorded, so a chain
 * usually stops short: the moment an ancestor was not recorded, its own parents
 * are unknown too, and the trail genuinely ends. That is reported rather than
 * hidden — a lineage is watched to see what changed from one generation to the
 * next, and silently splicing across a six-generation hole would misrepresent
 * exactly the thing being looked at.
 */
export function lineageOf(entries, start) {
  const byId = new Map(entries.map((e) => [e.id, e]));
  const items = [];
  const seen = new Set();
  let cur = start;
  let missing = null;
  while (cur && !seen.has(cur.id)) {
    seen.add(cur.id);
    items.push(cur);
    const parents = (cur.parents || []).filter(Boolean);
    if (!parents.length) break; // a founder: the chain is complete
    const known = parents.find((p) => byId.has(p));
    if (known === undefined) {
      missing = parents[0];
      break;
    }
    cur = byId.get(known);
  }
  items.reverse();
  return { items, missing };
}

/** The guided sequences offered above the list. */
const TOURS = [
  {
    label: 'Best in run',
    title: 'The single highest-scoring organism that was recorded',
    build: bestInRun,
  },
  {
    label: 'Champions',
    title: 'The best of each recorded generation, oldest first — evolution as a flipbook',
    build: championsByGeneration,
  },
  {
    label: 'Lineage',
    title: "The selected organism's recorded ancestors, oldest first",
    build: null, // needs a starting organism; built at click time
  },
];

const el = (id) => document.getElementById(id);

/** Read `file` up to the start of its trajectory and parse that as JSON. */
async function readHeader(file) {
  for (const size of [HEADER_SLICE, file.size]) {
    const text = await file.slice(0, size).text();
    const cut = text.indexOf(',"trace":');
    if (cut > 0) return JSON.parse(`${text.slice(0, cut)}}`);
    if (size >= file.size) break;
  }
  // No `trace` key at all: not a replay we can draw.
  return null;
}

/** Compact description of what an organism is made of, e.g. "2 capsule, box". */
function describeShapes(genome) {
  const counts = new Map();
  for (const p of genome.parts || []) {
    const k = p.shape || 'box';
    counts.set(k, (counts.get(k) || 0) + 1);
  }
  return (
    [...counts]
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .map(([k, n]) => (n > 1 ? `${n} ${k}` : k))
      .join(', ') || '—'
  );
}

function entryFrom(header, file) {
  const m = header.metrics || {};
  const duration = m.duration || 1;
  const dx = m.displacement_x || 0;
  const effort = m.actuation || 0;
  return {
    file,
    id: header.organism_id,
    generation: header.generation,
    parents: header.parents || [0, 0],
    fitness: header.fitness ?? 0,
    m,
    speed: (m.displacement || 0) / duration,
    // How far it strayed from a straight line: a tumbler racks this up, a walker
    // barely moves it.
    wander: (m.path_length || 0) - (m.displacement || 0),
    // Distance bought per unit of motor impulse. Guarded so an organism that
    // never actuated does not sort to the top on a division by zero.
    economy: effort > 1e-6 ? dx / effort : 0,
    parts: (header.genome?.parts || []).length,
    caution: header.genome?.caution ?? 0,
    shapeLabel: describeShapes(header.genome || {}),
    breaks: m.joints_lost || 0,
    diverged: !!m.diverged,
  };
}

export function initLibrary({ onSelect }) {
  let entries = [];
  let sortKey = 'fitness';
  let dir = -1;
  let groupKey = 'none';
  let filter = '';
  let activeId = null;
  let activePreset = 'Best';

  // The active sequence, if any: { label, items, at, note }.
  let tour = null;

  const list = el('lib-list');
  const sortSel = el('lib-sort');
  const groupSel = el('lib-group');
  const filterInput = el('lib-filter');
  const presets = el('lib-presets');
  const tourBar = el('tour-bar');
  const tourPos = el('tour-pos');
  const tourNote = el('tour-note');
  const tourAdvance = el('tour-advance');

  for (const s of SORTS) sortSel.add(new Option(s.label, s.key));
  for (const g of GROUPS) groupSel.add(new Option(g.label, g.key));
  sortSel.value = sortKey;
  groupSel.value = groupKey;

  for (const p of PRESETS) {
    const b = document.createElement('button');
    b.className = 'chip';
    b.textContent = p.label;
    b.addEventListener('click', () => {
      activePreset = p.label;
      sortKey = p.sort;
      dir = p.dir;
      filter = p.filter || '';
      sortSel.value = sortKey;
      filterInput.value = filter;
      render();
    });
    presets.appendChild(b);
  }

  /** Start a sequence and play its first entry. */
  function startTour(label, built, note) {
    if (!built.items.length) {
      tour = null;
      renderTour();
      return;
    }
    tour = { label, items: built.items, at: 0, note: note || '' };
    renderTour();
    play(0);
  }

  /** Move to `i` in the current sequence and hand that replay to the viewer. */
  function play(i) {
    if (!tour) return;
    tour.at = Math.max(0, Math.min(i, tour.items.length - 1));
    const e = tour.items[tour.at];
    activeId = e.id;
    renderTour();
    render();
    onSelect(e, { fromTour: true });
  }

  function endTour() {
    tour = null;
    renderTour();
    render();
  }

  function renderTour() {
    tourBar.hidden = !tour;
    for (const b of tourPicks.children) {
      b.classList.toggle('on', !!tour && b.dataset.tour === tour.label);
    }
    if (!tour) return;
    const e = tour.items[tour.at];
    tourPos.textContent =
      `${tour.label} · ${tour.at + 1}/${tour.items.length} · gen ${e.generation}`;
    tourNote.textContent = tour.at === 0 && tour.note ? tour.note : '';
    tourNote.hidden = !tourNote.textContent;
  }

  const tourPicks = el('tour-picks');
  for (const t of TOURS) {
    const b = document.createElement('button');
    b.className = 'chip';
    b.dataset.tour = t.label;
    b.textContent = t.label;
    b.title = t.title;
    b.addEventListener('click', () => {
      if (!entries.length) return;
      if (t.label === 'Lineage') {
        // Trace whatever is on screen; failing that, the best in the run, since
        // that is the lineage anyone asking the question most likely wants.
        const seed = entries.find((e) => e.id === activeId) || bestInRun(entries).items[0];
        if (!seed) return;
        const built = lineageOf(entries, seed);
        const note = built.missing
          ? `chain ends here — ancestor ${built.missing} was not recorded`
          : 'traced back to a founder';
        startTour(t.label, built, `org ${seed.id}: ${note}`);
      } else {
        startTour(t.label, t.build(entries), '');
      }
    });
    tourPicks.appendChild(b);
  }

  el('tour-prev').addEventListener('click', () => play(tour ? tour.at - 1 : 0));
  el('tour-next').addEventListener('click', () => play(tour ? tour.at + 1 : 0));
  el('tour-exit').addEventListener('click', endTour);

  sortSel.addEventListener('change', () => {
    sortKey = sortSel.value;
    activePreset = null;
    render();
  });
  groupSel.addEventListener('change', () => {
    groupKey = groupSel.value;
    render();
  });
  filterInput.addEventListener('input', () => {
    filter = filterInput.value.trim().toLowerCase();
    activePreset = null;
    render();
  });

  function num(v, d = 2) {
    return typeof v === 'number' && isFinite(v) ? v.toFixed(d) : '—';
  }

  /**
   * Free-text match over everything a row displays, so "capsule", "690",
   * "broke" or a parent id all narrow the list without needing their own
   * control.
   */
  function matches(e) {
    if (!filter) return true;
    const hay = [
      e.id,
      `gen ${e.generation}`,
      e.shapeLabel,
      `${e.parts} parts`,
      e.parents.join(' '),
      e.breaks > 0 ? 'broke broken lost' : 'intact',
      e.diverged ? 'diverged' : '',
    ]
      .join(' ')
      .toLowerCase();
    return filter.split(/\s+/).every((term) => hay.includes(term));
  }

  function render() {
    for (const b of presets.children) b.classList.toggle('on', b.textContent === activePreset);

    const sort = SORTS.find((s) => s.key === sortKey) || SORTS[0];
    const group = GROUPS.find((g) => g.key === groupKey) || GROUPS[0];
    const rows = entries.filter(matches).sort((a, b) => {
      const d = (sort.of(a) ?? 0) - (sort.of(b) ?? 0);
      // Ties break on fitness so the order is total and stable between renders.
      return (d || (a.fitness - b.fitness)) * dir;
    });

    list.textContent = '';
    if (!entries.length) {
      list.innerHTML = '<div id="lib-empty">No run loaded.</div>';
      return;
    }
    if (!rows.length) {
      list.innerHTML = '<div id="lib-empty">Nothing matches that filter.</div>';
      return;
    }

    let heading = null;
    for (const e of rows) {
      if (group.of) {
        const h = group.of(e);
        if (h !== heading) {
          heading = h;
          const g = document.createElement('div');
          g.className = 'lib-group';
          g.textContent = h;
          list.appendChild(g);
        }
      }
      const item = document.createElement('div');
      item.className = 'lib-item' + (e.id === activeId ? ' on' : '');
      const broke = e.breaks > 0 ? ` · <span class="broke">lost ${e.breaks}</span>` : '';
      item.innerHTML =
        `<div class="lib-name">gen ${e.generation} · org ${e.id}</div>` +
        `<div class="lib-score">${num(e.fitness)}</div>` +
        `<div class="lib-meta">${num(e.m.displacement_x)} m · ${num(e.speed)} m/s · ` +
        `${num(e.m.upright_seconds, 1)} s up · ${e.parts} parts · ${e.shapeLabel}${broke}</div>`;
      item.addEventListener('click', () => {
        // Picking from the list by hand means leaving the sequence, rather than
        // leaving a stale position behind to surprise the next Next.
        tour = null;
        renderTour();
        activeId = e.id;
        render();
        onSelect(e);
      });
      list.appendChild(item);
    }
  }

  return {
    /** Note which organism is on screen, so the list can highlight it. */
    setActive(id) {
      activeId = id;
      render();
    },
    /**
     * Step to the next entry of the running sequence, if one is running and the
     * viewer is set to advance on its own. Returns whether it did, so the
     * caller can fall back to its normal end-of-replay behaviour.
     */
    autoAdvance() {
      if (!tour || !tourAdvance.checked) return false;
      if (tour.at >= tour.items.length - 1) return false;
      play(tour.at + 1);
      return true;
    },
    /**
     * Ingest a directory chosen with a directory picker. Only the manifest and
     * the replays are read; the multi-gigabyte per-organism log is deliberately
     * left alone, because only recorded organisms can actually be played.
     */
    async open(fileList, onProgress) {
      const files = [...fileList];
      const manifestFile = files.find((f) => f.name === 'manifest.json');
      const replays = files
        .filter((f) => /replays[\\/][^\\/]+\.json$/.test(f.webkitRelativePath || f.name))
        .sort((a, b) => a.name.localeCompare(b.name));

      let manifest = null;
      if (manifestFile) {
        try {
          manifest = JSON.parse(await manifestFile.text());
        } catch {
          manifest = null;
        }
      }

      if (!replays.length) {
        el('lib-title').textContent = 'No replays found';
        el('lib-sub').textContent =
          'Choose a run directory — the one holding manifest.json and replays/.';
        entries = [];
        render();
        return 0;
      }

      el('lib-title').textContent =
        manifest?.experiment_name || manifest?.experiment_id || 'run';
      // Read headers a batch at a time. Strictly sequentially, a few hundred
      // replays take long enough to feel broken; the files are independent, so
      // there is no reason to wait on each one in turn.
      const found = new Array(replays.length).fill(null);
      let next = 0;
      let done = 0;
      const worker = async () => {
        for (;;) {
          const i = next++;
          if (i >= replays.length) return;
          try {
            const header = await readHeader(replays[i]);
            if (header) found[i] = entryFrom(header, replays[i]);
          } catch {
            // A truncated or half-written replay is skipped rather than
            // aborting the whole directory: a run killed mid-write is a normal
            // thing to want to look at.
          }
          onProgress?.(++done, replays.length);
        }
      };
      await Promise.all(
        Array.from({ length: Math.min(CONCURRENCY, replays.length) }, worker),
      );
      // Kept in filename order so the initial, unsorted list reads sensibly.
      entries = found.filter(Boolean);
      const gens = new Set(entries.map((e) => e.generation));
      el('lib-sub').textContent =
        `${entries.length} replays across ${gens.size} recorded generations` +
        (manifest?.experiment_id ? ` · ${manifest.experiment_id}` : '');
      render();
      return entries.length;
    },
  };
}
