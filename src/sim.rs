//! Evaluating one organism.
//!
//! An evaluation is a pure function of `(genome, config)`: build the world, run
//! it for a fixed number of steps driving the joints from the controller, and
//! reduce the behaviour to [`Metrics`]. Nothing is shared between evaluations,
//! nothing is read from the environment, and nothing depends on wall-clock time.
//!
//! That purity is the whole basis of the performance plan. Evaluations can run on
//! every core, in any order, and later on any machine, and the results are
//! identical. It is also what makes `evo replay` possible: a champion from
//! generation 8421 can be reconstructed from its genome alone.

use serde::{Deserialize, Serialize};

use crate::brain::{self, input, BrainScratch};
use crate::config::{Aggregate, Config};
use crate::fitness::{self, Metrics};
use crate::genome::Genome;
use crate::math::{clamp, triangle, vec3, Real, Vec3};
use crate::phenotype::{self, BodySpec, Phenotype};
use crate::rng::{derive_seed, Rng};

/// Clearance above the terrain, in metres, before an organism counts as
/// airborne.
///
/// A contact only exists once a point is *below* the ground, so "touching
/// nothing" also describes a body hovering imperceptibly above it. Asked for
/// hang time on that basis, evolution produced organisms that spent half the
/// trial airborne while never rising above the grass. A couple of centimetres
/// of margin is the difference between leaving the ground and skimming it.
const AIRBORNE_CLEARANCE: Real = 0.02;

/// Stream tag separating trial perturbations from every other derived stream.
const TRIAL_STREAM: u64 = 0x5452_4941_4c53_0001;

/// Widest start perturbations at `start_jitter = 1`.
const MAX_START_YAW: Real = 0.35;
const MAX_START_TILT: Real = 0.2;
const MAX_START_OFFSET: Real = 0.5;

/// How far a per-trial terrain shift can slide the landscape, in units of its
/// own largest feature. Wide enough that two trials share no feature at all.
const TERRAIN_SHIFT_SPAN: Real = 64.0;

/// How many places to try before setting an organism down on whatever is there.
const SPAWN_SEARCH_TRIES: u32 = 12;
/// Radius of the patch that has to be level, metres. About the span of the
/// largest organism the body limits allow.
const SPAWN_FOOTPRINT: Real = 0.6;
/// How level that patch has to be, as the `y` component of the surface normal.
/// 0.96 is a slope of about 16 degrees — a hillside, not a wall.
const SPAWN_MIN_LEVELNESS: Real = 0.96;

/// Frequency of the controller's clock inputs, Hz.
///
/// Two out-of-phase triangle waves give the network a pacemaker to build a gait
/// around, without having to evolve an oscillator from scratch first. Triangle
/// rather than sine so it stays exact for arbitrarily long simulations.
pub const CLOCK_HZ: Real = 1.0;

/// One recorded instant: body poses at a point in time.
///
/// Poses are stored flat, seven values per body (position xyz, orientation xyzw),
/// which keeps the JSON compact and maps directly onto what a renderer wants.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Frame {
    pub t: Real,
    pub poses: Vec<Real>,
}

/// A recorded trajectory, sufficient to replay an organism's behaviour.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Trace {
    pub bodies: Vec<BodySpec>,
    pub record_hz: Real,
    /// Simulation time at which measurement began; frames before this are the
    /// settling drop.
    pub measure_start_t: Real,
    /// The ground the organism ran on, so a viewer can draw the same surface the
    /// physics used rather than assuming a flat one.
    #[serde(default = "flat_ground")]
    pub terrain: crate::physics::TerrainModel,
    /// Ground heights at a fixed handful of points, as flat `x, z, height`
    /// triples. Empty for flat ground, where there is nothing to get wrong.
    ///
    /// The viewer mirrors the height field in JavaScript rather than being
    /// shipped a sampled patch — a patch costs tens of kilobytes per replay,
    /// and the mirror is a few dozen lines. The risk a mirror carries is drift:
    /// get the hash subtly wrong and the viewer draws a different world, with
    /// organisms apparently floating above ground that looks plausible. These
    /// samples are what turns that from a silent failure into a loud one. See
    /// `terrainCheck` in `viewer/terrain.js`, and `viewer/terrain_check.mjs` for
    /// the same check run offline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terrain_check: Vec<Real>,
    pub frames: Vec<Frame>,
    /// Joints that failed during the run, as `(body detached, time)`. Empty for
    /// any experiment whose joints cannot break, and omitted from the JSON
    /// entirely in that case.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<JointBreak>,
}

