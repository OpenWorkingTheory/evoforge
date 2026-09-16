//! `evo bench` — measure evaluation throughput and its scaling across cores.

use anyhow::Result;

use evoforge::bench;
use evoforge::config::Config;

use super::BenchArgs;

pub(super) fn cmd_bench(args: BenchArgs) -> Result<()> {
    let mut cfg = match &args.config {
        Some(path) => Config::load(path)?,
        None => Config::default(),
    };
    if let Some(p) = args.population {
        cfg.evolution.population_size = p;
    }
    cfg.validate()?;

    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let thread_counts = args.threads.unwrap_or_else(|| bench::default_thread_counts(cores));

    println!(
        "benchmarking: population {}, {:.1}s simulated per organism at {}Hz ({} steps), \
         control {}Hz, {} solver iterations",
        cfg.evolution.population_size,
        cfg.simulation.duration + cfg.simulation.settle_time,
        (1.0 / cfg.simulation.timestep).round(),
        cfg.total_steps(),
        cfg.simulation.control_hz,
        cfg.simulation.solver_iterations,
    );
    println!(
        "machine reports {cores} cores; best of {} runs per thread count",
        args.repeats.max(1)
    );
    println!();

    let report = bench::run(&cfg, &thread_counts, args.repeats)?;

    println!("mean body: {:.2} parts, {:.2} joints", report.mean_parts, report.mean_joints);
    println!(
        "memory: ~{} B per genome, ~{:.1} KiB for the live population{}",
        report.genome_bytes,
        report.population_bytes as f64 / 1024.0,
        match report.resident_bytes {
            Some(rss) => format!(", {:.1} MiB resident", rss as f64 / (1024.0 * 1024.0)),
            None => String::new(),
        }
    );
    println!();
    println!(
        "threads |   org/s |    steps/s | speedup | efficiency | USD/M evals | gens/USD @ ${:.4}/core-h",
        args.price
    );
    for r in &report.results {
        println!(
            "{:7} | {:7.1} | {:10.3e} | {:6.2}x | {:9.0}% | {:11.4} | {:8.0}",
            r.threads,
            r.organisms_per_second,
            r.steps_per_second,
            r.speedup,
            r.efficiency * 100.0,
            report.usd_per_million_evaluations(args.price, r),
            report.generations_per_usd(args.price, r),
        );
    }

    if let Some(best) = report.best() {
        println!();
        println!(
            "best: {:.0} organisms/s at {} threads ({:.0}% scaling efficiency)",
            best.organisms_per_second,
            best.threads,
            best.efficiency * 100.0
        );
        println!(
            "at ${:.4}/core-hour that is ${:.4} per million evaluations, \
             {:.0} generations of {} per dollar",
            args.price,
            report.usd_per_million_evaluations(args.price, best),
            report.generations_per_usd(args.price, best),
            report.population,
        );
    }
    Ok(())
}
