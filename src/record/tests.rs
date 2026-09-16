use super::*;
use crate::evolution;
use crate::stats;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "evoforge-test-{tag}-{}-{:?}",
            unix_time(),
            std::thread::current().id()
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

fn test_config(dir: &TempDir) -> Config {
    let mut cfg = Config::default();
    cfg.experiment.name = "unit test".into();
    cfg.experiment.output_dir = dir.0.clone();
    cfg.evolution.population_size = 8;
    cfg.simulation.duration = 0.5;
    cfg.simulation.settle_time = 0.1;
    cfg
}

fn evaluated(cfg: &Config) -> Population {
    let mut pop = Population::founding(cfg);
    evolution::evaluate_population_serial(&mut pop, cfg);
    pop
}

#[test]
fn create_lays_out_the_run_directory() {
    let tmp = TempDir::new("layout");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();

    assert!(run.dir.join(MANIFEST_FILE).exists());
    assert!(run.dir.join(CONFIG_FILE).exists());
    assert!(run.dir.join(STATS_FILE).exists());
    assert!(run.dir.join(CHECKPOINT_DIR).is_dir());
    assert!(run.dir.join(REPLAY_DIR).is_dir());
    // Directory names must be filesystem-safe even when the experiment is not.
    assert!(!run.manifest.experiment_id.contains(' '));

    let reopened = Run::open(&run.dir).unwrap();
    assert_eq!(reopened.manifest, run.manifest);
    assert_eq!(reopened.manifest.format, ARTIFACT_FORMAT);
    assert_eq!(reopened.config().unwrap(), cfg);
}

#[test]
fn write_json_replaces_an_existing_file() {
    let tmp = TempDir::new("atomic");
    let path = tmp.0.join("value.json");
    write_json(&path, &1u32).unwrap();
    write_json(&path, &2u32).unwrap();
    let loaded: u32 = read_json(&path).unwrap();
    assert_eq!(loaded, 2);
    assert!(!path.with_extension("json.tmp").exists());
    // The sibling temp name is `value.json.tmp`, not `value.tmp`.
    assert!(!tmp.0.join("value.json.tmp").exists());
}

#[test]
fn truncated_jsonl_lines_are_skipped() {
    let tmp = TempDir::new("truncated");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let pop = evaluated(&cfg);
    run.append_genome(&pop.individuals[0]).unwrap();
    {
        let mut f = append_file(&run.dir.join(GENOMES_FILE)).unwrap();
        write!(f, "{{\"format\":1,\"id\":").unwrap();
    }
    let found = run.find_genome(pop.individuals[0].id).unwrap().unwrap();
    assert_eq!(found.format, ARTIFACT_FORMAT);
    assert_eq!(found.genome, pop.individuals[0].genome);
}

#[test]
fn concurrent_runs_get_distinct_directories() {
    let tmp = TempDir::new("distinct");
    let cfg = test_config(&tmp);
    let a = Run::create(&cfg).unwrap();
    let b = Run::create(&cfg).unwrap();
    let c = Run::create(&cfg).unwrap();
    assert_ne!(a.dir, b.dir);
    assert_ne!(b.dir, c.dir);
    assert_ne!(a.manifest.experiment_id, b.manifest.experiment_id);
}

#[test]
fn stats_file_accumulates_rows() {
    let tmp = TempDir::new("stats");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let pop = evaluated(&cfg);
    for g in 0..3 {
        let mut s = stats::summarise(&pop, 1.0, 1.0);
        s.generation = g;
        run.append_stats(&s).unwrap();
    }
    let text = fs::read_to_string(run.dir.join(STATS_FILE)).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "header plus three rows");
    assert_eq!(lines[0], GenerationStats::CSV_HEADER);
}

#[test]
fn organism_records_are_one_line_each() {
    let tmp = TempDir::new("organisms");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let pop = evaluated(&cfg);
    run.append_organisms(&pop).unwrap();
    run.append_organisms(&pop).unwrap();

    let text = fs::read_to_string(run.dir.join(ORGANISMS_FILE)).unwrap();
    assert_eq!(text.lines().count(), pop.len() * 2);
    let first: OrganismRecord = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first.id, pop.individuals[0].id);
}

