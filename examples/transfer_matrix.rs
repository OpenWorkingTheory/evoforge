//! How does each population score in each environment?
//!
//! The question behind every cross-environment experiment, laid out as the
//! table it naturally is: one row per population (identified by the runs its
//! founders came from), one column per environment (the configuration it was
//! scored under), and in each cell what `evo evaluate` measured there.
//!
//!   cargo run --release --example transfer_matrix -- runs/transfer-*-eval-*
//!   cargo run --release --example transfer_matrix -- runs      # every run under it
//!
//! Two tables. The first is raw: best / median fitness per cell. The second is
//! each cell's median divided by the same population's median *at home* — the
//! environment it evolved in — which is the number that says how much a
//! population loses by moving. Raw fitness must never be compared across
//! columns: a flat-ground score and a fractal score are measured in the same
//! metres but not on the same terms. Compare down a column (two populations in
//! one environment, which faced identical trials) or along a row against home.
//!
//! "Home" is recognised by name: an environment is a population's home when the
//! population's source run was named after it (`transfer-flat-1789…` came from
//! `transfer-flat.toml`). A population founded from several runs has no single
//! home and its relative row is left blank.
//!
//! Rows marked `*` come from directories that evolved for more than one
//! generation; their figures are the *last* generation's and are not the same
//! measurement as an `evo evaluate` cell. See docs/RESULTS.md on the checkpoint
//! offset for why the two should not be mixed in one table.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use evoforge::record::{self, Run};

struct Cell {
    best: f64,
    median: f64,
    generations: usize,
}

fn main() {
    let roots: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if roots.is_empty() {
        eprintln!("usage: transfer_matrix <evaluate-dir|runs-dir> [...]");
        std::process::exit(2);
    }
    let dirs = expand(&roots);
    if dirs.is_empty() {
        eprintln!("no run directories found (a run directory holds manifest.json)");
        std::process::exit(2);
    }

    // (population label, environment) -> cell
    let mut cells: BTreeMap<(String, String), Cell> = BTreeMap::new();
    // population label -> the source run ids behind it, for home detection
    let mut sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut environments: Vec<String> = Vec::new();
    let mut failures = 0;

    for dir in &dirs {
        match read(dir) {
            Ok((population, environment, cell, srcs)) => {
                if !environments.contains(&environment) {
                    environments.push(environment.clone());
                }
                if cells.insert((population.clone(), environment.clone()), cell).is_some() {
                    eprintln!(
                        "note: more than one directory scores {population} in {environment}; \
                         keeping {}",
                        dir.display()
                    );
                }
                sources.insert(population, srcs);
            }
            Err(e) => {
                eprintln!("skipping {}: {e}", dir.display());
                failures += 1;
            }
        }
    }
    environments.sort();
    let populations: Vec<String> = sources.keys().cloned().collect();
    if populations.is_empty() {
        eprintln!("nothing to tabulate");
        std::process::exit(1);
    }

    let width = populations.iter().map(String::len).max().unwrap_or(10).max(10);
    let col = environments.iter().map(String::len).max().unwrap_or(16).max(16);

    println!("best / median fitness, by population (rows) and environment (columns)");
    println!();
    print!("{:width$}", "population");
    for env in &environments {
        print!("  {env:>col$}");
    }
    println!();
    for pop in &populations {
        let starred = environments
            .iter()
            .any(|e| cells.get(&(pop.clone(), e.clone())).is_some_and(|c| c.generations > 1));
        print!("{:width$}", if starred { format!("{pop}*") } else { pop.clone() });
        for env in &environments {
            match cells.get(&(pop.clone(), env.clone())) {
                Some(c) => print!("  {:>col$}", format!("{:.2} / {:.2}", c.best, c.median)),
                None => print!("  {:>col$}", "-"),
            }
        }
        println!();
    }

    println!();
    println!("median relative to the population's home environment");
    println!();
    print!("{:width$}", "population");
    for env in &environments {
        print!("  {env:>col$}");
    }
    println!("  home");
    for pop in &populations {
        let home = home_of(&sources[pop], &environments);
        print!("{pop:width$}");
        let home_median = home.and_then(|h| cells.get(&(pop.clone(), h.clone()))).map(|c| c.median);
        for env in &environments {
            let cell = cells.get(&(pop.clone(), env.clone()));
            match (cell, home_median) {
                (Some(c), Some(h)) if h.abs() > 1e-9 => {
                    print!("  {:>col$}", format!("{:.2}x", c.median / h))
                }
                _ => print!("  {:>col$}", "-"),
            }
        }
        println!("  {}", home.map(String::as_str).unwrap_or("(none)"));
    }

    if populations.iter().any(|p| sources[p].len() > 1) {
        println!();
        println!("populations founded from more than one run have no single home:");
        for (pop, srcs) in &sources {
            if srcs.len() > 1 {
                println!("  {pop} <- {}", srcs.join(" + "));
            }
        }
    }
    if cells.values().any(|c| c.generations > 1) {
        println!();
        println!("* evolved for more than one generation; figures are its last generation's");
    }
    if failures > 0 {
        std::process::exit(1);
    }
}

