//! `evo rescore` — re-weight a finished run without re-simulating anything.

use anyhow::{bail, Context, Result};

use evoforge::fitness::{self, Metrics};
use evoforge::math::Real;
use evoforge::record::{self, Run};

use super::RescoreArgs;

/// One organism as `organisms.jsonl` records it.
#[derive(serde::Deserialize)]
struct OrganismRecord {
    id: u64,
    generation: u32,
    fitness: Real,
    metrics: Metrics,
}

/// Re-score a finished run under different fitness weights.
///
/// This works at all because `fitness::score` is a pure function of
/// `(FitnessCfg, &Metrics)` and full metrics are written for every organism, so
/// asking "what would this population have looked like under a different
/// question" costs a file read rather than a re-simulation. With no weights
/// overridden it is a round trip, and must reproduce the recorded fitness
/// exactly — which is what makes the rest of its output trustworthy.
pub(super) fn cmd_rescore(args: RescoreArgs) -> Result<()> {
    let run =
        Run::open(&args.run).with_context(|| format!("opening run {}", args.run.display()))?;
    let cfg = run.config()?;

    let mut new = cfg.fitness.clone();
    let mut changed = Vec::new();
    let mut set = |name: &str, slot: &mut Real, v: Option<Real>| {
        if let Some(v) = v {
            if *slot != v {
                changed.push(format!("{name} {slot} -> {v}"));
            }
            *slot = v;
        }
    };
    set("climb_bonus", &mut new.climb_bonus, args.climb_bonus);
    set("descent_penalty", &mut new.descent_penalty, args.descent_penalty);
    set("cumulative_climb_bonus", &mut new.cumulative_climb_bonus, args.cumulative_climb_bonus);
    set(
        "cumulative_descent_penalty",
        &mut new.cumulative_descent_penalty,
        args.cumulative_descent_penalty,
    );
    set("upright_bonus", &mut new.upright_bonus, args.upright_bonus);
    set("energy_penalty", &mut new.energy_penalty, args.energy_penalty);

    let path = args.run.join(record::ORGANISMS_FILE);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    // A kill mid-append leaves a truncated final line, as everywhere else that
    // reads a `.jsonl` here.
    let mut records: Vec<OrganismRecord> =
        text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    if records.is_empty() {
        bail!("{} holds no readable organism records", path.display());
    }

    // Runs that predate the elevation metrics stored `start` and `end` but not
    // the derived pair. Net elevation is recoverable from them, so those runs can
    // still be re-scored on it — approximately, because the stored endpoints are
    // already averaged across trials and the clamp is not linear. Anything with
    // one trial, or that never crosses zero, is exact.
    let mut backfilled = 0usize;
    for r in &mut records {
        if r.metrics.net_gain == 0.0 && r.metrics.net_loss == 0.0 {
            let net = r.metrics.end.y - r.metrics.start.y;
            if net.abs() > 1e-6 {
                r.metrics.net_gain = net.max(0.0);
                r.metrics.net_loss = (-net).max(0.0);
                backfilled += 1;
            }
        }
    }

    // The round trip. Under the run's own weights, re-scoring has to give back
    // exactly what the simulator wrote, or nothing else printed here means
    // anything.
    let mut worst_drift = 0.0f32;
    for r in &records {
        let drift = (fitness::score(&cfg.fitness, &r.metrics) - r.fitness).abs();
        worst_drift = worst_drift.max(drift);
    }

    println!("run           {}", run.manifest.experiment_id);
    println!("objective     {:?}", cfg.fitness.objective);
    println!("organisms     {}", records.len());
    if backfilled > 0 {
        println!(
            "backfilled    {backfilled} records' net elevation from stored start/end \
             (pre-dates the metric; averaged across trials, so approximate)"
        );
    }
    println!(
        "round trip    worst |rescored - recorded| = {worst_drift:.3e}{}",
        if worst_drift > 1e-3 { "   *** MISMATCH ***" } else { "" }
    );
    if changed.is_empty() {
        println!("\nno weights overridden; nothing to compare");
        return Ok(());
    }
    println!("changed       {}", changed.join(", "));
    println!();

    let mut generations: Vec<u32> = records.iter().map(|r| r.generation).collect();
    generations.sort_unstable();
    generations.dedup();

    println!("  gen |   best now   best then |  median now  median then | top-10 kept");
    for &g in generations.iter().skip(generations.len().saturating_sub(args.tail)) {
        let mut gen: Vec<&OrganismRecord> = records.iter().filter(|r| r.generation == g).collect();
        if gen.is_empty() {
            continue;
        }
        let rescored: Vec<Real> = gen.iter().map(|r| fitness::score(&new, &r.metrics)).collect();

        let mut old_sorted: Vec<Real> = gen.iter().map(|r| r.fitness).collect();
        let mut new_sorted = rescored.clone();
        old_sorted.sort_by(|a, b| b.total_cmp(a));
        new_sorted.sort_by(|a, b| b.total_cmp(a));
        let median = |v: &[Real]| v[v.len() / 2];

        // How much the ranking actually moved: of the ten best under the new
        // weights, how many were in the ten best under the old. A scoring that
        // reorders nothing is not asking a new question.
        let top = 10.min(gen.len());
        let mut by_new: Vec<usize> = (0..gen.len()).collect();
        by_new.sort_by(|&a, &b| rescored[b].total_cmp(&rescored[a]));
        let mut by_old: Vec<usize> = (0..gen.len()).collect();
        by_old.sort_by(|&a, &b| gen[b].fitness.total_cmp(&gen[a].fitness));
        let old_top: std::collections::HashSet<u64> =
            by_old[..top].iter().map(|&i| gen[i].id).collect();
        let kept = by_new[..top].iter().filter(|&&i| old_top.contains(&gen[i].id)).count();

        println!(
            "  {g:>4} | {:10.3} {:11.3} | {:11.3} {:12.3} | {kept:>2}/{top}",
            new_sorted[0],
            old_sorted[0],
            median(&new_sorted),
            median(&old_sorted),
        );

        if args.show_generation == Some(g) {
            gen.sort_by(|a, b| b.fitness.total_cmp(&a.fitness));
            println!("\n    generation {g}: what each scoring promotes");
            println!("      rank | id     | then   | now    | travel | net dy | climb  descent");
            let mut ranked: Vec<(usize, &&OrganismRecord)> = gen.iter().enumerate().collect();
            ranked.sort_by(|a, b| {
                fitness::score(&new, &b.1.metrics).total_cmp(&fitness::score(&new, &a.1.metrics))
            });
            for (rank, (old_rank, r)) in ranked.iter().take(10).enumerate() {
                let m = &r.metrics;
                println!(
                    "      {:>4} | {:6} | {:6.2} | {:6.2} | {:6.2} | {:+6.3} | {:5.2}  {:5.2}   (was #{})",
                    rank + 1,
                    r.id,
                    r.fitness,
                    fitness::score(&new, m),
                    m.displacement,
                    m.end.y - m.start.y,
                    m.climb,
                    m.descent,
                    old_rank + 1,
                );
            }
            println!();
        }
    }
    Ok(())
}
