//! Does this build still reproduce what a recorded run recorded?
//!
//! Every run directory is already a regression fixture, and a far wider one
//! than the test suite. `config.toml` is the fully resolved configuration,
//! `genomes.jsonl` holds the genomes that were stored, and `organisms.jsonl`
//! holds the fitness and metrics each of them was scored with. Evaluation is a
//! pure function of `(genome, config)`, so re-evaluating a stored genome under
//! its own config has to reproduce the recorded numbers bit for bit.
//!
//! The useful part is that nothing has to be kept alongside it: the recorded
//! numbers *are* the baseline, so this needs no second build to compare
//! against. `tests/golden.rs` pins four generations of sixteen organisms on
//! flat ground; an archive of runs pins thousands of evolved organisms across
//! every terrain, body plan and objective ever run.
//!
//! Covered: the whole evaluation path — phenotype, solver, terrain, sensors,
//! controller, metrics, fitness — against organisms evolution actually found
//! rather than ones a test constructed.
//!
//! Not covered: selection, crossover and mutation, because `organisms.jsonl`
//! records no genome for an organism that was never stored. Re-running
//! evolution is what covers those: `evo verify` across thread counts, or a diff
//! of two builds' `organisms.jsonl`.
//!
//! Only the fields a record actually claims are compared. A run written under
//! an older `ARTIFACT_FORMAT` has no opinion about metrics added since, so
//! those are skipped — which makes this a standing check of "off is exact" as
//! well: everything the old record *does* claim must still hold.
//!
//! A mismatch is a regression until proven otherwise. The one legitimate
//! exception: a run recorded before a deliberate dynamics change will differ,
//! and should. Check the run's `evoforge_version` and `CHANGELOG.md` against
//! the commit history before assuming this build is at fault.
//!
//!   cargo run --release --example reproduce_probe -- runs/<run> [<run>...]
//!   cargo run --release --example reproduce_probe -- runs
//!   cargo run --release --example reproduce_probe -- runs --limit 8

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde_json::{Map, Value};

use evoforge::config::Config;
use evoforge::genome::Genome;
use evoforge::record;
use evoforge::sim;

fn main() {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut limit = usize::MAX;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--limit" {
            match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => limit = n,
                None => {
                    eprintln!("--limit needs a number");
                    std::process::exit(2);
                }
            }
        } else {
            roots.push(PathBuf::from(a));
        }
    }
    if roots.is_empty() {
        eprintln!("usage: reproduce_probe <run-dir|runs-dir> [...] [--limit N]");
        eprintln!("re-evaluates each run's stored genomes against what it recorded");
        std::process::exit(2);
    }

    let runs = expand(&roots);
    if runs.is_empty() {
        eprintln!("no run directories found (a run directory holds manifest.json)");
        std::process::exit(2);
    }

    let mut checked = 0usize;
    let mut differing = 0usize;
    let mut skipped = 0usize;
    for run in &runs {
        match check(run, limit) {
            Some((n, bad)) => {
                checked += n;
                differing += bad;
            }
            None => skipped += 1,
        }
    }

    let tail = if skipped > 0 { format!(", {skipped} run(s) skipped") } else { String::new() };
    println!();
    println!(
        "--- {} run(s): {checked} organism(s) re-evaluated, {} exact, {differing} differing{tail} ---",
        runs.len(),
        checked - differing,
    );
    if differing > 0 {
        std::process::exit(1);
    }
}

/// A path is either a run directory or a directory holding them.
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

struct Outcome {
    id: u64,
    generation: u64,
    unmatched: bool,
    diffs: Vec<(String, String, String)>,
}

