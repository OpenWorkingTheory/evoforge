# Plan: populations that move between environments

Status: **built, all seven stages.** Sits under Phase 2 of
[ROADMAP.md](ROADMAP.md) — varied environments — as the experiment that makes
the phase's point visible. The mechanism is small and finished; the five
tutorials were run and are written up in [TUTORIALS.md](TUTORIALS.md), with the
measured findings summarised in [RESULTS.md](RESULTS.md). What remains open is
replication across seeds, which is a loop over `--seed`, not a feature.

Goal: make `environment → selection pressure → differential fitness →
inherited traits → adaptation` demonstrable from the command line. Evolve a
population in A and another in B, score each in both without letting them
evolve, mix them, carry the mix into A, B or a third ground, chain environments
— every step reproducible, every organism traceable to where it came from.

## 1. What already existed

Almost everything. The design pass found one gap and built around it.

* `sim::evaluate(genome, cfg, record)` is pure and reaches the environment only
  through `cfg`. The body a genome builds does not depend on the ground; only
  the spawn drop reads terrain height. So a genome bred on flat ground can be
  dropped onto a fractal one and simulated without any change to the pipeline.
* A checkpoint already holds a whole population — every genome, `next_id`, and
  the dynamics digest — so it is the portable population artefact. It is
  written *after* breeding, which matters below.
* `Population` never reads the environment: it touches configuration only
  through the controller layout, `[evolution]`, `[mutation]`, `[body]` and
  `[brain]`.
* `Population::founding` derives generation 0 from `[seed, STREAM_FOUNDING]`
  and nothing else, so two configurations that share a seed and differ only in
  `[environment]` start from **byte-identical founders**. The cleanest possible
  control, and it was already there.
* Every organism evaluated under a configuration meets the same trial set —
  starts, headings and terrain shifts derive from `[seed, TRIAL_STREAM, trial]`,
  never from the organism. Common random numbers, already the norm.
* The gap: `Population::founding` was the **only** constructor of generation 0.
  Nothing could start a run from a population that already existed.

The resume path could not be the answer. It refuses any change to the
evolution digest, and `[environment]`, `[fitness]` and the seed are all in it —
correctly, since a resumed run must mean what its checkpoint meant. Carrying a
population into a new environment is a new run, not a continuation, and
CLAUDE.md's "no escape hatch for a changed fingerprint" stands untouched.

## 2. What was built

Two CLI additions, one new file, one probe, one family of configurations.

**`evo run <cfg> --founders <run-dir|checkpoint> [--founders …]`** founds
generation 0 from the population(s) named instead of from the seed. Repeated,
the union is the founding population. `Population::from_founders` renumbers
them `1..=n`, generation 0, parents `[0,0]`, and draws no randomness — which is
what keeps a run *without* `--founders` bit-identical to one made before the
flag existed. `tests/golden.rs` is the gate for that, and it did not move.

**`evo evaluate <cfg> --founders …`** scores a population under a configuration
without breeding it: an ordinary run directory holding exactly one generation,
every organism's record, **every** organism's genome (so the directory is a
`reproduce_probe` fixture and can itself be a `--founders` source), the
recording policy's replays, and no checkpoint. With no `--founders` it scores
the seed's own founders — the naive baseline.

**`founders.jsonl`**, one line per founder: `{id, source_run, source_id,
source_generation}`. Its presence is the marker that generation 0 was imported;
`evo inspect` says so, because the manifest's seed would otherwise suggest
generation 0 could be reconstructed from it. No `ARTIFACT_FORMAT` bump: the
record structs do not `deny_unknown_fields`, old binaries ignore the new file,
and a bump would have made every new run unreadable by 0.3.0 for a field
nothing reads.

**The import guard.** A genome is interpretable only under the controller
layout and body limits it was bred under, so an import is refused — naming the
run and the field — when `body.max_parts`, `brain.hidden`, the zero/non-zero
state of `body.joint_endurance`, `simulation.steer` or the sensor channel count
differ, when `[body]` differs at all (the shape roster is not checked by
`Genome::is_valid`), or when any founder fails `is_valid`. Nothing is clamped;
a clamped founder is a different organism. Differences in `[simulation]`,
`[mutation]`, `[fitness]` or the seed are reported, not refused: they are
legitimate things to vary.

