use super::*;
use crate::rng::Rng;

fn quick_config() -> Config {
    let mut cfg = Config::default();
    cfg.simulation.duration = 2.0;
    cfg.simulation.settle_time = 0.25;
    cfg
}

fn random_genome(cfg: &Config, seed: u64) -> Genome {
    Genome::random(&mut Rng::new(seed), &cfg.body, &cfg.brain, &cfg.brain_layout())
}

/// A structural test of the trial-aggregation contract, not of any one field.
///
/// `accumulate` and `scale_metrics` enumerate every member of `Metrics` by
/// hand, and neither fails to compile when a newly added field is missed.
/// Forgotten in `accumulate`, the field silently reads zero in any
/// multi-trial experiment; forgotten in `scale_metrics`, it reads `n` times
/// too large. With every trial made identical, running one trial and running
/// three must give exactly the same metrics — summed-then-scaled fields
/// because the mean of n copies is the value, and the `max`-aggregated ones
/// because the maximum of n copies is too.
#[test]
fn trial_count_does_not_change_the_metrics_when_the_trials_are_identical() {
    let mut cfg = quick_config();
    // Nothing may differ between trials: no start jitter, no steering, and
    // flat ground so there is no per-trial landscape shift either.
    cfg.simulation.start_jitter = 0.0;
    cfg.simulation.steer = false;
    cfg.environment.terrain = crate::config::Terrain::Flat;
    let g = random_genome(&cfg, 11);

    cfg.simulation.trials = 1;
    let one = evaluate(&g, &cfg, false).metrics;
    cfg.simulation.trials = 3;
    let three = evaluate(&g, &cfg, false).metrics;

    let close = |name: &str, a: Real, b: Real| {
        let tol = 1e-4 * a.abs().max(1.0);
        assert!(
            (a - b).abs() <= tol,
            "{name}: one trial gave {a}, three identical trials gave {b}.                  A field missing from `accumulate` reads zero; one missing from                  `scale_metrics` reads three times too large."
        );
    };
    close("start.y", one.start.y, three.start.y);
    close("end.y", one.end.y, three.end.y);
    close("displacement", one.displacement, three.displacement);
    close("displacement_x", one.displacement_x, three.displacement_x);
    close("path_length", one.path_length, three.path_length);
    close("max_displacement", one.max_displacement, three.max_displacement);
    close("mean_height", one.mean_height, three.mean_height);
    close("upright_seconds", one.upright_seconds, three.upright_seconds);
    close("actuation", one.actuation, three.actuation);
    close("duration", one.duration, three.duration);
    close("peak_height", one.peak_height, three.peak_height);
    close("airborne_seconds", one.airborne_seconds, three.airborne_seconds);
    close("heading_progress", one.heading_progress, three.heading_progress);
    close("net_gain", one.net_gain, three.net_gain);
    close("net_loss", one.net_loss, three.net_loss);
    close("climb", one.climb, three.climb);
    close("descent", one.descent, three.descent);
    assert_eq!(one.steps, three.steps, "steps");
    assert_eq!(one.joints_lost, three.joints_lost, "joints_lost");
}

/// Net gain and loss are clamped per trial and only then averaged, so an
/// organism that climbs on one trial and falls on another reports both
/// rather than netting them to nothing.
#[test]
fn net_gain_and_loss_are_never_both_zero_for_an_organism_that_moved_vertically() {
    let cfg = quick_config();
    let g = random_genome(&cfg, 12);
    let m = evaluate(&g, &cfg, false).metrics;
    let net = m.end.y - m.start.y;
    if net > 1e-4 {
        assert!(m.net_gain > 0.0 && m.net_loss == 0.0, "{m:?}");
    } else if net < -1e-4 {
        assert!(m.net_loss > 0.0 && m.net_gain == 0.0, "{m:?}");
    }
}

fn sensing_config() -> Config {
    let mut cfg = quick_config();
    cfg.body.sensor_probability = 1.0; // every part carries one
    cfg.sensor.rays = 3;
    cfg.sensor.range = 4.0;
    cfg
}

