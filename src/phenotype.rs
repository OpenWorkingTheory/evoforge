//! Genome expression: turning a [`Genome`] into a simulatable [`World`].
//!
//! This is the only place that knows how a gene becomes geometry. Keeping it
//! separate from the genome means the expression rules can be revised — different
//! attachment geometry, derived masses, symmetry operators — without changing
//! what is stored on disk, and means a genome is meaningful only in the context
//! of the code version and config that expressed it. Both facts are recorded with
//! every result.

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::genome::{Genome, JointKind, ShapeKind};
use crate::math::{dcos, vec3, Quat, Real, Vec3};
use crate::physics::{Joint, RigidBody, Shape, TerrainModel, World, WorldParams};

/// Height above the terrain at which an organism is spawned. Small but nonzero,
/// so the first step resolves a shallow contact rather than a deep overlap.
pub const SPAWN_CLEARANCE: Real = 0.02;

/// One face of a box, in body-local coordinates.
pub struct Face {
    pub normal: Vec3,
    /// The two in-plane directions. A hinge axis is always one of these, which
    /// guarantees every generated hinge is geometrically sensible.
    pub tangents: [Vec3; 2],
}

/// Face order used by [`crate::genome::PartGene::attach_face`].
pub const FACES: [Face; 6] = [
    Face { normal: vec3(1.0, 0.0, 0.0), tangents: [vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0)] },
    Face { normal: vec3(-1.0, 0.0, 0.0), tangents: [vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0)] },
    Face { normal: vec3(0.0, 1.0, 0.0), tangents: [vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0)] },
    Face { normal: vec3(0.0, -1.0, 0.0), tangents: [vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0)] },
    Face { normal: vec3(0.0, 0.0, 1.0), tangents: [vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0)] },
    Face { normal: vec3(0.0, 0.0, -1.0), tangents: [vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0)] },
];

/// Static description of a body, recorded with a replay so a viewer can draw the
/// organism without re-expressing the genome.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct BodySpec {
    /// Half-extents of the box bounding the part. Kept alongside `shape` because
    /// it is what the attachment rules and any bounding query want, and because
    /// it is enough on its own to draw a recognisable organism.
    pub half_extents: Vec3,
    /// The part's geometry. Absent in replays recorded before shapes existed,
    /// where every part was exactly the box `half_extents` describes.
    #[serde(default)]
    pub shape: Option<Shape>,
    pub slot: u8,
}

impl BodySpec {
    /// The part's geometry, filling in what an older replay meant by omission.
    pub fn geometry(&self) -> Shape {
        self.shape.unwrap_or(Shape::Box { half_extents: self.half_extents })
    }
}

/// An expressed organism, ready to simulate.
/// A range sensor as built: which body carries it, and where it looks in that
/// body's own frame.
///
/// The sensor has no existence apart from the body — it moves with it, rotates
/// with it, and is aimed by whatever drives that body's joint. The controller
/// slot is `body_slots[body]`, so nothing here needs its own indexing.
#[derive(Clone, Copy, Debug)]
pub struct SensorMount {
    pub body: u16,
    /// Unit direction in the carrying body's local frame.
    pub dir: Vec3,
}

pub struct Phenotype {
    pub world: World,
    /// Controller slot for each body, parallel to `world.bodies`.
    pub body_slots: Vec<u8>,
    /// Controller slot for each joint, parallel to `world.joints`. A joint takes
    /// the slot of its *child* part, so a limb's motor output and its angle
    /// sensor share an index.
    pub joint_slots: Vec<u8>,
    /// Sign applied to each joint's motor command, parallel to `world.joints`.
    /// Negative for the mirrored half of an antiphase pair.
    pub joint_drive: Vec<Real>,
    /// How many bodies answer to each slot, indexed by slot.
    pub slot_bodies: Vec<u8>,
    /// How many joints answer to each slot, indexed by slot.
    pub slot_joints: Vec<u8>,
    /// Every range sensor this body carries. Empty in an experiment without
    /// sensors, and empty for an organism that evolved none.
    pub sensors: Vec<SensorMount>,
    pub total_mass: Real,
}

