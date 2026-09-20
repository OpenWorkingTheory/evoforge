# EvoForge Tutorials — populations that move between environments

Five experiments, run in order, each a few commands. Together they make one
idea visible: **environment → selection pressure → differential fitness →
inherited traits → adaptation**. A population is evolved on flat ground and
another on broken ground; each is scored where the other evolved; the two are
bred together; the mix is carried into a third ground; a population is walked
through a sequence of worlds and brought home.

Every figure below was measured by running exactly these commands. Nothing was
predicted and then confirmed — some of it surprised us, and the surprises are
the point. Because EvoForge is deterministic, running the same commands on your
machine gives the same numbers, bit for bit.

## Before you start

Build once (`cargo build --release`; see the README if that sentence is new).
The commands use the Unix form `./target/release/evo`; on Windows write
`.\target\release\evo.exe`.

**The three grounds** live in `experiments/transfer/` and are identical in
everything but `[environment]`:

| arm | ground | what it asks of a body |
|---|---|---|
| `flat.toml` | a plane | nothing — vibrating is enough |
| `rough.toml` | a 5 cm sine ripple every 1.6 m | lift the feet, or stop buzzing |
| `fractal.toml` | seeded hills with 3 m of relief, fine detail, cliffs | a real gait, and a body that survives a drop |

All three share seed `20260906`, so all three start from the **same hundred
founders** — the only thing that differs between two populations at the end is
the selection they were under. Each organism is scored over the same three
trials as every other organism in its arm. And `terrain_seed` is pinned, so the
fractal landscape is the same one every time.

**Three rules for reading fitness.** Never compare raw scores *across*
columns: a flat-ground metre and a fractal metre are not the same currency, and
a population scoring 12 on flat and 4 on fractal may not have lost anything.
Compare *down* a column (two populations on the same ground, same trials) or
*along* a row against the population's home. Every cell comes from
`evo evaluate`, never from a run's own `stats.csv`: a run's final checkpoint is
one mutation step past its last scored generation, so scoring it fresh is the
only way to compare like with like. And one checkpoint is one sample: a
population whose median jumps from generation to generation will hand you
whichever jump the checkpoint caught, so before quoting a number, score several
checkpoints and look at the spread. Tutorial 2 shows what happens when you
don't.

**Time.** On a twelve-core laptop the flat arm takes about 1½ minutes, the
rough arm about 3½, the fractal arm about 13½. The whole sequence below is a
little under an hour of compute; tutorials 1–3 are a quarter of that.

**Two tools you will use throughout:**

```bash
# Score a population somewhere, without letting it evolve.
./target/release/evo evaluate experiments/transfer/<ground>.toml --founders runs/<population>

# Lay every evaluate directory out as a population x environment table.
cargo run --release --example transfer_matrix -- runs
```

A run directory *is* a population. `evo inspect` on anything founded from
another run says where its organisms came from.

---

## 1. Two worlds, two populations

Evolve the same founders in two worlds and watch what selection does to them.

```bash
./target/release/evo run experiments/transfer/flat.toml      # → runs/transfer-flat-<ts>
./target/release/evo run experiments/transfer/fractal.toml   # → runs/transfer-fractal-<ts>
```

Watch the `parts` and `uniq` columns as much as `best` and `median`: body size
and how many distinct body plans survive are where the two worlds pull apart
first.

**What we saw.** Identical starting points — we checked that the naive founders
scored in each world are, organism by organism, exactly generation 0 of that
world's run — and then two different stories:

| | gen 0 | gen 10 | gen 25 | gen 50 | gen 99 |
|---|---|---|---|---|---|
| flat — median fitness | 1.07 | 3.61 | 8.12 | 10.12 | **11.62** |
| flat — mean parts | 4.52 | 2.11 | 2.06 | 3.06 | 4.09 |
| flat — distinct bodies | 100 | 27 | 21 | 33 | 40 |
| fractal — median fitness | 0.80 | 1.92 | 2.25 | 3.40 | **3.84** |
| fractal — mean parts | 4.52 | 7.74 | 7.83 | 7.61 | 7.92 |
| fractal — distinct bodies | 100 | 98 | 100 | 97 | 90 |

