//! The constraint solver.
//!
//! # Why not a third-party physics engine?
//!
//! Rapier and friends are good libraries, but they solve a much larger problem
//! than this one: arbitrary geometry, broad-phase acceleration, continuous
//! collision, sleeping, scene graphs. We need a handful of convex primitives on a
//! ground plane connected by hinges, with organisms that by default do not
//! collide with themselves. Every one of those simplifications removes an entire
//! subsystem — and the last one is what keeps [`super::shape`] small, because it
//! means the only collision query is a shape against the terrain. What is left is small
//! enough to read in one sitting, has no version-drift risk to reproducibility,
//! and has no per-evaluation setup cost worth mentioning — which matters when the
//! workload is millions of very short evaluations rather than one long one.
//!
//! # Method
//!
//! Semi-implicit Euler with sequential-impulse constraint solving in maximal
//! coordinates (each body carries its own 6 degrees of freedom, and joints are
//! constraints rather than a reduced parameterisation), with Baumgarte
//! stabilisation for position error.
//!
//! # The upgrade path
//!
//! Organisms are *trees* with no self-collision. That is precisely the case
//! where a reduced-coordinate articulated-body formulation (Featherstone's ABA)
//! is both faster and dramatically more stable — joints become exactly satisfied
//! by construction rather than approximately satisfied by iteration, so the
//! solver-iteration budget disappears. That is the intended replacement, and
//! this module is deliberately kept behind a narrow surface ([`World::step`],
//! plus accessors) so it can be swapped without touching evolution, fitness or
//! recording. Sequential impulses come first because they are far harder to get
//! *wrong*.
//!
//! # Self-collision
//!
//! Off by default, and every simplification above assumes it stays that way:
//! with it off an organism's blocks pass through each other, which is the usual
//! choice in this class of experiment (Sims 1994 did the same), removes the
//! broad phase entirely, and avoids the jitter that overlapping newly-mutated
//! limbs would otherwise cause.
//!
//! Setting `WorldParams::self_collision` turns on a deliberately cheap version
//! instead: parts approximated by capsules, an unapologetically quadratic
//! pairing behind a bounding-sphere reject, jointed pairs skipped because they
//! are meant to touch, and a normal impulse only — no friction between an
//! organism's own parts. See [`contacts`] for why occupying a volume is worth
//! paying for.

use crate::genome::JointKind;
use crate::math::{clamp, vec3, Mat3, Real, Vec3};

use super::body::RigidBody;
use super::terrain::TerrainModel;

mod contacts;
mod integration;
mod joints;
mod queries;
mod util;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug)]
pub struct WorldParams {
    pub gravity: Vec3,
    pub terrain: TerrainModel,
    pub friction: Real,
    pub restitution: Real,
    pub iterations: u32,
    pub linear_damping: Real,
    pub angular_damping: Real,
    /// Fraction of positional error corrected per step, for joints and contacts.
    pub baumgarte: Real,
    /// Penetration tolerated before positional correction kicks in. Prevents
    /// contacts from jittering against the numerical noise floor.
    pub slop: Real,
    /// Ceiling on Baumgarte-injected velocity, so a deeply penetrating body is
    /// pushed out steadily rather than launched.
    pub max_correction_speed: Real,
    /// Hard velocity clamps. A constraint solver can diverge given a pathological
    /// mutated body; clamping keeps a bad organism merely bad rather than letting
    /// it produce infinities that poison fitness statistics.
    pub max_linear_speed: Real,
    pub max_angular_speed: Real,
    /// Whether an organism's own parts collide with each other.
    pub self_collision: bool,
}

impl Default for WorldParams {
    fn default() -> Self {
        WorldParams {
            gravity: vec3(0.0, -9.81, 0.0),
            terrain: TerrainModel::Flat { height: 0.0 },
            friction: 0.8,
            restitution: 0.0,
            iterations: 10,
            linear_damping: 0.02,
            angular_damping: 0.05,
            baumgarte: 0.2,
            slop: 0.002,
            max_correction_speed: 2.0,
            max_linear_speed: 60.0,
            max_angular_speed: 40.0,
            self_collision: false,
        }
    }
}