/// Where a copy of a part sits, and which side of the midline it is on.
#[derive(Clone, Copy, Debug, Default)]
struct Mount {
    body: usize,
    /// `+1` right, `-1` left, `0` on the midline.
    side: Real,
}

/// The copies of one part. There are never more than two — a mirrored pair, or
/// the single inherited side of an already-paired parent — so this is a fixed
/// array rather than a `Vec`.
///
/// It is fixed for a reason. Expression runs once per trial per organism, which
/// is millions of times in a run, and a `Vec` here meant an allocation per part
/// plus a clone per part on top of it. On a twelve-thread machine that much
/// allocator traffic stops being free: threads block on the heap rather than
/// working, and the cores go quiet.
#[derive(Clone, Copy, Debug, Default)]
struct Mounts {
    slots: [Mount; 2],
    count: u8,
}

impl Mounts {
    fn push(&mut self, m: Mount) {
        debug_assert!((self.count as usize) < self.slots.len());
        self.slots[self.count as usize] = m;
        self.count += 1;
    }

    fn as_slice(&self) -> &[Mount] {
        &self.slots[..self.count as usize]
    }
}

/// Reflection across the sagittal plane.
#[inline]
fn mirror_z(v: Vec3) -> Vec3 {
    vec3(v.x, v.y, -v.z)
}

/// Force a shape that sits *on* the midline to be symmetric about it.
///
/// A skull, a spine, a ribcage: the structures an animal carries on its
/// centreline are all mirror-symmetric about that line, and they have to be —
/// anything else makes the whole organism lopsided no matter how carefully its
/// limbs are paired. Only a taper running left-to-right offends, and it is
/// turned to run along its longest other axis instead.
fn midline_symmetric(shape: Shape) -> Shape {
    match shape {
        Shape::Taper { half_extents: h, axis: 2, top_scale, flip } => {
            let axis = if h.x >= h.y { 0 } else { 1 };
            Shape::Taper { half_extents: h, axis, top_scale, flip }
        }
        other => other,
    }
}

/// A shape as seen in the sagittal mirror.
///
/// Boxes, spheres, capsules and cylinders are all symmetric about the plane, so
/// they reflect onto themselves. A taper is not: it is wide at one end, and
/// reflecting one that runs along Z has to swap which end that is.
fn reflect_shape(shape: Shape) -> Shape {
    match shape {
        Shape::Taper { half_extents, axis, top_scale, flip } if axis == 2 => {
            Shape::Taper { half_extents, axis, top_scale, flip: !flip }
        }
        other => other,
    }
}

/// How far off the midline the first of a pair is pushed when its gene would
/// have anchored it on the centreline, as a fraction of the parent's half-width.
const MIN_PAIR_SEPARATION: Real = 0.35;

impl Phenotype {
    pub fn body_specs(&self) -> Vec<BodySpec> {
        self.world
            .bodies
            .iter()
            .zip(&self.body_slots)
            .map(|(b, &slot)| BodySpec { half_extents: b.bounds(), shape: Some(b.shape), slot })
            .collect()
    }
}

/// Turn a shape gene and the box it is inscribed in into geometry.
///
/// Every primitive is *inscribed* in `half_extents` rather than sized by its own
/// genes. One size gene therefore keeps doing one job, the existing size
/// mutation operator works unchanged for every shape, and a part never grows
/// when its shape changes — only its mass and the way it meets the ground do.
///
/// Shapes with a long direction take the box's longest axis. That means a size
/// mutation which makes a different axis the longest will swing a capsule
/// through ninety degrees, which is a large morphological jump from a small
/// mutation; it is also exactly the kind of jump that a limb becoming a leg
/// needs, so it is left in deliberately.
pub fn carve(kind: ShapeKind, half_extents: Vec3, taper_top_scale: Real) -> Shape {
    let h = half_extents;
    match kind {
        ShapeKind::Box => Shape::Box { half_extents: h },
        ShapeKind::Taper => Shape::Taper {
            half_extents: h,
            axis: longest_axis(h),
            top_scale: taper_top_scale,
            flip: false,
        },
        ShapeKind::Sphere => Shape::Sphere { radius: h.x.min(h.y).min(h.z) },
        ShapeKind::Capsule => {
            let axis = longest_axis(h);
            let radius = shortest_cross_extent(h, axis);
            // The hemispherical caps are part of the length, so the cylindrical
            // section is what is left of the box's half-extent after them.
            Shape::Capsule { radius, half_length: (component(h, axis) - radius).max(0.0), axis }
        }
        ShapeKind::Cylinder => {
            let axis = longest_axis(h);
            Shape::Cylinder {
                radius: shortest_cross_extent(h, axis),
                half_length: component(h, axis),
                axis,
            }
        }
    }
}