#[test]
fn checkpoints_roundtrip_and_are_ordered_newest_first() {
    let tmp = TempDir::new("checkpoint");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let mut pop = evaluated(&cfg);
    run.write_checkpoint(&pop, &cfg).unwrap();
    pop.generation = 12;
    run.write_checkpoint(&pop, &cfg).unwrap();

    let paths = run.checkpoint_paths().unwrap();
    assert_eq!(paths.len(), 2);
    let latest = run.latest_checkpoint().unwrap().unwrap();
    assert_eq!(latest.format, ARTIFACT_FORMAT);
    assert_eq!(latest.population.generation, 12);
    assert_eq!(latest.population, pop);
    assert_eq!(latest.config_digest, cfg.evolution_digest());
}

/// A checkpoint is written *after* the next generation has been bred but
/// before it is evaluated, so it necessarily contains unevaluated
/// individuals. JSON cannot represent infinities, so the sentinel fitness has
/// to stay finite or resume breaks.
#[test]
fn an_unevaluated_population_survives_a_checkpoint_round_trip() {
    let tmp = TempDir::new("unevaluated");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();

    let mut pop = evaluated(&cfg);
    pop = evolution::next_generation(&pop, &cfg);
    assert!(pop.individuals.iter().all(|i| i.fitness == evolution::UNEVALUATED_FITNESS));

    let path = run.write_checkpoint(&pop, &cfg).unwrap();
    let loaded: Checkpoint = read_json(&path).unwrap();
    assert_eq!(loaded.population, pop);
}

#[test]
fn genomes_can_be_found_by_id_from_either_source() {
    let tmp = TempDir::new("find");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let pop = evaluated(&cfg);

    // Only the first organism is written to genomes.jsonl.
    run.append_genome(&pop.individuals[0]).unwrap();
    let found = run.find_genome(pop.individuals[0].id).unwrap().unwrap();
    assert_eq!(found.genome, pop.individuals[0].genome);

    // The rest are only recoverable via a checkpoint.
    let last = pop.individuals.last().unwrap();
    assert!(run.find_genome(last.id).unwrap().is_none());
    run.write_checkpoint(&pop, &cfg).unwrap();
    let found = run.find_genome(last.id).unwrap().unwrap();
    assert_eq!(found.genome, last.genome);

    assert!(run.find_genome(999_999).unwrap().is_none());
}

#[test]
fn replays_roundtrip() {
    let tmp = TempDir::new("replay");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let pop = evaluated(&cfg);
    let individual = &pop.individuals[0];
    let trace = crate::sim::evaluate(&individual.genome, &cfg, true)
        .trace
        .expect("a settled organism should produce a trace");

    let path = run.write_replay(individual, trace.clone(), &cfg).unwrap();
    let loaded: Replay = read_json(&path).unwrap();
    assert_eq!(loaded.format, ARTIFACT_FORMAT);
    assert_eq!(loaded.organism_id, individual.id);
    assert_eq!(loaded.genome, individual.genome);
    assert_eq!(loaded.trace, trace);
    assert_eq!(loaded.config_digest, cfg.evolution_digest());
}

#[test]
fn recording_selection_respects_the_policy() {
    let tmp = TempDir::new("select");
    let mut cfg = test_config(&tmp);
    cfg.recording.every_generations = 5;
    cfg.recording.top_n = 2;
    cfg.recording.random_samples = 3;

    let mut pop = evaluated(&cfg);

    pop.generation = 5;
    let chosen = selection_for_recording(&pop, &cfg);
    assert_eq!(chosen.len(), 5);
    assert_eq!(chosen[0], pop.best().id);
    let unique: HashSet<u64> = chosen.iter().copied().collect();
    assert_eq!(unique.len(), chosen.len(), "no organism recorded twice");
    // Deterministic.
    assert_eq!(chosen, selection_for_recording(&pop, &cfg));

    // Not a recording generation.
    pop.generation = 6;
    assert!(selection_for_recording(&pop, &cfg).is_empty());
}

/// `gen_{:06}` stops being lexicographically ordered past a million
/// generations, which ARCHITECTURE.md explicitly contemplates. Ordering must
/// come from the number, not the string, or a resume silently loads an older
/// population.
#[test]
fn checkpoint_ordering_survives_seven_digit_generations() {
    let tmp = TempDir::new("wide");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let mut pop = evaluated(&cfg);

    for generation in [999_999u32, 1_000_000, 42] {
        pop.generation = generation;
        run.write_checkpoint(&pop, &cfg).unwrap();
    }

    let ordered: Vec<u32> =
        run.checkpoint_paths().unwrap().iter().map(|p| checkpoint_generation(p).unwrap()).collect();
    assert_eq!(ordered, [1_000_000, 999_999, 42], "newest first, numerically");
    assert_eq!(run.latest_checkpoint().unwrap().unwrap().population.generation, 1_000_000);
}

