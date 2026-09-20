# EvoForge — Measured Results

This document records findings from actual runs. All figures come from the experiments and probes named; rerun them before trusting specific numbers — the probes are in [examples/](../examples) and the configurations are in [experiments/](../experiments).

For how to interpret findings versus faults see [ROADMAP.md](ROADMAP.md). For the configuration knobs that produced each experiment see [CONFIG.md](CONFIG.md).

---

## Unexpected Behaviour Is the Point

Evolution under a simple objective finds simple answers, and they are frequently not the answers anyone had in mind. An organism that discovers an unanticipated way to score well, while staying inside the rules of the simulation, has done exactly what it was asked to do. That is a **result**, not a defect.

Three things look alike from a fitness curve and are worth telling apart:

| | What happened | What it demands |
|---|---|---|
| **Simulation fault** | The score required the simulator to violate the physics it claims to implement. A body gained energy nothing supplied. | A bug fix and a standing test. Not a fitness change. |
| **Measurement fault** | The physics was right, but the metric did not measure what its name says. | Repair the instrument. Still not a fitness change, and never a penalty. |
| **Strategy** | Correct physics, honest measurement, and it scores anyway. | Nothing. It is a result. If we wanted something else, we should have asked a different question. |

Both faults have occurred here and both were fixed at the level they occurred. The self-collision conveyor — organisms crossing 26 m with their motors switched off — was a solver bug: positional correction was being added to real velocity and kept. `self_collision_is_not_a_motor` and `a_dead_organism_does_not_travel` are the gates that stop it coming back. Shedding a limb used to move the measured centre of mass for free, worth 3.34 m to one organism; the fix cancelled the discontinuity in `World::centre_of_mass`, so detaching a part is now worth exactly zero metres rather than being forbidden. "Airborne" once meant "not touching", which a body hovering a millimetre up satisfies; `AIRBORNE_CLEARANCE` made the word mean what it says.

Note what none of those did: none of them made a behaviour illegal. An organism may still fling a limb, still ride a slope down, still commit everything to one launch. Those score what they genuinely earn under the objective in force.

The diagnostic tools in [examples/](../examples) exist to make the distinction decidable rather than arguable:

| Tool | Question it answers |
|---|---|
| `dead_organism_probe` | With motors off, how far does this champion still travel? Whatever it covers dead, it did not earn. |
| `conveyor_probe` | Which property of the ground gives distance away — steepness, feature size, or number of scales? |
| `drift_probe` | Is a bigger body genuinely better here, or is it collecting more free ride per part? |
| `energy_probe` | Does a passive body ever end with more mechanical energy than it started with? |
| `terrain_probe` | Is the ground actually crossable, and how is its difficulty distributed? |
| `leak_probe` | Where does un-earned travel come from — internal forces, positional correction, or friction? |
| `refine_probe` | Which subsystem loses its travel when the solver is refined? |
| `friction_probe` | Does a resting body get the friction Coulomb says it is owed? |

They are observation instruments. Their output belongs in the description of a result, not in a penalty term.

---

## Terrain A/B: What Each Ground Selects For

Three arms of `animals.toml`, identical but for `[environment]`, same seed, 300 generations of 100. Every arm starts from 4.52 parts and a median under 1.1 m. Three depths are shown in every cell — **generation 29 / 99 / 299** — because several of these readings reverse between them:

| arm | best | mean | median | mean parts | distinct structures |
|---|---|---|---|---|---|
| flat | 9.18 / 12.45 / 12.75 | 6.65 / 7.79 / 7.58 | 8.94 / 11.62 / 10.80 | 2.12 / 4.09 / 4.12 | 21 / 40 / 54 |
| rough | 7.53 / 8.92 / 9.20 | 4.21 / 5.67 / 5.76 | 4.98 / 7.42 / 7.35 | 3.14 / 2.97 / 3.06 | 46 / 35 / 34 |
| fractal | 3.97 / 6.71 / 7.53 | 2.44 / 3.53 / 3.63 | 2.53 / 3.84 / 3.53 | 7.82 / 7.92 / 7.85 | 97 / 90 / 86 |

A single generation's median is noisy — the flat arm reads 11.37, 8.39 and 11.62 at generations 80, 90 and 99 — so read the trend rather than the cell.

**All three arms plateau, and not at the same time.** Best-fitness gain per 50-generation block:

| arm | 0–50 | 50–100 | 100–150 | 150–200 | 200–250 |
|---|---|---|---|---|---|
| flat | +8.78 | +1.35 | +0.22 | +0.04 | +0.02 |
| rough | +6.62 | +0.33 | +0.23 | +0.01 | +0.04 |
| fractal | +3.92 | +0.73 | +0.52 | +0.30 | +0.00 |