/// What a replay recorded before terrain was written down: flat ground, which is
/// the only kind that existed then.
fn flat_ground() -> crate::physics::TerrainModel {
    crate::physics::TerrainModel::Flat { height: 0.0 }
}

/// Where the viewer's mirror of the height field is checked, in metres.
///
/// Deliberately awkward coordinates: on a lattice point every octave of gradient
/// noise is exactly zero, so a grid of round numbers would agree between two
/// completely different implementations. Sixteen points, spread over both signs
/// and several wavelengths, is enough that no plausible mistake survives.
const TERRAIN_CHECK_POINTS: [(Real, Real); 16] = [
    (0.0, 0.0),
    (0.37, -0.91),
    (-1.23, 0.58),
    (2.71, 2.09),
    (-3.17, -2.72),
    (5.55, -0.13),
    (-6.31, 4.41),
    (8.09, -7.63),
    (-9.81, -5.27),
    (11.37, 9.02),
    (-13.66, 3.19),
    (15.11, -11.48),
    (-17.29, -14.06),
    (19.73, 16.85),
    (-21.42, 8.37),
    (23.98, -19.51),
];

/// Ground heights at [`TERRAIN_CHECK_POINTS`], as flat `x, z, height` triples.
///
/// Empty for flat ground: a viewer that cannot draw a plane correctly has
/// larger problems than drift.
fn terrain_check(terrain: crate::physics::TerrainModel) -> Vec<Real> {
    if matches!(terrain, crate::physics::TerrainModel::Flat { .. }) {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(TERRAIN_CHECK_POINTS.len() * 3);
    for (x, z) in TERRAIN_CHECK_POINTS {
        out.push(x);
        out.push(z);
        out.push(terrain.height_at(x, z));
    }
    out
}

/// A joint failing mid-run, recorded so a viewer can mark the moment a limb
/// came off rather than leaving it to be inferred from the poses.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct JointBreak {
    /// Index into `Trace::bodies` of the body that came loose.
    pub body: u16,
    /// Simulation time of the failure, on the same clock as `Frame::t`.
    pub t: Real,
}

/// The outcome of one evaluation.
pub struct EvalResult {
    pub fitness: Real,
    pub metrics: Metrics,
    pub trace: Option<Trace>,
}

/// Reusable buffers for evaluating many organisms on one thread.
///
/// Evaluation is short, so per-evaluation allocation would be a measurable
/// fraction of the cost. One of these per worker thread removes it.
pub struct EvalWorkspace {
    scratch: BrainScratch,
}

impl EvalWorkspace {
    pub fn new(cfg: &Config) -> EvalWorkspace {
        EvalWorkspace { scratch: BrainScratch::new(&cfg.brain_layout()) }
    }
}

/// Evaluate `genome` under `cfg`, optionally recording a trajectory.
pub fn evaluate(genome: &Genome, cfg: &Config, record: bool) -> EvalResult {
    let mut ws = EvalWorkspace::new(cfg);
    evaluate_with(genome, cfg, record, &mut ws)
}