#[test]
fn unrecognised_checkpoint_filenames_are_ignored() {
    let tmp = TempDir::new("junk");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let pop = evaluated(&cfg);
    run.write_checkpoint(&pop, &cfg).unwrap();
    fs::write(run.dir.join(CHECKPOINT_DIR).join("notes.json"), "{}").unwrap();

    let paths = run.checkpoint_paths().unwrap();
    assert_eq!(paths.len(), 1);
    assert!(run.latest_checkpoint().unwrap().is_some());
}

/// An artefact from a future evoforge may have fields whose absence changes
/// meaning, so reading it is refused rather than guessed at. Older artefacts
/// stay readable — that is the point of recording the version.
#[test]
fn a_future_format_artefact_is_refused_and_an_older_one_is_not() {
    let tmp = TempDir::new("format");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();

    let mut manifest = run.manifest.clone();
    manifest.format = ARTIFACT_FORMAT + 1;
    write_json(&run.dir.join(MANIFEST_FILE), &manifest).unwrap();
    let err = Run::open(&run.dir).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("understands at most"), "{err}");

    // A legacy manifest with no `format` field at all still opens.
    let legacy = r#"{"experiment_id":"x","experiment_name":"x",
        "evoforge_version":"0.0.1","seed":1,"config_digest":7,"created_unix":0}"#;
    fs::write(run.dir.join(MANIFEST_FILE), legacy).unwrap();
    let opened = Run::open(&run.dir).unwrap();
    assert_eq!(opened.manifest.format, legacy_format());
}

#[test]
fn a_future_format_checkpoint_is_refused() {
    let tmp = TempDir::new("format-ckpt");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let pop = evaluated(&cfg);
    let path = run.write_checkpoint(&pop, &cfg).unwrap();

    let mut checkpoint: Checkpoint = read_json(&path).unwrap();
    checkpoint.format = ARTIFACT_FORMAT + 1;
    write_json(&path, &checkpoint).unwrap();

    let err = run.latest_checkpoint().unwrap_err();
    assert!(err.to_string().contains("understands at most"), "{err}");
}

/// `evo inspect` and `evo replay --best` go through this, and they are the
/// first commands reached for after a run was killed mid-append.
#[test]
fn best_stored_genome_tolerates_a_truncated_final_line() {
    let tmp = TempDir::new("best");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    let pop = evaluated(&cfg);

    assert!(run.best_stored_genome().unwrap().is_none(), "nothing stored yet");

    let ranked = pop.ranking();
    for &i in ranked.iter().take(3) {
        run.append_genome(&pop.individuals[i]).unwrap();
    }
    {
        let mut f = append_file(&run.dir.join(GENOMES_FILE)).unwrap();
        write!(f, "{{\"format\":2,\"id\":").unwrap();
    }

    let best = run.best_stored_genome().unwrap().unwrap();
    assert_eq!(best.id, pop.individuals[ranked[0]].id);
    assert_eq!(best.fitness, pop.individuals[ranked[0]].fitness);
}

#[test]
fn elapsed_seconds_so_far_reads_the_last_row() {
    let tmp = TempDir::new("elapsed");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();
    // A fresh run has no rows yet.
    assert_eq!(run.elapsed_seconds_so_far().unwrap(), 0.0);

    let pop = evaluated(&cfg);
    for elapsed in [1.5, 9.25] {
        let mut s = stats::summarise(&pop, 1.0, elapsed);
        s.generation = 0;
        run.append_stats(&s).unwrap();
    }
    assert!((run.elapsed_seconds_so_far().unwrap() - 9.25).abs() < 1e-6);
}

#[test]
fn a_variant_replay_never_collides_with_the_runs_own() {
    let tmp = TempDir::new("variant");
    let cfg = test_config(&tmp);
    let run = Run::create(&cfg).unwrap();

    let mut other = cfg.clone();
    other.simulation.duration *= 3.0;
    let own = run.replay_path(4, 17);
    let variant = run.variant_replay_path(4, 17, other.evolution_digest());
    assert_ne!(own, variant);
    assert!(variant.to_string_lossy().contains(&format!("{:016x}", other.evolution_digest())));
}

#[test]
fn checkpoint_schedule_can_be_disabled() {
    let tmp = TempDir::new("schedule");
    let mut cfg = test_config(&tmp);
    cfg.checkpoint.every_generations = 4;
    assert!(should_checkpoint(8, &cfg));
    assert!(!should_checkpoint(9, &cfg));
    cfg.checkpoint.every_generations = 0;
    assert!(!should_checkpoint(8, &cfg));
}
