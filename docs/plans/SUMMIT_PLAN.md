# Plan: a mountain, and the organisms that climb it

Status: **proposed, not started.** Sits under Phase 2 of [ROADMAP.md](../ROADMAP.md)
("Uphill and downhill") and picks up the two stages
[FITNESS_PLAN.md](archive/FITNESS_PLAN.md) deliberately left undone, `Objective::Climb`
and a run under it, now that there is a landscape worth running them on.

Goal, in three stages that each stand alone:

1. **A landscape that is foothills around one mountain.** Not statistically
   uniform hills: a single summit, gentle at the foot and steeper toward the top,
   with the existing fractal hills riding on it.
2. **Fitness that pays for gaining elevation on those slopes**, with the uphill
   direction random per trial so what evolves is climbing, not a heading.
3. **Fitness that pays for the altitude reached**, so that the summit, not the
   nearest knoll, is the answer.

Nothing here touches the physics solver, the genome, or the breeding loop. It is
one new terrain band, one new placement rule, two `Objective` variants, and the
measurements that decide the weights.

---

## 0. Where things stand

Most of the machinery this needs is shipped, and two findings shape the plan.

**Already built:**

- `TerrainModel::Fractal` (`src/physics/terrain.rs`) is hashed Perlin gradient
  noise with an exact analytic gradient, four bands (landscape, detail,
  modulation, terracing), domain warp, and a per-trial rigid motion of the field.
  No transcendentals; the viewer mirrors it in `viewer/terrain.js` and
  `viewer/terrain_check.mjs` checks the mirror against samples every trace
  carries.
- Elevation is already measured: `Metrics` has `start`, `end`, `peak_height`,
  `net_gain`, `net_loss`, `climb`, `descent`, `fall_distance`, and `FitnessCfg`
  has a weight for each. `evo rescore` re-weights a finished run in seconds.
- Spawn is terrain-aware: the field is moved under the organism, up to twelve
  placements are tried for one whose 0.6 m footprint is within 16° of level, and
  every ground point of every body is lifted clear of the sampled height.

**Two findings from [RESULTS.md](../RESULTS.md) that this plan is built around:**

- Under a distance objective on the shipped fractal ground, fitness correlates
  with elevation change at **−0.75** by generation 299, and 99 of 100 organisms
  end lower than they started. Distance on sloped ground pays for descent.
- `climb_bonus = 10` changed no ranking in 300 generations, because no organism
  ever ended higher than it started. The best net gain seen was 0.22 m. **A
  climb term on ground where climbing is not reachable within a trial is a
  constant**, and the plan's first job is to make climbing reachable, its second
  to measure how much before choosing a single weight.

## 1. Two decisions taken up front

**No simplex noise.** Simplex was the right tool when the problem was "seeded,
gradual, isotropic hills", and the repository already has that problem solved
with Perlin fBm: the gradual slopes come from the low-frequency band at a long
`terrain_wavelength`, and a second noise implementation would cost a second
viewer mirror and a second set of gradient tests for no shape the first cannot
make. What Perlin fBm *cannot* make, and simplex could not either, is a mountain:
noise is stationary, so it has no "here" and no highest point. That is a
deterministic envelope, not a noise property, and it is the only new geometry
this plan adds.