#[inline]
fn component(v: Vec3, axis: u8) -> Real {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// Index of the largest half-extent; ties go to the lowest axis, so the rule is
/// total and deterministic.
fn longest_axis(h: Vec3) -> u8 {
    if h.x >= h.y && h.x >= h.z {
        0
    } else if h.y >= h.z {
        1
    } else {
        2
    }
}

/// The smaller of the two half-extents perpendicular to `axis`, which is the
/// largest radius that still fits inside the box.
fn shortest_cross_extent(h: Vec3, axis: u8) -> Real {
    match axis {
        0 => h.y.min(h.z),
        1 => h.x.min(h.z),
        _ => h.x.min(h.y),
    }
}

/// Cap a joint's torque at what its own girth could physically host.
///
/// Muscle force scales with cross-sectional area, and the torque that force
/// exerts scales with a moment arm that itself grows with the limb's width — so
/// the ceiling goes as `stress * area^1.5`. The narrower of the two parts sets
/// it, because a joint is only as strong as the thinner side of it.
///
/// A `stress` of zero leaves the gene alone, which is the pre-existing
/// behaviour. Otherwise the gene may still ask for *less* than the ceiling: it
/// stays a real choice about how much muscle to invest, rather than being
/// replaced by geometry outright.
fn muscle_limited(requested: Real, child: Vec3, parent: Vec3, axis: Vec3, stress: Real) -> Real {
    if stress <= 0.0 {
        return requested;
    }
    let ceiling = stress * cross_section(child, axis).min(cross_section(parent, axis)).powf(1.5);
    requested.min(ceiling)
}

/// Area of the bounding box's cross-section perpendicular to `axis`.
fn cross_section(half_extents: Vec3, axis: Vec3) -> Real {
    // Perpendicular to the axis, the two remaining half-extents span the section.
    let a = half_extents.abs();
    let n = axis.abs();
    // Pick out the two components the axis is not aligned with.
    let along = a.dot(n);
    let volume_section = 8.0 * a.x * a.y * a.z;
    if along > 1e-9 {
        // (2x)(2y)(2z) / (2 * along) = the section perpendicular to the axis.
        volume_section / (2.0 * along)
    } else {
        4.0 * a.x * a.z
    }
}

/// Express `genome` into a world according to `cfg`.
///
/// Deterministic and allocation-light: this runs once per evaluation, so it sits
/// on the hot path of the whole experiment.
pub fn build(genome: &Genome, cfg: &Config) -> Phenotype {
    build_with_start(genome, cfg, None)
}

/// How an organism is set down at the start of a trial.
///
/// An organism evaluated once, from an identical pose, on identical ground, is
/// being asked for a stunt rather than a gait: a single well-timed lunge scores
/// as well as walking. Varying the start across trials is what makes the
/// difference between a strategy that works and one that merely worked.
#[derive(Clone, Copy, Debug, Default)]
pub struct StartPerturbation {
    /// Rotation about the vertical, radians. The organism must still travel
    /// along +X, so this asks it to cope with not being aimed there.
    pub yaw: Real,
    /// Rotation about the forward axis, radians: set down slightly off balance.
    pub tilt: Real,
    /// Horizontal displacement of the whole body, metres. On rolling ground this
    /// also changes the terrain underneath it.
    pub offset: Vec3,
    /// Where this trial's slice of the landscape is taken from. Identity for
    /// every terrain but `fractal`, and for that one too unless
    /// `environment.terrain_per_trial` is set.
    pub terrain: TerrainShift,
}

/// A rigid motion of the fractal landscape under the world.
///
/// Moving the *organism* is not enough to stop a gait being tuned to one hill:
/// `start_jitter` shifts the start by at most half a metre, which on ground with
/// a six-metre wavelength is the same hill seen from slightly along. Sliding and
/// turning the field itself gives each trial genuinely different ground, at no
/// cost beyond two multiplies per sample.
///
/// Identity is the default, and identity means bit-for-bit the unmoved field:
/// `rot_cos = 1`, everything else zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainShift {
    /// Translation in units of one wavelength, applied in field space.
    pub offset_x: Real,
    pub offset_z: Real,
    /// Rotation about the world origin, as its sine and cosine. Stored rather
    /// than the angle so that no trigonometry happens per sample.
    pub sin: Real,
    pub cos: Real,
}

