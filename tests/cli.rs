//! Tests for the `evo` command line.
//!
//! These drive the real binary rather than the library, because the behaviours
//! that matter here only exist at that level: which file a command decides to
//! write, and whether it survives a run directory that was damaged by a kill.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const EVO: &str = env!("CARGO_BIN_EXE_evo");

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "evoforge-cli-{tag}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
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

const TINY_EXPERIMENT: &str = r#"
[experiment]
name = "cli-test"
seed = 99

[evolution]
population_size = 6
generations = 2

[simulation]
duration = 0.3
settle_time = 0.1

[recording]
every_generations = 1
top_n = 1

[checkpoint]
every_generations = 0
on_finish = true
"#;

fn evo(args: &[&str]) -> Output {
    let output = Command::new(EVO).args(args).output().expect("failed to launch evo");
    if !output.status.success() {
        panic!(
            "evo {args:?} failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    output
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Like [`evo`], but for commands that are *supposed* to refuse.
fn evo_output(args: &[&str]) -> Output {
    Command::new(EVO).args(args).output().expect("failed to launch evo")
}

/// The single run directory created under `out`.
fn only_dir_in(out: &Path) -> PathBuf {
    fs::read_dir(out)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .expect("no run directory was created")
}

fn nonblank_lines(path: &Path) -> usize {
    fs::read_to_string(path).unwrap().lines().filter(|l| !l.trim().is_empty()).count()
}

/// Set up a run directory and return its path along with the config that made it.
fn run_experiment(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let config = tmp.0.join("experiment.toml");
    fs::write(&config, TINY_EXPERIMENT).unwrap();
    let out = tmp.0.join("runs");
    evo(&["run", config.to_str().unwrap(), "--out", out.to_str().unwrap(), "--quiet"]);
    let dir = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .expect("no run directory was created");
    (config, dir)
}

fn replays(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join("replays"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn inspect_summarises_a_run() {
    let tmp = TempDir::new("inspect");
    let (_, dir) = run_experiment(&tmp);
    let out = stdout_of(&evo(&["inspect", dir.to_str().unwrap()]));
    assert!(out.contains("cli-test"), "{out}");
    assert!(out.contains("2 generations recorded"), "{out}");
    assert!(out.contains("best stored genome"), "{out}");
}

/// `evo replay --hz` re-samples the same simulation, which is the command's
/// documented purpose, so refreshing the run's own recording in place is correct.
#[test]
fn replay_at_a_higher_rate_refreshes_the_runs_own_recording() {
    let tmp = TempDir::new("refresh");
    let (_, dir) = run_experiment(&tmp);
    let before = replays(&dir);

    let out = stdout_of(&evo(&["replay", dir.to_str().unwrap(), "--best", "--hz", "120"]));
    assert!(!out.contains("left untouched"), "{out}");
    assert_eq!(replays(&dir), before, "no new file should appear");
}

/// `--duration` changes the simulation, so the result is not the trajectory the
/// run recorded and must not be written over it.
#[test]
fn replay_with_different_dynamics_does_not_overwrite_the_runs_recording() {
    let tmp = TempDir::new("variant");
    let (_, dir) = run_experiment(&tmp);

    let before = replays(&dir);
    let originals: Vec<(String, u64)> = before
        .iter()
        .map(|name| {
            let len = fs::metadata(dir.join("replays").join(name)).unwrap().len();
            (name.clone(), len)
        })
        .collect();

    let out = stdout_of(&evo(&["replay", dir.to_str().unwrap(), "--best", "--duration", "2.0"]));
    assert!(out.contains("left untouched"), "the user must be told: {out}");

    // Every original replay is byte-for-byte the size it was.
    for (name, len) in &originals {
        assert_eq!(
            fs::metadata(dir.join("replays").join(name)).unwrap().len(),
            *len,
            "{name} was modified"
        );
    }

    // And the variant landed beside them, tagged with its own config digest.
    let after = replays(&dir);
    let added: Vec<&String> = after.iter().filter(|n| !before.contains(n)).collect();
    assert_eq!(added.len(), 1, "expected exactly one new file, got {after:?}");
    assert!(added[0].contains("_cfg_"), "{:?}", added[0]);
}

/// A run killed mid-append leaves a partial final line. The commands used to work
/// out what happened must still work — that is the entire reason the JSON Lines
/// reader is tolerant.
#[test]
fn inspect_and_replay_survive_a_truncated_genome_line() {
    let tmp = TempDir::new("truncated");
    let (_, dir) = run_experiment(&tmp);

    let genomes = dir.join("genomes.jsonl");
    let mut text = fs::read_to_string(&genomes).unwrap();
    text.push_str("{\"format\":2,\"id\":123,\"gen");
    fs::write(&genomes, text).unwrap();

    let out = stdout_of(&evo(&["inspect", dir.to_str().unwrap()]));
    assert!(out.contains("best stored genome"), "{out}");
    let out = stdout_of(&evo(&["replay", dir.to_str().unwrap(), "--best"]));
    assert!(out.contains("re-simulated fitness"), "{out}");
}

/// A resumed run must not append a second copy of a generation, whichever
/// checkpoint it landed on. Driven through the CLI because this is how a
/// preempted worker actually comes back.
#[test]
fn resuming_through_the_cli_does_not_duplicate_records() {
    let tmp = TempDir::new("resume");
    let config = tmp.0.join("experiment.toml");
    // Periodic checkpoints, no on-finish snapshot: a killed worker's situation.
    fs::write(
        &config,
        TINY_EXPERIMENT
            .replace("every_generations = 0", "every_generations = 1")
            .replace("on_finish = true", "on_finish = false"),
    )
    .unwrap();
    let out_dir = tmp.0.join("runs");
    let config = config.to_str().unwrap();
    let out_arg = out_dir.to_str().unwrap();

    evo(&["run", config, "--out", out_arg, "--quiet"]);
    let dir = fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .unwrap();

    evo(&[
        "run",
        config,
        "--out",
        out_arg,
        "--quiet",
        "--resume",
        dir.to_str().unwrap(),
        "--generations",
        "4",
    ]);

    let stats = fs::read_to_string(dir.join("stats.csv")).unwrap();
    let generations: Vec<&str> = stats
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split(',').next().unwrap())
        .collect();
    assert_eq!(generations, ["0", "1", "2", "3"], "stats.csv: {generations:?}");

    let organisms = fs::read_to_string(dir.join("organisms.jsonl")).unwrap();
    let count = organisms.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(count, 4 * 6, "one record per organism per generation");
}

#[test]
fn verify_proves_determinism_for_a_real_config() {
    let tmp = TempDir::new("verify");
    let config = tmp.0.join("experiment.toml");
    fs::write(&config, TINY_EXPERIMENT).unwrap();
    let out = stdout_of(&evo(&["verify", config.to_str().unwrap(), "--generations", "2"]));
    assert!(out.contains("identical"), "{out}");
}

/// `evo evaluate` writes a run directory a student can read like any other:
/// one generation, every organism, every genome, where each founder came from,
/// and nothing bred.
#[test]
fn evaluate_writes_a_self_describing_directory() {
    let tmp = TempDir::new("evaluate");
    let (config, source) = run_experiment(&tmp);
    let out = tmp.0.join("evals");

    let text = stdout_of(&evo(&[
        "evaluate",
        config.to_str().unwrap(),
        "--founders",
        source.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--quiet",
    ]));
    assert!(text.contains("results in"), "{text}");
    let dir = only_dir_in(&out);

    assert_eq!(nonblank_lines(&dir.join("stats.csv")), 2, "header plus one generation");
    assert_eq!(nonblank_lines(&dir.join("organisms.jsonl")), 6);
    assert_eq!(nonblank_lines(&dir.join("genomes.jsonl")), 6, "every genome is stored");
    assert_eq!(nonblank_lines(&dir.join("founders.jsonl")), 6, "every founder has provenance");
    let checkpoints = fs::read_dir(dir.join("checkpoints")).unwrap().count();
    assert_eq!(checkpoints, 0, "nothing was bred, so nothing to resume");
}

/// A population bred under one controller layout cannot be scored under
/// another; the refusal names the field and leaves no directory behind.
#[test]
fn evaluate_refuses_founders_from_a_different_controller_layout() {
    let tmp = TempDir::new("evaluate-refused");
    let (_, source) = run_experiment(&tmp);

    let wider = tmp.0.join("wider.toml");
    fs::write(&wider, format!("{TINY_EXPERIMENT}\n[brain]\nhidden = 13\n")).unwrap();
    let out = tmp.0.join("evals");

    let output = evo_output(&[
        "evaluate",
        wider.to_str().unwrap(),
        "--founders",
        source.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--quiet",
    ]);
    assert!(!output.status.success(), "should have refused");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("brain.hidden"), "names the field: {stderr}");
    assert!(!out.exists() || fs::read_dir(&out).unwrap().count() == 0, "no half-made directory");
}

/// A founded run's manifest still carries a seed, and that seed did *not*
/// produce generation 0. `inspect` has to say where it really came from, and
/// `verify` has to be able to prove determinism along the same path.
#[test]
fn inspect_and_verify_understand_a_founded_run() {
    let tmp = TempDir::new("founded-inspect");
    let (config, source) = run_experiment(&tmp);
    let config = config.to_str().unwrap();
    let source_arg = source.to_str().unwrap();

    let runs2 = tmp.0.join("runs2");
    evo(&["run", config, "--founders", source_arg, "--out", runs2.to_str().unwrap(), "--quiet"]);
    let founded = only_dir_in(&runs2);

    let out = stdout_of(&evo(&["inspect", founded.to_str().unwrap()]));
    assert!(out.contains("founded from  6 organism(s)"), "{out}");
    let source_id = source.file_name().unwrap().to_string_lossy().into_owned();
    assert!(out.contains(&source_id), "names the source run: {out}");

    let out = stdout_of(&evo(&["verify", config, "--generations", "2", "--founders", source_arg]));
    assert!(out.contains("imported founders"), "{out}");
    assert!(out.contains("identical"), "{out}");
}

/// An evaluate directory has no checkpoint, but it stored every genome, so it
/// founds a new run just as a finished run does.
#[test]
fn an_evaluate_directory_can_found_a_new_run() {
    let tmp = TempDir::new("evaluate-chain");
    let (config, source) = run_experiment(&tmp);
    let config = config.to_str().unwrap();

    let evals = tmp.0.join("evals");
    evo(&[
        "evaluate",
        config,
        "--founders",
        source.to_str().unwrap(),
        "--out",
        evals.to_str().unwrap(),
        "--quiet",
    ]);
    let scored = only_dir_in(&evals);

    let runs2 = tmp.0.join("runs2");
    evo(&[
        "run",
        config,
        "--founders",
        scored.to_str().unwrap(),
        "--out",
        runs2.to_str().unwrap(),
        "--quiet",
    ]);
    let founded = only_dir_in(&runs2);

    assert_eq!(nonblank_lines(&founded.join("founders.jsonl")), 6);
    // Generation 0 is the imported six; generation 1 is bred back to the
    // configured six; two generations of records in all.
    assert_eq!(nonblank_lines(&founded.join("organisms.jsonl")), 12);
    assert!(
        fs::read_dir(founded.join("checkpoints")).unwrap().count() > 0,
        "a real run checkpoints"
    );
}