Flat and rough are finished by about generation 150; the fractal arm keeps finding improvements for roughly sixty generations longer before it stops too. Harder ground buys a longer runway, not an open-ended one, and no arm gains more than +0.03 after generation 250 — worth knowing before spending a day of CPU on a thousand generations.

Medians do *not* decay after the plateau, which single generations make it easy to believe: averaged over twenty-generation windows the flat arm reads 10.09, 11.16, 10.59, 10.00, 10.47 from generation 80 to 299. What does keep moving is diversity. Flat goes from 17 distinct structures at generation 40 to 40 at 99 and 54 at 299, long after its fitness stops improving — once selection saturates, structures drift apart without being punished for it.

Scores fall monotonically with difficulty at every depth measured: 30, 100 and 300 generations. That is the result the earlier comparison (voided by the solver bug) could not produce: on the pre-0.3.0 solver the fractal run scored 28.1 against the sine field's 12.0 — harder ground scoring more than twice as high. The median rises in every arm, so all three populations are improving as populations rather than carrying one lucky champion.

**What the three grounds actually produce**, watched in the viewer:

* **Flat** selects small machines that *vibrate* — at first. Two parts, buzzing, and that is enough for the first forty-odd generations; nothing in a flat plane plus a distance objective asks for more until the buzzer has been optimised out. By generation 100 the same arm is back to four parts and still improving, so read this as the early answer on flat ground rather than the final one.
* **Rough** selects visibly less vibration: a 13 cm ripple is enough to stop buzzing working as well as it does on glass. It also selects larger bodies than flat does — 3.14 parts against 2.12 at generation 29 — but that gap closes and then inverts, with flat at 4.09 parts against rough's 2.97 by generation 99. Body size is not a clean proxy for terrain difficulty between these two; only the fractal arm separates cleanly.
* **Fractal** selects large machines — nearly the eight-part maximum — that are not obviously good at moving themselves. Roughly half the population recorded at generation 20 moves *just enough to fall off a nearby drop* and then stops. Organism 2032 is the clearest case: 96% of its travel comes in the first half of the measured window while it descends 0.34 m, after which it spends six seconds thrashing in place — path length still growing, displacement flat, 13,337 units of actuation spent on neither. On ground with 5.6 m of relief and cliffs to 82 degrees, that is a perfectly sound reading of "travel as far as you can".

  It is not the whole population, and at generation 20 it is not winning: the other half move steadily, two of the seven recorded organisms net *climb*, and the steady movers outscore the fallers (1.67 m against 1.31 m and below). So falling reads as a cheap competing optimum that caps out low rather than as the dominant strategy.

  **How to see this, since the obvious statistic does not.** Net elevation change over a run says how far down an organism ended up, not when it earned its distance, and on these bodies it is uniformly about −0.22 m whatever the organism is doing. The signature that works is *temporal*: what share of the final displacement was reached in the first half of the window. A gait splits it roughly 50/50, as every flat and rough organism does. A fall-and-stop puts 80% or more in the first half.

None of those three is a defect. They are correct answers to the question actually being asked.

**The corpse gate passes**, which is what makes the comparison valid:

Each cell is champion alive / motors off / share:

| arm | 30 generations | 100 generations | 300 generations |
|---|---|---|---|
| flat | 6.28 m / −0.14 m / −2% | 10.75 / 0.02 / 0% | 11.14 / −0.01 / −0% |
| rough | 2.96 / 0.20 / 7% | 7.28 / −0.20 / −3% | 7.60 / −0.25 / −3% |
| fractal | 1.54 / 0.34 / 22% | 5.01 / 0.81 / 16% | 6.05 / 0.79 / 13% |

Against 97% before the split-impulse fix. The gate is the absolute figure — under 2 m in eight seconds — and every arm clears it at all three depths. The 22% share at 30 generations was inflated by a small denominator, because locomotion on that ground was only 1.54 m to begin with; the denominator grows to 5.01 m and then 6.05 m while the free distance stays near 0.8 m, so the share falls to 16% and then 13%. That is the caveat resolving itself as locomotion improves, not an arm getting worse.

**One blind spot in that probe.** It measures free distance from where the organism *starts*. An organism that spends a little actuation getting itself to a cliff edge and then falls is not doing anything a motors-off corpse can imitate, because a corpse never reaches the edge. The figures above are a lower bound on how much of the fractal score the terrain is handing over, not a full accounting.

**Part count moves in opposite directions**, and it is not free drift doing it. `examples/drift_probe.rs`, motors off, 40 random organisms per part count:

| parts | flat | rough | fractal |
|---|---|---|---|
| 2 | −0.02 m | −0.07 m | 0.09 m |
| 4 | 0.01 m | −0.04 m | 0.10 m |
| 8 | 0.04 m | 0.04 m | 0.17 m |

Drift does rise with part count on the fractal field, but going from four parts to eight buys 0.07 m against a spread of roughly 1.5 m between the population mean and the best — about 5% of what selection is working on. So the growth to 7.8 parts reads as a real finding about hard ground rather than as bodies farming the solver.

**Half of that finding did not survive a longer run.** The fractal half is solid: part count reaches 7.74 by generation 20, sits at 7.92 at generation 100, and diversity holds at 90–97 distinct structures throughout. Hard ground wants big bodies, immediately and permanently.

The flat half was an artefact of stopping at 30. Flat does not collapse to small bodies — it dips and recovers:

| generation | 0 | 10 | 40 | 50 | 70 | 99 |
|---|---|---|---|---|---|---|
| mean parts | 4.52 | 2.11 | 2.08 | 3.06 | 4.01 | 4.09 |
| distinct structures | 100 | 27 | 17 | 33 | 50 | 40 |

That is a selective sweep, not a collapse. A cheap two-part vibrator takes over by generation 10 and holds until about 45 — which is where the first version of this section stopped looking — and body size then climbs back in discrete steps while fitness keeps rising, 9.50 at generation 40 to 12.45 at generation 99. Diversity recovers with it, 17 distinct structures to 40. Throughput corroborates it independently, since bigger bodies cost more to simulate: the flat arm runs at 1.5x its generation-0 rate at generation 29 and is back to 1.05x by generation 99.

What survives is the *ordering* — fractal bodies are larger than flat or rough ones at every generation measured — not the claim that easy ground permanently selects for small ones.

**Cost, decomposed.** At equal body size (generation 0, 4.52 parts in every arm) the terrain alone costs 5.6x from flat to fractal and 2.1x from rough to fractal. The rest is endogenous: hard ground evolves bigger bodies and bigger bodies cost more to simulate, so the fractal arm got 2x slower over the run while the flat arm got 1.5x faster. End to end the arms differed 11x in wall clock. Neither trend continues: by generation 100 both arms are back near their own generation-0 rate — flat 1.05x, fractal 0.92x — because both part-count curves flatten out, and the end-to-end spread narrows to 8.2x.

| arm | generation 0 | generation 29 |
|---|---|---|
| flat | 82.2 organisms/s @ 4.52 parts | 122.7 @ 2.12 parts |
| rough | 30.6 organisms/s @ 4.52 parts | 38.9 @ 3.14 parts |
| fractal | 14.7 organisms/s @ 4.52 parts | 7.4 @ 7.82 parts |

**One seed, until it was replicated.** The longer run this section used to ask for has been done twice over, to 100 and then 300 generations, and it settled the part-count question by overturning half of it. The ordering and the corpse gate survived both depths; the "flat collapses" reading did not. Every figure above comes from `seed = 20260906`. The replication on two further seeds is the next section, and it overturns more: the flat-beats-rough ordering, the flat sweep, and the downhill bias are all seed-specific. Treat the corpse gate and the transfer shape as established, the magnitudes as one sample, and re-read any finding that rests on a single stopping point or a single seed.

---

## Replication on New Seeds: What Survives a Different Founding Population

The three arms above, now `experiments/transfer/`, rerun at 300 generations with `--seed 20260919`, plus the flat arm a third time with `--seed 20260920`. `terrain_seed` is pinned in that family, so the ground is byte-identical to the runs above; only the founders and the trial set moved. Every founder set is distinct (no generation-0 fitness value is shared between seeds), and every arm cleared the corpse gate. Run directories: `transfer-flat-1789860021`, `transfer-rough-1789860837`, `transfer-fractal-1789861508`, `transfer-flat-1789864167`.

```bash
./target/release/evo run experiments/transfer/flat.toml    --seed 20260919 --generations 300
./target/release/evo run experiments/transfer/rough.toml   --seed 20260919 --generations 300
./target/release/evo run experiments/transfer/fractal.toml --seed 20260919 --generations 300
./target/release/evo run experiments/transfer/flat.toml    --seed 20260920 --generations 300
```

Each cell is **generation 29 / 99 / 299**, with the seed-20260906 figure from the table above in italics beneath:

