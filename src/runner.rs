//! Driving an experiment: the generation loop, recording policy and resume.
//!
//! This is the only module that does I/O during evolution, and the only one that
//! reads a clock. Everything it calls is deterministic; the loop's job is to
//! sequence those pure steps, persist their results, and report progress.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::evolution::{self, Population};
use crate::record::{self, Checkpoint, FounderRecord, Run, StoredGenome};
use crate::sim;
use crate::stats::{self, GenerationStats};

#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    /// Worker threads for population evaluation. `0` means one per core.
    pub threads: usize,
    /// Suppress the per-generation table.
    pub quiet: bool,
    /// Resume from the latest checkpoint in this existing run directory.
    pub resume: Option<PathBuf>,
    /// Allow resume when the checkpoint was written by a different evoforge
    /// version. Dynamics may not match; the default is to refuse.
    pub force_resume: bool,
    /// Found generation 0 from the populations in these run directories (or
    /// checkpoint files) rather than drawing it from the seed. More than one
    /// makes the union the founding population. Not a continuation: the new
    /// run starts at generation 0 under its own configuration, which is what
    /// lets a population evolved in one environment be carried into another.
    pub founders: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct RunSummary {
    pub dir: PathBuf,
    pub generations_completed: u32,
    pub organisms_evaluated: u64,
    pub wall_seconds: f64,
    pub final_stats: Option<GenerationStats>,
}

/// Run an experiment to completion.
pub fn run(cfg: &Config, opts: &RunOptions) -> Result<RunSummary> {
    // Validated here as well as in the CLI: this is a public entry point, and a
    // configuration that never went through `Config::load` has never been checked.
    cfg.validate().context("invalid configuration")?;

    let pool = build_pool(opts.threads)?;

    let (run, mut population) = match (&opts.resume, opts.founders.is_empty()) {
        (Some(_), false) => bail!(
            "--resume continues a run under its own configuration and --founders starts a \
             new one from an imported population; they cannot be combined"
        ),
        (Some(dir), true) => resume(dir, cfg, opts)?,
        (None, false) => {
            // Import before creating the directory, so a refused import leaves
            // nothing behind.
            let (population, provenance) = import_founders(cfg, &opts.founders, opts.quiet)?;
            let run = Run::create(cfg).context("creating run directory")?;
            run.append_founders(&provenance)?;
            (run, population)
        }
        (None, true) => {
            let run = Run::create(cfg).context("creating run directory")?;
            (run, Population::founding(cfg))
        }
    };

    // Continue the wall-clock column rather than restarting it, so a resumed run
    // does not appear to travel backwards in time at the resume boundary.
    let elapsed_before = run.elapsed_seconds_so_far()?;

    if !opts.quiet {
        println!(
            "experiment {}  seed {}  population {}  generations {}  threads {}",
            run.manifest.experiment_id,
            cfg.experiment.seed,
            population.len(),
            cfg.evolution.generations,
            pool.current_num_threads(),
        );
        println!("output {}", run.dir.display());
        println!("{}", GenerationStats::TABLE_HEADER);
    }

    let started = Instant::now();
    let mut organisms_evaluated: u64 = 0;
    let mut final_stats = None;
    let mut generations_completed = 0;

    let mut last_checkpointed: Option<u32> = None;

    while population.generation < cfg.evolution.generations {
        let eval_started = Instant::now();
        evolution::evaluate_population(&mut population, cfg, &pool);
        let eval_seconds = eval_started.elapsed().as_secs_f64();
        organisms_evaluated += population.len() as u64;

        let elapsed = elapsed_before + started.elapsed().as_secs_f64();
        let summary = stats::summarise(&population, eval_seconds, elapsed);
        if !opts.quiet {
            if summary.generation.is_multiple_of(20) && summary.generation > 0 {
                println!("{}", GenerationStats::TABLE_HEADER);
            }
            println!("{}", summary.to_table_row());
        }
        run.append_stats(&summary)?;
        run.append_organisms(&population)?;
        record_selected(&run, &population, cfg)?;

        let completed = population.generation;
        generations_completed += 1;
        final_stats = Some(summary);
        population = evolution::next_generation(&population, cfg);

        // Checkpoint *after* breeding, so the snapshot holds a generation that has
        // not yet been evaluated or written to disk. Checkpointing the generation
        // just finished instead would make a resume re-evaluate it and append a
        // second copy of its stats row and its organism records — which is exactly
        // what a preempted worker would hit, since it has no on-finish checkpoint
        // to land on. The schedule still counts completed generations; only the
        // population inside the file changed.
        if record::should_checkpoint(completed, cfg) {
            run.write_checkpoint(&population, cfg)?;
            last_checkpointed = Some(population.generation);
        }
    }

    // The loop leaves `population` holding the unevaluated next generation;
    // checkpointing it means a resume picks up exactly where this run stopped.
    // Skipped when the schedule already wrote this very generation.
    if cfg.checkpoint.on_finish && last_checkpointed != Some(population.generation) {
        run.write_checkpoint(&population, cfg)?;
    }

    Ok(RunSummary {
        dir: run.dir,
        generations_completed,
        organisms_evaluated,
        wall_seconds: started.elapsed().as_secs_f64(),
        final_stats,
    })
}