**A band on `FractalField`, not a fourth `TerrainModel` kind.** A new kind would
touch every `match` on the enum (fingerprint kind byte, `terrain_check`, the
corpse gate that runs across all three terrains, the viewer's `shapeGround`).
A band reuses the per-trial shift, the validation, the trace format, the
fingerprint gating discipline, and the mirror check, and is off-is-exact by
construction: `summit_height = 0.0` skips the block and every existing field
evaluates to the same bits.

## 2. Band 5: the massif

A radially symmetric mound centred on the field origin, added to the height
after everything else.

```
u        = min(r / summit_radius, 1)        r = sqrt(mx² + mz²), field metres
envelope = 1 − smootherstep(u)               1 at the summit, 0 at the foot, C²
h       += summit_height · envelope
∂h/∂mx   = −summit_height · smootherstep'(u) / summit_radius · (mx / r)
```

with `r < 1e-6` guarded to a zero gradient; `smootherstep'(0) = 0`, so the guard
changes no value, only avoids the division. The existing `smootherstep` helper
already returns `(s, ds)`.

**Where in the pipeline.** On the *unwarped* field coordinates `(mx, mz)`, added
*after* band 4, with its gradient added after the warp Jacobian and before the
final rotation. Two consequences, both intended:

- The summit is exactly at field origin. The spawn ring in §3 is measured from
  it, and the warp does not move it (warping the massif would displace the peak
  by up to `warp · wavelength`, 15 m on the shipped fractal config).
- Terracing quantises the fractal hills, not the mountain. The flanks are smooth
  slopes with the ordinary terraced hills riding on them. If a terraced mountain
  is ever wanted, that is a second knob, not a change to this one.

Max slope of the envelope is `1.875 · summit_height / summit_radius`, at half
the radius. Some starting points, with the slope the foothills add on top:

| summit_height | summit_radius | steepest flank | note |
|---|---|---|---|
| 2.0 m | 12 m | 0.31 (17°) | five body lengths for a 0.4 m organism; gentle |
| 3.0 m | 12 m | 0.47 (25°) | |
| 4.0 m | 10 m | 0.75 (37°) | probably unclimbable on legs; a ceiling, not a start |

Size the mountain to the trial, not to the eye: champions cover 6.5–7.5 m in an
8 s trial, so a summit 12 m from the ring is out of reach in one trial by design
(stage 3 lengthens the trial or founds from stage 2; see §6). Measure the real
slope distribution with `cargo run --release --example terrain_probe` once the
band exists, not from this table.

**`height_bound`** adds `summit_height`. It bounds the raycast range and the
spawn lift, so forgetting it is a real bug, not a tidy-up.

**Config**, in `EnvironmentCfg` (`src/config/sections.rs`), all `#[serde(default)]`
through the struct-level attribute:

| field | default | meaning |
|---|---|---|
| `terrain_summit_height` | `0.0` | metres; zero is off, exactly |
| `terrain_summit_radius` | `12.0` | metres from summit to foot |
| `terrain_spawn_radius` | `0.0` | metres from the summit to the trial's start; `0` means `terrain_summit_radius` (start at the foot) |

`FractalField` gains the same three fields (`summit_height`, `summit_radius`,
`spawn_radius` is *not* a field property; it stays in config and drives §3).
`phenotype::cfg_terrain` copies them across.

**Fingerprint** (`src/config/fingerprint.rs`): a `b"summit"` tag plus the three
fields, folded in only when `terrain_summit_height > 0.0`, next to the
`b"terrace"` block. Every existing digest stays where it is.

**Validation** (`src/config/validate.rs`): `summit_radius > 0` when the summit is
on; `spawn_radius ≤ summit_radius + a few wavelengths` (a ring far outside the
foot is just the fractal field, which is allowed but almost certainly a typo, so
warn rather than reject); `spawn_radius + summit_radius` well inside
`noise::FIELD_LIMIT`.

## 3. Placement: a ring around the summit

Today's per-trial shift slides the field by up to ±64 wavelengths and turns it
through a full circle. With a summit at the origin that puts almost every trial
on the featureless plain beyond the foot. When the summit is on, the shift is
drawn differently:

```
bearing  θ ~ U[0, τ)          where on the ring the organism starts
rotation φ ~ U[0, τ)          which world direction is uphill
offset   = (spawn_radius · cos θ, spawn_radius · sin θ) / wavelength
```

Retried up to `SPAWN_SEARCH_TRIES` against the same levelness test as now. The
ring is on the foot of the massif where the envelope's slope is zero, so
levelness is decided by the foothills alone and the search succeeds as often as
it does on the plain fractal config.

Rules that keep this honest:

- **Gated on `summit_height > 0`.** A config without a summit takes the existing
  branch and draws the existing numbers; the gate test is the same shape as
  `a_terrain_that_does_not_move_draws_nothing`.
- **`terrain_per_trial = false` still places on the ring**, at `θ = 0`, `φ = 0`,
  drawing nothing. Spawning on the summit is never what a summit config means.
- **Two independent angles.** The bearing chooses the piece of foothill; the
  rotation chooses which world direction is uphill. Distance-based objectives
  measure +X, so a fixed rotation would turn "climb" into "go −X". The
  controller gets no input that depends on either angle: uphill is sensed
  through `UP_Y`/`RIGHT_Y` (the body tilts on a slope) and the ground rays, both
  of which are senses under the roadmap's rule.

The `TerrainShift` struct does not change; the ring is just a different way of
choosing one.

## 4. The viewer

`viewer/terrain.js` gains the band in `terrainHeight`, value only, with
`terrain.summit_height ?? 0` so traces from before the band still draw. The
mirror check is already in place: the sixteen `TERRAIN_CHECK_POINTS` every
trace carries pass through `height_at`, so a summit trace checks the JS mirror
with no new test plumbing, and `examples/terrain_samples.rs` gets one summit
case for `node viewer/terrain_check.mjs`.

`shapeGround` sizes its sheet by kind (`ROUGH_SPAN`, `TERRACED_SPAN`). A 12 m
radius mountain needs a sheet of at least `2 · (spawn_radius + summit_radius)`
to show the summit from the ring; add a summit span, and let `finestFeature`
alone decide tessellation as it does now. The camera and the organism track are
unchanged.

## 5. Stage 2: paying for elevation gained

**Measure before weighting.** The failure recorded in [RESULTS.md](../RESULTS.md) was a weight
chosen against what was wished for rather than what was reachable. Before any
breeding under a climb term:

1. `evo evaluate experiments/summit-distance.toml --founders runs/<fractal-300>`
   scores the existing fractal champions on the new ground. Their `net_gain` and
   `end.y − start.y` distributions say what a population that has never been
   asked to climb manages when uphill is in front of it.
2. `evo run experiments/summit-distance.toml` for 30 generations, objective
   `distance`, as the control: on a massif, does the downhill bias reappear
   (organisms run *away* from the mountain), and how far up does anyone get by
   accident? If the best net gain is still ~0.2 m, the fix is `summit_height`,
   `spawn_radius` or `duration`, not the weights.

**`Objective::Climb`**: base term `m.end.y − m.start.y`, signed. Reads `Metrics`
only. Descent is negative rather than penalised separately, which is the honest
form of the descent penalty: it charges what was lost, no more, and it does not
need a distance term to avoid pricing immobility above effort, because standing
still scores zero and any uphill step scores more.

The hazard is the mirror image of the one `standing_still_does_not_beat_travelling`
guards: a population that cannot climb yet sees a flat fitness landscape at zero
where falling is the only way to move the score. Two answers, in order of
preference:

- **Keep `energy_penalty` at zero and accept the flat start.** The ring places
  every organism with the summit within a few metres; an organism that lurches
  a body length uphill scores positive, and the fractal foothills give ±0.3 m of
  local relief to stumble up. The control run in step 2 says whether accidental
  climb exists in generation 0. If it does, the landscape is not flat.
- **If it does not**, found the climb run from the distance control's population
  (`--founders`) so that locomotion exists before climbing is asked for. This is
  what `--founders` is for and costs nothing new.

Not proposed: `climb_bonus` on top of `distance`. The measured problem with that
combination is that distance pays for descent at ten metres per metre, and any
climb weight small enough not to be noise at generation 0 is small enough to be
outweighed by that.

**Gates**, in `src/fitness.rs`:

- `a_climb_objective_charges_descent_what_it_pays_ascent`.
- `a_climb_objective_does_not_reward_standing_still`: an organism that climbs
  0.3 m and gives 0.1 m back out-scores one that never moved.
- The existing bobbing gate already covers `climb`; `end.y − start.y` needs
  none, it is one subtraction with no filter to game.

## 6. Stage 3: paying for altitude reached

**`Objective::Altitude`**: base term `m.end.y`, absolute. Every trial starts on
the ring at the same envelope height, so this differs from `Climb` only by the
foothill height at the start point, which is exactly the point: an organism that
begins on a knoll has less to climb, and one that ends on a knoll below the
summit has not reached the highest ground. `end.y` rather than `peak_height` so
that touching the summit and falling off it is not the winning answer; the
existing `fall_penalty` is available if a population learns to tumble.

**The summit is out of reach in one 8 s trial by design.** Three ways to bring it
within reach, to be chosen from the stage 2 measurements:

- lengthen `simulation.duration` (a new experiment, not a resume);
- shrink `spawn_radius` below `summit_radius` so trials start partway up, where
  the slope is steeper;
- found the altitude run from the climb run's population.

**One new metric, optional:** `summit_approach`, the closest horizontal distance
to the summit over the measured window. It is the beacon closest-approach
statistic from [ROADMAP.md](../ROADMAP.md) with the mountain as the beacon, and it is honest under
the sensing rule because the mountain is sensed, not announced. It is a
*diagnostic* first (did anyone reach the top, or only the shoulder?), and a
fitness term only if `end.y` proves too coarse. If added it follows the
`fall_distance` recipe exactly: `#[serde(default)]` on `Metrics`, into
`accumulate` and `scale_metrics`, a zero-default weight, `ARTIFACT_FORMAT` bump.

## 7. Artifact format and the fingerprint

`ARTIFACT_FORMAT` 10 → 11: `FractalField` serialises three more fields
(defaulted on read, so a v10 trace is the same landscape), and `Objective` has
two more variants (no existing config names them). One bump covers stages 1–3
if they land together; if stage 3 adds `summit_approach` later, that is 12.

`fingerprint()` already folds the objective in as a byte; the two new variants
extend that match. No existing digest moves.

## 8. Experiments

Three configs, named after the independent variable, sharing everything but
`[fitness].objective`:

- `experiments/summit-distance.toml`: the control. Summit on, `objective =
  "distance"`. Copied from `experiments/transfer/fractal.toml` with the terrace
  step at zero for the first run (cliffs are a separate difficulty; add them
  back once climbing exists).
- `experiments/summit-climb.toml`: `objective = "climb"`.
- `experiments/summit-altitude.toml`: `objective = "altitude"`.

Because only `[fitness]` differs, populations move between them with
`--founders`, and `evo evaluate` gives the cross table (each population under
each objective) that says whether climbing generalises to summiting.

Seeds: pin `terrain_seed` non-zero as the transfer family does, so `--seed`
replicates move founders and trials but never the mountain.

## 9. Staging

Tests first at every step; the test names are the acceptance criteria.

1. **The band.** `FractalField` fields, `height_and_gradient`, `height_bound`.
   Tests in `src/physics/terrain/tests.rs`:
   `a_summit_of_zero_height_leaves_the_field_bitwise_unchanged`;
   `every_band_has_an_exact_gradient` extended with a summit case;
   `the_summit_is_the_highest_point_of_the_field` (origin beats a ring of
   samples at every radius);
   `the_height_bound_covers_the_summit`.
2. **Config.** Fields, defaults, fingerprint gating, validation. Tests in
   `src/config/tests.rs`: extend
   `the_fractal_knobs_are_invisible_until_the_fractal_terrain_is_chosen` with
   the three fields; `validation_rejects_a_summit_without_a_radius`.
3. **Placement.** The ring branch of `terrain_shift`. Tests in
   `src/sim/tests.rs`: `a_field_without_a_summit_places_trials_as_before`
   (shift and the next RNG draw both identical);
   `every_trial_starts_on_the_ring`;
   `uphill_is_a_different_world_direction_each_trial`;
   the corpse gate `a_dead_organism_does_not_travel` run on a summit config.
4. **Viewer.** `terrain.js` band, summit span, one case in `terrain_samples.rs`,
   `node viewer/terrain_check.mjs` green against a summit trace.
5. **Objectives.** `Climb` and `Altitude` in `Objective`, `score`, fingerprint,
   the `--objective` CLI flag if `evo rescore` exposes one. Gates from §5.
6. **Format bump**, CHANGELOG entry, [`CONFIG.md`](../CONFIG.md) "Ground Types" gains a
   subsection, ROADMAP's "Uphill and downhill" item updated in place.
7. **Runs**: the §5 measurements, then `summit-climb` at 150 generations against
   the `summit-distance` control on the same seed, then `summit-altitude` founded
   from it. Findings to [RESULTS.md](../RESULTS.md).

Full gate before handing any stage back: `cargo test --workspace --all-targets`,
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`evo verify experiments/first-walkers.toml --generations 3`, and
`cargo run --release --example reproduce_probe -- runs` (the band and the
placement both sit on the evaluation path, and every existing run directory is a
fixture that must still reproduce bit for bit).

## 10. Risks and open questions

- **`HEIGHT` leaks the objective.** The controller input `HEIGHT` is
  `root.pos.y`, absolute. [ROADMAP.md](../ROADMAP.md) already marks it as failing the sensing
  rule; under an altitude objective it hands the organism the score directly.
  It does not tell it *where* the summit is, so it is not a beacon, and changing
  it moves every golden constant. Leave it for stage 1–2; decide before stage 3
  whether the altitude run is worth a versioned change to height-above-ground.
- **The spawn search prefers flat spots, and on foothills flat spots are
  knoll tops as often as valley floors.** A trial starting on a knoll begins
  with a descent in every direction but one. This is the "placed on level
  ground, nothing climbed" finding in a new coat. The ring keeps the massif's
  own slope at zero under the organism, which is deliberate for stage 1; if the
  measurements say knolls dominate, the answer is to bias the levelness search
  toward samples whose *inward* gradient is positive, not to loosen the 16°.
- **Foothill relief versus envelope slope.** If the fractal amplitude is large
  relative to `summit_height`, the mountain is a suggestion under the hills and
  `Altitude` rewards finding the tallest hill, not the summit. Keep the
  foothills to a fraction of the summit height (0.3–0.5 m under a 2–3 m summit)
  and let `terrain_probe` confirm the origin is the global maximum over a
  sampled disc.
- **A round mountain is a round mountain.** Every bearing is the same climb up
  to the foothills. That is a feature for stage 2 (varying slopes come from the
  radius, not the bearing) and a limit for later: ridged noise scaled by the
  envelope would give real flanks and valleys. Deliberately not in this plan.
- **Trial length.** An 8 s trial and a 12 m radius means no organism summits in
  stage 2, and that is intended: the climb objective needs no summit. Stage 3
  changes exactly one of duration, spawn radius, or founders, and says which.