On flat ground a two-part vibrator sweeps the population inside ten
generations — body size halves, four in five body plans vanish — and only after
generation 40 does size climb back as the buzzer is elaborated into something
bigger. On fractal ground bodies go to the eight-part maximum almost at once and
stay there, and nearly every body plan survives all hundred generations: hard
ground keeps many answers alive; easy ground finds one and runs with it.

**The concept.** Same genes, different pressure, different traits. Neither
population was told what a body should look like; the ground told them.

**What would have surprised us:** the two arms converging on the same body, or
flat ground *keeping* its diversity. Watch for either if you change the seed.

---

## 2. Swap their environments

Score each population in both worlds, plus the founders they started from.

```bash
./target/release/evo evaluate experiments/transfer/flat.toml    --founders runs/transfer-flat-<ts>
./target/release/evo evaluate experiments/transfer/fractal.toml --founders runs/transfer-flat-<ts>
./target/release/evo evaluate experiments/transfer/flat.toml    --founders runs/transfer-fractal-<ts>
./target/release/evo evaluate experiments/transfer/fractal.toml --founders runs/transfer-fractal-<ts>
./target/release/evo evaluate experiments/transfer/flat.toml        # the founders, naive
./target/release/evo evaluate experiments/transfer/fractal.toml
cargo run --release --example transfer_matrix -- runs
```

**What we saw** (best / median fitness; the probe prints exactly this):

| population | in flat | in fractal |
|---|---|---|
| naive founders | 2.32 / 1.07 | 2.07 / 0.80 |
| evolved on flat | **12.46 / 11.80** | 4.30 / 3.32 |
| evolved on fractal | 8.97 / 4.86 | **6.71 / 3.73** |

Read down each column. On flat ground the resident wins by 2.4× at the median
(11.80 against 4.86); on fractal ground by only 1.12× (3.73 against 3.32). The
flat population is a specialist: moved to broken ground it keeps 28% of its
home score. The fractal population looks like a generalist: moved to flat it
scores *more* than at home, 1.30× — not because it adapted to flat, but because
flat is easier, which is exactly why raw scores must not be compared across
columns.

**Now read the small print, because half of that turned out to be noise.**
Each cell above scores *one* checkpoint — one generation's children — and the
flat population's median swings by two or three points from one generation to
the next. Continuing both runs to 200 generations and scoring every 25th
checkpoint (see *Going deeper*, below) puts the home advantage at **1.80× on
flat and 1.57× on fractal**: nearly symmetric. The 2.4× and 1.12× were the same
noise pulling in opposite directions. What survives, and sharpens, is the
fragility: the flat population keeps 25% of its score on fractal across all
five checkpoints, and that figure *falls* as it keeps adapting at home, while
the fractal population's score improves in every world at once.

Two more things the table says. Both visitors beat the naive founders by four
to four-and-a-half times in the foreign world — locomotion as such transfers,
even when the specialised part of it does not. And the best fractal-evolved
organism scores 8.97 on flat, twice what the best flat-evolved organism manages
on fractal (4.30). The corpse gate confirms all four cells are real gaits, not
tumbling: motors off, the champions cover 0–10% of their distance.

**The concept.** Fitness is not a property of an organism; it is a property of
an organism *in an environment*. Both populations are better at home than
their visitors are; the asymmetry is in what happens abroad. Adapting to the
easy world bought a high peak and a fragility that deepens with further
adaptation; adapting to the hard world bought locomotion that keeps improving
everywhere.

**What surprised us.** Twice. First the 100-generation table, which said the
hard world produced a generalist with almost no home advantage. Then the
checkpoint sweep, which said most of that was one noisy cell. The durable
finding is the fragility of the specialist, not the modesty of the generalist —
and the method finding is that a population whose median jumps around has to
be scored across several checkpoints before one number is quoted.

### Going deeper: the same experiment at 200 generations

Whether a result depends on where you stopped is the first thing to check, and
resume makes it cheap. Continue each arm on a *copy*, so the run directories the
other tutorials use keep their generation-100 checkpoints, then score every
checkpoint the copy wrote:

```bash
cp -r runs/transfer-flat-<ts> runs/transfer-flat-200
./target/release/evo run experiments/transfer/flat.toml --resume runs/transfer-flat-200 --generations 200
for g in 100 126 151 176 200; do   # the schedule writes children, so 126 not 125
  ./target/release/evo evaluate experiments/transfer/flat.toml    --founders runs/transfer-flat-200/checkpoints/gen_000$g.json
  ./target/release/evo evaluate experiments/transfer/fractal.toml --founders runs/transfer-flat-200/checkpoints/gen_000$g.json
done
```

and the same for the fractal arm. Fifteen minutes of compute, the fractal
continuation being most of it. Median fitness by checkpoint:

| population, scored in | gen 100 | 126 | 151 | 176 | 200 | mean |
|---|---|---|---|---|---|---|
| flat-evolved, in flat | 11.80 | 11.57 | 9.42 | 11.41 | 9.30 | **10.70** |
| flat-evolved, in fractal | 3.32 | 2.61 | 2.35 | 2.64 | 2.24 | **2.63** |
| fractal-evolved, in flat | 4.86 | 6.77 | 6.24 | 5.57 | 6.27 | **5.94** |
| fractal-evolved, in fractal | 3.73 | 4.45 | 4.27 | 4.24 | 3.97 | **4.13** |

The flat population at home spans 9.30 to 11.80 across five checkpoints of a
run whose best was climbing smoothly (12.46 → 12.71) — the median is jumpy, the
champion is not. On the means: home advantage 1.80× on flat, 1.57× on fractal.
The flat population's fractal score drifts *down* with more home adaptation;
the fractal population's flat score jumps after generation 100 and holds near
6. Over the second hundred generations the flat arm's best rose +0.26 and was
done by 175; the fractal arm's rose +0.72 and was still moving at 200, which
matches the ~210-generation plateau RESULTS.md found for that ground. The
corpse gate on every 200-generation cell reads 0–13%.

---

## 3. Breed the populations

Put both populations on ground neither evolved on, look before selection acts,
then cross them under controlled conditions.

```bash
# Each alone on rough, then both together — the union before any selection.
./target/release/evo evaluate experiments/transfer/rough.toml --founders runs/transfer-flat-<ts>
./target/release/evo evaluate experiments/transfer/rough.toml --founders runs/transfer-fractal-<ts>
./target/release/evo evaluate experiments/transfer/rough.toml --founders runs/transfer-flat-<ts> --founders runs/transfer-fractal-<ts>

# One generation of pure crossover between uniformly chosen parents.
./target/release/evo run experiments/transfer/mating.toml --founders runs/transfer-flat-<ts> --founders runs/transfer-fractal-<ts>
./target/release/evo evaluate experiments/transfer/rough.toml --founders runs/transfer-mating-<ts>
```

`mating.toml` sets `tournament_size = 1`, `crossover_rate = 1.0`, no elites,
no immigrants and a population of 200, so its one bred generation is nothing
but crossover children of randomly paired parents. Its checkpoint *is* the
hybrid population; the last command scores it.

**What we saw.** The union on rough is two populations, not one: flat-born
organisms score a median of 1.49, fractal-born 4.69, and the pooled
interquartile range runs from 1.46 to 4.69 — the two modes. (Each source's
median equals its alone-evaluation exactly, because the same organisms met the
same trials. That is the common-random-numbers guarantee working.) The flat
specialists keep just **13%** of their home score on a 5 cm ripple.

Of the 200 children, 104 (52%) had one parent from each population. Scored on
rough, alongside the two controls that make the number meaningful:

| children of | n | median | best |
|---|---|---|---|
| fractal × fractal | 46 | 3.30 | 7.67 |
| flat × flat | 50 | 1.49 | 2.43 |
| **flat × fractal** | **104** | **1.47** | **5.21** |
| *their parents, for reference* | | *4.69 / 1.49* | *7.84 / 2.98* |