/// A path is a run directory, or a directory holding them.
fn expand(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        if root.join(record::MANIFEST_FILE).is_file() {
            out.push(root.clone());
            continue;
        }
        let Ok(entries) = fs::read_dir(root) else {
            eprintln!("cannot read {}", root.display());
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join(record::MANIFEST_FILE).is_file())
            .collect();
        found.sort();
        out.extend(found);
    }
    out
}

/// `(population label, environment, cell, source run ids)` for one directory.
fn read(dir: &Path) -> Result<(String, String, Cell, Vec<String>), String> {
    let run = Run::open(dir).map_err(|e| e.to_string())?;
    let environment = run.manifest.experiment_name.clone();

    let mut srcs: Vec<String> =
        run.read_founders().map_err(|e| e.to_string())?.into_iter().map(|f| f.source_run).collect();
    srcs.sort();
    srcs.dedup();
    let population = if srcs.is_empty() {
        format!("seed {}", run.manifest.seed)
    } else {
        srcs.iter().map(|s| short(s)).collect::<Vec<_>>().join("+")
    };

    let text = fs::read_to_string(dir.join(record::STATS_FILE)).map_err(|e| e.to_string())?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<&str> = lines.next().ok_or("empty stats.csv")?.split(',').collect();
    let col =
        |name: &str| header.iter().position(|h| *h == name).ok_or(format!("no {name} column"));
    let (best_col, median_col) = (col("best")?, col("median")?);
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
    let last = rows.last().ok_or("stats.csv has no generations")?;
    let num = |i: usize| last.get(i).and_then(|v| v.trim().parse::<f64>().ok());
    let cell = Cell {
        best: num(best_col).ok_or("unreadable best")?,
        median: num(median_col).ok_or("unreadable median")?,
        generations: rows.len(),
    };
    // An evolved run is a different population at its last generation than the
    // one it started from, so it must not share a row with an evaluate of the
    // same founders.
    let population = if cell.generations > 1 {
        format!("{population} @gen{}", cell.generations - 1)
    } else {
        population
    };
    Ok((population, environment, cell, srcs))
}

/// The environment a population's source run was named after, if exactly one.
fn home_of<'a>(srcs: &[String], environments: &'a [String]) -> Option<&'a String> {
    if srcs.len() != 1 {
        return None;
    }
    environments.iter().find(|env| srcs[0].starts_with(&format!("{env}-")))
}

/// A run id without its timestamp, for a label a table can afford.
fn short(run_id: &str) -> String {
    match run_id.rsplit_once('-') {
        Some((name, stamp)) if stamp.chars().all(|c| c.is_ascii_digit()) => name.to_string(),
        _ => run_id.to_string(),
    }
}
