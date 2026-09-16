//! `evo` — the EvoForge command line.
//!
//! Six verbs, no daemon, no state outside the run directory:
//!
//! ```text
//! evo run     experiments/first-walkers.toml   # evolve
//! evo bench   experiments/first-walkers.toml   # measure throughput
//! evo inspect runs/first-walkers-1700000000    # what happened
//! evo replay  runs/... --organism 1837         # re-simulate one organism
//! evo verify  experiments/first-walkers.toml   # prove determinism
//! evo rescore runs/... --upright-bonus 0.5     # re-weight without re-simulating
//! ```
//!
//! This file holds only the argument surface and the dispatch; each verb is
//! implemented in its own module beside it.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use evoforge::math::Real;

mod bench;
mod inspect;
mod replay;
mod rescore;
mod run;
mod verify;

#[derive(Parser)]
#[command(
    name = "evo",
    version,
    about = "EvoForge - a headless evolutionary artificial-life simulator"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a headless evolutionary experiment.
    Run(RunArgs),
    /// Measure evaluation throughput and its scaling across cores.
    Bench(BenchArgs),
    /// Summarise a completed or in-progress run.
    Inspect(InspectArgs),
    /// Re-simulate a recorded organism, optionally at higher fidelity.
    Replay(ReplayArgs),
    /// Check that an experiment reproduces exactly across thread counts.
    Verify(VerifyArgs),
    /// Re-score a finished run under different fitness weights, without
    /// re-simulating anything.
    Rescore(RescoreArgs),
}

#[derive(Args)]
struct RunArgs {
    /// Experiment configuration (TOML).
    config: PathBuf,
    /// Worker threads; 0 uses one per core.
    #[arg(long, default_value_t = 0)]
    threads: usize,
    /// Resume from the newest checkpoint in this run directory.
    #[arg(long)]
    resume: Option<PathBuf>,
    /// Override the output directory.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Override the experiment seed.
    #[arg(long)]
    seed: Option<u64>,
    /// Override the number of generations.
    #[arg(long)]
    generations: Option<u32>,
    /// Override the population size.
    #[arg(long)]
    population: Option<usize>,
    /// Override fitness climb_bonus: per metre ended above the settled start.
    #[arg(long)]
    climb_bonus: Option<Real>,
    /// Override fitness descent_penalty: per metre ended below it.
    #[arg(long)]
    descent_penalty: Option<Real>,
    /// Suppress the per-generation table.
    #[arg(long)]
    quiet: bool,
    /// Resume a checkpoint written by a different evoforge version.
    #[arg(long)]
    force_resume: bool,
}

#[derive(Args)]
struct BenchArgs {
    /// Experiment configuration to benchmark. Defaults to built-in defaults.
    config: Option<PathBuf>,
    /// Comma-separated thread counts, e.g. `1,2,4,8`. Defaults to a sweep up to
    /// the core count.
    #[arg(long, value_delimiter = ',')]
    threads: Option<Vec<usize>>,
    /// Measurements per thread count; the fastest is reported.
    #[arg(long, default_value_t = 3)]
    repeats: usize,
    /// Override the population size used for the measurement.
    #[arg(long)]
    population: Option<usize>,
    /// Price assumption for the cost model, USD per core-hour.
    #[arg(long, default_value_t = 0.01)]
    price: f64,
}

#[derive(Args)]
struct InspectArgs {
    /// Run directory.
    run: PathBuf,
    /// How many of the most recent generations to show.
    #[arg(long, default_value_t = 10)]
    tail: usize,
}

#[derive(Args)]
struct RescoreArgs {
    /// Run directory.
    run: PathBuf,
    /// How many of the most recent generations to show.
    #[arg(long, default_value_t = 10)]
    tail: usize,
    /// Per metre ended above the settled start.
    #[arg(long)]
    climb_bonus: Option<Real>,
    /// Per metre ended below it.
    #[arg(long)]
    descent_penalty: Option<Real>,
    /// Per metre of total (hysteresis-filtered) ascent.
    #[arg(long)]
    cumulative_climb_bonus: Option<Real>,
    /// Per metre of total descent.
    #[arg(long)]
    cumulative_descent_penalty: Option<Real>,
    /// Per second spent upright.
    #[arg(long)]
    upright_bonus: Option<Real>,
    /// Per unit of actuation impulse.
    #[arg(long)]
    energy_penalty: Option<Real>,
    /// Show the organisms each scoring promotes, for this generation.
    #[arg(long)]
    show_generation: Option<u32>,
}

#[derive(Args)]
struct ReplayArgs {
    /// Run directory.
    run: PathBuf,
    /// Organism id to replay.
    #[arg(long)]
    organism: Option<u64>,
    /// Replay the best organism whose genome was stored.
    #[arg(long)]
    best: bool,
    /// Recording rate for the regenerated trajectory.
    #[arg(long)]
    hz: Option<f32>,
    /// Simulate for this many seconds instead of the experiment's duration.
    #[arg(long)]
    duration: Option<f32>,
    /// Where to write the replay. Defaults to the run's `replays/` directory.
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Args)]
struct VerifyArgs {
    /// Experiment configuration (TOML).
    config: PathBuf,
    /// Generations to compare.
    #[arg(long, default_value_t = 3)]
    generations: u32,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Run(args) => run::cmd_run(args),
        Command::Bench(args) => bench::cmd_bench(args),
        Command::Inspect(args) => inspect::cmd_inspect(args),
        Command::Replay(args) => replay::cmd_replay(args),
        Command::Verify(args) => verify::cmd_verify(args),
        Command::Rescore(args) => rescore::cmd_rescore(args),
    }
}