/// Score a population under a configuration without breeding it.
///
/// Writes an ordinary run directory holding exactly one generation — generation
/// 0, founded from `opts.founders` or, with none, drawn from the seed as the
/// naive baseline — with its stats row, every organism's record, **every**
/// organism's genome, and the recording policy's replays. No checkpoint,
/// because nothing was bred.
///
/// This is how a population evolved in one environment is scored in another.
/// The founders are the very organisms `--founders` would hand to [`run`], and
/// they meet the same trial set as everything else evaluated under this
/// configuration, so two populations scored here are directly comparable.
/// Storing every genome makes the directory both a fixture for
/// `reproduce_probe` and a source for further `--founders`.
pub fn evaluate(cfg: &Config, opts: &RunOptions) -> Result<RunSummary> {
    cfg.validate().context("invalid configuration")?;
    if opts.resume.is_some() {
        bail!("evaluate scores a population once; there is nothing to resume");
    }
    let pool = build_pool(opts.threads)?;

    let (mut population, provenance) = if opts.founders.is_empty() {
        (Population::founding(cfg), Vec::new())
    } else {
        import_founders(cfg, &opts.founders, opts.quiet)?
    };
    let run = Run::create(cfg).context("creating run directory")?;
    if !provenance.is_empty() {
        run.append_founders(&provenance)?;
    }

    if !opts.quiet {
        println!(
            "evaluating {} organism(s) as {}  seed {}  threads {}",
            population.len(),
            run.manifest.experiment_id,
            cfg.experiment.seed,
            pool.current_num_threads(),
        );
        println!("output {}", run.dir.display());
        println!("{}", GenerationStats::TABLE_HEADER);
    }

    let started = Instant::now();
    evolution::evaluate_population(&mut population, cfg, &pool);
    let eval_seconds = started.elapsed().as_secs_f64();
    let summary = stats::summarise(&population, eval_seconds, eval_seconds);
    if !opts.quiet {
        println!("{}", summary.to_table_row());
    }
    run.append_stats(&summary)?;
    run.append_organisms(&population)?;
    for individual in &population.individuals {
        run.append_genome(individual)?;
    }
    // The recording policy still chooses which trajectories to keep, but it must
    // not store genomes a second time: every one is already on disk above.
    let mut recording = cfg.clone();
    recording.recording.store_genomes = false;
    record_selected(&run, &population, &recording)?;

    Ok(RunSummary {
        dir: run.dir,
        generations_completed: 1,
        organisms_evaluated: population.len() as u64,
        wall_seconds: started.elapsed().as_secs_f64(),
        final_stats: Some(summary),
    })
}

/// Re-simulate the organisms chosen by the recording policy, this time with
/// trajectory capture, and write them out.
///
/// Re-simulating rather than recording every organism speculatively is the whole
/// point of keeping evaluation pure: the extra work is a handful of evaluations
/// per recorded generation, and in exchange the hot loop never allocates a frame
/// buffer it is going to throw away.
fn record_selected(run: &Run, pop: &Population, cfg: &Config) -> Result<()> {
    for id in record::selection_for_recording(pop, cfg) {
        let Some(individual) = pop.find(id) else { continue };
        if cfg.recording.store_genomes {
            run.append_genome(individual)?;
        }
        let result = sim::evaluate(&individual.genome, cfg, true);
        if let Some(trace) = result.trace {
            run.write_replay(individual, trace, cfg)?;
        }
    }
    Ok(())
}