/// Evaluate using caller-provided scratch space. This is the form the parallel
/// evaluator uses.
pub fn evaluate_with(
    genome: &Genome,
    cfg: &Config,
    record: bool,
    ws: &mut EvalWorkspace,
) -> EvalResult {
    let trials = cfg.simulation.trials.max(1);
    // The canonical start, when nothing varies between trials. Terrain that
    // moves per trial varies even at one trial, so it takes the loop below.
    if trials == 1 && cfg.simulation.start_jitter <= 0.0 && !terrain_varies(cfg) {
        return run_trial(genome, cfg, record, ws, None);
    }

    // Every organism in the experiment faces the *same* set of starts, because
    // the perturbations are drawn from the experiment seed and the trial index
    // and nothing else. Common random numbers: two organisms differ in their
    // scores because they differ, not because one drew an easier world.
    let mut total = 0.0;
    let mut worst = Real::INFINITY;
    let mut acc: Option<Metrics> = None;
    let mut first_trace = None;
    for trial in 0..trials {
        let mut rng = Rng::new(derive_seed(&[cfg.experiment.seed, TRIAL_STREAM, trial as u64]));
        let mut start = perturbation(&mut rng, cfg.simulation.start_jitter);
        let heading = commanded_heading(&mut rng, cfg);
        // Drawn last, and only when it is used, so that adding it left every
        // stream every existing experiment had exactly where it was.
        start.terrain = terrain_shift(&mut rng, cfg);
        let r = run_trial_towards(genome, cfg, record && trial == 0, ws, Some(start), heading);
        total += r.fitness;
        worst = worst.min(r.fitness);
        if trial == 0 {
            first_trace = r.trace;
        }
        acc = Some(match acc {
            None => r.metrics,
            Some(a) => accumulate(a, r.metrics),
        });
    }

    let n = trials as Real;
    let mut metrics = acc.unwrap_or_default();
    scale_metrics(&mut metrics, 1.0 / n);
    // An organism is scored on the aggregate of its trials, not on a single
    // lucky one. `Worst` asks for a strategy with no bad day at all.
    let fitness = match cfg.simulation.aggregate {
        Aggregate::Mean => total / n,
        Aggregate::Worst => worst,
    };
    EvalResult { fitness, metrics, trace: first_trace }
}

/// Draw a start pose. `jitter` of 0 leaves the organism exactly where it would
/// have been, so a single-trial experiment is unaffected.
fn perturbation(rng: &mut Rng, jitter: Real) -> phenotype::StartPerturbation {
    phenotype::StartPerturbation {
        yaw: rng.signed() * MAX_START_YAW * jitter,
        tilt: rng.signed() * MAX_START_TILT * jitter,
        offset: vec3(rng.signed(), 0.0, rng.signed()) * (MAX_START_OFFSET * jitter),
        terrain: phenotype::TerrainShift::NONE,
    }
}

/// Whether the ground itself differs from trial to trial.
fn terrain_varies(cfg: &Config) -> bool {
    cfg.environment.terrain == crate::config::Terrain::Fractal && cfg.environment.terrain_per_trial
}