| arm | best | median | mean parts | distinct structures | corpse gate at 299 |
|---|---|---|---|---|---|
| flat, seed 20260919 | 4.01 / 5.29 / 6.48 | 1.98 / 2.24 / 1.95 | 7.49 / 6.09 / 5.98 | 97 / 79 / 81 | 4.87 m alive, 0.00 m dead, 0% |
| *flat, 20260906* | *9.18 / 12.45 / 12.75* | *8.94 / 11.62 / 10.80* | *2.12 / 4.09 / 4.12* | *21 / 40 / 54* | *0%* |
| flat, seed 20260920 | 5.83 / 9.26 / 9.83 | 2.08 / 7.77 / 5.87 | 3.99 / 4.06 / 3.94 | 66 / 38 / 42 | 8.24 / 0.05 / 1% |
| rough, seed 20260919 | 3.95 / 7.66 / 10.40 | 2.55 / 3.37 / 5.66 | 4.11 / 5.27 / 5.97 | 64 / 78 / 79 | 8.80 / 0.44 / 5% |
| *rough, 20260906* | *7.53 / 8.92 / 9.20* | *4.98 / 7.42 / 7.35* | *3.14 / 2.97 / 3.06* | *46 / 35 / 34* | *−3%* |
| fractal, seed 20260919 | 2.86 / 3.32 / 4.09 | 2.13 / 2.61 / 2.44 | 7.92 / 7.64 / 7.02 | 92 / 93 / 86 | 2.40 / 0.60 / 25% |
| *fractal, 20260906* | *3.97 / 6.71 / 7.53* | *2.53 / 3.84 / 3.53* | *7.82 / 7.92 / 7.85* | *97 / 90 / 86* | *13%* |

Best-fitness gain per 50-generation block, and the last generation at which the best moved by more than 0.05:

| arm | 0–50 | 50–100 | 100–150 | 150–200 | 200–250 | 250–299 | last gain |
|---|---|---|---|---|---|---|---|
| flat, 20260919 | +2.09 | +0.86 | +1.07 | +0.11 | +0.00 | +0.01 | 151 |
| flat, 20260920 | +5.22 | +1.63 | +0.10 | +0.12 | +0.23 | +0.12 | 235 |
| rough, 20260919 | +2.22 | +3.34 | +1.17 | +0.97 | +0.59 | +0.01 | 232 |
| fractal, 20260919 | +1.12 | +0.14 | +0.43 | +0.12 | +0.08 | +0.15 | 285 |

**The flat arm on seed 20260919 never learned to walk.** Median heading progress at generation 299 is 0.48 m; the 1.95 median fitness is mostly the upright bonus, which pays 1.6 for standing still for eight seconds. The population went to eight-part bodies by generation 40 — the opposite of the two-part sweep on seed 20260906 — and held 80–97 distinct structures throughout, because nothing fit enough to sweep ever appeared. Its champion is real (4.87 m, 0% free) but barely heritable: at generation 299 only 3 of 100 organisms score above 6 and 30 score below 1, against 59 and 10 on the reference seed. Three checks locate the failure:

| check | result |
|---|---|
| rough-evolved population of the same seed, scored on flat | best 9.01, median 2.81 — above the flat arm's own 6.48 / 1.98 after 300 generations |
| seed-20260906 flat population, scored under seed-20260919 trials | best 11.12, median 9.32 — the trial set is not harder |
| seed-20260919 flat population, scored under seed-20260906 trials | best 6.73, median 2.04 — the deficit travels with the population |

So flat ground is not a ceiling on this seed; it is a search that failed from these founders. Seed 20260920 found a gait (median 7.77 at generation 99) and reached 9.83, still short of 12.75. Three seeds on the same flat plane: 12.75, 9.83, 6.48.

**Rough beats flat on seed 20260919** — 10.40 against 6.48, and the rough arm was still improving at generation 232 where the reference stopped near 150. The "scores fall monotonically with difficulty" reading from the table above therefore does not hold in general; what holds across both seeds is that the fractal arm scores lowest and improves longest.

**The fractal arm is not the downhill population it was.** On seed 20260906 every organism ended lower than it started by generation 149 and fitness correlated with elevation change at −0.60, deepening to −0.75. On seed 20260919 the correlation is **+0.82** at generation 299, heading progress alone correlates with elevation change at +0.81, only 30 of 100 end lower, and 29 rather than 73 of 100 spend less than seven seconds upright. This population stays on its feet and travels a median of 0.95 m against the reference's 7.22 m. The entrenched downhill bias was a property of one lineage, not of the ground: distance on sloped ground *can* pay for descent, but it takes a population that has found a gait first.

**What holds on both seeds.** The corpse gate, on every arm at every depth. Fractal bodies larger than rough bodies at every generation, and the fractal arm keeping the most distinct structures. The fractal population improving longest. And the shape of the transfer matrix — every cell a fresh `evo evaluate` under seed 20260919, median fitness:

| evolved on | in flat | in fractal | in rough |
|---|---|---|---|
| naive founders | 1.31 | 0.88 | 1.12 |
| flat | **1.98** | 0.58 | 1.21 |
| fractal | 2.94 | **2.47** | 2.93 |
| rough | 2.81 | 1.33 | **3.13** |

The fractal population scores higher abroad than at home (1.19× on flat, 1.18× on rough), as it did on seed 20260906; the flat population keeps 30% of its home median on fractal, against 25% before. The rough population loses most in the one-mutation checkpoint step (5.66 in its last scored generation, 3.13 when its checkpoint is scored fresh), where the flat and fractal arms lose nothing — the rough gait on this seed is the fragile one under mutation.

**What does not hold.** Every magnitude. The flat sweep to two parts, and with it "easy ground selects small bodies" (seed 20260920 held four parts throughout; seed 20260919 went to eight). Flat above rough. The plateau generations, which land sixty to eighty generations later here. The downhill bias on fractal ground.

**Read as a method finding.** Two seeds disagree by a factor of two on the flat plane and invert the flat/rough ordering; a third splits the difference. The transfer tutorials' "three seeds is where a finding starts to earn the word" is the right bar, and by it the only earned findings in this document are the corpse gate, the fractal population's generality and the flat population's fragility. Everything stated as a number should be read as one sample until its spread across seeds is known.

---

## What a Long Run on the Fractal Field Actually Does

The fractal arm was extended to generation 150 to find out whether it plateaus, and later to 300 to find out where. It does not plateau by 150, and the answer changes what the numbers above appear to say.

| | gen 29 | gen 149 | gen 299 |
|---|---|---|---|
| best fitness | 3.97 | **7.24** | **7.53** |
| median fitness | 2.53 | 4.14 | 3.53 |
| median travel, recorded organisms | 1.31 m | **6.58 m** | **7.22 m** |
| fall-and-stop share of recorded organisms | 3 of 7 | **0–1 of 7** | **0 of 7** |
| median temporal split | 51% | 45% | 48% |
| distance covered dead (corpse gate) | 22% | 13% | 13% |

**The crude strategy dissolved on its own.** The fall-and-stop organisms of generation 20 are essentially gone by generation 120, without any intervention: the median temporal split settles at 45%, which is what a gait looks like, and median travel among recorded organisms rises eighteenfold to 6.58 m — level with what *flat* ground produced. Best fitness gains per 30-generation block run +2.21, +0.27, +0.36, +0.43, so progress is decelerating but had not stopped at 150.

**It stops at about generation 210.** Carrying the same run to 300 continues that sequence +0.18, +0.12, +0.00, +0.00: the last improvement of any size lands around generation 210, and the arm holds 7.53 from there to 300. Everything the gate measures holds across those extra 150 generations — no fall-and-stop organisms among the recorded seven, a temporal split of 48%, and a corpse-gate share steady at 13% — so the population is not decaying, it is finished. Flat and rough stop earlier still, around 150. The fractal field buys about sixty extra generations of progress, not an open-ended supply.

**A subtler bias entrenched instead, and kept deepening.** At generation 149 every organism in the population ends lower than it started — 100 of 100, spanning −0.010 to −0.725 m — and fitness correlates with elevation change at **−0.60**. At generation 29 that correlation was −0.15; by generation 299 it is **−0.75**, with 99 of 100 organisms still ending lower than they started. The bias goes on strengthening for ninety generations after best fitness has stopped moving, which is the clearest sign available that it is the objective being satisfied rather than the search still working. So while the obvious downhill strategy was disappearing, selection under a pure distance objective was quietly getting *better* at travelling downhill: champions now cover about ten metres of ground per metre of height they give up, a ratio stable since generation 40.

That is a specification finding rather than an optimisation one. Distance on sloped ground pays for descent, and 150 generations is long enough for that to become the population's defining characteristic. It is what [FITNESS_PLAN.md](plans/archive/FITNESS_PLAN.md) exists to address — not by penalising the behaviour, but by asking a question that elevation is part of the answer to.

---

## What 300 Generations with a Sensor Actually Showed

Three arms, same seed and terrain, 300 generations each, differing only where stated. Travel is `heading_progress`, the distance an organism was actually asked to cover.

| arm | controller weights | median travel | best travel | distinct structures |
|---|---|---|---|---|
| blind | 632 | 1.63 m | 2.90 m | 49 |
| 1 ray | 728 | 3.47 m | 3.59 m | 19 |
| 1 ray, `immigrant_rate` 0.12 | 728 | **3.87 m** | **4.04 m** | 26 |

**Sensing pays, but only if it is cheap.** An earlier three-ray arm cost 920 weights — 46% more than blind — and lost to the blind control on every measure over 150 generations. One ray costs 15% more and more than doubles median travel. The information was never the problem; the search space was.