/// The layout must grow by exactly one input per ray per slot, and the
/// sensor channels must sit *after* the proprioceptive ones so that adding
/// them never moves an input that already existed.
#[test]
fn sensors_add_inputs_without_moving_the_existing_ones() {
    let blind = quick_config().brain_layout();
    let seeing = sensing_config().brain_layout();
    assert_eq!(seeing.sensor_inputs, 3);
    assert_eq!(seeing.slot_inputs, blind.slot_inputs + 3);
    assert_eq!(seeing.global_inputs, blind.global_inputs, "sensing is not a global sense");
    for slot in 0..blind.max_slots {
        assert_eq!(
            seeing.sensor_input_base(slot),
            seeing.slot_input_base(slot) + blind.slot_inputs,
            "slot {slot}: sensor channels must follow the proprioceptive ones"
        );
    }
}

/// Off is exact, not approximately off.
///
/// An experiment that declares no sensors must draw the identical random
/// stream and evaluate identically to one built before sensors existed. The
/// genome is the sensitive part: a sensor gene drawn unconditionally would
/// shift every later draw and silently invalidate every stored result.
#[test]
fn an_experiment_without_sensors_is_untouched_by_them() {
    let cfg = quick_config();
    assert_eq!(cfg.sensor_channels(), 0);
    assert!(!cfg.brain_layout().senses_range());
    for seed in 0..32 {
        let g = random_genome(&cfg, seed);
        assert!(
            g.parts.iter().all(|p| !p.sensor && p.sensor_dir == crate::math::Vec3::ZERO),
            "seed {seed}: a sensorless experiment drew a sensor gene"
        );
    }
}

/// A sensor pointing straight down from a resting body must measure the gap
/// beneath it, which `ground_clearance` computes by a completely different
/// route.
#[test]
fn a_downward_sensor_measures_the_ground_beneath_it() {
    use crate::physics::TerrainModel;
    let terrain = TerrainModel::Rough { amplitude: 0.2, wavelength: 3.0 };
    for (x, z) in [(0.0, 0.0), (0.7, -1.3), (-2.2, 0.9)] {
        let h = terrain.height_at(x, z);
        let origin = crate::math::vec3(x, h + 0.75, z);
        let d = terrain
            .raycast(origin, crate::math::vec3(0.0, -1.0, 0.0), 4.0)
            .expect("ground is below");
        assert!((d - 0.75).abs() < 0.02, "at ({x}, {z}) the sensor read {d} for a 0.75 m gap");
    }
}

/// A sensor is only worth having if it distinguishes situations. Facing a
/// rising slope must read differently from facing open air.
#[test]
fn a_sensor_reads_terrain_and_says_so() {
    let cfg = sensing_config();
    let layout = cfg.brain_layout();
    let g = random_genome(&cfg, 3);
    assert!(g.parts.iter().any(|p| p.sensor), "the fixture should carry a sensor");
    let pheno = crate::phenotype::build(&g, &cfg);
    assert!(!pheno.sensors.is_empty(), "a sensor gene produced no mount");
    for m in &pheno.sensors {
        assert!(
            (m.dir.length() - 1.0).abs() < 1e-4,
            "a mount direction must be normalised, got {:?}",
            m.dir
        );
        assert!((m.body as usize) < pheno.world.bodies.len());
    }
    // Every mount answers to a slot that the layout has channels for.
    for m in &pheno.sensors {
        let slot = pheno.body_slots[m.body as usize] as usize;
        assert!(layout.sensor_input_base(slot) + layout.sensor_inputs <= layout.inputs());
    }
}

/// Sensor readings must be bounded, or a freshly initialised network is
/// saturated by them and the input is worse than useless.
#[test]
fn sensor_readings_stay_within_the_unit_range() {
    let cfg = sensing_config();
    let layout = cfg.brain_layout();
    for seed in 0..12 {
        let g = random_genome(&cfg, seed);
        let mut ws = EvalWorkspace::new(&cfg);
        let r = evaluate_with(&g, &cfg, false, &mut ws);
        assert!(!r.metrics.diverged || r.fitness <= 0.0);
        // Re-run the controller once against the settled organism and read
        // the inputs it produced.
        let pheno = crate::phenotype::build(&g, &cfg);
        let mut pheno = pheno;
        apply_control(
            &mut pheno,
            &layout,
            &g.weights,
            &mut ws,
            &ControlContext { t: 0.0, drive: 1.0, heading: Vec3::X, sensor: &cfg.sensor },
        );
        for slot in 0..layout.max_slots {
            let base = layout.sensor_input_base(slot);
            for r in 0..layout.sensor_inputs {
                let v = ws.scratch.inputs[base + r];
                assert!(
                    (0.0..=1.0).contains(&v),
                    "seed {seed} slot {slot} ray {r}: reading {v} is outside [0, 1]"
                );
            }
        }
    }
}