/// Load the newest checkpoint from an existing run directory.
fn resume(dir: &Path, cfg: &Config, opts: &RunOptions) -> Result<(Run, Population)> {
    let force = opts.force_resume;
    let run = Run::open(dir).with_context(|| format!("opening run {}", dir.display()))?;
    let Some(checkpoint) = run.latest_checkpoint()? else {
        bail!("{} has no checkpoints to resume from", dir.display());
    };

    // A checkpoint older than `MIN_RESUMABLE_FORMAT` holds a population that has
    // already been evaluated and written out, so resuming it would duplicate a
    // generation's records. The layout is still readable — `evo inspect` and
    // `evo replay` work fine against it — but continuing the run is not safe.
    if checkpoint.format < record::MIN_RESUMABLE_FORMAT && !force {
        bail!(
            "checkpoint is in format {} and holds an already-evaluated generation; \
             resuming it would append a second copy of generation {}'s records. \
             Start a new run, or pass --force-resume to accept the duplication",
            checkpoint.format,
            checkpoint.population.generation
        );
    }

    // A checkpoint is only meaningful under the configuration that produced it.
    // Refusing loudly is better than silently continuing an experiment whose
    // parameters changed underneath it.
    if checkpoint.config_digest != cfg.evolution_digest() {
        bail!(
            "config does not match the checkpoint (digest {:016x} vs {:016x}); \
             resume with the run's own {} or start a new run",
            cfg.evolution_digest(),
            checkpoint.config_digest,
            record::CONFIG_FILE
        );
    }
    if checkpoint.evoforge_version != crate::VERSION {
        if !force {
            bail!(
                "checkpoint was written by evoforge {}, this is {}; \
                 resume with --force-resume if you intend to continue anyway",
                checkpoint.evoforge_version,
                crate::VERSION
            );
        }
        eprintln!(
            "warning: checkpoint was written by evoforge {}, this is {}; \
             --force-resume accepted, results may not be comparable",
            checkpoint.evoforge_version,
            crate::VERSION
        );
    }

    let population = checkpoint.population;
    if !opts.quiet {
        println!(
            "resuming {} from generation {}",
            run.manifest.experiment_id, population.generation
        );
    }
    Ok((run, population))
}

/// Build generation 0 from the populations of one or more existing runs.
///
/// Each source contributes every genome it can offer — its latest checkpoint,
/// else the genomes it stored — and the union is renumbered as a fresh
/// generation 0, with a [`FounderRecord`] per founder saying where it came from.
///
/// # The guard
///
/// A genome is only interpretable under a controller layout and body limits
/// like the ones it was bred under: the weight vector has the layout's length,
/// every slot is below `max_parts`, every shape is on the roster. Those have to
/// match the importing configuration exactly, and the import refuses when they
/// do not. Clamping a founder to fit would import a different organism, which is
/// a confound no experiment wants. Everything else that may differ — mutation
/// rates, fitness weights, the seed, the simulation protocol — is a legitimate
/// thing to vary between the source and the new run, so it is reported rather
/// than refused.
pub fn import_founders(
    cfg: &Config,
    sources: &[PathBuf],
    quiet: bool,
) -> Result<(Population, Vec<FounderRecord>)> {
    let layout = cfg.brain_layout();
    let mut genomes = Vec::new();
    let mut provenance = Vec::new();

    for source in sources {
        let (run, stored) = open_source(source)?;
        let name = &run.manifest.experiment_id;
        let source_cfg = run
            .config()
            .with_context(|| format!("reading the configuration {name} was bred under"))?;

        if let Some(field) = layout_mismatch(&source_cfg, cfg) {
            bail!(
                "cannot found a run from {name}: its organisms were bred under a different \
                 controller layout ({field} differs), so their genomes do not fit this \
                 configuration"
            );
        }
        if source_cfg.body != cfg.body {
            bail!(
                "cannot found a run from {name}: its [body] limits differ from this \
                 configuration's, so its organisms may not be valid here (compare the two \
                 {})",
                record::CONFIG_FILE
            );
        }
        if !quiet {
            for (what, differs) in [
                ("[simulation]", source_cfg.simulation != cfg.simulation),
                ("[mutation]", source_cfg.mutation != cfg.mutation),
                ("[fitness]", source_cfg.fitness != cfg.fitness),
                ("experiment.seed", source_cfg.experiment.seed != cfg.experiment.seed),
            ] {
                if differs {
                    eprintln!("note: {what} differs between {name} and this run");
                }
            }
        }
        if stored.is_empty() {
            bail!("{name} has no checkpoint and no stored genomes to found a run from");
        }

        for s in stored {
            if !s.genome.is_valid(&cfg.body, &layout) {
                bail!(
                    "cannot found a run from {name}: organism {} is not a valid genome under \
                     this configuration",
                    s.id
                );
            }
            provenance.push(FounderRecord {
                format: record::ARTIFACT_FORMAT,
                id: genomes.len() as u64 + 1,
                source_run: name.clone(),
                source_id: s.id,
                source_generation: s.generation,
            });
            genomes.push(s.genome);
        }
    }

    if !quiet {
        println!("founding from {} organism(s) across {} run(s)", genomes.len(), sources.len());
    }
    Ok((Population::from_founders(genomes), provenance))
}

