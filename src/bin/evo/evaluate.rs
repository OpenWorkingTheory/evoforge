//! `evo evaluate` — score a population under a configuration without evolving it.

use anyhow::Result;

use evoforge::config::Config;
use evoforge::runner::{self, RunOptions};

use super::EvaluateArgs;

pub(super) fn cmd_evaluate(args: EvaluateArgs) -> Result<()> {
    let mut cfg = Config::load(&args.config)?;
    if let Some(out) = args.out {
        cfg.experiment.output_dir = out;
    }
    if let Some(seed) = args.seed {
        cfg.experiment.seed = seed;
    }
    cfg.validate()?;

    let summary = runner::evaluate(
        &cfg,
        &RunOptions {
            threads: args.threads,
            quiet: args.quiet,
            founders: args.founders,
            ..Default::default()
        },
    )?;

    println!();
    println!("{} organisms evaluated in {:.1}s", summary.organisms_evaluated, summary.wall_seconds);
    if let Some(s) = &summary.final_stats {
        println!(
            "best {:.3} (organism {}), mean {:.3}, median {:.3}, {} distinct structures",
            s.best_fitness, s.best_id, s.mean_fitness, s.median_fitness, s.unique_structures
        );
    }
    println!("results in {}", summary.dir.display());
    Ok(())
}