#[test]
fn evaluation_is_reproducible() {
    let cfg = quick_config();
    let g = random_genome(&cfg, 4);
    let a = evaluate(&g, &cfg, false);
    let b = evaluate(&g, &cfg, false);
    assert_eq!(a.fitness, b.fitness);
    assert_eq!(a.metrics, b.metrics);
}

#[test]
fn recording_does_not_change_the_simulation() {
    let cfg = quick_config();
    let g = random_genome(&cfg, 5);
    let plain = evaluate(&g, &cfg, false);
    let recorded = evaluate(&g, &cfg, true);
    assert_eq!(plain.fitness, recorded.fitness);
    assert_eq!(plain.metrics, recorded.metrics);
    assert!(recorded.trace.is_some());
    assert!(plain.trace.is_none());
}

#[test]
fn trace_shape_matches_the_organism() {
    let cfg = quick_config();
    let g = random_genome(&cfg, 6);
    let r = evaluate(&g, &cfg, true);
    let tr = r.trace.unwrap();
    assert_eq!(tr.bodies.len(), g.part_count());
    assert!(!tr.frames.is_empty());
    for f in &tr.frames {
        assert_eq!(f.poses.len(), g.part_count() * 7);
        assert!(f.poses.iter().all(|v| v.is_finite()));
    }
    // Frames are in time order.
    for w in tr.frames.windows(2) {
        assert!(w[1].t > w[0].t);
    }
}

#[test]
fn metrics_are_self_consistent() {
    let cfg = quick_config();
    for seed in 0..40 {
        let g = random_genome(&cfg, 200 + seed);
        let m = evaluate(&g, &cfg, false).metrics;
        if m.diverged {
            continue;
        }
        assert!(m.displacement >= 0.0);
        // A straight-line displacement can never exceed the path walked.
        assert!(
            m.path_length >= m.displacement - 1e-3,
            "seed {seed}: path {} < displacement {}",
            m.path_length,
            m.displacement
        );
        assert!(m.max_displacement >= m.displacement - 1e-3);
        assert!(m.upright_seconds <= m.duration + 1e-4);
        assert!((m.duration - cfg.simulation.duration).abs() < 0.05);
    }
}

#[test]
fn a_still_organism_scores_near_zero() {
    // Zero weights mean zero motor targets, so nothing should move much
    // after settling. Note this does *not* mean zero actuation: a motor
    // commanded to zero velocity still spends impulse holding the joint
    // against gravity, which is exactly what a real actuator does.
    let cfg = quick_config();
    let mut g = random_genome(&cfg, 7);
    for w in g.weights.iter_mut() {
        *w = 0.0;
    }
    let r = evaluate(&g, &cfg, false);
    assert!(r.metrics.displacement < 0.2, "displacement {}", r.metrics.displacement);
    assert!(r.fitness < 0.2);
}

#[test]
fn an_active_controller_actually_actuates() {
    let cfg = quick_config();
    let mut any_actuation = false;
    for seed in 0..30 {
        let g = random_genome(&cfg, 300 + seed);
        if g.hinge_count() == 0 {
            continue;
        }
        let m = evaluate(&g, &cfg, false).metrics;
        if m.actuation > 0.0 {
            any_actuation = true;
            break;
        }
    }
    assert!(any_actuation, "no random organism moved a joint at all");
}

#[test]
fn most_random_organisms_do_not_diverge() {
    let cfg = quick_config();
    let mut diverged = 0;
    let n = 100;
    for seed in 0..n {
        if evaluate(&random_genome(&cfg, 500 + seed), &cfg, false).metrics.diverged {
            diverged += 1;
        }
    }
    assert!(diverged < n / 10, "{diverged}/{n} organisms diverged");
}

