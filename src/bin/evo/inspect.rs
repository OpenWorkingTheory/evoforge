//! `evo inspect` — summarise a completed or in-progress run.

use anyhow::{Context, Result};

use evoforge::record::{self, Run};
use evoforge::stats;

use super::InspectArgs;

pub(super) fn cmd_inspect(args: InspectArgs) -> Result<()> {
    let run =
        Run::open(&args.run).with_context(|| format!("opening run {}", args.run.display()))?;
    let cfg = run.config()?;

    println!("experiment    {}", run.manifest.experiment_id);
    println!("name          {}", run.manifest.experiment_name);
    println!("evoforge      {}", run.manifest.evoforge_version);
    println!("seed          {}", run.manifest.seed);
    println!("config digest {:016x}", run.manifest.config_digest);
    println!(
        "population    {}   generations configured  {}",
        cfg.evolution.population_size, cfg.evolution.generations
    );
    println!("objective     {:?}", cfg.fitness.objective);
    // Without this line the seed above would suggest generation 0 can be
    // reconstructed from it, which for a founded run is exactly wrong.
    let founders = run.read_founders()?;
    if !founders.is_empty() {
        let mut sources: Vec<&str> = founders.iter().map(|f| f.source_run.as_str()).collect();
        sources.sort_unstable();
        sources.dedup();
        println!(
            "founded from  {} organism(s) imported from {} — generation 0 did not come from the seed",
            founders.len(),
            sources.join(", ")
        );
    }
    println!();

    let stats_path = args.run.join(record::STATS_FILE);
    let text = std::fs::read_to_string(&stats_path)
        .with_context(|| format!("reading {}", stats_path.display()))?;
    let rows: Vec<&str> = text.lines().skip(1).filter(|l| !l.trim().is_empty()).collect();
    if rows.is_empty() {
        println!("no generations recorded yet");
        return Ok(());
    }

    println!("{} generations recorded", rows.len());
    println!("{}", stats::GenerationStats::TABLE_HEADER);
    for row in rows.iter().skip(rows.len().saturating_sub(args.tail)) {
        let f: Vec<&str> = row.split(',').collect();
        if f.len() < stats::GenerationStats::CSV_COLUMNS {
            continue;
        }
        println!(
            "{:>5} | {:>7} {:>8} {:>8} {:>8} | {:>4} {:>6} {:>4} | {:>7} {:>8}s",
            f[0],
            trim(f[2]),
            trim(f[3]),
            trim(f[4]),
            trim(f[5]),
            f[7],
            trim(f[8]),
            f[9],
            trim(f[11]),
            trim(f[12])
        );
    }

    let checkpoints = run.checkpoint_paths()?;
    println!();
    println!(
        "{} checkpoint(s), newest {}",
        checkpoints.len(),
        checkpoints.first().map(|p| p.display().to_string()).unwrap_or_else(|| "none".into())
    );

    if let Some(b) = run.best_stored_genome()? {
        println!(
            "best stored genome: organism {} from generation {}, fitness {:.3}, {} parts",
            b.id,
            b.generation,
            b.fitness,
            b.genome.part_count()
        );
        println!("  evo replay {} --organism {}", args.run.display(), b.id);
    }
    Ok(())
}

fn trim(s: &str) -> String {
    match s.parse::<f64>() {
        Ok(v) => format!("{v:.3}"),
        Err(_) => s.to_string(),
    }
}
