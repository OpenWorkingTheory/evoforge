//! `evo run` — evolve a population and write a run directory.

use anyhow::Result;

use evoforge::config::Config;
use evoforge::runner::{self, RunOptions};

use super::RunArgs;

pub(super) fn cmd_run(args: RunArgs) -> Result<()> {
    let mut cfg = Config::load(&args.config)?;
    if let Some(out) = args.out {
        cfg.experiment.output_dir = out;
    }
    if let Some(seed) = args.seed {
        cfg.experiment.seed = seed;
    }
    if let Some(g) = args.generations {
        cfg.evolution.generations = g;
    }
    if let Some(p) = args.population {
        cfg.evolution.population_size = p;
    }
    // Overrides go through the config before the digest is taken, exactly as the
    // ones above do, so the run directory still records what actually produced it
    // rather than the file it started from.
    if let Some(v) = args.climb_bonus {
        cfg.fitness.climb_bonus = v;
    }
    if let Some(v) = args.descent_penalty {
        cfg.fitness.descent_penalty = v;
    }
    cfg.validate()?;

    let summary = runner::run(
        &cfg,
        &RunOptions {
            threads: args.threads,
            quiet: args.quiet,
            resume: args.resume,
            force_resume: args.force_resume,
        },
    )?;

    println!();
    println!(
        "{} generations, {} organisms evaluated in {:.1}s ({:.0} organisms/s overall)",
        summary.generations_completed,
        summary.organisms_evaluated,
        summary.wall_seconds,
        summary.organisms_evaluated as f64 / summary.wall_seconds.max(1e-9),
    );
    if let Some(s) = &summary.final_stats {
        println!(
            "final generation {}: best {:.3} (organism {}), mean {:.3}, {} distinct structures",
            s.generation, s.best_fitness, s.best_id, s.mean_fitness, s.unique_structures
        );
    }
    println!("results in {}", summary.dir.display());
    Ok(())
}