/// Shapes are new geometry meeting an old solver. A sphere resting on one
/// contact point and a cylinder standing on its rim are both cases a box
/// never produced, so the divergence budget has to be checked against them
/// specifically rather than inferred from the box result.
#[test]
fn most_random_shaped_organisms_do_not_diverge() {
    let mut cfg = quick_config();
    cfg.body.shapes = vec![
        crate::genome::ShapeKind::Box,
        crate::genome::ShapeKind::Taper,
        crate::genome::ShapeKind::Sphere,
        crate::genome::ShapeKind::Capsule,
        crate::genome::ShapeKind::Cylinder,
    ];
    let mut diverged = 0;
    let n = 100;
    for seed in 0..n {
        if evaluate(&random_genome(&cfg, 500 + seed), &cfg, false).metrics.diverged {
            diverged += 1;
        }
    }
    assert!(diverged < n / 10, "{diverged}/{n} shaped organisms diverged");
}

fn repeated_config() -> Config {
    let mut cfg = quick_config();
    cfg.simulation.trials = 4;
    cfg.simulation.start_jitter = 0.6;
    cfg
}

/// Repeating a trial must not make evaluation any less of a pure function.
/// The perturbations come from the experiment seed and the trial index, so
/// the same organism always meets the same four worlds.
#[test]
fn repeated_trials_are_deterministic() {
    let cfg = repeated_config();
    for seed in 0..12 {
        let g = random_genome(&cfg, 900 + seed);
        let a = evaluate(&g, &cfg, false);
        let b = evaluate(&g, &cfg, false);
        assert_eq!(a.fitness.to_bits(), b.fitness.to_bits(), "seed {seed}");
    }
}

/// Every organism faces the same starts — common random numbers — so a
/// score difference is a difference between organisms, not between the
/// worlds they happened to draw. Changing the experiment seed changes the
/// worlds; changing the organism must not.
#[test]
fn every_organism_meets_the_same_worlds() {
    let mut cfg = repeated_config();
    let g = random_genome(&cfg, 4242);
    let first = evaluate(&g, &cfg, false).fitness;
    cfg.experiment.seed = cfg.experiment.seed.wrapping_add(1);
    let moved = evaluate(&g, &cfg, false).fitness;
    assert!(
        first.to_bits() != moved.to_bits(),
        "the trial starts ignored the experiment seed, so they are not varying at all"
    );
}

/// The worst trial can never flatter an organism more than the mean of them.
#[test]
fn the_worst_trial_never_scores_above_the_mean() {
    let mut cfg = repeated_config();
    for seed in 0..12 {
        let g = random_genome(&cfg, 700 + seed);
        cfg.simulation.aggregate = crate::config::Aggregate::Mean;
        let mean = evaluate(&g, &cfg, false).fitness;
        cfg.simulation.aggregate = crate::config::Aggregate::Worst;
        let worst = evaluate(&g, &cfg, false).fitness;
        assert!(worst <= mean + 1e-4, "seed {seed}: worst {worst} above mean {mean}");
    }
}

/// A jittered start actually moves the organism, or repeating the trial is
/// theatre.
#[test]
fn a_jittered_start_actually_varies_the_outcome() {
    let cfg = repeated_config();
    let g = random_genome(&cfg, 31);
    let mut seen = Vec::new();
    for trial in 0..4u64 {
        let mut rng = Rng::new(derive_seed(&[cfg.experiment.seed, TRIAL_STREAM, trial]));
        let start = perturbation(&mut rng, cfg.simulation.start_jitter);
        let mut ws = EvalWorkspace::new(&cfg);
        seen.push(run_trial(&g, &cfg, false, &mut ws, Some(start)).fitness);
    }
    assert!(
        seen.windows(2).any(|w| w[0].to_bits() != w[1].to_bits()),
        "every trial produced an identical score: {seen:?}"
    );
}

fn fractal_config() -> Config {
    let mut cfg = repeated_config();
    cfg.environment.terrain = crate::config::Terrain::Fractal;
    cfg.environment.terrain_amplitude = 0.25;
    cfg.environment.terrain_wavelength = 6.0;
    cfg
}