/// How far along the landscape this trial is run, and at what bearing.
///
/// Half a metre of `start_jitter` on ground whose largest feature is six metres
/// across is the same hill seen from slightly along it; an organism can still be
/// selected for that hill. Sliding the field by tens of wavelengths and turning
/// it through a full circle gives each trial ground it has not seen, so what
/// survives is coping with ground in general.
///
/// Draws nothing unless it is used, which is what leaves every stream in every
/// existing experiment exactly where it was.
fn terrain_shift(rng: &mut Rng, cfg: &Config) -> phenotype::TerrainShift {
    if !terrain_varies(cfg) {
        return phenotype::TerrainShift::NONE;
    }
    let mut candidate = phenotype::TerrainShift::NONE;
    for _ in 0..SPAWN_SEARCH_TRIES {
        let offset_x = rng.range(-TERRAIN_SHIFT_SPAN, TERRAIN_SHIFT_SPAN);
        let offset_z = rng.range(-TERRAIN_SHIFT_SPAN, TERRAIN_SHIFT_SPAN);
        let (sin, cos) = crate::math::dsincos(rng.range(0.0, crate::math::TAU));
        candidate = phenotype::TerrainShift { offset_x, offset_z, sin, cos };
        // The organism is set down at the origin, so what matters is the ground
        // the *shifted* field puts there. On a terraced landscape roughly one
        // patch in ten is a cliff face, and starting half inside one is not a
        // trial, it is a coin toss.
        let terrain = phenotype::terrain_for(cfg, candidate);
        if terrain.levelness_near(0.0, 0.0, SPAWN_FOOTPRINT) >= SPAWN_MIN_LEVELNESS {
            break;
        }
    }
    // If every candidate was a cliff the last one is used anyway: refusing to
    // place the organism at all would be worse, and a config whose ground is
    // that hostile everywhere is caught by `Config::validate`.
    candidate
}

/// The direction this trial asks the organism to travel.
///
/// Unsteered experiments always command +X, which is what every objective but
/// `Heading` measures anyway.
fn commanded_heading(rng: &mut Rng, cfg: &Config) -> Vec3 {
    if !cfg.simulation.steer {
        return Vec3::X;
    }
    let angle = rng.signed() * cfg.simulation.steer_spread;
    let (s, c) = crate::math::dsincos(angle);
    // Rotation about the vertical: +X turned by `angle` toward +Z.
    vec3(c, 0.0, s)
}

/// Sum two metric sets field by field, for later averaging.
fn accumulate(a: Metrics, b: Metrics) -> Metrics {
    Metrics {
        start: a.start + b.start,
        end: a.end + b.end,
        displacement: a.displacement + b.displacement,
        displacement_x: a.displacement_x + b.displacement_x,
        path_length: a.path_length + b.path_length,
        max_displacement: a.max_displacement + b.max_displacement,
        fall_distance: a.fall_distance + b.fall_distance,
        net_gain: a.net_gain + b.net_gain,
        net_loss: a.net_loss + b.net_loss,
        climb: a.climb + b.climb,
        descent: a.descent + b.descent,
        mean_height: a.mean_height + b.mean_height,
        upright_seconds: a.upright_seconds + b.upright_seconds,
        actuation: a.actuation + b.actuation,
        duration: a.duration + b.duration,
        heading_progress: a.heading_progress + b.heading_progress,
        peak_height: a.peak_height + b.peak_height,
        airborne_seconds: a.airborne_seconds + b.airborne_seconds,
        // Counts and flags describe the whole set rather than averaging.
        steps: a.steps.max(b.steps),
        joints_lost: a.joints_lost.max(b.joints_lost),
        diverged: a.diverged || b.diverged,
    }
}

fn scale_metrics(m: &mut Metrics, k: Real) {
    m.start = m.start * k;
    m.end = m.end * k;
    m.displacement *= k;
    m.displacement_x *= k;
    m.path_length *= k;
    m.max_displacement *= k;
    m.fall_distance *= k;
    m.net_gain *= k;
    m.net_loss *= k;
    m.climb *= k;
    m.descent *= k;
    m.mean_height *= k;
    m.upright_seconds *= k;
    m.actuation *= k;
    m.duration *= k;
    m.heading_progress *= k;
    m.peak_height *= k;
    m.airborne_seconds *= k;
}

/// Hysteresis filter over the centre-of-mass height, accumulating total ascent
/// and descent.
///
/// The band is what separates climbing from a gait's bobbing. `reference` moves
/// only when a move is *registered*, which is the whole point: a per-step
/// threshold would let a slow steady drift through unrecorded, because no single
/// step clears the band, while this holds the reference still until the drift
/// itself clears it. Conversely an organism oscillating within the band moves the
/// reference never, and accumulates nothing however long it bounces.
struct ElevationTracker {
    reference: Real,
    deadband: Real,
    climb: Real,
    descent: Real,
}

