//! The resume guard: a canonical hash of everything that affects dynamics.

use super::*;

/// Version of the dynamics fingerprint layout. Bump when a field is added,
/// removed or reinterpreted — existing checkpoints will then correctly refuse
/// to resume rather than silently continue under a different hash.
const FINGERPRINT_VERSION: u32 = 1;

/// Canonical, serializer-independent fingerprint.
///
/// New dynamics fields must be appended here. Pretty-printed TOML is not used:
/// field order, comments and crate upgrades must not change the digest.
/// Whether a shape roster asks for anything a pre-shapes build would not have done.
fn uses_shapes(shapes: &[ShapeKind]) -> bool {
    shapes.len() > 1 || (shapes.len() == 1 && shapes[0] != ShapeKind::Box)
}

pub(super) fn fingerprint(cfg: &Config, include_bookkeeping: bool) -> u64 {
    let mut f = Fingerprint::new();
    f.u32(FINGERPRINT_VERSION);
    f.bool(include_bookkeeping);

    f.tag(b"experiment");
    f.u64(cfg.experiment.seed);
    if include_bookkeeping {
        f.str(&cfg.experiment.name);
        f.str(&cfg.experiment.output_dir.to_string_lossy());
    }

    f.tag(b"evolution");
    f.usize(cfg.evolution.population_size);
    f.usize(cfg.evolution.elite_count);
    f.usize(cfg.evolution.tournament_size);
    f.real(cfg.evolution.crossover_rate);
    f.real(cfg.evolution.immigrant_rate);
    if include_bookkeeping {
        f.u32(cfg.evolution.generations);
    }

    f.tag(b"mutation");
    f.real(cfg.mutation.weight_rate);
    f.real(cfg.mutation.weight_sigma);
    f.real(cfg.mutation.weight_reset_rate);
    f.real(cfg.mutation.size_rate);
    f.real(cfg.mutation.size_sigma);
    f.real(cfg.mutation.attach_rate);
    f.real(cfg.mutation.attach_sigma);
    f.real(cfg.mutation.joint_limit_rate);
    f.real(cfg.mutation.joint_limit_sigma);
    f.real(cfg.mutation.joint_kind_rate);
    f.real(cfg.mutation.joint_axis_rate);
    f.real(cfg.mutation.motor_rate);
    f.real(cfg.mutation.motor_sigma);
    f.real(cfg.mutation.add_part_rate);
    f.real(cfg.mutation.remove_part_rate);

    f.tag(b"body");
    f.usize(cfg.body.min_parts);
    f.usize(cfg.body.max_parts);
    f.real(cfg.body.min_half_extent);
    f.real(cfg.body.max_half_extent);
    f.real(cfg.body.density);
    f.real(cfg.body.min_joint_limit);
    f.real(cfg.body.max_joint_limit);
    f.real(cfg.body.max_motor_speed);
    f.real(cfg.body.max_motor_torque);
    f.real(cfg.body.hinge_probability);
    // Folded in only when the experiment actually uses shapes. A box-only
    // configuration therefore keeps the digest it had before shapes existed,
    // which is what lets a run started before this feature still be resumed.
    // As with shapes: folded in only when the feature is enabled, so an
    // experiment that cannot break joints keeps the digest it always had.
    if cfg.body.muscle_stress > 0.0 {
        f.tag(b"muscle");
        f.real(cfg.body.muscle_stress);
    }
    if cfg.body.pair_probability > 0.0 || cfg.body.max_repeat > 1 {
        f.tag(b"bodyplan");
        f.real(cfg.body.pair_probability);
        f.u32(cfg.body.max_repeat as u32);
        f.real(cfg.mutation.pair_rate);
        f.real(cfg.mutation.repeat_rate);
    }
    if cfg.body.tendon_frequency > 0.0 {
        f.tag(b"tendon");
        f.real(cfg.body.tendon_frequency);
        f.real(cfg.body.tendon_damping);
    }
    if cfg.joints_can_break() {
        f.tag(b"joint_health");
        f.real(cfg.body.joint_endurance);
        f.real(cfg.body.min_drive);
        f.real(cfg.mutation.caution_rate);
        f.real(cfg.mutation.caution_sigma);
    }
    if uses_shapes(&cfg.body.shapes) {
        f.tag(b"shapes");
        for kind in &cfg.body.shapes {
            f.u32(*kind as u32);
        }
        f.real(cfg.body.taper_top_scale);
        f.real(cfg.mutation.shape_rate);
    }

    f.tag(b"brain");
    f.usize(cfg.brain.hidden);
    f.real(cfg.brain.init_sigma);
    f.real(cfg.brain.weight_limit);

    f.tag(b"simulation");
    f.real(cfg.simulation.timestep);
    f.real(cfg.simulation.duration);
    f.real(cfg.simulation.control_hz);
    f.u32(cfg.simulation.solver_iterations);
    f.real(cfg.simulation.settle_time);
    f.real(cfg.simulation.baumgarte);
    f.real(cfg.simulation.slop);
    f.real(cfg.simulation.max_correction_speed);
    f.real(cfg.simulation.max_linear_speed);
    f.real(cfg.simulation.max_angular_speed);

    // Folded in only when the experiment repeats trials, so a single-trial
    // experiment keeps the digest it always had.
    if cfg.simulation.trials > 1 || cfg.simulation.start_jitter > 0.0 {
        f.tag(b"trials");
        f.usize(cfg.simulation.trials);
        f.real(cfg.simulation.start_jitter);
        f.u8(match cfg.simulation.aggregate {
            Aggregate::Mean => 0,
            Aggregate::Worst => 1,
        });
    }
    if cfg.simulation.steer {
        f.tag(b"steer");
        f.real(cfg.simulation.steer_spread);
    }

    f.tag(b"environment");
    f.u8(match cfg.environment.terrain {
        Terrain::Flat => 0,
        Terrain::Rough => 1,
        Terrain::Fractal => 2,
    });
    // Folded in only for the terrain that uses them, so a flat experiment keeps
    // the digest it had before rolling ground existed.
    if cfg.environment.terrain == Terrain::Rough || cfg.environment.terrain == Terrain::Fractal {
        f.real(cfg.environment.terrain_amplitude);
        f.real(cfg.environment.terrain_wavelength);
    }
    // And the fractal knobs only for the fractal, so a `rough` experiment keeps
    // the digest it had before this terrain existed. `experiment.seed` is
    // already part of the digest, so a derived terrain seed needs no extra
    // fold — but an explicit one is not otherwise represented anywhere.
    if cfg.environment.terrain == Terrain::Fractal {
        f.tag(b"fractal");
        f.u64(cfg.environment.terrain_seed);
        f.u32(cfg.environment.terrain_octaves);
        f.real(cfg.environment.terrain_lacunarity);
        f.real(cfg.environment.terrain_gain);
        f.real(cfg.environment.terrain_warp);
        f.bool(cfg.environment.terrain_per_trial);
    }
    // Each later band is folded in only when it is switched on, so a fractal
    // experiment that predates a band keeps the digest it had — the same rule
    // shapes, tendons and breakable joints already follow.
    if cfg.environment.terrain == Terrain::Fractal && cfg.environment.terrain_detail_amplitude > 0.0
    {
        f.tag(b"detail");
        f.real(cfg.environment.terrain_detail_amplitude);
        f.real(cfg.environment.terrain_detail_wavelength);
        f.u32(cfg.environment.terrain_detail_octaves);
    }
    if cfg.environment.terrain == Terrain::Fractal && cfg.environment.terrain_modulation > 0.0 {
        f.tag(b"modulation");
        f.real(cfg.environment.terrain_modulation);
        f.real(cfg.environment.terrain_modulation_wavelength);
    }
    if cfg.environment.terrain == Terrain::Fractal && cfg.environment.terrain_step > 0.0 {
        f.tag(b"terrace");
        f.real(cfg.environment.terrain_step);
        f.real(cfg.environment.terrain_riser);
        f.bool(cfg.environment.terrain_terrace_mask);
    }
    f.real(cfg.environment.gravity);
    f.real(cfg.environment.friction);
    f.real(cfg.environment.restitution);
    f.real(cfg.environment.linear_damping);
    f.real(cfg.environment.angular_damping);
    if cfg.environment.self_collision {
        f.tag(b"selfcollide");
    }

    // Guarded like shapes, tendons and breakable joints: an experiment with no
    // sensors keeps the digest it had before sensing existed, and therefore stays
    // resumable and comparable.
    if cfg.uses_sensors() {
        f.tag(b"sensor");
        f.real(cfg.body.sensor_probability);
        f.real(cfg.sensor.range);
        f.usize(cfg.sensor.rays);
        f.real(cfg.sensor.spread);
        f.real(cfg.mutation.sensor_rate);
        f.real(cfg.mutation.sensor_dir_sigma);
    }

    f.tag(b"fitness");
    f.u8(match cfg.fitness.objective {
        Objective::Distance => 0,
        Objective::DistanceX => 1,
        Objective::Speed => 2,
        Objective::Heading => 3,
    });
    f.real(cfg.fitness.energy_penalty);
    // Guarded like every other opt-in term: an experiment that does not reward
    // leaving the ground keeps the digest it had before jumping was scorable.
    if cfg.fitness.air_bonus != 0.0 || cfg.fitness.height_bonus != 0.0 {
        f.tag(b"jump");
        f.real(cfg.fitness.air_bonus);
        f.real(cfg.fitness.height_bonus);
    }
    // Same rule for elevation. `climb_deadband` changes the recorded metrics
    // whatever the weights are, but it can only change the *dynamics* when a
    // cumulative term reads them, so it is folded in with the terms that use it.
    if cfg.fitness.climb_bonus != 0.0
        || cfg.fitness.descent_penalty != 0.0
        || cfg.fitness.cumulative_climb_bonus != 0.0
        || cfg.fitness.cumulative_descent_penalty != 0.0
        || cfg.fitness.fall_penalty != 0.0
    {
        f.tag(b"elevation");
        f.real(cfg.fitness.fall_penalty);
        f.real(cfg.fitness.climb_bonus);
        f.real(cfg.fitness.descent_penalty);
        f.real(cfg.fitness.cumulative_climb_bonus);
        f.real(cfg.fitness.cumulative_descent_penalty);
        f.real(cfg.fitness.climb_deadband);
    }
    f.real(cfg.fitness.upright_bonus);

    if include_bookkeeping {
        f.tag(b"recording");
        f.usize(cfg.recording.top_n);
        f.usize(cfg.recording.random_samples);
        f.real(cfg.recording.record_hz);
        f.u32(cfg.recording.every_generations);
        f.bool(cfg.recording.store_genomes);

        f.tag(b"checkpoint");
        f.u32(cfg.checkpoint.every_generations);
        f.bool(cfg.checkpoint.on_finish);
    }

    f.finish()
}

struct Fingerprint(Vec<u8>);

impl Fingerprint {
    fn new() -> Fingerprint {
        Fingerprint(Vec::with_capacity(512))
    }

    fn tag(&mut self, bytes: &[u8]) {
        self.u32(bytes.len() as u32);
        self.0.extend_from_slice(bytes);
    }

    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn usize(&mut self, v: usize) {
        self.u64(v as u64);
    }

    fn real(&mut self, v: Real) {
        self.0.extend_from_slice(&v.to_bits().to_le_bytes());
    }

    fn bool(&mut self, v: bool) {
        self.0.push(u8::from(v));
    }

    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.0.extend_from_slice(s.as_bytes());
    }

    fn finish(&self) -> u64 {
        fnv1a(&self.0)
    }
}

/// FNV-1a, used for configuration and structure fingerprints.
///
/// Not cryptographic; it only needs to be fast, stable across versions and
/// unlikely to collide by accident.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}