/// The point of layer 2: each trial is run on a different piece of the
/// landscape, so a gait tuned to one hill is not a gait.
#[test]
fn each_trial_gets_its_own_piece_of_the_landscape() {
    let cfg = fractal_config();
    assert!(cfg.environment.terrain_per_trial);
    let mut seen = Vec::new();
    for trial in 0..4u64 {
        let mut rng = Rng::new(derive_seed(&[cfg.experiment.seed, TRIAL_STREAM, trial]));
        let mut start = perturbation(&mut rng, cfg.simulation.start_jitter);
        let _ = commanded_heading(&mut rng, &cfg);
        start.terrain = terrain_shift(&mut rng, &cfg);
        assert_ne!(start.terrain, phenotype::TerrainShift::NONE, "trial {trial} unmoved");
        seen.push(start.terrain);
    }
    for (i, a) in seen.iter().enumerate() {
        for b in &seen[i + 1..] {
            assert_ne!(a, b, "two trials drew the same landscape");
        }
    }
}

/// The shift is drawn from the trial index and the experiment seed and
/// nothing else, so every organism still meets the same set of worlds.
#[test]
fn the_terrain_shift_is_a_pure_function_of_the_trial() {
    let cfg = fractal_config();
    let draw = |trial: u64, cfg: &Config| {
        let mut rng = Rng::new(derive_seed(&[cfg.experiment.seed, TRIAL_STREAM, trial]));
        let _ = perturbation(&mut rng, cfg.simulation.start_jitter);
        let _ = commanded_heading(&mut rng, cfg);
        terrain_shift(&mut rng, cfg)
    };
    assert_eq!(draw(2, &cfg), draw(2, &cfg));
    let mut moved = cfg.clone();
    moved.experiment.seed += 1;
    assert_ne!(draw(2, &cfg), draw(2, &moved));
}

/// Compatibility discipline: the shift is drawn *after* everything that
/// existed before it, and only when it is used. Every terrain but `fractal`
/// — and `fractal` with the feature off — must therefore leave the trial
/// stream exactly where it was, or every recorded experiment stops
/// reproducing.
#[test]
fn a_terrain_that_does_not_move_draws_nothing() {
    let mut cfg = repeated_config();
    for terrain in [
        crate::config::Terrain::Flat,
        crate::config::Terrain::Rough,
        crate::config::Terrain::Fractal,
    ] {
        cfg.environment.terrain = terrain;
        cfg.environment.terrain_per_trial = false;
        let mut rng = Rng::new(1234);
        assert_eq!(terrain_shift(&mut rng, &cfg), phenotype::TerrainShift::NONE);
        // The stream is untouched: the very next draw is the first draw.
        let mut fresh = Rng::new(1234);
        assert_eq!(rng.next_u64(), fresh.next_u64());
    }
}

/// A corpse must not travel.
///
/// The single most useful test in this file, and it did not exist for the
/// two months in which every headline result was inflated by its absence.
/// An organism with its motors switched off has nothing to move it: no
/// muscle, no tendon, and — once it has settled — no potential energy to
/// spend. Whatever ground it covers is ground the *world* gave it, and any
/// locomotion score is only meaningful above that number.
///
/// Run over ordinary random genomes rather than evolved ones on purpose:
/// evolution is what finds the exploit, so a test that waits for evolution
/// to find it has already let a run be wasted. Big terrain makes this
/// sharper, not softer — five metres of relief is metres of free
/// displacement for anything that will roll downhill.
#[test]
fn a_dead_organism_does_not_travel() {
    for terrain in [
        crate::config::Terrain::Flat,
        crate::config::Terrain::Rough,
        crate::config::Terrain::Fractal,
    ] {
        let mut cfg = fractal_config();
        cfg.environment.terrain = terrain;
        // The organism's own throttle, turned to zero. Not a special case in
        // the simulator: `caution` and `min_drive` are how an evolved
        // organism holds back, and this is that mechanism at its limit. It
        // is only consulted when joints can wear out, so that has to be on.
        cfg.body.joint_endurance = 150.0;
        cfg.body.min_drive = 0.0;
        assert!(cfg.joints_can_break());

        let mut worst: Real = 0.0;
        for seed in 0..24 {
            let mut g = random_genome(&cfg, 5_000 + seed);
            g.caution = 1.0;
            let m = evaluate(&g, &cfg, false).metrics;
            worst = worst.max(m.displacement);
        }
        assert!(
            worst < 2.0,
            "{terrain:?}: a motorless organism covered {worst} m in {} s",
            cfg.simulation.duration
        );
    }
}

