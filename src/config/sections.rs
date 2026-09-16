//! The `[section]` tables of an experiment file, and their defaults.
//!
//! Every field has a default so a minimal config is legal; see [`super`] for
//! why unknown fields are nevertheless rejected.

use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ExperimentCfg {
    /// Human-readable name; also used to name the run directory.
    pub name: String,
    /// Root seed. Every other random stream in the experiment is derived from
    /// this plus a stable identity, never from wall-clock time.
    pub seed: u64,
    /// Directory under which run directories are created.
    pub output_dir: PathBuf,
}

impl Default for ExperimentCfg {
    fn default() -> Self {
        ExperimentCfg { name: "unnamed".into(), seed: 1, output_dir: PathBuf::from("runs") }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct EvolutionCfg {
    pub population_size: usize,
    pub generations: u32,
    /// Top individuals copied unchanged into the next generation.
    pub elite_count: usize,
    /// Tournament size; larger means stronger selection pressure.
    pub tournament_size: usize,
    /// Probability that an offspring is produced by crossover rather than
    /// cloning a single parent.
    pub crossover_rate: Real,
    /// Fraction of each generation replaced by freshly generated random
    /// genomes. A small amount of immigration is cheap insurance against the
    /// population collapsing onto one lineage.
    pub immigrant_rate: Real,
}

impl Default for EvolutionCfg {
    fn default() -> Self {
        EvolutionCfg {
            population_size: 100,
            generations: 100,
            elite_count: 2,
            tournament_size: 4,
            crossover_rate: 0.7,
            immigrant_rate: 0.02,
        }
    }
}

/// Per-gene mutation rates and step sizes.
///
/// Rates are per-gene probabilities, not per-genome, so their effect does not
/// change as organisms grow more parts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MutationParams {
    /// Probability per part, per generation, that its sensor is added or removed.
    pub sensor_rate: Real,
    /// Standard deviation of the perturbation applied to a sensor's direction.
    pub sensor_dir_sigma: Real,
    pub weight_rate: Real,
    pub weight_sigma: Real,
    /// Probability that a mutated weight is redrawn from scratch instead of
    /// perturbed, which lets the search escape a deep local basin.
    pub weight_reset_rate: Real,
    pub size_rate: Real,
    pub size_sigma: Real,
    pub attach_rate: Real,
    pub attach_sigma: Real,
    pub joint_limit_rate: Real,
    pub joint_limit_sigma: Real,
    pub joint_kind_rate: Real,
    pub joint_axis_rate: Real,
    pub motor_rate: Real,
    pub motor_sigma: Real,
    /// Probability per genome of perturbing the caution trait, and the size of
    /// that perturbation. Only consulted when `body.joint_endurance` is positive.
    pub caution_rate: Real,
    pub caution_sigma: Real,
    /// Probability per part of flipping whether it is a mirrored pair, or which
    /// way its halves are driven. Only consulted when `body.pair_probability`
    /// is positive.
    pub pair_rate: Real,
    /// Probability per part of redrawing its segment count. Only consulted when
    /// `body.max_repeat` exceeds one.
    pub repeat_rate: Real,
    /// Probability per part of redrawing its shape. Only consulted when
    /// `body.shapes` offers more than one, so a box-only experiment never spends
    /// a draw on it.
    pub shape_rate: Real,
    /// Probability per genome of appending one new part.
    pub add_part_rate: Real,
    /// Probability per genome of deleting one leaf part.
    pub remove_part_rate: Real,
}

impl Default for MutationParams {
    fn default() -> Self {
        MutationParams {
            sensor_rate: 0.03,
            sensor_dir_sigma: 0.15,
            weight_rate: 0.08,
            weight_sigma: 0.25,
            weight_reset_rate: 0.05,
            size_rate: 0.05,
            size_sigma: 0.05,
            attach_rate: 0.05,
            attach_sigma: 0.15,
            joint_limit_rate: 0.05,
            joint_limit_sigma: 0.2,
            joint_kind_rate: 0.02,
            joint_axis_rate: 0.03,
            motor_rate: 0.05,
            motor_sigma: 0.15,
            caution_rate: 0.08,
            caution_sigma: 0.12,
            pair_rate: 0.04,
            repeat_rate: 0.03,
            shape_rate: 0.04,
            add_part_rate: 0.06,
            remove_part_rate: 0.05,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct BodyLimits {
    /// Chance that a newly drawn part carries a range sensor.
    ///
    /// Zero disables sensing entirely and exactly: no gene is drawn, no
    /// controller input is added, and every result from before sensors existed
    /// reproduces bit for bit.
    pub sensor_probability: Real,
    pub min_parts: usize,
    pub max_parts: usize,
    pub min_half_extent: Real,
    pub max_half_extent: Real,
    /// kg/m^3. Deliberately far below water: heavy blocks need implausible
    /// torques to move and make early evolution uninteresting.
    pub density: Real,
    /// Hinge limits are symmetric (+/- limit) in radians.
    pub min_joint_limit: Real,
    pub max_joint_limit: Real,
    pub max_motor_speed: Real,
    pub max_motor_torque: Real,
    /// Probability that a newly generated joint is a hinge rather than fixed.
    pub hinge_probability: Real,
    /// Which primitives parts may be carved from, e.g.
    /// `shapes = ["box", "capsule", "sphere"]`.
    ///
    /// A single-entry list means every part is that shape and *no randomness is
    /// spent choosing*, which is what lets a box-only experiment reproduce
    /// results recorded before shapes existed, bit for bit.
    pub shapes: Vec<ShapeKind>,
    /// Cross-section of a taper's far end as a fraction of its base. Fixed per
    /// experiment rather than evolved, because a second size gene would mostly
    /// duplicate what `half_extents` already says.
    pub taper_top_scale: Real,
    /// How much overwork a joint survives, in radians of undelivered rotation.
    ///
    /// A motorised joint takes damage only while its motor is saturated — the
    /// controller is asking for a speed the joint's genetic `motor_torque`
    /// cannot deliver — and the damage is the rotation it fell short by. When
    /// the total reaches this, the joint fails and the limb detaches.
    ///
    /// `0` disables joint damage entirely, which is the default: with it off no
    /// randomness is spent on the caution gene and the controller keeps its
    /// original input count, so an experiment reproduces exactly what it did
    /// before joints could break.
    pub joint_endurance: Real,
    /// Floor on how far the caution gene may throttle motor demand, so a maximally
    /// cautious organism is still able to move.
    pub min_drive: Real,
    /// Muscle strength per unit of joint cross-section. `0` disables the cap and
    /// leaves `motor_torque` as a free gene, which is how every experiment
    /// before this behaved.
    ///
    /// In an animal, the force a muscle can produce scales with its
    /// cross-sectional area, and the torque it exerts with that force times a
    /// moment arm that scales with the limb's width. So the ceiling here goes as
    /// `stress * area^1.5`, and a limb cannot be stronger than its own girth
    /// allows. Without it, `motor_torque` is drawn independently of size and a
    /// matchstick can be as strong as a thigh — which is a large part of why
    /// evolved bodies here look nothing like animals.
    pub muscle_stress: Real,
    /// Natural frequency of every hinge's passive spring, rad/s — a tendon.
    /// Zero leaves joints purely servo-driven, as before.
    ///
    /// A frequency rather than a stiffness so that the spring means the same
    /// thing on a thigh and on a toe. Tendon elasticity is most of why animal
    /// running and hopping are efficient: energy stored on landing comes back on
    /// push-off instead of being paid for again by the muscle.
    pub tendon_frequency: Real,
    /// Damping ratio of that spring. 1 is critically damped.
    pub tendon_damping: Real,
    /// Probability that a newly drawn part is a mirrored pair. `0` disables
    /// bilateral symmetry entirely and spends no randomness on it.
    pub pair_probability: Real,
    /// Longest chain of repeated segments a part may become. `1` disables
    /// segmentation and spends no randomness on it.
    pub max_repeat: u8,
}

impl Default for BodyLimits {
    fn default() -> Self {
        BodyLimits {
            sensor_probability: 0.0,
            min_parts: 2,
            max_parts: 6,
            min_half_extent: 0.08,
            max_half_extent: 0.35,
            density: 250.0,
            min_joint_limit: 0.3,
            max_joint_limit: 1.4,
            max_motor_speed: 6.0,
            max_motor_torque: 120.0,
            hinge_probability: 0.85,
            shapes: vec![ShapeKind::Box],
            taper_top_scale: 0.45,
            joint_endurance: 0.0,
            min_drive: 0.2,
            muscle_stress: 0.0,
            tendon_frequency: 0.0,
            tendon_damping: 0.5,
            pair_probability: 0.0,
            max_repeat: 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct BrainCfg {
    pub hidden: usize,
    /// Standard deviation used when drawing fresh weights.
    pub init_sigma: Real,
    /// Weights are clamped to +/- this. Unbounded weights saturate `tanh` and
    /// turn the controller into a constant, which evolution finds embarrassingly
    /// quickly.
    pub weight_limit: Real,
}

impl Default for BrainCfg {
    fn default() -> Self {
        BrainCfg { hidden: 10, init_sigma: 0.8, weight_limit: 8.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SimulationCfg {
    /// Physics step, seconds.
    pub timestep: Real,
    /// Measured simulation time per evaluation, seconds.
    pub duration: Real,
    /// Controller update rate. Decoupled from the physics rate because the
    /// network only needs to act on the timescale the body can respond to, and
    /// evaluating it less often is free performance.
    pub control_hz: Real,
    pub solver_iterations: u32,
    /// Time the organism falls and settles before measurement begins, so that
    /// the initial drop does not count as locomotion. The controller is held
    /// off during this window.
    pub settle_time: Real,
    /// Fraction of positional error corrected per step (Baumgarte).
    pub baumgarte: Real,
    /// Penetration tolerated before positional correction kicks in.
    pub slop: Real,
    /// Ceiling on Baumgarte-injected velocity.
    pub max_correction_speed: Real,
    /// Hard linear velocity clamp, m/s. Keeps a pathological body finite.
    pub max_linear_speed: Real,
    /// Hard angular velocity clamp, rad/s.
    pub max_angular_speed: Real,
    /// How many times each organism is evaluated. `1` is a single trial, which
    /// is what every experiment did before this.
    ///
    /// One trial from one pose rewards a stunt as readily as a gait: a single
    /// well-timed lunge scores like walking, and a strategy that works exactly
    /// once cannot be told from one that works. Repeating the trial from varied
    /// starts is the cheapest pressure there is toward behaviour that is
    /// actually repeatable.
    pub trials: usize,
    /// How much the start pose varies between trials, `0` to `1`. Zero means
    /// every trial is identical, which makes repeating them pointless.
    pub start_jitter: Real,
    /// How trials combine into one score.
    pub aggregate: Aggregate,
    /// Whether each trial commands a direction of travel, given to the
    /// controller as an input and scored by `objective = "heading"`.
    pub steer: bool,
    /// Widest angle, radians, that a commanded heading may stray from +X.
    pub steer_spread: Real,
}

/// How an organism's trials combine into the number it is selected on.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Aggregate {
    /// Average. Rewards being good on balance.
    #[default]
    Mean,
    /// The worst trial. Rewards having no bad day at all, which is a much
    /// stronger demand and the one that most favours a robust gait.
    Worst,
}

impl Default for SimulationCfg {
    fn default() -> Self {
        SimulationCfg {
            timestep: 1.0 / 120.0,
            duration: 8.0,
            control_hz: 20.0,
            solver_iterations: 10,
            settle_time: 0.5,
            // Deliberately small, and this is load-bearing.
            //
            // Positional correction is applied at a contact point offset from
            // the centre of mass, so it induces rotation as well as separation,
            // and integrating that rotation moves the body. With few solver
            // iterations, contacts stay deeply penetrated, the correction stays
            // large, and an organism that arranges to penetrate the ground in a
            // rhythm converts the correction into travel. Measured on evolved
            // champions: at beta = 0.2 they cover 2.72 m, of which refining the
            // solver removes 94%; at 0.05 they cover 0.36 m and refinement
            // removes almost nothing. Raising `solver_iterations` fixes it too,
            // and costs 1.7x to 2.8x; this costs 1.08x.
            baumgarte: 0.05,
            slop: 0.002,
            max_correction_speed: 2.0,
            max_linear_speed: 60.0,
            max_angular_speed: 40.0,
            trials: 1,
            start_jitter: 0.0,
            aggregate: Aggregate::Mean,
            steer: false,
            steer_spread: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Terrain {
    Flat,
    /// Rolling ground. See [`crate::physics::TerrainModel::Rough`].
    Rough,
    /// Seeded fractal landscape. See [`crate::physics::TerrainModel::Fractal`].
    Fractal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct EnvironmentCfg {
    pub terrain: Terrain,
    /// Whether an organism's own parts collide with each other.
    ///
    /// Off by default, which is how every experiment before this behaved and the
    /// usual choice in this class of work. Turning it on is what stops a body
    /// being a cloud of overlapping blocks: limbs have to be somewhere the torso
    /// is not, which is the most basic thing that makes an animal an animal.
    pub self_collision: bool,
    /// Scale of the ground's relief, metres. Only consulted when the terrain is
    /// not flat.
    ///
    /// Peak-to-trough is about `2.5 * terrain_amplitude` for both `rough` and
    /// `fractal`. Steepness is set by the *ratio* of amplitude to wavelength,
    /// not by amplitude alone: doubling one and doubling the other leaves the
    /// slope distribution where it was.
    pub terrain_amplitude: Real,
    /// Distance between crests, metres. For `fractal` this is the size of the
    /// *largest* feature; each further octave is `terrain_lacunarity` times
    /// finer.
    pub terrain_wavelength: Real,
    /// Which fractal landscape to generate. `0` derives one from
    /// `experiment.seed`, so two experiments with different seeds get different
    /// ground; any other value names a specific landscape, which is what to use
    /// when comparing two experiments on identical terrain. Only consulted when
    /// `terrain = "fractal"`.
    pub terrain_seed: u64,
    /// How many octaves of noise are summed. One is a single smooth scale; four
    /// spans a factor of eight in feature size, which is about where ground
    /// starts reading as landscape rather than as a pattern.
    pub terrain_octaves: u32,
    /// Frequency step between octaves. Two is the conventional choice — each
    /// octave half the size of the last.
    pub terrain_lacunarity: Real,
    /// Amplitude step between octaves. Below 0.5 the fine detail vanishes;
    /// above it the ground gets rougher at every scale at once.
    pub terrain_gain: Real,
    /// Domain warp strength, in units of `terrain_wavelength`. Zero is plain
    /// fractional Brownian motion.
    ///
    /// Warping bends the field into ridges and basins rather than blobs. It
    /// buys the tail of the slope distribution — at amplitude 0.25 and
    /// wavelength 6 the steepest slope anywhere goes from 19 degrees to 29 —
    /// and it does make the ground more heterogeneous: the spread of mean slope
    /// across 12 m tiles roughly doubles, from 0.04 of its mean to 0.11.
    ///
    /// An earlier version of this comment claimed the opposite, on the strength
    /// of measuring the spread of *relief* per tile rather than of slope. A
    /// tile on the flank of a large hill has enormous relief and can still be
    /// billiard-smooth, so relief answers a different question. What warping
    /// cannot do is produce cliffs; that is `terrain_step`.
    pub terrain_warp: Real,
    /// Amplitude of a second, finer band of noise laid over the landscape,
    /// metres. Zero — the default — leaves the field exactly as it was before
    /// this band existed.
    ///
    /// The landscape band sets how big the hills are; this one sets what the
    /// ground under an organism's feet is like, and they want different
    /// wavelengths. One band cannot do both: fractional Brownian motion has a
    /// single steepness, set by amplitude over wavelength, and it applies it at
    /// every scale at once.
    pub terrain_detail_amplitude: Real,
    pub terrain_detail_wavelength: Real,
    pub terrain_detail_octaves: u32,
    /// How strongly a slow field varies the detail band's amplitude, in
    /// `[0, 1]`. Zero is uniform detail everywhere.
    ///
    /// Warping the domain also varies the ground's character, but only by
    /// rearranging one stationary field; scaling a band's amplitude by a
    /// second, slower field is the direct way to say "calm here, savage
    /// there". Measured as the spread of mean slope across 12 m tiles, the
    /// landscape band alone sits at 0.09 of its mean, this raises it to 0.12,
    /// and with `terrain_terrace_mask` it reaches 0.23.
    pub terrain_modulation: Real,
    /// Size of the calm and savage regions, metres.
    pub terrain_modulation_wavelength: Real,
    /// Terrace height, metres. Zero — the default — is a smooth field.
    ///
    /// Quantising height to terraces is the only thing here that produces a
    /// genuinely sheer face. Scaling the noise up does not: ten metres of
    /// relief still tops out near 48 degrees, and the median slope climbs with
    /// the maximum, which is uniformly steep ground rather than occasional
    /// cliffs. A terrace is flat for most of its span and climbs through the
    /// rest, so the difficulty sits in a small fraction of the area and the
    /// rest stays crossable — measured at 91% of the plane under 40 degrees
    /// with a 99th-percentile slope of 82.
    pub terrain_step: Real,
    /// Fraction of a terrace spent climbing. The riser is steeper than the
    /// underlying slope by exactly `1 / terrain_riser`.
    ///
    /// Its floor is physics, not taste: a body at 3 m/s covers 25 mm per step,
    /// and a wall it crosses in one step is a wall the solver meets as a single
    /// enormous penetration. `validate` refuses a combination that makes the
    /// walls too thin for the timestep.
    pub terrain_riser: Real,
    /// Terrace only where `terrain_modulation` says the ground is savage,
    /// blending back into untouched hills elsewhere. Concentrates the cliffs
    /// rather than tiling the world with them, at the cost of shorter walls.
    pub terrain_terrace_mask: bool,
    /// Whether each trial slides and turns the landscape underneath the
    /// organism.
    ///
    /// Without this every organism in every trial of the whole experiment meets
    /// the same surface, which is memorisable in principle — an organism can be
    /// selected for a gait that suits one particular hill. With it, coping with
    /// ground in general is the only thing that survives.
    pub terrain_per_trial: bool,
    /// Downward acceleration magnitude, m/s^2.
    pub gravity: Real,
    pub friction: Real,
    pub restitution: Real,
    pub linear_damping: Real,
    pub angular_damping: Real,
}

impl Default for EnvironmentCfg {
    fn default() -> Self {
        EnvironmentCfg {
            terrain: Terrain::Flat,
            self_collision: false,
            terrain_amplitude: 0.06,
            terrain_wavelength: 1.5,
            terrain_seed: 0,
            terrain_octaves: 4,
            terrain_lacunarity: 2.0,
            terrain_gain: 0.5,
            terrain_warp: 0.3,
            terrain_detail_amplitude: 0.0,
            terrain_detail_wavelength: 3.0,
            terrain_detail_octaves: 4,
            terrain_modulation: 0.0,
            terrain_modulation_wavelength: 35.0,
            terrain_step: 0.0,
            terrain_riser: 0.12,
            terrain_terrace_mask: false,
            terrain_per_trial: true,
            gravity: 9.81,
            friction: 0.8,
            restitution: 0.0,
            linear_damping: 0.02,
            angular_damping: 0.05,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    /// Horizontal distance from the starting position, any direction.
    Distance,
    /// Signed displacement along +X. Harder than `distance`: the organism must
    /// commit to a direction rather than fall over impressively.
    DistanceX,
    /// Mean horizontal speed over the measured window.
    Speed,
    /// Displacement along the direction the organism was told to go.
    ///
    /// With `simulation.steer` on, that direction changes between trials, so an
    /// organism cannot succeed by committing to one heading and hoping. It has
    /// to be steerable, which is a far stronger demand than being fast — and it
    /// is what forces a controllable body rather than a one-shot launcher.
    Heading,
}

/// Physical parameters of a range sensor, shared by every sensor in an
/// experiment.
///
/// These are experiment constants rather than genes. `rays` in particular sets
/// how many controller inputs a sensing slot contributes, so it has to be fixed
/// across a population for the weight vector to keep a uniform length — which is
/// what makes aligned crossover a one-liner.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SensorCfg {
    /// How far a ray reaches, metres. Beyond it the sensor reports nothing.
    ///
    /// This is the difficulty knob: long sight makes terrain-following easy,
    /// short sight makes it a matter of feeling the way. Fixed per experiment
    /// rather than evolved, because a free range gene would simply be maximised.
    pub range: Real,
    /// Rays per sensor, fanned in the plane containing the sensor's direction
    /// and its part's local up axis.
    ///
    /// One ray already separates rising ground from falling — ground that climbs
    /// ahead returns a shorter range than level ground does. Telling a gentle
    /// slope from a wall needs at least two, because a single distance carries no
    /// gradient. What any of it *means* is for evolution to work out: whether a
    /// slope can be climbed depends on the body and controller meeting it, not on
    /// the ground.
    pub rays: usize,
    /// Angular spread between adjacent rays, radians, applied as a linear offset
    /// perpendicular to the sensor's direction. Small-angle, and deliberately so:
    /// it needs no transcendental and therefore adds nothing to the determinism
    /// surface.
    pub spread: Real,
}

impl Default for SensorCfg {
    fn default() -> Self {
        SensorCfg { range: 4.0, rays: 3, spread: 0.35 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct FitnessCfg {
    pub objective: Objective,
    /// Subtracted per unit of actuation impulse. Zero by default: energy
    /// pressure before locomotion exists just selects for doing nothing.
    pub energy_penalty: Real,
    /// Added per second spent with the root block upright.
    pub upright_bonus: Real,
    /// Added per second with no attached part touching the ground.
    ///
    /// This is what makes hopping beat sliding. Zero by default: without it an
    /// organism has no reason ever to leave the ground, and the cheapest way to
    /// travel is to stay on it.
    pub air_bonus: Real,
    /// Added per metre the centre of mass rises above where it started.
    ///
    /// Pairs with `air_bonus`: hang time alone rewards a long low skim, and
    /// height alone rewards a rear-up that never leaves the ground. Together
    /// they ask for a jump.
    pub height_bonus: Real,

    /// Added per metre the organism *ends* above where it settled.
    ///
    /// This is the "go uphill" term. Distance and elevation are not comparable
    /// per metre: on the shipped fractal terrain an evolved champion covers
    /// about ten metres of ground per metre of height it gives up, so a weight
    /// near 10 is what makes half a metre of climb worth as much as a whole
    /// run. Zero by default.
    pub climb_bonus: Real,
    /// Subtracted per metre the organism ends *below* where it settled.
    ///
    /// Beware the failure mode `energy_penalty` documents: an organism that
    /// never moves loses no elevation, and unlike actuation it is not even
    /// charged for standing there. Keep a distance term in the objective, or
    /// the highest-scoring strategy is to do nothing. Zero by default.
    pub descent_penalty: Real,
    /// Added per metre of *total* ascent, hysteresis-filtered — a hill climbed
    /// and then descended still counts. Richer than `climb_bonus` and the one
    /// that has to be watched, since anything paying per unit of vertical
    /// movement invites bobbing on the spot. Zero by default.
    pub cumulative_climb_bonus: Real,
    /// Subtracted per metre of total descent, filtered by the same band.
    /// Zero by default.
    pub cumulative_descent_penalty: Real,
    /// How far the centre of mass must leave its last registered height before
    /// the move counts toward `Metrics::climb` or `Metrics::descent`, in metres.
    ///
    /// A band rather than a per-step threshold: the reference height only moves
    /// when a move is registered, so a slow drift still accumulates while a
    /// gait's bobbing does not. Measured on organisms evolved under a pure
    /// distance objective, an honest gait produces at most 0.05 m of incidental
    /// ascent over a whole run on rough ground, and 0.05 m of band removes all
    /// of it. Affects the recorded metrics whether or not they are scored.
    pub climb_deadband: Real,
    /// Subtracted per metre of height lost while out of contact with the ground.
    ///
    /// The targeted form of `descent_penalty`. That term charges a controlled
    /// walk downhill exactly what it charges a fall, so an objective leaning on
    /// it makes standing still the safest strategy — measured: at
    /// `descent_penalty = 8` a population converged on rising slightly while
    /// travelling 0.20 m, against 1.57 m for the arm scored more gently. This
    /// charges only for the descent an organism did not choose, so going
    /// downhill on purpose stays free. Zero by default.
    pub fall_penalty: Real,
}

impl Default for FitnessCfg {
    fn default() -> Self {
        FitnessCfg {
            objective: Objective::Distance,
            energy_penalty: 0.0,
            upright_bonus: 0.0,
            air_bonus: 0.0,
            height_bonus: 0.0,
            climb_bonus: 0.0,
            descent_penalty: 0.0,
            cumulative_climb_bonus: 0.0,
            cumulative_descent_penalty: 0.0,
            climb_deadband: 0.05,
            fall_penalty: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RecordingCfg {
    /// Record trajectories for the top N organisms of a recorded generation.
    pub top_n: usize,
    /// Additionally record this many uniformly sampled organisms, which keeps
    /// some record of what the unsuccessful majority was doing.
    pub random_samples: usize,
    /// Trajectory sample rate. Independent of the physics rate.
    pub record_hz: Real,
    /// Record only every Nth generation (1 = every generation).
    pub every_generations: u32,
    /// Store the genomes of recorded organisms in `genomes.jsonl`, which is what
    /// makes `evo replay` able to re-simulate them at higher fidelity later.
    pub store_genomes: bool,
}

impl Default for RecordingCfg {
    fn default() -> Self {
        RecordingCfg {
            top_n: 1,
            random_samples: 0,
            record_hz: 30.0,
            every_generations: 10,
            store_genomes: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CheckpointCfg {
    /// Write a full-population checkpoint every N generations (0 disables).
    pub every_generations: u32,
    /// Also write a checkpoint when the run finishes.
    pub on_finish: bool,
}

impl Default for CheckpointCfg {
    fn default() -> Self {
        CheckpointCfg { every_generations: 25, on_finish: true }
    }
}
