# EvoForge — Copilot instructions

A headless evolutionary artificial-life simulator in Rust. Organisms built from
jointed primitive parts are evaluated in a purpose-built rigid-body simulator;
fitness drives selection, crossover and mutation. The `evo` CLI is the only
user-facing surface. No game engine, no ML framework, no GPU path, no rendering.

Pipeline, dependencies pointing downward only:
`genome → phenotype → physics → sim → fitness → evolution`

## Four invariants — correctness contracts, not style

1. **Evaluation is a pure function of `(genome, config)`.** `sim::evaluate` reads
   no globals, no clock, no shared state.
2. **Randomness is derived, never ambient.** Every stream comes from
   `rng::derive_seed`. Never draw from a shared generator whose position depends
   on execution order.
3. **Off is exact.** A feature disabled by setting its rate/probability/weight to
   zero must draw the identical random stream, keep the identical controller
   input count, and reproduce earlier results bit for bit. Never "almost
   disable" something with a tiny nonzero value.
4. **`fitness::score` reads `Metrics` and nothing else.** New objectives go
   through recorded metrics + `evo rescore`, never through the solver.

Also: **no `std` transcendentals in `src/`.** `f32::sin`, `cos`, `ln`, `tanh` are
not bitwise portable — use the `crate::math` versions. Arithmetic and `sqrt` are
IEEE-exact and used freely.

## Golden tests

`tests/golden.rs` holds committed numeric constants compared on Linux, Windows
and macOS. A changed constant means either a deliberate versioned change (bump
`ARTIFACT_FORMAT` in `src/record.rs`, update `CHANGELOG.md`, regenerate with the
`golden_probe` example) or a bug. **Treat it as a bug until proven otherwise.**
Never `#[ignore]` a failing test.

These standing gates must not be deleted or weakened:
`a_dead_organism_does_not_travel`, `travel_survives_refining_the_solver`,
`self_collision_is_not_a_motor`, `standing_still_does_not_beat_travelling`.

## The four gates every change must pass

```bash
cargo test --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release --bin evo -- verify experiments/first-walkers.toml --generations 3
```

Style: `max_width = 100`, `use_small_heuristics = "Max"`. Fix clippy warnings
rather than `#[allow]`ing them without a comment saying why.

## Going deeper

`CLAUDE.md` has the full architecture notes and the recipes for adding a
`Metrics` field, a config field, an experiment or a probe. `ARCHITECTURE.md`
explains why the code is shaped this way. `CONFIG.md` is the TOML guide.