**Premature convergence was the binding constraint, not compute.** The 1-ray arm reached 5.884 by generation 49 and 6.015 by generation 299 — nothing in 250 generations, with 12 distinct structures left out of 100. Raising `immigrant_rate` from 0.02 to 0.12 kept 26–34 distinct, was still improving at generation 300, and finished 10% higher. Both settings are now the shipped defaults in `experiments/sensing-climbers.toml`.

**And a caution about reading fitness rather than behaviour.** Under the objective these arms actually ran (`climb_bonus = 10`), the *blind* arm scores highest — because its champion gained 0.22 m of elevation against the sensing arm's 0.09 m, and at weight 10 that 0.13 m outweighs a 0.7 m travel advantage. Re-scoring the same populations tells the real story:

| weighting | blind | 1 ray | 1 ray + diversity |
|---|---|---|---|
| climb 10, descent 8 (as run) | **5.105** | 4.415 | 5.006 |
| climb 2, descent 1 | 3.339 | 3.754 | **4.217** |
| pure travel | 2.897 | 3.588 | **4.035** |

`climb_bonus = 10` was set so half a metre of climb would be worth a whole run. Nothing climbs half a metre — the best organism in 300 generations managed 0.22 m — so the weight is an order of magnitude above what it was calibrated against, and it turns noise in elevation into the dominant term. Weights want calibrating to what is *achievable*, which `evo rescore` will tell you from a finished run in seconds.

---

## Positional Correction and Why the Baumgarte Default Is Low

`simulation.baumgarte` controls how much of a contact's penetration is corrected per step. It defaults to **0.05**, which is low, and the reason is worth knowing before raising it.

Correction is applied at the contact point, which is offset from the body's centre of mass, so it induces rotation as well as separation — and integrating that rotation moves the body. With few solver iterations contacts stay deeply penetrated, the correction stays large, and an organism that arranges to penetrate the ground rhythmically converts the correction into travel. It looks exactly like vibration-driven locomotion in a replay.

Measured on evolved champions at `baumgarte = 0.2`: they covered 2.72 m, and refining the solver from 12 iterations to 96 removed **94%** of it. At 0.05 they cover 0.36 m and refinement removes almost nothing. Raising `solver_iterations` fixes it too — 48 iterations costs 1.7x and 96 costs 2.8x — where lowering `baumgarte` costs 1.08x.

The general test is `travel_survives_refining_the_solver`: distance that exists only at a coarse solve is the integrator propelling the organism. Runs made before this default changed record `baumgarte = 0.2` in their own `config.toml`, so the setting travels with them and they still resume — but their distances should be read as upper bounds, and they no longer *re-evaluate* to the numbers they recorded. The split-impulse fix that landed in 0.3.0 is solver code rather than configuration, so nothing in an old run directory can carry it; `reproduce_probe` scores every pre-0.3.0 run in the archive as differing, and differing downwards. `examples/leak_probe.rs` will say how much of any given champion was real.

---

## Climb-Weight Calibration: What Happened When Weights Were Not Calibrated

`climb_bonus = 10` was chosen so half a metre of climb would be worth a full run of distance. On the shipped fractal terrain, in 300 generations, not one organism ever ended higher than it started. So at that setting `climb_bonus` changes no score and no ranking — it is an order of magnitude above what is achievable and converts noise in elevation into the dominant fitness term.

The lesson: **calibrate weights against what is actually reachable**, not against what you wish were reachable. `evo rescore` run on a finished population takes seconds and shows the distribution of net elevation across every recorded organism. If the best net gain in 150 generations is 0.22 m, a weight of 10 per metre is noise amplification, not incentive.

---

## The Standing-Still Hazard and `fall_penalty`

Any `descent_penalty` has an unavoidable side effect: an organism that never moves loses no elevation, and unlike `energy_penalty` it is not even charged for standing there. If `descent_penalty` outweighs what distance pays, the best strategy is to do nothing.

Measured over 150 generations at `descent_penalty = 8`: a population converged on rising slightly while travelling a median of **0.20 m**, against **1.57 m** for an arm under gentler scoring. It had found that standing still is the cheapest way to not descend. `standing_still_does_not_beat_travelling` is the standing gate.

`fall_penalty` is the targeted replacement. It charges only for height given away *while no attached part is touching the ground* — the part of a descent the organism did not choose. Walking down a slope keeps contact and costs nothing; stepping off a terrace does not. That makes "do not fall" expressible without also making "do not go downhill" expressible. `falling_is_charged_where_walking_downhill_is_not` and `fall_penalty_does_not_reward_standing_still` are the gates.