/// A founder source is a run directory or a checkpoint file inside one.
fn open_source(path: &Path) -> Result<(Run, Vec<StoredGenome>)> {
    if path.is_file() {
        let run_dir = path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| anyhow::anyhow!("{} is not inside a run directory", path.display()))?;
        let run = Run::open(run_dir)
            .with_context(|| format!("opening the run that holds {}", path.display()))?;
        let checkpoint: Checkpoint = record::read_json(path)
            .with_context(|| format!("reading checkpoint {}", path.display()))?;
        record::check_readable(&path.display().to_string(), checkpoint.format)?;
        let stored = checkpoint.population.individuals.iter().map(StoredGenome::from).collect();
        return Ok((run, stored));
    }
    let run = Run::open(path).with_context(|| format!("opening run {}", path.display()))?;
    let stored = run.importable_genomes()?;
    Ok((run, stored))
}

/// The first of the five configuration fields that shape the controller's weight
/// vector on which two configurations disagree, if any.
fn layout_mismatch(a: &Config, b: &Config) -> Option<&'static str> {
    if a.body.max_parts != b.body.max_parts {
        Some("body.max_parts")
    } else if a.brain.hidden != b.brain.hidden {
        Some("brain.hidden")
    } else if a.joints_can_break() != b.joints_can_break() {
        Some("body.joint_endurance (zero against non-zero)")
    } else if a.simulation.steer != b.simulation.steer {
        Some("simulation.steer")
    } else if a.sensor_channels() != b.sensor_channels() {
        Some("sensor channels (body.sensor_probability / sensor.rays)")
    } else {
        None
    }
}

