//! `evo verify` — prove an experiment reproduces exactly across thread counts.

use anyhow::{bail, Result};

use evoforge::config::Config;
use evoforge::evolution::{self, Population};
use evoforge::runner;

use super::VerifyArgs;

/// Prove the reproducibility claim rather than asserting it: evolve the same
/// experiment single-threaded and multi-threaded and compare every fitness.
pub(super) fn cmd_verify(args: VerifyArgs) -> Result<()> {
    let mut cfg = Config::load(&args.config)?;
    cfg.evolution.generations = args.generations;
    cfg.validate()?;

    // Founded runs take the same path `evo run --founders` does, so the claim
    // being proved covers imported populations as well as drawn ones.
    let founding = if args.founders.is_empty() {
        Population::founding(&cfg)
    } else {
        runner::import_founders(&cfg, &args.founders, true)?.0
    };

    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    println!(
        "verifying {} generations of {} organisms{}: 1 thread vs {cores}",
        args.generations,
        founding.len(),
        if args.founders.is_empty() { "" } else { " (imported founders)" }
    );

    let trace = |threads: usize| -> Result<Vec<(u64, [u64; 2], u32)>> {
        let pool = runner::build_pool(threads)?;
        let mut pop = founding.clone();
        let mut out = Vec::new();
        for _ in 0..cfg.evolution.generations {
            evolution::evaluate_population(&mut pop, &cfg, &pool);
            for i in &pop.individuals {
                // Compare the exact bit patterns; "close enough" would hide
                // precisely the kind of drift this command exists to detect.
                out.push((i.id, i.parents, i.fitness.to_bits()));
            }
            pop = evolution::next_generation(&pop, &cfg);
        }
        Ok(out)
    };

    let single = trace(1)?;
    let multi = trace(cores)?;

    if single == multi {
        println!("identical: {} organism results matched exactly", single.len());
        Ok(())
    } else {
        let first = single
            .iter()
            .zip(&multi)
            .position(|(a, b)| a != b)
            .unwrap_or(single.len().min(multi.len()));
        bail!(
            "results diverged at result {} of {} (organism {:?} vs {:?})",
            first,
            single.len(),
            single.get(first),
            multi.get(first)
        );
    }
}