See [FITNESS_PLAN.md](plans/archive/FITNESS_PLAN.md) for the full calibration history.

---

## Cross-Environment Transfer

The three Terrain A/B arms, now `experiments/transfer/`, evolved from the same hundred founders for 100 generations, then scored in each other's worlds with `evo evaluate` and mixed with `--founders`. [TUTORIALS.md](TUTORIALS.md) is the procedure and the full account; this is what was measured. One seed, `20260906`, throughout.

**Specialist and generalist, not two specialists.** Median fitness by population (rows) and ground (columns), every cell a fresh evaluation of the same organisms over the same trials:

| evolved on | in flat | in fractal | in rough |
|---|---|---|---|
| naive founders | 1.07 | 0.80 | — |
| flat | **11.80** | 3.32 | 1.49 |
| fractal | 4.86 | **3.73** | 4.69 |

At this one checkpoint the flat resident beats the fractal visitor 2.43× on its own ground and the fractal resident beats the flat visitor only 1.12× — which read as a specialist and a generalist. **Most of that asymmetry was noise.** A cell scores one checkpoint, one generation's children, and the flat population's median swings by two to three points between consecutive checkpoints. Continuing both arms to 200 generations on copies and scoring every 25th checkpoint:

| median, scored in | gen 100 | 126 | 151 | 176 | 200 | mean |
|---|---|---|---|---|---|---|
| flat-evolved, in flat | 11.80 | 11.57 | 9.42 | 11.41 | 9.30 | 10.70 |
| flat-evolved, in fractal | 3.32 | 2.61 | 2.35 | 2.64 | 2.24 | 2.63 |
| fractal-evolved, in flat | 4.86 | 6.77 | 6.24 | 5.57 | 6.27 | 5.94 |
| fractal-evolved, in fractal | 3.73 | 4.45 | 4.27 | 4.24 | 3.97 | 4.13 |

On the means the home advantage is **1.80× on flat and 1.57× on fractal** — nearly symmetric; the single-checkpoint figures were the same noise pulling opposite ways. What holds across all five checkpoints is the fragility of the flat population — it keeps 25% of its home median on fractal and 13–16% on rough, and the fractal figure *falls* as it keeps adapting at home — and the breadth of the fractal population, whose score rises in every world at once (its flat score, 1.44× its home median, is flat being easier, not adaptation to flat). Corpse gate on every cell at both depths: 0–13% free.

**The method finding matters as much as the result.** A population whose median jumps generation to generation cannot be characterised by one `evo evaluate` of one checkpoint; the run's own best fitness climbed smoothly (12.46 → 12.71) while its median cells ranged 9.30–11.80. Periodic checkpoints make the remedy free: score several and report the spread. The 100-generation tutorial figures are kept above because they are what a student will see first, and learning why they mislead is the lesson.

**Outbreeding depression, isolated.** One generation of pure crossover between uniformly paired parents (`transfer/mating.toml`, 200 children, 104 with a parent from each population), scored on rough:

| children of | median | best |
|---|---|---|
| fractal × fractal | 3.30 | 7.67 |
| flat × flat | 1.49 | 2.43 |
| flat × fractal | **1.47** | 5.21 |
| *parents* | *4.69 / 1.49* | |

The within-population row is the control: one unselected step of crossover-and-mutation costs a fractal lineage ~30% (4.69 → 3.30). Crossing populations costs the rest, down to the flat level — although 83 of the 104 cross children inherited a fractal body, the fitter parent's. It is the controller that breaks.

**Whether the genes persist depends on how the populations meet.** Fifty generations on rough, ancestry traced through `founders.jsonl` and recorded parents:

| founded from | median gens 40–49 | best | ancestry at generation 49 |
|---|---|---|---|
| flat alone | 5.22 | 8.52 | flat |
| fractal alone | 5.98 | 12.07 | fractal |
| raw union | 8.73 | 12.87 | 98 fractal-only |
| mating generation | **10.63** | **13.24** | 98 mixed |

Thrown together, flat ancestry is extinct by generation 5 and the union arm is thereafter the fractal lineage on another random path — so its lead over fractal-alone measures run-to-run variance, not mixing. Passed through controlled crossover first, mixed lineages fall to 36% at generation 3, then overtake the pure-fractal lineage that had been gaining and are 98% of every generation from 10 on. The depression above was gone within five generations of selection.

**Path dependence.** The flat population, carried flat → fractal → rough → flat for 100 generations each, kept its four-part body throughout. On fractal its median moved 3.32 → 3.53, finishing below the 3.84 that naive founders reached from 0.80: arriving adapted to the wrong thing was worse than arriving naive. Returning to flat it scored **11.99 at generation 0** — above the 11.62 it left with — and finished at 12.59 with the highest single score measured anywhere here, 13.86. It never lost what nothing had selected against.