impl TerrainShift {
    pub const NONE: TerrainShift =
        TerrainShift { offset_x: 0.0, offset_z: 0.0, sin: 0.0, cos: 1.0 };
}

impl Default for TerrainShift {
    fn default() -> Self {
        TerrainShift::NONE
    }
}

/// Express `genome`, optionally setting it down perturbed.
pub fn build_with_start(
    genome: &Genome,
    cfg: &Config,
    start: Option<StartPerturbation>,
) -> Phenotype {
    let n = genome.parts.len();
    debug_assert!(n >= 1);

    let density = cfg.body.density;
    let top_scale = cfg.body.taper_top_scale;
    let paired_allowed = cfg.body.pair_probability > 0.0;

    // Geometric centres, not centres of mass: the attachment rules below are
    // written about the box a part is inscribed in, and for a taper the two
    // differ. The conversion happens once, where each body is constructed.
    // Upper bound on how many bodies this genome can express: every part
    // mirrored, and every one of those a full chain of segments. Reserving it up
    // front turns each of these into one allocation instead of a run of
    // doubling reallocations, on a path that runs once per trial per organism.
    let cap = n * 2 * (cfg.body.max_repeat.max(1) as usize);
    let mut centres: Vec<Vec3> = Vec::with_capacity(cap);
    let mut shapes: Vec<Shape> = Vec::with_capacity(cap);
    let mut bodies: Vec<RigidBody> = Vec::with_capacity(cap);
    let mut joints: Vec<Joint> = Vec::with_capacity(cap);
    let mut body_slots: Vec<u8> = Vec::with_capacity(cap);
    let mut joint_slots: Vec<u8> = Vec::with_capacity(cap);
    let mut joint_drive: Vec<Real> = Vec::with_capacity(cap);

    // A part no longer maps to one body. A paired part appears twice, mirrored;
    // a repeated part appears as a chain. `mounts[i]` records where part i's
    // children attach — one entry per copy, carrying the body index and which
    // side of the midline that copy sits on.
    let mut mounts: Vec<Mounts> = vec![Mounts::default(); n];

    let mut root = carve(genome.parts[0].shape, genome.parts[0].half_extents, top_scale);
    if paired_allowed {
        root = midline_symmetric(root);
    }
    centres.push(Vec3::ZERO);
    shapes.push(root);
    bodies.push(RigidBody::new(root.com_offset(), root, density));
    body_slots.push(genome.parts[0].slot);
    let mut sensors: Vec<SensorMount> = Vec::new();
    if genome.parts[0].sensor {
        sensors.push(SensorMount {
            body: (bodies.len() - 1) as u16,
            dir: sensor_dir_of(&genome.parts[0]),
        });
    }
    // The root straddles the midline, so it has no side of its own.
    mounts[0].push(Mount { body: 0, side: 0.0 });

    for i in 1..n {
        let part = &genome.parts[i];
        let parent_index = part.parent as usize;
        let parent_mounts = mounts[parent_index];
        let parent = parent_mounts.as_slice();

        // Which copies of this part exist, and on which side each sits.
        //
        // A part hanging off an already-paired parent inherits its parent's side
        // rather than pairing again — that is what makes a segment part of *a*
        // limb rather than the start of four of them.
        let mut sides: [(Mount, Real); 2] = Default::default();
        let side_count = if parent.len() > 1 {
            sides[0] = (parent[0], parent[0].side);
            sides[1] = (parent[1], parent[1].side);
            2
        } else if paired_allowed && part.paired {
            sides[0] = (parent[0], 1.0);
            sides[1] = (parent[0], -1.0);
            2
        } else {
            sides[0] = (parent[0], parent[0].side);
            1
        };

        for &(parent_mount, side) in &sides[..side_count] {
            let mut attach_to = parent_mount.body;
            let segments = if cfg.body.max_repeat > 1 { part.repeat.max(1) } else { 1 };

            for segment in 0..segments {
                let mut shape = carve(part.shape, part.half_extents, top_scale);
                // A wedge that points outward on one side has to point outward
                // on the other too. Only a taper along the mirrored axis is
                // affected; every other shape is its own reflection.
                if side < 0.0 {
                    shape = reflect_shape(shape);
                } else if paired_allowed && side == 0.0 {
                    shape = midline_symmetric(shape);
                }
                let extents = shape.bounds();
                let parent_extents = shapes[attach_to].bounds();
                let face = &FACES[(part.attach_face % 6) as usize];

                let n_abs = face.normal.abs();
                let t0_abs = face.tangents[0].abs();
                let t1_abs = face.tangents[1].abs();
                // Later segments sit squarely on the one before, so a repeated
                // part grows into a straight run — a spine, a tail, a limb —
                // rather than a staircase.
                let (u, v) = if segment == 0 { (part.attach_u, part.attach_v) } else { (0.0, 0.0) };
                let mut anchor_in_parent = face.normal * parent_extents.dot(n_abs)
                    + face.tangents[0] * (u * parent_extents.dot(t0_abs))
                    + face.tangents[1] * (v * parent_extents.dot(t1_abs));
                let mut child_offset = face.normal * extents.dot(n_abs);

                let axis_index = (part.joint.axis & 1) as usize;
                let mut axis = face.tangents[axis_index];
                let mut reference = face.tangents[1 - axis_index];

                // A pair anchored on the midline would be two limbs in the
                // same place, so push it off-centre first. This has to happen
                // *before* the reflection below and on both sides alike:
                // nudging only the right-hand copy would leave the two halves
                // no longer mirror images of each other.
                if part.paired && paired_allowed && segment == 0 {
                    let least = MIN_PAIR_SEPARATION * parent_extents.z;
                    if anchor_in_parent.z.abs() < least {
                        anchor_in_parent.z = least;
                    }
                }

                if side < 0.0 {
                    // Mirror across the sagittal plane. Anatomy, not convention:
                    // +X is the direction fitness measures and +Y is up, so left
                    // and right are +/-Z, and reflecting Z is what turns a limb
                    // into its opposite number.
                    anchor_in_parent = mirror_z(anchor_in_parent);
                    child_offset = mirror_z(child_offset);
                    axis = mirror_z(axis);
                    reference = mirror_z(reference);
                }

                let centre = centres[attach_to] + anchor_in_parent + child_offset;
                let body = bodies.len();
                centres.push(centre);
                shapes.push(shape);
                // Parts are spawned axis-aligned, so the centre-of-mass offset
                // needs no rotation here; it does once the body starts moving,
                // which is why `Shape::ground_points` rotates it.
                bodies.push(RigidBody::new(centre + shape.com_offset(), shape, density));
                body_slots.push(part.slot);
                if part.sensor {
                    // Both halves of a mirrored pair carry one, and both feed the
                    // same slot — their readings are averaged there, exactly as
                    // the pair's joint angles are.
                    sensors.push(SensorMount {
                        body: (bodies.len() - 1) as u16,
                        dir: sensor_dir_of(part),
                    });
                }

                joints.push(Joint {
                    body_a: attach_to as u16,
                    body_b: body as u16,
                    kind: part.joint.kind,
                    anchor_a: anchor_in_parent - shapes[attach_to].com_offset(),
                    anchor_b: -child_offset - shape.com_offset(),
                    axis_a: axis,
                    axis_b: axis,
                    ref_a: reference,
                    ref_b: reference,
                    cos_limit: match part.joint.kind {
                        JointKind::Hinge => dcos(part.joint.limit),
                        // A fixed joint's angular constraint is handled
                        // separately; the limit is never consulted.
                        JointKind::Fixed => -1.0,
                    },
                    motor_speed_max: part.joint.motor_speed,
                    motor_torque_max: match part.joint.kind {
                        JointKind::Hinge => muscle_limited(
                            part.joint.motor_torque,
                            extents,
                            parent_extents,
                            axis,
                            cfg.body.muscle_stress,
                        ),
                        JointKind::Fixed => 0.0,
                    },
                    motor_target: 0.0,
                    // Tendons act only across joints that can actually flex.
                    tendon_frequency: match part.joint.kind {
                        JointKind::Hinge => cfg.body.tendon_frequency,
                        JointKind::Fixed => 0.0,
                    },
                    tendon_damping: cfg.body.tendon_damping,
                    // A fixed weld has no motor, so nothing can overwork it and
                    // it never wears out. Only driven joints can be asked for
                    // more than they have.
                    endurance: match part.joint.kind {
                        JointKind::Hinge => cfg.body.joint_endurance,
                        JointKind::Fixed => 0.0,
                    },
                    health: match part.joint.kind {
                        JointKind::Hinge => cfg.body.joint_endurance,
                        JointKind::Fixed => 0.0,
                    },
                    broken: false,
                });
                joint_slots.push(part.slot);
                // A mirrored limb driven by the same signal moves as its
                // reflection, because its axis is reflected too. For a hinge
                // that swings fore and aft that gives an alternating gait;
                // negating it gives a bounding one. Which is better is not
                // obvious, so it is a gene.
                joint_drive.push(if side < 0.0 && part.antiphase { -1.0 } else { 1.0 });

                attach_to = body;
            }

            mounts[i].push(Mount { body: attach_to, side });
        }
    }

    // Drop the organism onto the terrain: translate straight up until no corner
    // is below the ground beneath *that corner*, plus a little clearance.
    //
    // Sampling the terrain per corner rather than once under the root is what
    // keeps this correct on the non-flat `TerrainModel`. For `Flat` it reduces
    // to exactly the same arithmetic.
    // Perturb before the drop, so a body set down at an angle still lands on the
    // ground rather than through it.
    if let Some(start) = start {
        let (sy, cy) = crate::math::dsincos(start.yaw * 0.5);
        let (st, ct) = crate::math::dsincos(start.tilt * 0.5);
        let spin = Quat { x: 0.0, y: sy, z: 0.0, w: cy }
            .mul(Quat { x: st, y: 0.0, z: 0.0, w: ct })
            .normalize();
        for b in bodies.iter_mut() {
            b.pos = spin.rotate(b.pos) + start.offset;
            b.orient = spin.mul(b.orient).normalize();
        }
    }

    let shift = start.map_or(TerrainShift::NONE, |s| s.terrain);
    let terrain = cfg_terrain(cfg, shift);
    let mut deepest = Real::NEG_INFINITY;
    for body in &bodies {
        let (points, count) = body.ground_points(Vec3::Y);
        for p in &points[..count] {
            deepest = deepest.max(terrain.height_at(p.x, p.z) - p.y);
        }
    }
    let lift = deepest + SPAWN_CLEARANCE;
    for b in bodies.iter_mut() {
        b.pos.y += lift;
    }

    let total_mass = bodies.iter().map(|b| b.mass()).sum();
    // How many bodies and joints answer to each controller slot. A paired or
    // repeated part has several, and they share one set of weights — which is
    // the point: two legs controlled by one leg controller move as a pair, and
    // that is what a gait is.
    let max_slots = cfg.brain_layout().max_slots;
    let mut slot_bodies = vec![0u8; max_slots];
    let mut slot_joints = vec![0u8; max_slots];
    for &s in &body_slots {
        slot_bodies[s as usize] = slot_bodies[s as usize].saturating_add(1);
    }
    for &s in &joint_slots {
        slot_joints[s as usize] = slot_joints[s as usize].saturating_add(1);
    }

    Phenotype {
        world: World::new(bodies, joints, world_params_on(cfg, shift)),
        body_slots,
        joint_slots,
        joint_drive,
        slot_bodies,
        slot_joints,
        sensors,
        total_mass,
    }
}