The fractal × fractal row is the control: one step of crossover-and-mutation
with no selection costs a fractal lineage about 30% at the median (4.69 → 3.30),
which is what unselected offspring of a complex body look like. The cross
children then lose the rest — down to the flat level, 1.47 — even though **83 of
the 104 inherited a fractal body** (the fitter parent donates the body, and on
rough that was the fractal parent four times in five). A fractal body driven by
a controller half-inherited from a flat buzzer is worth about as much as a flat
buzzer. That is outbreeding depression, and it is the controller, not the body,
that breaks.

**The concept.** Two gene pools that adapted apart become incompatible even
when each half is good: bodies and controllers co-adapt, and recombining across
the divide separates parts that only worked together.

**What would have surprised us:** hybrid vigour — cross children beating both
parents. The best cross child (5.21) beat every flat-born parent but no
fractal-born one. Try the mating on flat or fractal ground instead of rough and
see which parent donates the body then.

---

## 4. Adapt the hybrid population

Carry the mix into the third world and let selection sort it out — with the
controls that make the result mean something: each parent population alone on
the same ground for the same fifty generations.

```bash
./target/release/evo run experiments/transfer/rough.toml --generations 50 --founders runs/transfer-flat-<ts> --founders runs/transfer-fractal-<ts>
./target/release/evo run experiments/transfer/rough.toml --generations 50 --founders runs/transfer-mating-<ts>
./target/release/evo run experiments/transfer/rough.toml --generations 50 --founders runs/transfer-flat-<ts>
./target/release/evo run experiments/transfer/rough.toml --generations 50 --founders runs/transfer-fractal-<ts>
```

Single generations are noisy, so judge the end state on the median averaged
over generations 40–49. Each of these took one to three minutes.

**What we saw.**

| founded from | median, gens 40–49 | best ever | ancestry at generation 49 |
|---|---|---|---|
| flat alone | 5.22 | 8.52 | flat |
| fractal alone | 5.98 | 12.07 | fractal |
| the raw union | 8.73 | 12.87 | **98 fractal-only**, 2 immigrants |
| the mating generation | **10.63** | **13.24** | **98 mixed**, 2 immigrants |

The last column is the finding, and it comes from `founders.jsonl` plus each
organism's recorded parents, traced back through the mating run.

*The raw union purges.* Generation 1 already has only two flat-only organisms
and sixteen mixed; by generation 3, one mixed; by generation 5, **none**. Every
organism from then on descends from fractal founders alone. Selection on rough
eliminated the flat lineage — and the depressed hybrids it briefly produced —
before recombination could do anything with it. Read the union arm's 8.73
against fractal-alone's 5.98 with that in mind: by generation 5 they are the
*same gene pool* on different random paths, so the gap is not evidence that
mixing helped. It is a measurement of how much two runs of one lineage can
differ, and a reminder of why one seed proves little.

