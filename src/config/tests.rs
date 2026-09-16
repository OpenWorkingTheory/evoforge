use super::*;

#[test]
fn bundled_experiments_validate() {
    for name in [
        "first-walkers.toml",
        "directed-walkers.toml",
        "animals.toml",
        "fractal-animals.toml",
        "brittle-walkers.toml",
        "jumpers.toml",
        "shaped-walkers.toml",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments").join(name);
        Config::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn empty_config_is_the_default() {
    let cfg = Config::from_toml_str("").unwrap();
    assert_eq!(cfg, Config::default());
}

#[test]
fn partial_config_overrides_only_named_fields() {
    let cfg = Config::from_toml_str(
        r#"
        [experiment]
        name = "walkers"
        seed = 99

        [evolution]
        population_size = 32
        "#,
    )
    .unwrap();
    assert_eq!(cfg.experiment.name, "walkers");
    assert_eq!(cfg.experiment.seed, 99);
    assert_eq!(cfg.evolution.population_size, 32);
    // Untouched fields keep their defaults.
    assert_eq!(cfg.evolution.generations, EvolutionCfg::default().generations);
    assert_eq!(cfg.brain.hidden, BrainCfg::default().hidden);
}

#[test]
fn unknown_fields_are_rejected() {
    let err = Config::from_toml_str(
        r#"
        [evolution]
        populaton_size = 32
        "#,
    );
    assert!(matches!(err, Err(ConfigError::Parse(_))));
}

#[test]
fn roundtrips_through_toml() {
    let cfg = Config::default();
    let text = cfg.to_toml_string();
    let back = Config::from_toml_str(&text).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn validation_catches_bad_values() {
    let bad = Config::from_toml_str(
        r#"
        [evolution]
        population_size = 4
        elite_count = 4
        "#,
    );
    assert!(matches!(bad, Err(ConfigError::Invalid(_))));

    let bad = Config::from_toml_str(
        r#"
        [body]
        min_parts = 5
        max_parts = 2
        "#,
    );
    assert!(matches!(bad, Err(ConfigError::Invalid(_))));
}

#[test]
fn digest_is_sensitive_to_changes() {
    let a = Config::default();
    let mut b = Config::default();
    b.experiment.seed = 2;
    assert_ne!(a.digest(), b.digest());
    assert_eq!(a.digest(), Config::default().digest());
}

#[test]
fn evolution_digest_ignores_bookkeeping_but_not_dynamics() {
    let a = Config::default();

    for tweak in [
        |c: &mut Config| c.evolution.generations = 9999,
        |c: &mut Config| c.experiment.output_dir = "elsewhere".into(),
        |c: &mut Config| c.experiment.name = "renamed".into(),
        |c: &mut Config| c.recording.top_n = 17,
        |c: &mut Config| c.checkpoint.every_generations = 3,
    ] {
        let mut b = Config::default();
        tweak(&mut b);
        assert_eq!(a.evolution_digest(), b.evolution_digest());
    }

    for tweak in [
        |c: &mut Config| c.experiment.seed = 2,
        |c: &mut Config| c.evolution.tournament_size = 9,
        |c: &mut Config| c.mutation.weight_sigma = 0.9,
        |c: &mut Config| c.simulation.timestep = 0.01,
        |c: &mut Config| c.simulation.baumgarte = 0.35,
        |c: &mut Config| c.environment.gravity = 3.7,
        |c: &mut Config| c.fitness.objective = Objective::DistanceX,
    ] {
        let mut b = Config::default();
        tweak(&mut b);
        assert_ne!(a.evolution_digest(), b.evolution_digest());
    }
}

/// The compatibility rule this repository lives by: a feature that is off
/// must leave the digest exactly where it was, or every run directory
/// started before it stops being resumable.
#[test]
fn the_fractal_knobs_are_invisible_until_the_fractal_terrain_is_chosen() {
    for terrain in [Terrain::Flat, Terrain::Rough] {
        let mut a = Config::default();
        a.environment.terrain = terrain;
        for tweak in [
            |c: &mut Config| c.environment.terrain_seed = 12345,
            |c: &mut Config| c.environment.terrain_octaves = 7,
            |c: &mut Config| c.environment.terrain_lacunarity = 2.5,
            |c: &mut Config| c.environment.terrain_gain = 0.75,
            |c: &mut Config| c.environment.terrain_warp = 0.0,
            |c: &mut Config| c.environment.terrain_per_trial = false,
        ] {
            let mut b = a.clone();
            tweak(&mut b);
            assert_eq!(a.digest(), b.digest(), "{terrain:?} noticed a fractal knob");
        }
    }

    // And on the fractal terrain every one of them counts.
    let mut a = Config::default();
    a.environment.terrain = Terrain::Fractal;
    for tweak in [
        |c: &mut Config| c.environment.terrain_seed = 12345,
        |c: &mut Config| c.environment.terrain_octaves = 7,
        |c: &mut Config| c.environment.terrain_lacunarity = 2.5,
        |c: &mut Config| c.environment.terrain_gain = 0.75,
        |c: &mut Config| c.environment.terrain_warp = 0.0,
        |c: &mut Config| c.environment.terrain_per_trial = false,
        |c: &mut Config| c.environment.terrain_amplitude = 0.3,
        |c: &mut Config| c.environment.terrain_wavelength = 9.0,
    ] {
        let mut b = a.clone();
        tweak(&mut b);
        assert_ne!(a.digest(), b.digest());
    }

    // The three terrains are three different experiments even at identical
    // amplitude and wavelength.
    let mut rough = Config::default();
    rough.environment.terrain = Terrain::Rough;
    assert_ne!(rough.digest(), a.digest());
    assert_ne!(Config::default().digest(), rough.digest());
}

/// Terracing is the one setting whose floor is physics rather than taste: a
/// wall thinner than a few integration steps is not a cliff, it is a
/// tunnelling bug waiting for evolution to find it.
#[test]
fn validation_measures_terrace_wall_width() {
    let base = "[environment]\nterrain = \"fractal\"\nterrain_amplitude = 3.0\n\
                terrain_wavelength = 25.0\nterrain_octaves = 5\n\
                terrain_detail_amplitude = 0.35\nterrain_step = 0.8\n";
    // As shipped: 150 mm walls, six steps at 3 m/s.
    Config::from_toml_str(&format!("{base}terrain_riser = 0.12\n")).unwrap();

    // A quarter of the riser is a quarter of the wall, and below what the
    // contact solver can meet as a surface.
    let err = Config::from_toml_str(&format!("{base}terrain_riser = 0.03\n"))
        .expect_err("a 40 mm wall should be refused");
    assert!(err.to_string().contains("terrain_riser"), "{err}");
    assert!(err.to_string().contains("mm wide"), "{err}");

    // The same walls become fine at a finer timestep, because what the rule
    // is really about is how far a body moves between contacts.
    Config::from_toml_str(&format!(
        "{base}terrain_riser = 0.03\n\n[simulation]\ntimestep = 0.001\n"
    ))
    .unwrap();

    // And with terracing off there are no walls to be too thin.
    Config::from_toml_str("[environment]\nterrain = \"fractal\"\nterrain_riser = 0.001\n").unwrap();
}

#[test]
fn validation_catches_bad_fractal_terrain() {
    for (field, value) in [
        ("terrain_octaves", "0"),
        ("terrain_octaves", "9"),
        ("terrain_lacunarity", "0.5"),
        ("terrain_gain", "1.5"),
        ("terrain_gain", "-0.1"),
        ("terrain_warp", "-1.0"),
    ] {
        let toml = format!("[environment]\nterrain = \"fractal\"\n{field} = {value}\n");
        let err = Config::from_toml_str(&toml).expect_err("{field} = {value} should fail");
        assert!(err.to_string().contains(field), "{field} = {value}: {err}");
    }
    // And the defaults are inside every one of those bounds.
    let mut ok = Config::default();
    ok.environment.terrain = Terrain::Fractal;
    ok.validate().unwrap();
}

#[test]
fn evolution_digest_is_independent_of_toml_formatting() {
    let compact = Config::from_toml_str("[experiment]\nseed = 99\n").unwrap();
    let padded = Config::from_toml_str(
        "# comment\n\n[experiment]\nseed = 99\n\n[evolution]\ngenerations = 100\n",
    )
    .unwrap();
    assert_eq!(compact.evolution_digest(), padded.evolution_digest());
    assert_eq!(
        compact.digest(),
        Config::from_toml_str("[experiment]\nseed = 99\n").unwrap().digest()
    );
}

#[test]
fn validation_rejects_rates_outside_unit_interval() {
    let err = Config::from_toml_str(
        r#"
        [evolution]
        crossover_rate = 1.5
        "#,
    );
    assert!(matches!(err, Err(ConfigError::Invalid(_))));

    let err = Config::from_toml_str(
        r#"
        [mutation]
        weight_rate = -0.1
        "#,
    );
    assert!(matches!(err, Err(ConfigError::Invalid(_))));
}

#[test]
fn validation_rejects_hinge_limits_outside_the_cosine_range() {
    let err = Config::from_toml_str(
        r#"
        [body]
        min_joint_limit = 0.3
        max_joint_limit = 2.0
        "#,
    );
    assert!(matches!(err, Err(ConfigError::Invalid(_))));
}

#[test]
fn step_counts_are_consistent() {
    let cfg = Config::default();
    assert_eq!(cfg.settle_steps(), 60);
    assert_eq!(cfg.total_steps(), 1020);
}