impl ElevationTracker {
    fn new(start_y: Real, deadband: Real) -> ElevationTracker {
        ElevationTracker { reference: start_y, deadband, climb: 0.0, descent: 0.0 }
    }

    /// Registered in whole moves, not in excess-over-band: crediting only
    /// `dy - deadband` would lose one band per registration and undercount a long
    /// climb taken in small steps.
    #[inline]
    fn observe(&mut self, y: Real) {
        let dy = y - self.reference;
        if dy > self.deadband {
            self.climb += dy;
            self.reference = y;
        } else if dy < -self.deadband {
            self.descent -= dy;
            self.reference = y;
        }
    }
}

/// One trial: build the organism, simulate it, and score it.
fn run_trial(
    genome: &Genome,
    cfg: &Config,
    record: bool,
    ws: &mut EvalWorkspace,
    start: Option<phenotype::StartPerturbation>,
) -> EvalResult {
    run_trial_towards(genome, cfg, record, ws, start, Vec3::X)
}

/// One trial, travelling toward `heading` — a horizontal unit vector.
fn run_trial_towards(
    genome: &Genome,
    cfg: &Config,
    record: bool,
    ws: &mut EvalWorkspace,
    start: Option<phenotype::StartPerturbation>,
    heading: Vec3,
) -> EvalResult {
    let mut pheno = phenotype::build_with_start(genome, cfg, start);
    let layout = cfg.brain_layout();
    debug_assert_eq!(genome.weights.len(), layout.weight_count());

    let dt = cfg.simulation.timestep;
    let settle_steps = cfg.settle_steps();
    let total_steps = cfg.total_steps();
    let control_interval = steps_per(cfg.simulation.control_hz, dt);
    let record_interval = steps_per(cfg.recording.record_hz, dt);

    let mut trace = record.then(|| Trace {
        bodies: pheno.body_specs(),
        record_hz: cfg.recording.record_hz,
        measure_start_t: settle_steps as Real * dt,
        // Plus the closing frame and the measurement-boundary frame.
        frames: Vec::with_capacity((total_steps / record_interval) as usize + 2),
        breaks: Vec::new(),
        terrain: pheno.world.params.terrain,
        terrain_check: terrain_check(pheno.world.params.terrain),
    });

    // How hard this organism drives its motors. With joint damage off, caution is
    // always 0 and drive is exactly 1, so nothing changes.
    let drive = if cfg.joints_can_break() {
        crate::math::clamp(1.0 - genome.caution, cfg.body.min_drive, 1.0)
    } else {
        1.0
    };

    let mut metrics = Metrics::default();
    let mut previous_com = Vec3::ZERO;
    let mut height_sum = 0.0;
    let mut measured_steps: u32 = 0;
    let mut measurement_began = false;
    let mut elevation = ElevationTracker::new(0.0, cfg.fitness.climb_deadband.max(0.0));
    // Actuation spent during the settle drop belongs to no measured window: the
    // controller is held off, so it is impulse the organism could not have
    // influenced. Subtracting the settle total keeps every metric on `Metrics`
    // describing the same interval.
    let mut settle_actuation = 0.0;

    for step in 0..total_steps {
        let t = step as Real * dt;

        // Measurement starts from the state the settled organism is *in*, before
        // the first controlled step acts on it. Capturing it after that step
        // would put `Trace::measure_start_t` one step ahead of the pose the
        // window is actually measured from.
        if step == settle_steps {
            metrics.start = pheno.world.centre_of_mass();
            previous_com = metrics.start;
            elevation = ElevationTracker::new(metrics.start.y, elevation.deadband);
            settle_actuation = pheno.world.actuation_impulse;
            measurement_began = true;
        }

        if should_apply_control(step, settle_steps, control_interval) {
            let measured_t = (step - settle_steps) as Real * dt;
            apply_control(
                &mut pheno,
                &layout,
                &genome.weights,
                ws,
                &ControlContext { t: measured_t, drive, heading, sensor: &cfg.sensor },
            );
        }

        if let Some(tr) = trace.as_mut() {
            // The measurement boundary always gets a frame, even when it does not
            // fall on the recording grid. Without it a viewer cannot draw the pose
            // that `measure_start_t` names and has to interpolate towards it.
            if step % record_interval == 0 || step == settle_steps {
                tr.frames.push(capture_frame(&pheno, t));
            }
        }

        pheno.world.step(dt);

        if pheno.world.diverged {
            metrics.diverged = true;
            break;
        }

        if step >= settle_steps {
            let com = pheno.world.centre_of_mass();
            let previous_y = previous_com.y;
            let delta = horizontal(com - previous_com);
            metrics.path_length += delta.length();
            previous_com = com;

            let offset = horizontal(com - metrics.start);
            metrics.max_displacement = metrics.max_displacement.max(offset.length());
            height_sum += com.y;

            let up_y = pheno.world.bodies[0].orient.rotate(Vec3::Y).y;
            if up_y > 0.7 {
                metrics.upright_seconds += dt;
            }

            metrics.peak_height = metrics.peak_height.max(com.y);

            elevation.observe(com.y);

            // Airborne means the whole organism has cleared the ground by a real
            // margin. Debris is excluded deliberately: a shed limb bouncing
            // along is not the organism flying.
            if pheno.world.ground_clearance() > AIRBORNE_CLEARANCE {
                metrics.airborne_seconds += dt;
                // Height given away with nothing underfoot. Walking down a slope
                // keeps contact and costs nothing here; stepping off a terrace
                // does not, and that is the distinction `descent` cannot make.
                let dropped = previous_y - com.y;
                if dropped > 0.0 {
                    metrics.fall_distance += dropped;
                }
            }

            measured_steps += 1;
        }
    }

    metrics.joints_lost = pheno.world.breaks.len() as u32;

    if !metrics.diverged {
        if let Some(tr) = trace.as_mut() {
            tr.frames.push(capture_frame(&pheno, total_steps as Real * dt));
            // The world reports breaks against joint indices; a viewer only knows
            // bodies, and a joint's child body is what visibly comes off.
            tr.breaks = pheno
                .world
                .breaks
                .iter()
                .map(|&(joint, t)| JointBreak {
                    body: pheno.world.joints[joint as usize].body_b,
                    t,
                })
                .collect();
        }
        // Guarded on measurement having begun at all: a configuration whose
        // `settle_time` rounds up to the whole evaluation never sets
        // `metrics.start`, and differencing against a default origin would report
        // the organism's absolute position as displacement.
        if measurement_began {
            let com = pheno.world.centre_of_mass();
            metrics.heading_progress = horizontal(com - metrics.start).dot(heading);
            metrics.end = com;
            let offset = horizontal(com - metrics.start);
            metrics.displacement = offset.length();
            metrics.displacement_x = offset.x;
            // Clamped here, per trial, and only averaged afterwards. Clamping
            // after the average would net a climb on one trial against a fall on
            // another and report neither.
            let net = com.y - metrics.start.y;
            metrics.net_gain = net.max(0.0);
            metrics.net_loss = (-net).max(0.0);
        }
        metrics.mean_height =
            if measured_steps > 0 { height_sum / measured_steps as Real } else { 0.0 };
    }

    metrics.climb = elevation.climb;
    metrics.descent = elevation.descent;
    metrics.steps = measured_steps;
    metrics.duration = measured_steps as Real * dt;
    metrics.actuation = pheno.world.actuation_impulse - settle_actuation;

    let fitness = fitness::score(&cfg.fitness, &metrics);
    EvalResult { fitness, metrics, trace: if metrics.diverged { None } else { trace } }
}