/// Returns `(organisms checked, organisms differing)`, or `None` if the run
/// could not be read at all.
fn check(run: &Path, limit: usize) -> Option<(usize, usize)> {
    println!();
    println!("=== {} ===", run.display());

    let manifest: Value = read_json(&run.join(record::MANIFEST_FILE))?;
    let format = manifest["format"].as_u64().unwrap_or(0) as u32;
    if let Err(e) = record::check_readable("run", format) {
        println!("  skipped: {e}");
        return None;
    }

    let cfg = match Config::load(&run.join(record::CONFIG_FILE)) {
        Ok(c) => c,
        Err(e) => {
            println!("  skipped: {e}");
            return None;
        }
    };

    // Fitness and metrics for every organism, keyed by id.
    let mut recorded: HashMap<u64, Value> = HashMap::new();
    for line in read_lines(&run.join(record::ORGANISMS_FILE))? {
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let Some(id) = v["id"].as_u64() {
                recorded.insert(id, v);
            }
        }
    }

    // Only stored organisms carry a genome, so only they can be re-evaluated.
    let stored: Vec<Value> = read_lines(&run.join(record::GENOMES_FILE))?
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .take(limit)
        .collect();

    let terrain = serde_json::to_string(&cfg.environment.terrain).unwrap_or_default();
    let version = manifest["evoforge_version"].as_str().unwrap_or("?");
    println!(
        "  format {format} | evoforge {version} | terrain {terrain} | {} stored genome(s)",
        stored.len()
    );
    if stored.is_empty() {
        println!("  nothing to check: this run stored no genomes");
        return Some((0, 0));
    }

    // Evaluation is pure, so running these in parallel cannot change any of them.
    let outcomes: Vec<Outcome> = stored
        .par_iter()
        .map(|rec| {
            let id = rec["id"].as_u64().unwrap_or(0);
            let generation = rec["generation"].as_u64().unwrap_or(0);
            let truth = recorded.get(&id);
            let genome = serde_json::from_value::<Genome>(rec["genome"].clone());
            let (Some(truth), Ok(genome)) = (truth, genome) else {
                return Outcome { id, generation, unmatched: true, diffs: Vec::new() };
            };

            let fresh = sim::evaluate(&genome, &cfg, false);
            let mut diffs = Vec::new();
            compare_field("fitness", &truth["fitness"], &as_written(fresh.fitness), &mut diffs);
            if let Some(claimed) = truth["metrics"].as_object() {
                compare_claimed(claimed, &as_written(fresh.metrics), &mut diffs);
            }
            Outcome { id, generation, unmatched: false, diffs }
        })
        .collect();

    let mut shown = 0;
    let mut differing = 0;
    let mut unmatched = 0;
    for o in &outcomes {
        if o.unmatched {
            unmatched += 1;
            continue;
        }
        if o.diffs.is_empty() {
            continue;
        }
        differing += 1;
        if shown < 3 {
            shown += 1;
            println!("  org {} (gen {})", o.id, o.generation);
            for (field, was, now) in o.diffs.iter().take(6) {
                println!("      {field:<20} recorded {was:<16} now {now}");
            }
            if o.diffs.len() > 6 {
                println!("      and {} more field(s)", o.diffs.len() - 6);
            }
        }
    }
    if differing > shown {
        println!("  and {} more organism(s) differ", differing - shown);
    }

    let checked = outcomes.len() - unmatched;
    if differing == 0 {
        println!("  {checked}/{checked} reproduced exactly");
    } else {
        println!("  {}/{checked} reproduced exactly - {differing} DIFFER", checked - differing);
    }
    if unmatched > 0 {
        println!("  ({unmatched} stored genome(s) had no matching organism record)");
    }
    Some((checked, differing))
}

/// Round-trip through JSON *text*, so both sides take the same serialiser path
/// the recorder took.
///
/// `serde_json::to_value` promotes a `Real` to `f64` and loses the shortest-f32
/// formatting the file on disk actually used, which makes every field look
/// different while being bit-identical.
fn as_written<T: serde::Serialize>(v: T) -> Value {
    serde_json::to_string(&v)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

/// Compare only the keys the record claims, so metrics added after it was
/// written are not held against it.
fn compare_claimed(
    claimed: &Map<String, Value>,
    now: &Value,
    out: &mut Vec<(String, String, String)>,
) {
    let empty = Map::new();
    let now = now.as_object().unwrap_or(&empty);
    for (field, was) in claimed {
        let fresh = now.get(field).cloned().unwrap_or(Value::Null);
        compare_field(field, was, &fresh, out);
    }
}

/// JSON text is an exact comparison here: a `Real` is serialised through the
/// same path on both sides, so equal text means equal bits, and different bits
/// mean different text.
fn compare_field(field: &str, was: &Value, now: &Value, out: &mut Vec<(String, String, String)>) {
    let (a, b) = (was.to_string(), now.to_string());
    if a != b {
        out.push((field.to_string(), a, b));
    }
}

fn read_json(path: &Path) -> Option<Value> {
    let parsed = fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok());
    if parsed.is_none() {
        println!("  skipped: cannot read {}", path.display());
    }
    parsed
}

fn read_lines(path: &Path) -> Option<Vec<String>> {
    match fs::read_to_string(path) {
        Ok(t) => Some(t.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect()),
        Err(e) => {
            println!("  skipped: {} ({e})", path.display());
            None
        }
    }
}