/// Moving the ground has to change what happens on it, or layer 2 is
/// bookkeeping.
#[test]
fn a_moved_landscape_changes_the_outcome() {
    let cfg = fractal_config();
    let g = random_genome(&cfg, 31);
    let mut ws = EvalWorkspace::new(&cfg);
    let flat_start = phenotype::StartPerturbation::default();
    let base = run_trial(&g, &cfg, false, &mut ws, Some(flat_start)).fitness;
    let moved = run_trial(
        &g,
        &cfg,
        false,
        &mut ws,
        Some(phenotype::StartPerturbation {
            terrain: phenotype::TerrainShift {
                offset_x: 19.0,
                offset_z: -23.0,
                sin: 0.6,
                cos: 0.8,
            },
            ..flat_start
        }),
    )
    .fitness;
    assert_ne!(base.to_bits(), moved.to_bits(), "the landscape did not move");
}

/// A single-trial fractal experiment still varies its ground, so it takes
/// the trial loop rather than the canonical-start fast path.
#[test]
fn one_trial_on_moving_ground_still_takes_the_trial_path() {
    let mut cfg = fractal_config();
    cfg.simulation.trials = 1;
    cfg.simulation.start_jitter = 0.0;
    let g = random_genome(&cfg, 8);
    let varied = evaluate(&g, &cfg, false).fitness;
    cfg.environment.terrain_per_trial = false;
    let fixed = evaluate(&g, &cfg, false).fitness;
    assert_ne!(varied.to_bits(), fixed.to_bits());
}

/// A trace has to carry enough for the viewer to prove it is drawing the
/// same ground the physics used.
#[test]
fn a_trace_records_samples_of_the_ground_it_ran_on() {
    let mut cfg = fractal_config();
    cfg.recording.record_hz = 10.0;
    let g = random_genome(&cfg, 12);
    let trace = evaluate(&g, &cfg, true).trace.expect("recorded");
    assert_eq!(trace.terrain_check.len(), TERRAIN_CHECK_POINTS.len() * 3);
    for s in trace.terrain_check.chunks(3) {
        assert_eq!(s[2], trace.terrain.height_at(s[0], s[1]));
    }
    // The samples describe the ground this *trial* ran on, moved and all.
    assert!(trace.terrain_check.chunks(3).any(|s| s[2] != 0.0));

    // Flat ground has nothing to check, and the field is left out entirely.
    cfg.environment.terrain = crate::config::Terrain::Flat;
    let flat = evaluate(&g, &cfg, true).trace.expect("recorded");
    assert!(flat.terrain_check.is_empty());
    let json = serde_json::to_string(&flat).unwrap();
    assert!(!json.contains("terrain_check"));
}

/// A steered experiment gives the controller the command and scores what it
/// did with it. Progress is measured along the commanded heading, so an
/// organism sent one way and travelling another earns nothing.
#[test]
fn heading_progress_measures_the_commanded_direction() {
    let mut cfg = quick_config();
    cfg.simulation.steer = true;
    assert!(cfg.brain_layout().is_steered());
    assert!(cfg.brain_layout().weight_count() > quick_config().brain_layout().weight_count());

    let g = random_genome(&cfg, 77);
    let mut ws = EvalWorkspace::new(&cfg);
    // Straight down +X: progress and displacement_x are the same measurement.
    let ahead = run_trial_towards(&g, &cfg, false, &mut ws, None, Vec3::X);
    assert!(
        (ahead.metrics.heading_progress - ahead.metrics.displacement_x).abs() < 1e-4,
        "along +X the two should agree: {} vs {}",
        ahead.metrics.heading_progress,
        ahead.metrics.displacement_x
    );

    // Commanded sideways, progress is what it did along +Z instead.
    let across = run_trial_towards(&g, &cfg, false, &mut ws, None, Vec3::Z);
    let moved = across.metrics.end - across.metrics.start;
    assert!(
        (across.metrics.heading_progress - moved.z).abs() < 1e-4,
        "across +Z progress should be the Z displacement: {} vs {}",
        across.metrics.heading_progress,
        moved.z
    );
}