**`examples/transfer_matrix.rs`** reads evaluate directories and prints the
population × environment table, raw and relative to each population's home.

**`experiments/transfer/{flat,rough,fractal,mating}.toml`** — the Terrain A/B
arms from RESULTS.md renamed, identical outside `[environment]`, with
`terrain_seed` pinned to the value seed 20260906 derives so the fractal arm
stays byte-identical to the runs behind the documented table while `--seed`
replicates can no longer move the ground.

## 3. Breeding

Union as founders, and the existing tournament + crossover + mutate does the
mixing in generation 1. `next_generation` already targets
`evolution.population_size` regardless of the current size, so a 200-founder
union shrinks to 100 with no new code, and nothing in the loop assumed the two
were ever equal.

That conflates selection with recombination — in environment A the A-founders
win most tournaments and much of B is purged before it recombines. Real, and a
lesson, but not a controlled cross. `mating.toml` is the controlled cross with
no new operator: `tournament_size = 1` (uniform picks), `crossover_rate = 1.0`,
`elite_count = 0`, `immigrant_rate = 0`, `population_size = 200`, one
generation. Its checkpoint is a hybrid population; `--founders` carries it on.
Two configurations instead of a `--hybridize` flag.

What a hybrid is, given `genome::crossover`: the **body of the fitter parent
with a controller blended from both**. Topology comes whole from `primary`,
sizes and joints flip per slot, weights flip per weight, sensors never
recombine. The mating environment therefore still decides who donates the body.
A first-generation hybrid is any organism whose two `parents` trace, through
`founders.jsonl`, to different `source_run`s.

## 4. Things that bite

* **The checkpoint offset.** Founders are one mutation step past the last
  *evaluated* generation and appear in no `organisms.jsonl`. Every cell of a
  transfer matrix — the home cell included — must come from `evo evaluate`,
  never from a source run's `stats.csv`. One code path for every cell.
* **Not resume.** `--founders A` under A's own configuration restarts the
  reproduction stream at generation 0. Deterministic, but a re-founding.
* **Seed.** `experiment.seed` drives the founders *and* the trials. One seed
  across a family, or both move at once.
* **Fitness scale.** Flat scores ~12 where fractal scores ~7 for reasons that
  have nothing to do with adaptation. Compare down a column (two populations in
  one environment, identical trials) or along a row against home; never raw
  across columns.
* **Extra generations.** A mix run 50 generations is 150 old against 100-old
  parents. Every arm of a comparison gets the same `--generations`, with
  `--founders A` and `--founders B` alone as controls beside the union.
* **Bottleneck.** 200 → 100 in one generation of selection. `mating.toml`
  keeps 200; `unique_structures` from generation 0 to 1 is the gauge.
* **Immigrants** reintroduce `[0,0]` lineages that look like founders; mixing
  runs set `immigrant_rate = 0`.

## 5. Stages

| stage | what | gate |
|---|---|---|
| 1 | `Population::from_founders` | field contract; union breeds down to size; golden unchanged |
| 2 | `FounderRecord`, `founders.jsonl`, `Run::importable_genomes` (checkpoint, else stored genomes) | round trip; fallback order; `Run::open` unchanged |
| 3 | `--founders` on `run`, guard, provenance | gen-0 fitness equals `sim::evaluate` of the source genomes; two sources union then shrink; refusals name the field; hybrids identifiable; 1 vs 4 threads identical |
| 4 | `evo evaluate` | one stats row, every genome stored, no checkpoint, each genome re-evaluates to its record, output chains into `--founders` |
| 5 | `transfer_matrix` probe | — |
| 6 | `inspect` provenance, `verify --founders` | CLI test |
| 7 | environment family, five tutorials run and written up | TUTORIALS.md, RESULTS.md |

Stages 1–6 added fifteen tests (262 in all), left `tests/golden.rs` byte-identical,
and passed the four CI gates at every step.