pub fn build_pool(threads: usize) -> Result<rayon::ThreadPool> {
    let builder = rayon::ThreadPoolBuilder::new().num_threads(threads);
    builder.build().context("building the worker thread pool")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!(
                "evoforge-runner-{tag}-{:?}-{}",
                std::thread::current().id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tiny_config(dir: &TempDir) -> Config {
        let mut cfg = Config::default();
        cfg.experiment.name = "runner-test".into();
        cfg.experiment.output_dir = dir.0.clone();
        cfg.evolution.population_size = 8;
        cfg.evolution.generations = 4;
        cfg.simulation.duration = 0.4;
        cfg.simulation.settle_time = 0.1;
        cfg.recording.every_generations = 2;
        cfg.checkpoint.every_generations = 2;
        cfg
    }

    #[test]
    fn a_run_produces_the_expected_artefacts() {
        let tmp = TempDir::new("artefacts");
        let cfg = tiny_config(&tmp);
        let summary =
            run(&cfg, &RunOptions { threads: 2, quiet: true, ..Default::default() }).unwrap();

        assert_eq!(summary.generations_completed, 4);
        assert_eq!(summary.organisms_evaluated, 32);

        let stats_text = fs::read_to_string(summary.dir.join(record::STATS_FILE)).unwrap();
        assert_eq!(stats_text.lines().count(), 5, "header plus four generations");

        let organisms = fs::read_to_string(summary.dir.join(record::ORGANISMS_FILE)).unwrap();
        assert_eq!(organisms.lines().count(), 32);

        let replays: Vec<_> = fs::read_dir(summary.dir.join(record::REPLAY_DIR))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!replays.is_empty(), "recording policy produced no replays");

        let checkpoints: Vec<_> = fs::read_dir(summary.dir.join(record::CHECKPOINT_DIR))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!checkpoints.is_empty());
    }

    #[test]
    fn identical_seeds_produce_identical_runs() {
        let tmp = TempDir::new("determinism");
        let cfg = tiny_config(&tmp);

        let read_stats = |dir: &Path| fs::read_to_string(dir.join(record::STATS_FILE)).unwrap();
        let strip_timings = |text: String| -> Vec<String> {
            text.lines().map(|l| l.split(',').take(10).collect::<Vec<_>>().join(",")).collect()
        };

        let a = run(&cfg, &RunOptions { threads: 1, quiet: true, ..Default::default() }).unwrap();
        let b = run(&cfg, &RunOptions { threads: 4, quiet: true, ..Default::default() }).unwrap();

        assert_eq!(
            strip_timings(read_stats(&a.dir)),
            strip_timings(read_stats(&b.dir)),
            "runs differed despite identical seeds"
        );
    }

    fn quiet(threads: usize) -> RunOptions {
        RunOptions { threads, quiet: true, ..Default::default() }
    }

    fn founded(threads: usize, from: &[&Path]) -> RunOptions {
        RunOptions { founders: from.iter().map(|p| p.to_path_buf()).collect(), ..quiet(threads) }
    }

    fn generation_records(dir: &Path, generation: u32) -> Vec<record::OrganismRecord> {
        fs::read_to_string(dir.join(record::ORGANISMS_FILE))
            .unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str::<record::OrganismRecord>(l).ok())
            .filter(|r| r.generation == generation)
            .collect()
    }

    /// The founders of a run built from another are exactly that run's latest
    /// checkpoint, evaluated under the new configuration — nothing drawn, nothing
    /// altered on the way in.
    #[test]
    fn a_run_founded_from_another_starts_from_its_population() {
        let tmp = TempDir::new("founded");
        let cfg = tiny_config(&tmp);
        let source = run(&cfg, &quiet(2)).unwrap();
        let checkpoint = Run::open(&source.dir).unwrap().latest_checkpoint().unwrap().unwrap();

        let child = run(&cfg, &founded(2, &[&source.dir])).unwrap();
        let child_run = Run::open(&child.dir).unwrap();

        let provenance = child_run.read_founders().unwrap();
        assert_eq!(provenance.len(), checkpoint.population.len());
        let source_id = &Run::open(&source.dir).unwrap().manifest.experiment_id;
        for (k, (record, founder)) in
            provenance.iter().zip(&checkpoint.population.individuals).enumerate()
        {
            assert_eq!(record.id, k as u64 + 1, "founders are renumbered from one");
            assert_eq!(&record.source_run, source_id);
            assert_eq!(record.source_id, founder.id);
            assert_eq!(record.source_generation, founder.generation);
        }

        // Generation 0 of the child scores exactly what those genomes score.
        let gen0 = generation_records(&child.dir, 0);
        assert_eq!(gen0.len(), checkpoint.population.len());
        for (record, founder) in gen0.iter().zip(&checkpoint.population.individuals) {
            let fresh = sim::evaluate(&founder.genome, &cfg, false);
            assert_eq!(record.fitness.to_bits(), fresh.fitness.to_bits(), "organism {}", record.id);
        }
    }

    /// Two sources make a union: generation 0 carries every founder, breeding
    /// brings the next generation back to the configured size, and a child with
    /// one parent from each source is a hybrid that provenance can identify.
    #[test]
    fn founding_from_two_runs_unions_them_and_can_produce_hybrids() {
        let tmp = TempDir::new("union");
        let cfg = tiny_config(&tmp);
        let mut other = cfg.clone();
        other.experiment.seed += 1;
        let a = run(&cfg, &quiet(2)).unwrap();
        let b = run(&other, &quiet(2)).unwrap();

        // Every child a crossover child, no elites, no immigrants: the mixing
        // recipe the tutorials use.
        let mut mating = cfg.clone();
        mating.evolution.crossover_rate = 1.0;
        mating.evolution.immigrant_rate = 0.0;
        mating.evolution.elite_count = 0;
        mating.evolution.tournament_size = 1;
        mating.evolution.generations = 2;
        let mixed = run(&mating, &founded(2, &[&a.dir, &b.dir])).unwrap();
        let mixed_run = Run::open(&mixed.dir).unwrap();

        let gen0 = generation_records(&mixed.dir, 0);
        let gen1 = generation_records(&mixed.dir, 1);
        assert_eq!(gen0.len(), 2 * cfg.evolution.population_size, "the union is generation 0");
        assert_eq!(gen1.len(), cfg.evolution.population_size, "breeding restores the size");

        let provenance = mixed_run.read_founders().unwrap();
        let source_of: std::collections::HashMap<u64, &str> =
            provenance.iter().map(|f| (f.id, f.source_run.as_str())).collect();
        assert!(gen0.iter().all(|r| source_of.contains_key(&r.id)), "every founder has provenance");
        let sources: std::collections::HashSet<&str> = source_of.values().copied().collect();
        assert_eq!(sources.len(), 2, "two distinct source runs");

        let hybrids = gen1
            .iter()
            .filter(|r| {
                r.parents.iter().all(|&p| p != 0)
                    && source_of.get(&r.parents[0]) != source_of.get(&r.parents[1])
            })
            .count();
        assert!(hybrids > 0, "uniform pairing across two sources never crossed them");
    }

    #[test]
    fn founders_bred_under_a_different_controller_layout_are_refused() {
        let tmp = TempDir::new("refused");
        let cfg = tiny_config(&tmp);
        let source = run(&cfg, &quiet(2)).unwrap();

        let mut wider = cfg.clone();
        wider.brain.hidden += 1;
        let err = run(&wider, &founded(2, &[&source.dir])).unwrap_err().to_string();
        assert!(err.contains("brain.hidden"), "message names the field: {err}");

        let mut bigger = cfg.clone();
        bigger.body.max_parts += 1;
        let err = run(&bigger, &founded(2, &[&source.dir])).unwrap_err().to_string();
        assert!(err.contains("body.max_parts"), "message names the field: {err}");

        // A refused import leaves no half-made run directory behind.
        let dirs = fs::read_dir(&tmp.0).unwrap().filter(|e| e.as_ref().unwrap().path().is_dir());
        assert_eq!(dirs.count(), 1, "only the source run exists");
    }

    #[test]
    fn founded_runs_do_not_depend_on_thread_count() {
        let tmp = TempDir::new("founded-threads");
        let cfg = tiny_config(&tmp);
        let source = run(&cfg, &quiet(2)).unwrap();

        let read = |dir: &Path| -> Vec<String> {
            fs::read_to_string(dir.join(record::STATS_FILE))
                .unwrap()
                .lines()
                .map(|l| l.split(',').take(10).collect::<Vec<_>>().join(","))
                .collect()
        };
        let one = run(&cfg, &founded(1, &[&source.dir])).unwrap();
        let many = run(&cfg, &founded(4, &[&source.dir])).unwrap();
        assert_eq!(read(&one.dir), read(&many.dir));
    }

    /// An evaluate directory holds one scored generation and every genome, so it
    /// is at once a fixture (each genome re-evaluates to what it recorded) and a
    /// source of founders in its own right.
    #[test]
    fn evaluate_scores_a_population_once_and_stores_every_genome() {
        let tmp = TempDir::new("evaluate");
        let cfg = tiny_config(&tmp);
        let source = run(&cfg, &quiet(2)).unwrap();
        let checkpoint = Run::open(&source.dir).unwrap().latest_checkpoint().unwrap().unwrap();

        // Score it somewhere it did not evolve.
        let mut elsewhere = cfg.clone();
        elsewhere.environment.terrain = crate::config::Terrain::Rough;
        let scored = evaluate(&elsewhere, &founded(2, &[&source.dir])).unwrap();
        let scored_run = Run::open(&scored.dir).unwrap();
        let n = checkpoint.population.len();

        let stats = fs::read_to_string(scored.dir.join(record::STATS_FILE)).unwrap();
        assert_eq!(stats.lines().count(), 2, "header plus exactly one generation");
        assert_eq!(generation_records(&scored.dir, 0).len(), n);
        let genomes = fs::read_to_string(scored.dir.join(record::GENOMES_FILE)).unwrap();
        assert_eq!(genomes.lines().filter(|l| !l.trim().is_empty()).count(), n, "every genome");
        assert!(scored_run.checkpoint_paths().unwrap().is_empty(), "nothing was bred");
        assert_eq!(scored_run.read_founders().unwrap().len(), n);
        assert_eq!(scored.organisms_evaluated, n as u64);

        let records = generation_records(&scored.dir, 0);
        for stored in scored_run.importable_genomes().unwrap() {
            let recorded = records.iter().find(|r| r.id == stored.id).unwrap();
            let fresh = sim::evaluate(&stored.genome, &elsewhere, false);
            assert_eq!(
                recorded.fitness.to_bits(),
                fresh.fitness.to_bits(),
                "organism {}",
                stored.id
            );
            assert_eq!(stored.fitness.to_bits(), fresh.fitness.to_bits());
        }

        // Chaining: the evaluate directory founds a new run through the
        // stored-genome fallback, since it has no checkpoint.
        let chained = run(&cfg, &founded(2, &[&scored.dir])).unwrap();
        assert_eq!(generation_records(&chained.dir, 0).len(), n);
    }

    #[test]
    fn evaluate_without_founders_scores_the_seed_population() {
        let tmp = TempDir::new("evaluate-naive");
        let cfg = tiny_config(&tmp);
        let scored = evaluate(&cfg, &quiet(2)).unwrap();

        let mut naive = Population::founding(&cfg);
        evolution::evaluate_population_serial(&mut naive, &cfg);
        let records = generation_records(&scored.dir, 0);
        assert_eq!(records.len(), naive.len());
        for (r, i) in records.iter().zip(&naive.individuals) {
            assert_eq!(r.id, i.id);
            assert_eq!(r.fitness.to_bits(), i.fitness.to_bits());
        }
        assert!(Run::open(&scored.dir).unwrap().read_founders().unwrap().is_empty());
    }

    #[test]
    fn resume_and_founders_cannot_be_combined() {
        let tmp = TempDir::new("exclusive");
        let cfg = tiny_config(&tmp);
        let source = run(&cfg, &quiet(2)).unwrap();
        let err = run(
            &cfg,
            &RunOptions { resume: Some(source.dir.clone()), ..founded(2, &[&source.dir]) },
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("cannot be combined"), "{err}");
    }

    #[test]
    fn a_resumed_run_continues_the_same_trajectory() {
        let tmp = TempDir::new("resume");
        let mut short = tiny_config(&tmp);
        short.evolution.generations = 2;
        short.checkpoint.every_generations = 0; // rely on the on-finish checkpoint

        let mut full = short.clone();
        full.evolution.generations = 4;

        // The uninterrupted reference run.
        let reference =
            run(&full, &RunOptions { threads: 2, quiet: true, ..Default::default() }).unwrap();

        // A run stopped after two generations, then extended. Raising
        // `generations` must not invalidate the checkpoint.
        let partial =
            run(&short, &RunOptions { threads: 2, quiet: true, ..Default::default() }).unwrap();
        let resumed = run(
            &full,
            &RunOptions {
                threads: 2,
                quiet: true,
                resume: Some(partial.dir.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(resumed.generations_completed, 2, "should only run the remaining two");

        let fitness_column = |dir: &Path| -> Vec<String> {
            fs::read_to_string(dir.join(record::STATS_FILE))
                .unwrap()
                .lines()
                .skip(1)
                .map(|l| l.split(',').take(6).collect::<Vec<_>>().join(","))
                .collect()
        };

        let reference_rows = fitness_column(&reference.dir);
        let resumed_rows = fitness_column(&resumed.dir);
        assert_eq!(resumed_rows.len(), 4, "resume appends to the existing stats file");
        assert_eq!(reference_rows, resumed_rows);
    }

    /// The preemption path: a run killed between checkpoints has no on-finish
    /// snapshot to land on, so it resumes from a periodic one. That must not
    /// replay a generation whose stats row and organism records are already on
    /// disk.
    #[test]
    fn resume_from_a_periodic_checkpoint_does_not_duplicate_a_generation() {
        let tmp = TempDir::new("periodic");
        let mut short = tiny_config(&tmp);
        short.evolution.generations = 3;
        short.checkpoint.every_generations = 1;
        short.checkpoint.on_finish = false; // as if killed mid-run

        let mut full = short.clone();
        full.evolution.generations = 5;

        let partial =
            run(&short, &RunOptions { threads: 1, quiet: true, ..Default::default() }).unwrap();
        run(
            &full,
            &RunOptions {
                threads: 1,
                quiet: true,
                resume: Some(partial.dir.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        let text = fs::read_to_string(partial.dir.join(record::STATS_FILE)).unwrap();
        let generations: Vec<&str> = text
            .lines()
            .skip(1)
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split(',').next().unwrap())
            .collect();
        assert_eq!(
            generations,
            ["0", "1", "2", "3", "4"],
            "every generation must appear exactly once in stats.csv"
        );

        let organisms = fs::read_to_string(partial.dir.join(record::ORGANISMS_FILE)).unwrap();
        let mut ids: Vec<u64> = organisms
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<record::OrganismRecord>(l).unwrap().id)
            .collect();
        let total = ids.len();
        assert_eq!(total, 5 * 8, "one record per organism per generation");
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "organisms.jsonl contains duplicate ids");
    }

    /// A checkpoint must hold a generation that has *not* been written out yet,
    /// which is what makes resume idempotent.
    #[test]
    fn a_checkpoint_holds_the_next_unevaluated_generation() {
        let tmp = TempDir::new("unevaluated-checkpoint");
        let mut cfg = tiny_config(&tmp);
        cfg.evolution.generations = 2;
        cfg.checkpoint.every_generations = 1;
        cfg.checkpoint.on_finish = false;

        let summary =
            run(&cfg, &RunOptions { threads: 1, quiet: true, ..Default::default() }).unwrap();
        let opened = Run::open(&summary.dir).unwrap();
        for path in opened.checkpoint_paths().unwrap() {
            let checkpoint: record::Checkpoint = record::read_json(&path).unwrap();
            assert!(
                checkpoint
                    .population
                    .individuals
                    .iter()
                    .all(|i| i.fitness == evolution::UNEVALUATED_FITNESS),
                "{} holds an already-evaluated population",
                path.display()
            );
        }
    }

    #[test]
    fn elapsed_seconds_does_not_go_backwards_across_a_resume() {
        let tmp = TempDir::new("elapsed");
        let mut short = tiny_config(&tmp);
        short.evolution.generations = 2;
        let mut full = short.clone();
        full.evolution.generations = 4;

        let partial =
            run(&short, &RunOptions { threads: 1, quiet: true, ..Default::default() }).unwrap();
        run(
            &full,
            &RunOptions {
                threads: 1,
                quiet: true,
                resume: Some(partial.dir.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        let text = fs::read_to_string(partial.dir.join(record::STATS_FILE)).unwrap();
        let elapsed: Vec<f64> = text
            .lines()
            .skip(1)
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                l.split(',').nth(GenerationStats::ELAPSED_SECONDS_COLUMN).unwrap().parse().unwrap()
            })
            .collect();
        assert_eq!(elapsed.len(), 4);
        for w in elapsed.windows(2) {
            assert!(w[1] >= w[0], "elapsed went backwards: {elapsed:?}");
        }
    }

    /// A pre-v2 checkpoint holds an evaluated generation, so resuming it would
    /// duplicate records. It must be refused by default and only accepted when
    /// the operator says so.
    #[test]
    fn resume_refuses_a_pre_v2_checkpoint_unless_forced() {
        let tmp = TempDir::new("legacy");
        let cfg = tiny_config(&tmp);
        let first =
            run(&cfg, &RunOptions { threads: 1, quiet: true, ..Default::default() }).unwrap();

        let opened = Run::open(&first.dir).unwrap();
        let mut checkpoint = opened.latest_checkpoint().unwrap().unwrap();
        checkpoint.format = 1;
        record::write_json(&opened.checkpoint_path(checkpoint.population.generation), &checkpoint)
            .unwrap();

        let opts = |force| RunOptions {
            threads: 1,
            quiet: true,
            resume: Some(first.dir.clone()),
            force_resume: force,
            founders: Vec::new(),
        };
        let err = run(&cfg, &opts(false)).unwrap_err();
        assert!(err.to_string().contains("already-evaluated"), "{err}");
        assert!(run(&cfg, &opts(true)).is_ok());
    }

    #[test]
    fn run_rejects_an_invalid_configuration() {
        let tmp = TempDir::new("invalid");
        let mut cfg = tiny_config(&tmp);
        cfg.evolution.elite_count = cfg.evolution.population_size;
        let err = run(&cfg, &RunOptions { quiet: true, ..Default::default() }).unwrap_err();
        assert!(err.to_string().contains("invalid configuration"), "{err}");
    }

    #[test]
    fn resume_rejects_a_mismatched_config() {
        let tmp = TempDir::new("mismatch");
        let cfg = tiny_config(&tmp);
        let first =
            run(&cfg, &RunOptions { threads: 1, quiet: true, ..Default::default() }).unwrap();

        let mut changed = cfg.clone();
        changed.evolution.tournament_size += 1;
        let err = run(
            &changed,
            &RunOptions {
                threads: 1,
                quiet: true,
                resume: Some(first.dir.clone()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not match"), "{err}");
    }

    #[test]
    fn resume_rejects_a_version_mismatch_unless_forced() {
        let tmp = TempDir::new("version");
        let cfg = tiny_config(&tmp);
        let first =
            run(&cfg, &RunOptions { threads: 1, quiet: true, ..Default::default() }).unwrap();

        let opened = Run::open(&first.dir).unwrap();
        let mut checkpoint = opened.latest_checkpoint().unwrap().unwrap();
        checkpoint.evoforge_version = "0.0.0-test".into();
        record::write_json(&opened.checkpoint_path(checkpoint.population.generation), &checkpoint)
            .unwrap();

        let err = run(
            &cfg,
            &RunOptions {
                threads: 1,
                quiet: true,
                resume: Some(first.dir.clone()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("written by evoforge"), "{err}");

        let forced = run(
            &cfg,
            &RunOptions {
                threads: 1,
                quiet: true,
                resume: Some(first.dir.clone()),
                force_resume: true,
                founders: Vec::new(),
            },
        );
        assert!(forced.is_ok(), "{forced:?}");
    }
}