/// Commanded headings must actually vary between trials, or steering is
/// nothing but an extra input.
#[test]
fn commanded_headings_vary_between_trials() {
    let mut cfg = quick_config();
    cfg.simulation.steer = true;
    cfg.simulation.steer_spread = 1.0;
    let mut seen = Vec::new();
    for trial in 0..6u64 {
        let mut rng = Rng::new(derive_seed(&[cfg.experiment.seed, TRIAL_STREAM, trial]));
        let _ = perturbation(&mut rng, cfg.simulation.start_jitter);
        let h = commanded_heading(&mut rng, &cfg);
        assert!((h.length() - 1.0).abs() < 1e-4, "heading not a unit vector: {h:?}");
        seen.push(h);
    }
    assert!(
        seen.windows(2).any(|w| (w[0] - w[1]).length() > 1e-3),
        "every trial commanded the same direction"
    );
    // And an unsteered experiment always commands +X.
    let plain = quick_config();
    let mut rng = Rng::new(1);
    assert_eq!(commanded_heading(&mut rng, &plain), Vec3::X);
}

#[test]
fn steps_per_rounds_sensibly() {
    assert_eq!(steps_per(20.0, 1.0 / 120.0), 6);
    assert_eq!(steps_per(30.0, 1.0 / 120.0), 4);
    // Faster than the physics rate still means every step, never zero.
    assert_eq!(steps_per(1000.0, 1.0 / 120.0), 1);
}

#[test]
fn control_stays_off_during_settle() {
    let interval = 6;
    for step in 0..60 {
        assert!(!should_apply_control(step, 60, interval), "step {step}");
    }
    assert!(should_apply_control(60, 60, interval));
    assert!(!should_apply_control(61, 60, interval));
    assert!(should_apply_control(66, 60, interval));
}

/// Effort must be charged for the measured window only. The settle drop
/// spends real motor impulse holding joints against gravity, but the
/// controller is switched off for it, so including it would make
/// `energy_penalty` scale with `settle_time` — pricing a fall the organism
/// could not influence.
#[test]
fn settle_actuation_is_not_charged_to_the_measured_window() {
    // Same settle, so the physics up to the measurement boundary is identical
    // and only the window length differs. Shrinking the window towards zero
    // must take reported effort towards zero with it; while the settle total
    // was included, this left a large constant intercept instead.
    let mut brief = quick_config();
    brief.simulation.settle_time = 1.0; // 120 steps of holding impulse
    brief.simulation.duration = 0.025; // 3 measured steps
    let mut full = brief.clone();
    full.simulation.duration = 1.0; // 120 measured steps

    let mut checked = 0;
    for seed in 0..40 {
        let g = random_genome(&full, 700 + seed);
        if g.hinge_count() == 0 {
            continue;
        }
        let a = evaluate(&g, &brief, false).metrics;
        let b = evaluate(&g, &full, false).metrics;
        if a.diverged || b.diverged || b.actuation <= 0.0 {
            continue;
        }
        assert!(
            a.actuation < 0.1 * b.actuation,
            "seed {seed}: {} over 3 measured steps vs {} over 120 — \
             the settle window is leaking into the total",
            a.actuation,
            b.actuation
        );
        checked += 1;
    }
    assert!(checked > 5, "only {checked} organisms actuated at all");
}

#[test]
fn measurement_starts_where_the_trace_says_it_does() {
    let cfg = quick_config();
    let g = random_genome(&cfg, 12);
    let r = evaluate(&g, &cfg, true);
    let tr = r.trace.unwrap();
    // The frame at `measure_start_t` must be the pose that `metrics.start`
    // was taken from, or a viewer highlights the wrong window.
    let frame = tr
        .frames
        .iter()
        .find(|f| (f.t - tr.measure_start_t).abs() < 1e-6)
        .expect("no frame at the declared measurement start");
    let com = centre_of_mass_of(&frame.poses, &tr.bodies, &cfg);
    assert!(
        (com - r.metrics.start).length() < 1e-4,
        "trace says measurement starts at {:?}, metrics say {:?}",
        com,
        r.metrics.start
    );
}

/// Mass-weighted centre of a recorded frame, reconstructed the way a viewer
/// would have to.
fn centre_of_mass_of(poses: &[Real], bodies: &[BodySpec], cfg: &Config) -> Vec3 {
    let mut total = 0.0;
    let mut acc = Vec3::ZERO;
    for (i, spec) in bodies.iter().enumerate() {
        let (m, _) = spec.geometry().mass_properties(cfg.body.density);
        let p = crate::math::vec3(poses[i * 7], poses[i * 7 + 1], poses[i * 7 + 2]);
        acc += p * m;
        total += m;
    }
    acc * (1.0 / total)
}
