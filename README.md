# EvoForge

**A headless evolutionary artificial-life simulator written in Rust.**

EvoForge explores how complex physical structures and behaviors can emerge through evolution from relatively simple rules.

Organisms are constructed from primitive parts and joints, controlled by small neural networks, and evaluated in a simulated physical environment. Successful organisms reproduce, unsuccessful organisms are culled, and mutations introduce variation across generations.

The goal is not to build a game or a general-purpose AI framework. EvoForge is an **artificial-life laboratory**: a fast, reproducible environment for experimenting with evolution, morphology, neural control, and emergent behavior.

## Core Idea

```text
Genome
   ↓
Phenotype
   ↓
Physical Simulation
   ↓
Behavior
   ↓
Fitness
   ↓
Selection
   ↓
Crossover + Mutation
   ↓
Next Generation
```

Each organism has both a physical body and a controller. Its genome determines characteristics such as its body structure, joints, neural-network parameters, and other traits. The organism is placed in an environment, allowed to act, and its behavior determines its fitness. The fittest organisms become the parents of the next generation.

Repeat.

## Goals

* **Evolution first** — evolutionary algorithms are the primary mechanism for discovering behavior.
* **Physical embodiment** — organisms have bodies, constraints, sensors, and physical consequences.
* **Emergence** — interesting behavior should arise from simple rules rather than being explicitly programmed.
* **Performance** — maximize evolutionary progress per unit of compute.
* **Determinism** — experiments should be reproducible from their configuration and random seed.
* **Headless operation** — the simulator runs efficiently without a graphical environment.
* **Experimentation** — terrain, objectives, population parameters, morphology, and neural architectures are configurable.
* **Replayability** — interesting organisms can be recorded and visualized after simulation.

## Where This Is Going

Organisms are currently scored primarily on how far they travel. That is the *first* fitness criterion — it was chosen because it is the simplest measurement that separates a body which does something from a body which does nothing.

The direction is to make the evaluation system capable of asking harder questions — go uphill, go downhill, reach that beacon, reach as many beacons as you can — and to give organisms sensors so that what they are being asked about is something they can perceive. The progression is:

> evolved locomotion → richer evaluation → varied environments and tasks → sensors → bodies, controllers and environments under selection together

[ROADMAP.md](ROADMAP.md) sets that out in phases, each anchored to a seam that already exists in the code. Elevation metrics and the first lidar-like range sensor are now part of Phase 0; the open phases start from beacons and multi-task experiments.

## What EvoForge Is Not

EvoForge is intentionally **not**:

* A game engine or real-time game
* A general-purpose physics or AI framework
* A machine-learning or reinforcement-learning framework
* A GPU-first simulation
* A visualization project

The simulator runs for hours or days on a server without ever creating a window. Visualization is a separate concern.

## Never used Rust or a terminal before?

You do not need to know either one, and you will never have to write any Rust. You need three things:

1. **A terminal** — a window where you type a command, press Enter, and the program answers in text. On Windows: press Start, type `powershell`, press Enter. On macOS: press ⌘-Space, type `terminal`, press Enter.
2. **Rust**, the language this is written in. Install it from [rustup.rs](https://rustup.rs) and accept the defaults.
3. **This repository**, downloaded to your machine.

Then move the terminal into the downloaded folder and run:

```bash
cargo build --release
```

`cargo` is Rust's build tool and came with step 2. This turns the source into a program, and takes a few minutes the first time. There is no compiler version to choose: `rust-toolchain.toml` pins the exact one this project is tested against, and `cargo` fetches it for you.

**For parents and instructors:** the simulator makes no network connections and collects nothing. It writes only inside the project folder, under `runs/`. The optional browser viewer is the one exception — it loads its 3-D drawing library from a public CDN.

## A short glossary

| Word | What it means here |
|---|---|
| **organism** | One creature: a body of jointed blocks, plus a small brain driving those joints. |
| **genome** | The numbers describing an organism. Copy them with small random changes and you get a slightly different creature. |
| **brain** | A small fixed network turning what an organism senses into how hard it pushes each joint. It is inherited, not trained — there is no learning inside a lifetime. |
| **generation** | One whole population, evaluated and scored. The best become the parents of the next. |
| **fitness** | The single number each organism is scored on — usually how far it travelled. |

## Quick Start

Run this one first. It takes about fifteen seconds:

```bash
# Unix and macOS
./target/release/evo run experiments/beginner-demo.toml

# Windows (PowerShell)
.\target\release\evo.exe run experiments/beginner-demo.toml
```

You will get one row of table per generation. Watch `mean` and `median` rather than `best`: the best score can only ever go up, so it is the middle of the distribution rising that shows the *population* getting better rather than one lucky creature. At the end it prints where it put the results — something like `runs/beginner-demo-1789524059/`. Open that in the [viewer](#viewer) and press **Champions** to watch what evolved, oldest first.

The rest of the examples use the Unix form; on Windows substitute `.\target\release\evo.exe` as above.

```bash
# The fuller version of the same experiment: 100 generations of 100 organisms,
# under a minute on a laptop.
./target/release/evo run experiments/first-walkers.toml

# Stricter locomotion: signed +X progress and an upright bonus.
./target/release/evo run experiments/directed-walkers.toml

# The same experiment with all five part shapes enabled.
./target/release/evo run experiments/shaped-walkers.toml

# Every animal-oriented constraint at once: symmetry, muscle-limited torque,
# tendons, self-collision, rough ground, repeated trials, commanded headings.
./target/release/evo run experiments/animals.toml

# The same, on seeded fractal terrain that differs in every trial.
./target/release/evo run experiments/fractal-animals.toml

# Measure throughput and its scaling across cores.
./target/release/evo bench experiments/first-walkers.toml

# Summarise a run.
./target/release/evo inspect runs/first-walkers-<timestamp>

# Re-simulate the best recorded organism at higher recording fidelity.
./target/release/evo replay runs/first-walkers-<timestamp> --best --hz 60

# Prove the reproducibility claim: same results on 1 core and on N.
./target/release/evo verify experiments/first-walkers.toml

# Re-score a finished run under different fitness weights — no re-simulation needed.
./target/release/evo rescore runs/<run> --descent-penalty 8 --tail 10
./target/release/evo rescore runs/<run> --descent-penalty 8 --show-generation 149
```

`run` prints a table per generation and writes a self-describing run directory:

```text
runs/first-walkers-1788654317/
  manifest.json        experiment id, seed, version, config digest
  config.toml          the fully resolved configuration
  stats.csv            per-generation statistics, ready to plot
  organisms.jsonl      every organism: id, generation, parents, fitness, metrics
  genomes.jsonl        genomes of recorded organisms
  checkpoints/         full-population snapshots for resume
  replays/             recorded trajectories
```

Resume an interrupted run, or extend a finished one, with `--resume runs/<dir>`; raising `generations` is allowed, but changing anything that affects the dynamics is refused rather than silently accepted.

### Viewer

`viewer/` is a standalone browser player for recorded replays — plain HTML and JavaScript with Three.js from a CDN. No build step, no package manager.

```bash
# Windows (PowerShell)
python -m http.server 8000 --directory viewer

# Unix
python3 -m http.server 8000 --directory viewer
```

Open <http://localhost:8000>. Click **Load sample** for the checked-in two-part replay, or drag-drop any file from `runs/<run>/replays/`. **Open run folder…** takes the run directory itself and lists everything recorded — sortable by fitness, speed, actuation, joints lost, part count, generation, and more.

Three buttons answer the questions people usually arrive with, without hunting for a filename:

* **Best in run** — the highest-scoring organism that was recorded.
* **Champions** — the best of each recorded generation, oldest first. With ▶ and **auto** on, this plays as a flipbook of the whole run: the clearest single view of evolution happening.
* **Lineage** — the ancestors of whatever is on screen, oldest first. Only a generation's top few and a random sample are recorded, so a chain often stops early; the viewer says which ancestor it could not reach rather than quietly joining across the gap.

The full sortable list stays underneath all three, and clicking any row leaves the sequence and hands you back manual control.

## Sensing

An organism's controller receives proprioceptive inputs — orientation, velocity, height, joint angles, ground contact, optional joint health — that describe the organism to itself.

**The rule that governs external sensors: anything an organism knows about the world outside its own body must arrive through a sensor with a position and an orientation on that body.** World information is never handed to the controller as a free input.

A lidar-like range sensor (a fan of rays cast from a mounted part, returning distance to the terrain) is implemented and documented in [SENSOR_PLAN.md](SENSOR_PLAN.md). Try it with `experiments/sensing-climbers.toml`. Beacons, camera-like sensors, and recording of sensor readings remain open work; see [ROADMAP.md](ROADMAP.md) Phase 3.

## Documentation

| File | What it covers |
|---|---|
| [RESULTS.md](RESULTS.md) | Measured findings: terrain A/B, corpse gate, sensor arms, Baumgarte leak, climb calibration |
| [CONFIG.md](CONFIG.md) | Full experiment configuration guide with worked TOML examples |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Design rationale: purity, determinism, module map, replaceable seams |
| [ROADMAP.md](ROADMAP.md) | Phases, open questions, and the through-line |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to build, test, and add experiments or probes |
| [CHANGELOG.md](CHANGELOG.md) | Version history and artifact-format breaks |
| [SENSOR_PLAN.md](SENSOR_PLAN.md) | Range sensor design, implementation, and results |
| [FITNESS_PLAN.md](FITNESS_PLAN.md) | Elevation fitness design, calibration notes, and `fall_penalty` |
| [CURRICULUM_PLAN.md](CURRICULUM_PLAN.md) | Proposed difficulty ramp (not yet started) |

Diagnostic probes live in [examples/](examples/). They are observation instruments — `dead_organism_probe`, `leak_probe`, `refine_probe`, `terrain_probe`, `drift_probe`, `conveyor_probe`, `energy_probe`, `friction_probe`, `sensor_probe`, `terrain_samples`, `golden_probe` — each answering one question about whether what was measured was real.

## Project Status

**Phase 0 of [ROADMAP.md](ROADMAP.md) is complete: the pipeline works end to end, evolution demonstrably occurs, and it reproduces bit for bit.**

A first run of `experiments/first-walkers.toml` — 100 organisms, 100 generations, seventeen seconds of wall clock — took best fitness from 0.79 m to 4.76 m and the *median* from 0.08 m to 3.20 m, with 77 of 100 morphologies still distinct. The rising median is the part that matters.

What is implemented today:

* Tree-structured bodies of five primitive shapes (box, taper, sphere, capsule, cylinder)
* Fixed and hinged joints with limits, motors, optional passive tendons, and optional wear leading to failure
* Optional bilateral symmetry and segmental repetition; optional self-collision
* A fixed-topology feed-forward controller, evolved rather than trained
* A purpose-built impulse-based rigid-body solver with flat, sine, and seeded fractal terrain behind one analytic height function
* Four base objectives over a recorded metric set; repeated trials with per-trial start, terrain, and commanded-heading variation
* Elevation metrics: `climb_bonus`, `descent_penalty`, `cumulative_climb_bonus`, `cumulative_descent_penalty`, `fall_penalty`
* A lidar-like range sensor carried by a body part; `experiments/sensing-climbers.toml`
* `evo rescore`: re-weight a finished run without re-simulating
* Tournament selection with elitism, slot-aligned crossover and mutation, random immigration
* Bitwise determinism including hand-written transcendentals, independent of platform libm
* Multi-core evaluation whose results are independent of thread count
* Checkpoint, resume, and run extension
* Selective recording and exact re-simulation of any stored genome
* A browser replay viewer with run-browser, sorting, grouping, and terrain rendering

Open work: beacons, discrete obstacles, tasks other than travelling, beacon/camera sensors, recording sensor readings, the `HEIGHT` input audit, evolved network topology, cloud infrastructure. See [ROADMAP.md](ROADMAP.md).

A longer replicated run is still wanted before the part-count result from the terrain A/B comparison is treated as established.

## License

[MIT](LICENSE).