/// A constraint between two bodies, derived from a [`crate::genome::JointGene`].
///
/// Anchors, axes and reference vectors are in body-local coordinates and never
/// change; everything derived per step lives in [`JointPrep`].
#[derive(Clone, Copy, Debug)]
pub struct Joint {
    pub body_a: u16,
    pub body_b: u16,
    pub kind: JointKind,
    /// Anchor point in each body's local frame. The joint holds these coincident.
    pub anchor_a: Vec3,
    pub anchor_b: Vec3,
    /// Hinge axis in each body's local frame.
    pub axis_a: Vec3,
    pub axis_b: Vec3,
    /// Perpendicular reference direction in each local frame, coincident at zero
    /// angle. Measuring the hinge angle from these avoids ever calling `atan2`.
    pub ref_a: Vec3,
    pub ref_b: Vec3,
    /// `cos(limit)`. Comparing cosines rather than angles keeps the limit check
    /// to a dot product.
    pub cos_limit: Real,
    pub motor_speed_max: Real,
    pub motor_torque_max: Real,
    /// Target angular velocity about the hinge axis, written by the controller.
    pub motor_target: Real,
    /// Natural frequency of the joint's passive spring, rad/s. Zero for a joint
    /// with no tendon.
    ///
    /// A frequency rather than a torque, because a torque has to be matched to
    /// the limb it acts on: the same N m/rad that gently returns a thigh will
    /// fling a toe. Expressed this way the spring is scale-free — every joint
    /// oscillates at the same rate whatever its inertia — and it is stable for
    /// any `frequency * dt` below about one, which no plausible setting reaches.
    pub tendon_frequency: Real,
    /// Damping ratio of that spring. 1 is critically damped; 0 is a spring that
    /// rings forever.
    pub tendon_damping: Real,
    /// Radians of undelivered rotation this joint can absorb before it fails.
    /// Zero means the joint is indestructible, which is the behaviour every
    /// experiment had before joints could break.
    pub endurance: Real,
    /// Remaining health, counting down from `endurance`.
    pub health: Real,
    /// Set once health reaches zero. A broken joint stops constraining anything,
    /// so whatever hung from it falls away.
    pub broken: bool,
}

impl Joint {
    pub fn fixed(body_a: u16, body_b: u16, anchor_a: Vec3, anchor_b: Vec3) -> Joint {
        Joint {
            body_a,
            body_b,
            kind: JointKind::Fixed,
            anchor_a,
            anchor_b,
            axis_a: Vec3::X,
            axis_b: Vec3::X,
            ref_a: Vec3::Y,
            ref_b: Vec3::Y,
            cos_limit: -1.0,
            motor_speed_max: 0.0,
            motor_torque_max: 0.0,
            motor_target: 0.0,
            tendon_frequency: 0.0,
            tendon_damping: 0.0,
            endurance: 0.0,
            health: 0.0,
            broken: false,
        }
    }