*The hybrid generation introgresses.* Mixed-ancestry organisms are 59 of 100 at
generation 1, fall to 36 by generation 3 while the pure-fractal lineage gains
(62), then reverse: 78 at generation 5 and **98 from generation 10 to the end**.
The pure-fractal lineage that was winning at generation 3 is extinct by
generation 10, beaten head to head on the same ground and the same trials by
lineages that carry flat genes. The depression measured in tutorial 3 (median
1.47 against the fractal parents' 4.69) was gone within five generations of
selection, and the arm finished with the highest median and the highest single
score of the four.

One caveat on reading ancestry: once a lineage carries both sources it can
only stay mixed, so "mixed" grows through crossover and shrinks only through
selection. The union arm shows selection shrinking it to nothing; the hybrid
arm shows it growing to everything. Both movements are real; only the second
is a competition the mixed lineage won.

**The concept.** Gene flow between diverged populations depends on *how* the
populations meet. Thrown together under selection, the less fit lineage is gone
before it can contribute. Forced through recombination first, its genes ride in
on bodies that already work, survive the first generations of poor scores, and
end up everywhere.

**What surprised us:** that the answer to "does mixing help?" was *both* — no
for the union, apparently yes for the hybrids — and that the two differed by
nothing but the order of recombination and selection. Whether the hybrid arm's
lead survives other seeds is exactly what the variations below are for.

---

## 5. A population through a sequence of environments

Take the flat population through fractal, then rough, then bring it home —
a hundred generations in each — and compare its homecoming to its first time
on flat.

```bash
./target/release/evo run experiments/transfer/fractal.toml --founders runs/transfer-flat-<ts>          # → S1
./target/release/evo run experiments/transfer/rough.toml   --founders runs/transfer-fractal-<S1 ts>   # → S2
./target/release/evo run experiments/transfer/flat.toml    --founders runs/transfer-rough-<S2 ts>     # → S3
./target/release/evo evaluate experiments/transfer/flat.toml --founders runs/transfer-flat-<S3 ts>
```

About ten minutes in all — the fractal leg ran in less than half the time the
naive fractal arm did (336 s against 809 s), because four-part bodies are
cheaper to simulate than eight-part ones, which is itself a clue to what
follows.

**What we saw.**

| leg | median gen 0 → 99 | best gen 0 → 99 | mean parts |
|---|---|---|---|
| flat → fractal | 3.32 → 3.53 | 4.30 → 8.12 | 4.0 throughout |
| → rough | 1.40 → 9.00 | 7.21 → 11.69 | 4.0 throughout |
| → flat | **11.99** → 12.59 | 12.46 → **13.86** | 4.0 throughout |

*History constrains.* On fractal ground the flat population kept its four-part
body for all hundred generations — the same ground took naive founders to eight
parts within ten — and its median barely moved, 3.32 to 3.53, finishing *below*
the 3.84 a fresh population reached from a far worse start (0.80). Arriving
adapted to something else was, at the median, worse than arriving naive. Its
best organism did better than the fresh arm's best (8.12 against 6.71); the
population as a whole was stuck.

*History preserves.* The population came back to flat after two hundred
generations elsewhere and scored a median of **11.99 at generation 0** — above
the 11.62 it had when it left. It never lost flat competence; carried unchanged
through two worlds that never selected for it, the four-part vibrating body
came home as good as it went. Then it improved to 12.59, with the highest
single score in any of these experiments, 13.86.

| generation | naive founders on flat | returning population on flat |
|---|---|---|
| 0 | 1.07 | 11.99 |
| 5 | 1.61 | 11.66 |
| 10 | 3.61 | 12.46 |
| 25 | 8.12 | 12.22 |
| 50 | 10.12 | 11.99 |
| 99 | 11.62 | 12.59 |

Like for like — both populations' final checkpoints scored fresh on flat — the
returning population's median is 11.21 against the original's 11.80, with the
better best (13.86 against 12.46). Note that 11.21 sits below the run's own
generation-99 row of 12.59: the checkpoint holds that generation's *children*,
one unselected mutation step on, and tutorial 3 measured what a step costs.
The corpse gate on the returned population reads 1% free.

**The concept.** Path dependence. What a population can become depends on what
it already is: the flat body plan was a trap on fractal ground and a treasure on
the way home. Traits that nothing selects against persist, which is why a
population can remember an environment it left two hundred generations ago.

**What surprised us.** We asked whether the population would recover flat
fitness *faster* than the naive founders had gained it. It never had to recover
anything: it arrived home ahead of where it left. The interesting failure was
on the way out — a hundred generations on fractal ground could not move a
population that had already committed to a body.

---

## Variations worth trying

* **Another seed.** `--seed` on all three arms moves the founders and the
  trials together (the ground stays put). Does the flat specialist / fractal
  generalist asymmetry survive? Three seeds is where a finding starts to
  earn the word.
* **Mate on a different ground.** `mating.toml` decides who donates the body
  by who is fitter *there*. Copy it with the flat or fractal `[environment]`.
* **A fourth world.** Copy `flat.toml`, change only the terrain fields — a
  gentler ripple, a steeper fractal — and add a column to the matrix.
* **Vary the question, not the ground.** Change `[fitness]` instead of
  `[environment]`; the import guard allows it and reports it. That is a
  different experimental axis, and worth keeping separate from this one.