/// A sensor's aim, normalised, falling back to forward-and-down for a gene whose
/// direction has degenerated to nothing.
fn sensor_dir_of(part: &crate::genome::PartGene) -> Vec3 {
    part.sensor_dir.normalize_or(vec3(1.0, -0.5, 0.0).normalize_or(Vec3::X))
}

/// Stream tag for terrain seeds derived from `experiment.seed`.
const TERRAIN_STREAM: u64 = 0x5445_5252_4149_4e01;

/// The ground this config describes, with the landscape moved as `shift` says.
///
/// Public because choosing where to set an organism down needs to ask the
/// terrain what it looks like there, before there is a phenotype to build.
pub fn terrain_for(cfg: &Config, shift: TerrainShift) -> TerrainModel {
    cfg_terrain(cfg, shift)
}

fn cfg_terrain(cfg: &Config, shift: TerrainShift) -> TerrainModel {
    match cfg.environment.terrain {
        crate::config::Terrain::Flat => TerrainModel::Flat { height: 0.0 },
        crate::config::Terrain::Rough => TerrainModel::Rough {
            amplitude: cfg.environment.terrain_amplitude,
            wavelength: cfg.environment.terrain_wavelength,
        },
        crate::config::Terrain::Fractal => {
            let env = &cfg.environment;
            TerrainModel::Fractal(crate::physics::FractalField {
                // A seed of zero means "give me a landscape for this
                // experiment"; anything else names one, so two experiments can
                // be compared on identical ground.
                seed: match env.terrain_seed {
                    0 => crate::rng::derive_seed(&[cfg.experiment.seed, TERRAIN_STREAM]),
                    s => s,
                },
                amplitude: env.terrain_amplitude,
                wavelength: env.terrain_wavelength,
                octaves: env.terrain_octaves,
                lacunarity: env.terrain_lacunarity,
                gain: env.terrain_gain,
                warp: env.terrain_warp,
                detail_amplitude: env.terrain_detail_amplitude,
                detail_wavelength: env.terrain_detail_wavelength,
                detail_octaves: env.terrain_detail_octaves,
                modulation: env.terrain_modulation,
                modulation_wavelength: env.terrain_modulation_wavelength,
                step: env.terrain_step,
                riser: env.terrain_riser,
                terrace_mask: env.terrain_terrace_mask,
                offset_x: shift.offset_x,
                offset_z: shift.offset_z,
                rot_sin: shift.sin,
                rot_cos: shift.cos,
            })
        }
    }
}

pub fn world_params(cfg: &Config) -> WorldParams {
    world_params_on(cfg, TerrainShift::NONE)
}

/// The world this config describes, with the landscape moved as `shift` says.
pub fn world_params_on(cfg: &Config, shift: TerrainShift) -> WorldParams {
    WorldParams {
        gravity: vec3(0.0, -cfg.environment.gravity, 0.0),
        terrain: cfg_terrain(cfg, shift),
        friction: cfg.environment.friction,
        restitution: cfg.environment.restitution,
        iterations: cfg.simulation.solver_iterations,
        linear_damping: cfg.environment.linear_damping,
        angular_damping: cfg.environment.angular_damping,
        baumgarte: cfg.simulation.baumgarte,
        slop: cfg.simulation.slop,
        max_correction_speed: cfg.simulation.max_correction_speed,
        max_linear_speed: cfg.simulation.max_linear_speed,
        max_angular_speed: cfg.simulation.max_angular_speed,
        self_collision: cfg.environment.self_collision,
    }
}

#[cfg(test)]
mod tests;