    /// Remaining health as a fraction of capacity: 1 is pristine, 0 is failed.
    /// An indestructible joint always reports 1.
    #[inline]
    pub fn health_fraction(&self) -> Real {
        if self.endurance > 0.0 {
            clamp(self.health / self.endurance, 0.0, 1.0)
        } else {
            1.0
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct JointPrep {
    ra: Vec3,
    rb: Vec3,
    axis_w: Vec3,
    perp1: Vec3,
    perp2: Vec3,
    k_point: Mat3,
    k_ang: Mat3,
    /// Effective angular mass about the hinge axis and the two perpendiculars.
    k_axis: Real,
    k_perp1: Real,
    k_perp2: Real,
    motor_impulse: Real,
}

/// A contact between two of the organism's own parts.
#[derive(Clone, Copy, Debug)]
struct PairContact {
    a: u16,
    b: u16,
    /// Offsets from each body's centre of mass to the shared contact point.
    r_a: Vec3,
    r_b: Vec3,
    /// Points from `a` toward `b`.
    normal: Vec3,
    depth: Real,
    k_n: Real,
    pn: Real,
    /// Accumulated *positional* normal impulse, kept apart from `pn` so the two
    /// halves of the solve each converge against their own history.
    pn_bias: Real,
}

#[derive(Clone, Copy, Debug)]
struct Contact {
    body: u16,
    /// Offset from the body's centre of mass to the contact point, world frame.
    r: Vec3,
    normal: Vec3,
    tangent1: Vec3,
    tangent2: Vec3,
    depth: Real,
    k_n: Real,
    k_t1: Real,
    k_t2: Real,
    /// Accumulated impulses, needed so the friction cone can be clamped against
    /// the normal impulse actually applied.
    pn: Real,
    pt1: Real,
    pt2: Real,
    /// Accumulated positional normal impulse. See [`World::bias_lin`].
    pn_bias: Real,
    /// Restitution target captured before solving.
    bounce: Real,
}

/// The simulation world for one organism.
pub struct World {
    pub bodies: Vec<RigidBody>,
    pub joints: Vec<Joint>,
    pub params: WorldParams,
    /// Per-body inverse inertia in world coordinates, refreshed once per step
    /// rather than once per solver iteration.
    inv_inertia: Vec<Mat3>,
    /// Positional correction, held as a velocity that never becomes momentum.
    ///
    /// Every constraint here has two jobs: stop bodies moving into each other,
    /// and undo the overlap they are already in. The second used to be done by
    /// adding a Baumgarte bias straight into `lin_vel`, which works and is also
    /// a motor: the velocity it injects to separate two bodies is still there
    /// after they have separated. An organism whose own parts kept
    /// re-penetrating collected `max_correction_speed` every step and kept it,
    /// and evolution found that long before it found walking — champions that
    /// crossed twenty-six metres with their motors switched off.
    ///
    /// So the correction is accumulated here instead, used only to displace
    /// bodies in [`Self::integrate_positions`], and discarded at the end of the
    /// step. Bodies still separate; separating no longer pays. This is Catto's
    /// split impulse, and it is why `solve_*` below each have a velocity half
    /// and a position half that look almost the same.
    bias_lin: Vec<Vec3>,
    bias_ang: Vec<Vec3>,
    prep: Vec<JointPrep>,
    contacts: Vec<Contact>,
    pair_contacts: Vec<PairContact>,
    /// Row-major `n x n` table of which body pairs are joined by a joint, and so
    /// are meant to touch. Built once, because it never changes.
    jointed: Vec<bool>,
    /// Whether each body has been cut loose from the root by a broken joint.
    /// Detached bodies still fall, tumble and collide with the ground — they are
    /// debris, not deletions — but they stop counting as part of the organism.
    detached: Vec<bool>,
    /// Joints that failed since the last time this was drained, with the time
    /// each failed at, so a recording can note when a limb came off.
    pub breaks: Vec<(u16, Real)>,
    /// Sum of `|motor angular impulse|` applied so far. A cheap, monotone proxy
    /// for actuation effort, used by energy-penalising fitness functions.
    pub actuation_impulse: Real,
    /// Simulation time accumulated by `step`, used only to timestamp breakages.
    elapsed: Real,
    /// Accumulated bookkeeping shift to subtract from the reported centre of
    /// mass. See [`World::centre_of_mass`].
    com_correction: Vec3,
    /// Set once any body leaves the representable range; the evaluation is then
    /// abandoned rather than allowed to produce meaningless fitness.
    pub diverged: bool,
}

impl World {
    pub fn new(bodies: Vec<RigidBody>, joints: Vec<Joint>, params: WorldParams) -> World {
        let n = bodies.len();
        let j = joints.len();
        // Parts joined by a joint are supposed to be in contact; only parts with
        // no joint between them are colliding when they overlap.
        let mut jointed = vec![false; n * n];
        for joint in &joints {
            let (a, b) = (joint.body_a as usize, joint.body_b as usize);
            jointed[a * n + b] = true;
            jointed[b * n + a] = true;
        }
        World {
            bodies,
            joints,
            params,
            inv_inertia: vec![Mat3::ZERO; n],
            bias_lin: vec![Vec3::ZERO; n],
            bias_ang: vec![Vec3::ZERO; n],
            prep: vec![JointPrep::default(); j],
            contacts: Vec::with_capacity(n * 8),
            pair_contacts: Vec::new(),
            jointed,
            detached: vec![false; n],
            breaks: Vec::new(),
            actuation_impulse: 0.0,
            elapsed: 0.0,
            com_correction: Vec3::ZERO,
            diverged: false,
        }
    }

    /// Advance the simulation by one fixed step.
    pub fn step(&mut self, dt: Real) {
        if self.diverged {
            return;
        }
        self.integrate_velocities(dt);
        self.refresh_inertia();
        // Tendons are a *force*, not a constraint, so they are applied once per
        // step alongside gravity rather than inside the solver's iteration loop.
        // Applied per iteration they would fire a dozen times a step and pump
        // energy into the organism — which, tried once, produced bodies
        // travelling thirty metres a second.
        self.build_contacts();
        self.build_pair_contacts();
        self.prepare_joints(dt);
        self.apply_tendons(dt);

        // The positional correction starts each step from nothing. It is a
        // property of the overlap that exists right now, not a quantity a body
        // is allowed to carry from one step to the next — carrying it is
        // exactly what made it a motor. See [`Self::bias_lin`].
        for i in 0..self.bodies.len() {
            self.bias_lin[i] = Vec3::ZERO;
            self.bias_ang[i] = Vec3::ZERO;
        }

        for _ in 0..self.params.iterations {
            self.solve_joints(dt);
            self.solve_contacts(dt);
            self.solve_pair_contacts(dt);
        }

        self.integrate_positions(dt);
        self.wear_joints(dt);
        self.check_finite();
    }
}