/// Gather sensors, run the controller, and write motor targets.
/// What the world is asking of the organism on this control tick, and what its
/// senses are made of. Grouped because they travel together and are constant for
/// the tick, unlike the organism's own state.
struct ControlContext<'a> {
    /// Seconds since the measured window opened, for the pacemaker clocks.
    t: Real,
    /// The organism's own throttle on how hard motors are driven.
    drive: Real,
    /// The direction it has been told to travel: an instruction, fixed for the
    /// trial, not something it perceives.
    heading: Vec3,
    sensor: &'a crate::config::SensorCfg,
}

fn apply_control(
    pheno: &mut Phenotype,
    layout: &crate::brain::BrainLayout,
    weights: &[Real],
    ws: &mut EvalWorkspace,
    ctx: &ControlContext,
) {
    let s = &mut ws.scratch;
    s.clear_inputs();

    s.inputs[input::BIAS] = 1.0;
    s.inputs[input::CLOCK_A] = triangle(ctx.t * CLOCK_HZ);
    s.inputs[input::CLOCK_B] = triangle(ctx.t * CLOCK_HZ + 0.25);

    let root = &pheno.world.bodies[0];
    s.inputs[input::UP_Y] = root.orient.rotate(Vec3::Y).y;
    s.inputs[input::RIGHT_Y] = root.orient.rotate(Vec3::X).y;
    // Velocities and heights are scaled into roughly [-1, 1] so that a freshly
    // initialised network is not immediately saturated.
    s.inputs[input::VEL_X] = clamp(root.lin_vel.x * 0.2, -1.0, 1.0);
    s.inputs[input::VEL_Y] = clamp(root.lin_vel.y * 0.2, -1.0, 1.0);
    s.inputs[input::VEL_Z] = clamp(root.lin_vel.z * 0.2, -1.0, 1.0);
    s.inputs[input::HEIGHT] = clamp(root.pos.y * 0.5, 0.0, 1.0);
    // Where it has been told to go. Absent in an unsteered experiment, where the
    // command is always +X and telling the controller so would be nine tenths of
    // a wasted weight.
    if layout.is_steered() {
        s.inputs[input::COMMAND_X] = ctx.heading.x;
        s.inputs[input::COMMAND_Z] = ctx.heading.z;
    }

    // Range sensing. Each carrying body casts a fan in the plane containing its
    // aim and its own local up, so the fan tilts with the part and an organism
    // that can move that part can sweep it.
    //
    // Readings are *added* into the slot's channels and divided by how many
    // bodies answer to that slot, which is the same averaging the joint angles
    // below use: a mirrored pair sees with one pair of eyes, not two.
    if layout.senses_range() {
        let rays = layout.sensor_inputs;
        let range = ctx.sensor.range;
        let spread = ctx.sensor.spread;
        for mount in &pheno.sensors {
            let body = &pheno.world.bodies[mount.body as usize];
            let slot = pheno.body_slots[mount.body as usize] as usize;
            let n = pheno.slot_bodies[slot].max(1) as Real;
            let base = layout.sensor_input_base(slot);

            let aim = body.orient.rotate(mount.dir);
            // Fan within the plane of the aim and the body's own up axis, so a
            // rolling body rolls its fan with it. Gram-Schmidt, with a fallback
            // for a sensor pointing straight along that axis.
            let up = body.orient.rotate(Vec3::Y);
            let perp = up - aim * aim.dot(up);
            let perp = perp.normalize_or({
                let x = body.orient.rotate(Vec3::X);
                (x - aim * aim.dot(x)).normalize_or(Vec3::Y)
            });

            let origin = body.pos;
            for r in 0..rays {
                // Offsets straddle the aim: one ray is the aim itself when the
                // count is odd. Linear rather than angular, which is what keeps
                // the fan free of transcendentals.
                let offset = r as Real - (rays as Real - 1.0) * 0.5;
                let dir = (aim + perp * (offset * spread)).normalize_or(aim);
                // Nothing found reads zero, which is also what an absent sensor
                // reads — so a blind slot and a slot seeing open sky agree, and
                // neither disturbs a freshly initialised network.
                let reading = match pheno.world.params.terrain.raycast(origin, dir, range) {
                    Some(d) => 1.0 - clamp(d / range, 0.0, 1.0),
                    None => 0.0,
                };
                s.inputs[base + r] += reading / n;
            }
        }
    }

    // A slot can own more than one joint and more than one body: a mirrored
    // pair, or a chain of repeated segments. They share one set of controller
    // weights, so their senses are averaged into one reading and their motors
    // take one command. That sharing is the point — two legs driven by one leg
    // controller move as a pair, which is what a gait is, whereas two
    // independently wired legs mostly flail.
    for (j, &slot) in pheno.joint_slots.iter().enumerate() {
        let (cos_a, sin_a) = pheno.world.hinge_angle_cos_sin(j);
        let base = layout.slot_input_base(slot as usize);
        let n = pheno.slot_joints[slot as usize].max(1) as Real;
        s.inputs[base] += cos_a / n;
        s.inputs[base + 1] += sin_a / n;
    }
    for (b, &slot) in pheno.body_slots.iter().enumerate() {
        let base = layout.slot_input_base(slot as usize);
        let n = pheno.slot_bodies[slot as usize].max(1) as Real;
        if pheno.world.body_in_contact(b) {
            s.inputs[base + 2] += 1.0 / n;
        }
    }
    // The reactive half of the wear trade-off: an organism that can feel a joint
    // failing can ease off it. Present only when the experiment lets joints
    // break, because the input count sets the weight-vector length.
    if layout.senses_health() {
        for (j, &slot) in pheno.joint_slots.iter().enumerate() {
            let base = layout.slot_input_base(slot as usize);
            let n = pheno.slot_joints[slot as usize].max(1) as Real;
            s.inputs[base + 3] += pheno.world.joints[j].health_fraction() / n;
        }
    }

    brain::evaluate(layout, weights, s);

    // `drive` is the standing half: how hard this organism is willing to push
    // regardless of what it senses. Throttling the requested *speed* lowers the
    // velocity error the motor has to close, so a cautious organism saturates
    // its motors less often and wears its joints more slowly — at the cost of
    // being slower.
    for (j, &slot) in pheno.joint_slots.iter().enumerate() {
        let sign = pheno.joint_drive[j];
        let joint = &mut pheno.world.joints[j];
        joint.motor_target = s.outputs[slot as usize] * joint.motor_speed_max * ctx.drive * sign;
    }
}

fn capture_frame(pheno: &Phenotype, t: Real) -> Frame {
    let mut poses = Vec::with_capacity(pheno.world.bodies.len() * 7);
    for b in &pheno.world.bodies {
        poses.extend_from_slice(&[
            b.pos.x, b.pos.y, b.pos.z, b.orient.x, b.orient.y, b.orient.z, b.orient.w,
        ]);
    }
    Frame { t, poses }
}

#[inline]
fn horizontal(v: Vec3) -> Vec3 {
    crate::math::vec3(v.x, 0.0, v.z)
}

/// Steps between events occurring at `hz`, at least one.
fn steps_per(hz: Real, dt: Real) -> u32 {
    let interval = (1.0 / hz) / dt;
    (interval.round() as u32).max(1)
}

/// The controller stays off while the organism drops onto the terrain, so
/// settle is a passive fall rather than a powered twitch that measurement
/// then treats as the starting pose.
fn should_apply_control(step: u32, settle_steps: u32, control_interval: u32) -> bool {
    step >= settle_steps && step.is_multiple_of(control_interval)
}

#[cfg(test)]
mod elevation_tests;
#[cfg(test)]
mod tests;