**Read with the caveats it earns.** One seed; the run-to-run spread the union arm exposed (8.73 against 5.98 for one lineage) is the scale of noise a single seed carries, and the hybrid arm's lead should be replicated before it is called a result. Every comparison cell comes from `evo evaluate`, because a checkpoint holds bred, unscored children: the returned population's checkpoint scores 11.21 on flat where its last scored generation read 12.59, which is one mutation step's cost and not a discrepancy.

---

## Proving a Large Refactor Changed Nothing

A refactor that splits 7,000 lines has no business changing behaviour, and saying so is not the same as knowing it. The golden tests cover four generations of sixteen organisms on flat ground — sixty-four evaluations, one frozen config — and would not notice a terrain band quietly dropped on the way out of a 3,000-line file.

The check that does notice: build the commit *before* the refactor, force both builds onto the same compiler so the toolchain is not a second variable, run identical configs through both binaries, and diff the artefacts. Evaluation is a pure function of `(genome, config)` and every stream is derived, so bit-exactness is not a hope — anything else is a regression.

Three arms, chosen for coverage rather than depth. `sensing-climbers.toml` earns its place by driving the fractal field with all four bands *including terracing* and casting sensor rays at it, which is most of `physics/terrain.rs` in one config:

| arm | generations | `organisms.jsonl` | checkpoint population | replay traces |
|---|---|---|---|---|
| flat | 30 | 3000 records, 1,777,641 B — identical | ~900 KB identical | — |
| rough | 30 | 3000 records — identical | ~940 KB identical | — |
| sensing-climbers | 6 | 600 records — identical | ~1,088 KB identical | 7 / 7 identical |

`stats.csv` matched on every deterministic column, and the resume digest matched on every arm. Across roughly 6,600 organisms the only field that differed anywhere was `experiment_id`, which embeds the run timestamp.

**The corpse gate reproduced the Terrain A/B table exactly** — 6.28 m alive against −0.14 m dead on flat (−2%), 2.96 m against 0.20 m on rough (7%). Those numbers were recorded on Rust 1.82 and `toml` 0.8; they came back to the last digit on a split solver, a split config module, Rust 1.98.1, and a `toml` major-version migration that relocked six packages.

**One trap worth knowing before you run this.** The `config_digest` in `manifest.json` will differ between two such runs, and it is not a regression: `experiment.output_dir` is part of the *bookkeeping* fingerprint, so giving each build its own `--out` changes it by construction. The digest that gates resume is the one stored in the checkpoint, which excludes bookkeeping. Compare that one.

**What this does not establish.** The fractal corpse gate is untested here. Six generations leaves heading progress at 0.04 m, and a share computed against that denominator is arithmetic rather than evidence — the same small-denominator caveat the Terrain A/B section records for its own 22% figure. Reaching the documented 1.54 m needs a full 30-generation fractal arm, roughly twelve minutes on its own.

The general lesson outlived the refactor: this project already carries a stronger regression test than its test suite. Any recorded run is a fixture, because its `config.toml` is fully resolved and evaluation is pure, so the numbers it recorded *are* a baseline — no second build required to hold them up against.

Run over the whole archive — 34 runs, 3,737 stored genomes — it takes 79 seconds and splits cleanly on the version that recorded each run:

| recorded by | runs exact | organisms | runs differing | organisms |
|---|---|---|---|---|
| evoforge 0.3.0 | 13 | 1,398 | 0 | 0 |
| evoforge 0.2.0 | 0 | 0 | 18 | 2,264 |
| evoforge 0.1.0 | 0 | 0 | 1 | 40 |

The split is on the version, not the artefact format, and the archive contains its own control: two `ab-rough` runs, both format 6, both rough ground, recorded an hour apart either side of the 0.3.0 solver fix. The 0.2.0 one differs; the 0.3.0 one reproduces all 56. Pre-0.3.0 scores come back *lower* than recorded, which is the split-impulse fix taking away travel the old solver was giving out — the same effect the Terrain A/B section describes. Those runs are not regressions; they are a record of a bug that was fixed, and their fitness figures should not be compared against anything current.

`examples/reproduce_probe.rs` is that idea made runnable. It re-evaluates every stored genome in a run and compares against the fitness and metrics the run recorded, checking only the fields that record actually claims, so a run written under an older `ARTIFACT_FORMAT` is not blamed for metrics added since. Pointed at the whole archive it covers the evaluation path across every terrain, body plan and objective ever run, and exits non-zero on a mismatch.
