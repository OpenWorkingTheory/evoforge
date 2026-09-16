use super::util::*;
use super::*;
use crate::math::dsincos;

fn flat_params() -> WorldParams {
    WorldParams::default()
}

fn drop_box(height: Real) -> World {
    let body = RigidBody::box_body(vec3(0.0, height, 0.0), vec3(0.25, 0.25, 0.25), 250.0);
    World::new(vec![body], vec![], flat_params())
}

#[test]
fn a_dropped_box_falls_and_comes_to_rest_on_the_ground() {
    let mut w = drop_box(2.0);
    let dt = 1.0 / 120.0;
    for _ in 0..600 {
        w.step(dt);
    }
    assert!(!w.diverged);
    let b = &w.bodies[0];
    // Resting on a face: centre sits one half-extent above the ground.
    assert!((b.pos.y - 0.25).abs() < 0.02, "settled at y = {}", b.pos.y);
    assert!(b.lin_vel.length() < 0.05, "still moving: {:?}", b.lin_vel);
}

#[test]
fn free_fall_matches_analytic_solution() {
    let mut w = drop_box(100.0);
    w.params = WorldParams { linear_damping: 0.0, ..WorldParams::default() };
    let dt = 1.0 / 240.0;
    let n = 240;
    for _ in 0..n {
        w.step(dt);
    }
    let t = n as Real * dt;
    let expected = 100.0 - 0.5 * 9.81 * t * t;
    // Semi-implicit Euler overshoots by exactly g*dt*t/2; allow for it.
    assert!(
        (w.bodies[0].pos.y - expected).abs() < 0.05,
        "y = {}, expected ~{expected}",
        w.bodies[0].pos.y
    );
}

#[test]
fn friction_stops_a_sliding_box() {
    let mut w = drop_box(0.25);
    w.bodies[0].lin_vel = vec3(4.0, 0.0, 0.0);
    let dt = 1.0 / 120.0;
    for _ in 0..600 {
        w.step(dt);
    }
    assert!(w.bodies[0].lin_vel.x.abs() < 0.1, "vx = {}", w.bodies[0].lin_vel.x);
    assert!(w.bodies[0].pos.x > 0.1, "it should have slid some distance first");
}

#[test]
fn frictionless_box_keeps_sliding() {
    let mut w = drop_box(0.25);
    w.params.friction = 0.0;
    w.params.linear_damping = 0.0;
    w.bodies[0].lin_vel = vec3(4.0, 0.0, 0.0);
    let dt = 1.0 / 120.0;
    for _ in 0..240 {
        w.step(dt);
    }
    assert!(w.bodies[0].lin_vel.x > 3.5, "vx = {}", w.bodies[0].lin_vel.x);
}

/// Two boxes welded together must behave as one rigid object.
#[test]
fn a_fixed_joint_holds_bodies_together() {
    let a = RigidBody::box_body(vec3(0.0, 3.0, 0.0), vec3(0.2, 0.2, 0.2), 250.0);
    let b = RigidBody::box_body(vec3(0.4, 3.0, 0.0), vec3(0.2, 0.2, 0.2), 250.0);
    let joint = Joint::fixed(0, 1, vec3(0.2, 0.0, 0.0), vec3(-0.2, 0.0, 0.0));
    let mut w = World::new(vec![a, b], vec![joint], flat_params());
    let dt = 1.0 / 120.0;
    for _ in 0..900 {
        w.step(dt);
    }
    assert!(!w.diverged);
    let separation = (w.bodies[1].pos - w.bodies[0].pos).length();
    assert!((separation - 0.4).abs() < 0.02, "separation drifted to {separation}");
    // A weld also holds orientation.
    let q_rel = w.bodies[1].orient.mul(w.bodies[0].orient.conjugate());
    assert!(q_rel.vec().length() < 0.05, "relative rotation {q_rel:?}");
}

fn hinge_pair(limit_cos: Real, torque: Real) -> World {
    let a = RigidBody::box_body(vec3(0.0, 3.0, 0.0), vec3(0.2, 0.2, 0.2), 250.0);
    let b = RigidBody::box_body(vec3(0.4, 3.0, 0.0), vec3(0.2, 0.2, 0.2), 250.0);
    let joint = Joint {
        body_a: 0,
        body_b: 1,
        kind: JointKind::Hinge,
        anchor_a: vec3(0.2, 0.0, 0.0),
        anchor_b: vec3(-0.2, 0.0, 0.0),
        axis_a: Vec3::Z,
        axis_b: Vec3::Z,
        ref_a: Vec3::Y,
        ref_b: Vec3::Y,
        cos_limit: limit_cos,
        motor_speed_max: 4.0,
        motor_torque_max: torque,
        motor_target: 0.0,
        tendon_frequency: 0.0,
        tendon_damping: 0.0,
        endurance: 0.0,
        health: 0.0,
        broken: false,
    };
    let mut p = flat_params();
    p.gravity = Vec3::ZERO; // isolate joint behaviour from falling
    World::new(vec![a, b], vec![joint], p)
}

#[test]
fn a_hinge_motor_rotates_the_child() {
    let mut w = hinge_pair(-1.0, 200.0);
    w.joints[0].motor_target = 3.0;
    let dt = 1.0 / 240.0;
    for _ in 0..240 {
        w.step(dt);
    }
    assert!(!w.diverged);
    let (cos_t, _) = w.hinge_angle_cos_sin(0);
    assert!(cos_t < 0.9, "hinge barely moved, cos = {cos_t}");
    // The anchors must still coincide.
    let anchor_a = w.bodies[0].pos + w.bodies[0].orient.rotate(vec3(0.2, 0.0, 0.0));
    let anchor_b = w.bodies[1].pos + w.bodies[1].orient.rotate(vec3(-0.2, 0.0, 0.0));
    assert!((anchor_a - anchor_b).length() < 0.02);
}

/// A velocity-target motor has to brake as well as drive.
///
/// This exists because an evolved organism was found riding a wheel that
/// turned at 9.8 rad/s across a joint whose motor was capped at 6.0 — which
/// is legitimate only if the *ground* is spinning the wheel and the motor is
/// merely losing the argument. If instead the motor were one-directional,
/// any joint could be spun up for free and every fast organism in the
/// repository would be an artefact. With no contacts and no gravity there is
/// nothing to sustain the overspeed, so the motor must pull it back to
/// target on its own.
#[test]
fn a_hinge_motor_brakes_a_joint_spun_past_its_target() {
    let mut w = hinge_pair(-1.0, 200.0);
    w.joints[0].motor_target = 2.0;

    // Spin the child far beyond what the motor would ever drive.
    let overspeed = 20.0;
    w.bodies[1].ang_vel = Vec3::Z * overspeed;

    let dt = 1.0 / 240.0;
    for _ in 0..480 {
        w.step(dt);
    }
    assert!(!w.diverged);

    let rel = (w.bodies[1].ang_vel - w.bodies[0].ang_vel).dot(Vec3::Z);
    assert!(
        rel < 2.5,
        "motor did not brake an overspeeding joint: {rel} rad/s against a target of 2.0"
    );
    // And it brakes *to* the target rather than through it to a standstill.
    assert!(rel > 1.5, "motor overshot its target and stalled the joint: {rel} rad/s");
}

/// The other half: a motor may not drive a free joint past its own cap, so
/// the speed limit means something in the absence of outside help.
#[test]
fn a_hinge_motor_does_not_exceed_its_speed_cap() {
    let mut w = hinge_pair(-1.0, 200.0);
    // `motor_speed_max` is 4.0 in the fixture; ask for far more.
    w.joints[0].motor_target = 50.0;
    let dt = 1.0 / 240.0;
    for _ in 0..480 {
        w.step(dt);
    }
    assert!(!w.diverged);
    let rel = (w.bodies[1].ang_vel - w.bodies[0].ang_vel).dot(Vec3::Z);
    assert!(rel <= 4.5, "motor drove past its own speed cap: {rel} rad/s against 4.0");
}

/// A motor asked for more than its torque can deliver wears its joint out,
/// and when the joint fails the limb stops being part of the organism.
#[test]
fn an_overworked_joint_breaks_and_sheds_its_limb() {
    let mut w = hinge_pair(-1.0, 0.5); // a very weak motor
    w.joints[0].endurance = 1.0;
    w.joints[0].health = 1.0;
    w.joints[0].motor_target = 4.0; // far beyond what 0.5 N m can achieve
    assert!(w.is_attached(1));

    let dt = 1.0 / 240.0;
    for _ in 0..240 {
        w.step(dt);
    }
    assert!(!w.diverged);
    assert!(w.joints[0].broken, "joint survived with health {}", w.joints[0].health);
    assert!(!w.is_attached(1), "the limb is still counted as part of the organism");
    assert_eq!(w.breaks.len(), 1);

    // Fitness measures only what is still attached, so where the wreckage
    // goes is no longer any of its business. (The reported centre of mass is
    // not the root's raw position: the shift caused by dropping the limb out
    // of the average is deliberately cancelled — see `centre_of_mass`.)
    let before = w.centre_of_mass();
    w.bodies[1].pos = vec3(500.0, -400.0, 300.0);
    let after = w.centre_of_mass();
    assert!(
        (after - before).length() < 1e-5,
        "debris still moves the organism's measured position: {before:?} -> {after:?}"
    );
}

/// The same joint, driven just as hard, is indestructible when the
/// experiment has not enabled wear. This is the switch every other
/// experiment in the repository is sitting on.
#[test]
fn a_joint_with_no_endurance_never_wears_out() {
    let mut w = hinge_pair(-1.0, 0.5);
    w.joints[0].motor_target = 4.0;
    let dt = 1.0 / 240.0;
    for _ in 0..480 {
        w.step(dt);
    }
    assert!(!w.joints[0].broken);
    assert!(w.is_attached(1));
    assert!(w.breaks.is_empty());
}

/// A motor working within its means costs its joint nothing, so wear is a
/// charge for overreach rather than for being used at all.
#[test]
fn a_joint_driven_within_its_torque_takes_no_damage() {
    let mut w = hinge_pair(-1.0, 400.0); // plenty of torque
    w.joints[0].endurance = 1.0;
    w.joints[0].health = 1.0;
    w.joints[0].motor_target = 1.0;
    let dt = 1.0 / 240.0;
    for _ in 0..480 {
        w.step(dt);
    }
    assert!(!w.joints[0].broken);
    assert!(w.joints[0].health > 0.99, "an unstressed joint lost health: {}", w.joints[0].health);
}

/// Internal forces cannot move a centre of mass.
///
/// A motor and a joint exchange momentum between an organism's own parts.
/// They can spin it, fold it and tear it apart, but with no gravity and
/// nothing to touch, the mass-weighted mean of its parts must stay exactly
/// where it started. Anything else is the solver inventing momentum, and
/// evolution finds invented momentum faster than it finds walking.
///
/// Driven hard and reversed repeatedly, because that is the regime evolved
/// controllers actually use and the one where the per-body clamps in
/// `integrate_positions` could clip one half of an equal-and-opposite pair.
#[test]
fn a_motor_cannot_move_the_centre_of_mass() {
    let mut w = hinge_pair(-1.0, 40.0);
    w.params.gravity = Vec3::ZERO;
    // Far above any ground, and the terrain is flat at zero anyway.
    for b in w.bodies.iter_mut() {
        b.pos.y += 50.0;
    }

    let com = |w: &World| {
        let mut acc = Vec3::ZERO;
        let mut total = 0.0;
        for b in &w.bodies {
            let m = b.mass();
            acc += b.pos * m;
            total += m;
        }
        acc * (1.0 / total)
    };

    let start = com(&w);
    let dt = 1.0 / 120.0;
    for step in 0..1200 {
        // Slam the motor from one extreme to the other every few steps.
        w.joints[0].motor_target = if (step / 3) % 2 == 0 { 4.0 } else { -4.0 };
        w.step(dt);
    }
    let drift = (com(&w) - start).length();
    assert!(
        drift < 1e-3,
        "a motor with nothing to push against moved the centre of mass {drift} m"
    );
}

/// The same conservation law, with the parts a real organism actually has:
/// a joint limit to slam into and a tendon pulling back.
///
/// The plain hinge above conserves momentum exactly, so anything that leaks
/// leaks here — the limit's positional half and the tendon are the two
/// places a torque is applied that is not obviously equal and opposite.
#[test]
fn a_limit_and_a_tendon_cannot_move_the_centre_of_mass() {
    // cos_limit 0.5 is a +/- 60 degree range, so a motor driven flat out
    // hits the stop and stays there.
    let mut w = hinge_pair(0.5, 40.0);
    w.params.gravity = Vec3::ZERO;
    w.joints[0].tendon_frequency = 6.0;
    w.joints[0].tendon_damping = 0.5;
    for b in w.bodies.iter_mut() {
        b.pos.y += 50.0;
    }

    let com = |w: &World| {
        let mut acc = Vec3::ZERO;
        let mut total = 0.0;
        for b in &w.bodies {
            let m = b.mass();
            acc += b.pos * m;
            total += m;
        }
        acc * (1.0 / total)
    };

    let start = com(&w);
    let dt = 1.0 / 120.0;
    for step in 0..1200 {
        w.joints[0].motor_target = if (step / 3) % 2 == 0 { 4.0 } else { -4.0 };
        w.step(dt);
    }
    let drift = (com(&w) - start).length();
    assert!(
        drift < 1e-3,
        "a motor slamming a joint limit moved the centre of mass {drift} m with              nothing to push against"
    );
}

/// Shedding a limb must be worth exactly zero metres.
///
/// Dropping a body out of an average moves that average for free, and
/// `distance_x` is measured from that average. Without the correction in
/// [`World::centre_of_mass`] an organism could collect real fitness by
/// discarding a trailing part — which is what a run measured before this
/// test existed actually did, to the tune of a third of one organism's
/// recorded distance.
#[test]
fn detaching_a_limb_does_not_move_the_measured_centre_of_mass() {
    let mut w = hinge_pair(-1.0, 0.5);
    // Put the limb well to one side, so dropping it would shift the mean a
    // long way if the shift were not cancelled.
    w.bodies[1].pos = vec3(4.0, 3.0, 0.0);
    w.joints[0].endurance = 1.0;
    w.joints[0].health = 1.0;
    w.joints[0].motor_target = 4.0;

    let dt = 1.0 / 240.0;
    let mut previous = w.centre_of_mass();
    let mut worst_step = 0.0;
    let mut broke = false;
    for _ in 0..240 {
        w.step(dt);
        let com = w.centre_of_mass();
        worst_step = (com - previous).length().max(worst_step);
        previous = com;
        broke |= w.joints[0].broken;
    }
    assert!(broke, "the joint never failed, so nothing was tested");
    // Bodies move a little each step under their own momentum; a teleport
    // would be an order of magnitude larger than that.
    assert!(
        worst_step < 0.05,
        "the centre of mass jumped {worst_step} m in one step when the limb came off"
    );
}

/// Breaking one joint has to cut loose everything hanging below it, not just
/// the body immediately attached.
#[test]
fn breaking_a_joint_detaches_the_whole_subtree() {
    let mut w = hinge_pair(-1.0, 0.5);
    // Extend the chain: a third body welded to the second.
    let c = RigidBody::box_body(vec3(0.8, 3.0, 0.0), vec3(0.2, 0.2, 0.2), 250.0);
    w.bodies.push(c);
    w.joints.push(Joint::fixed(1, 2, vec3(0.2, 0.0, 0.0), vec3(-0.2, 0.0, 0.0)));
    let mut rebuilt = World::new(w.bodies.clone(), w.joints.clone(), w.params);
    rebuilt.joints[0].endurance = 1.0;
    rebuilt.joints[0].health = 1.0;
    rebuilt.joints[0].motor_target = 4.0;

    let dt = 1.0 / 240.0;
    for _ in 0..240 {
        rebuilt.step(dt);
    }
    assert!(rebuilt.joints[0].broken);
    assert!(!rebuilt.is_attached(1), "the limb is still attached");
    assert!(!rebuilt.is_attached(2), "the limb's own child is still attached");
    assert!(rebuilt.is_attached(0), "the root can never detach");
}

/// Two unjointed parts placed on top of each other must push apart.
#[test]
fn overlapping_parts_separate_when_self_collision_is_on() {
    let mut p = flat_params();
    p.gravity = Vec3::ZERO;
    p.self_collision = true;
    let a = RigidBody::box_body(vec3(0.0, 3.0, 0.0), vec3(0.2, 0.2, 0.2), 250.0);
    // Deliberately overlapping, and with no joint between them.
    let b = RigidBody::box_body(vec3(0.12, 3.0, 0.0), vec3(0.2, 0.2, 0.2), 250.0);
    let mut w = World::new(vec![a, b], vec![], p);

    let before = (w.bodies[0].pos - w.bodies[1].pos).length();
    for _ in 0..240 {
        w.step(1.0 / 240.0);
    }
    assert!(!w.diverged);
    let after = (w.bodies[0].pos - w.bodies[1].pos).length();
    assert!(after > before + 0.05, "parts did not separate: {before} -> {after}");
}

/// With it off they pass straight through, which is the behaviour every
/// experiment before this relied on.
#[test]
fn overlapping_parts_are_ignored_when_self_collision_is_off() {
    let mut p = flat_params();
    p.gravity = Vec3::ZERO;
    let a = RigidBody::box_body(vec3(0.0, 3.0, 0.0), vec3(0.2, 0.2, 0.2), 250.0);
    let b = RigidBody::box_body(vec3(0.12, 3.0, 0.0), vec3(0.2, 0.2, 0.2), 250.0);
    let mut w = World::new(vec![a, b], vec![], p);
    let before = (w.bodies[0].pos - w.bodies[1].pos).length();
    for _ in 0..240 {
        w.step(1.0 / 240.0);
    }
    let after = (w.bodies[0].pos - w.bodies[1].pos).length();
    assert!((after - before).abs() < 1e-4, "parts moved: {before} -> {after}");
}

/// Parts joined by a joint are *meant* to touch. If self-collision fought
/// the joint holding them together, every organism would tear itself apart.
#[test]
fn jointed_parts_do_not_collide_with_each_other() {
    let mut w = hinge_pair(-1.0, 0.0);
    w.params.self_collision = true;
    let dt = 1.0 / 240.0;
    for _ in 0..240 {
        w.step(dt);
    }
    assert!(!w.diverged);
    // The anchors must still coincide: nothing pushed them apart.
    let anchor_a = w.bodies[0].pos + w.bodies[0].orient.rotate(vec3(0.2, 0.0, 0.0));
    let anchor_b = w.bodies[1].pos + w.bodies[1].orient.rotate(vec3(-0.2, 0.0, 0.0));
    assert!(
        (anchor_a - anchor_b).length() < 0.02,
        "self-collision pulled a joint apart by {}",
        (anchor_a - anchor_b).length()
    );
}

/// The closest-point routine underpins every self-collision test above, and
/// its clamping is exactly the part that is easy to get wrong.
#[test]
fn closest_points_handles_parallel_crossing_and_degenerate_segments() {
    // Parallel, overlapping in their shared direction.
    let (a, b) = closest_points_on_segments(
        vec3(0.0, 0.0, 0.0),
        vec3(1.0, 0.0, 0.0),
        vec3(0.25, 1.0, 0.0),
        vec3(0.75, 1.0, 0.0),
    );
    assert!((a.y - b.y).abs() > 0.9 && (a - b).length() - 1.0 < 1e-5);

    // Crossing at right angles: the closest points are where they cross.
    let (a, b) = closest_points_on_segments(
        vec3(-1.0, 0.0, 0.0),
        vec3(1.0, 0.0, 0.0),
        vec3(0.0, 0.5, -1.0),
        vec3(0.0, 0.5, 1.0),
    );
    assert!(a.length() < 1e-5, "{a:?}");
    assert!((b - vec3(0.0, 0.5, 0.0)).length() < 1e-5, "{b:?}");

    // A point against a segment, and two points: a sphere is a segment of
    // zero length, so both have to work.
    let (a, b) = closest_points_on_segments(
        vec3(0.4, 2.0, 0.0),
        vec3(0.4, 2.0, 0.0),
        vec3(0.0, 0.0, 0.0),
        vec3(1.0, 0.0, 0.0),
    );
    assert!((a - vec3(0.4, 2.0, 0.0)).length() < 1e-5);
    assert!((b - vec3(0.4, 0.0, 0.0)).length() < 1e-5, "{b:?}");

    let (a, b) = closest_points_on_segments(
        vec3(1.0, 1.0, 1.0),
        vec3(1.0, 1.0, 1.0),
        vec3(-2.0, 0.0, 0.0),
        vec3(-2.0, 0.0, 0.0),
    );
    assert!((a - vec3(1.0, 1.0, 1.0)).length() < 1e-6);
    assert!((b - vec3(-2.0, 0.0, 0.0)).length() < 1e-6);
}

/// A tendon is passive: it may store and return energy, and it may lose it,
/// but it must never create any.
///
/// This exists because the first version did. Applied as an explicit torque
/// impulse, the damping term inverted for light limbs — `damping * dt /
/// inertia` above 2 amplifies instead of damping — and evolution found it
/// within a dozen generations, producing organisms crossing a hundred metres
/// in eight seconds. Expressing the spring as a frequency rather than a
/// stiffness is what makes it independent of the limb it acts on.
#[test]
fn a_tendon_never_adds_energy() {
    for freq in [2.0, 6.0, 20.0, 55.0] {
        for damping in [0.0, 0.5, 1.0] {
            let mut w = hinge_pair(-1.0, 0.0); // no motor at all
            w.joints[0].tendon_frequency = freq;
            w.joints[0].tendon_damping = damping;
            // Set it swinging, then leave it alone.
            w.bodies[1].ang_vel = Vec3::Z * 3.0;

            let dt = 1.0 / 120.0;
            let energy = |w: &World| -> Real {
                w.bodies
                    .iter()
                    .map(|b| {
                        let i = 1.0 / b.inv_inertia_local.z;
                        0.5 * b.mass() * b.lin_vel.length_sq() + 0.5 * i * b.ang_vel.length_sq()
                    })
                    .sum()
            };
            let start = energy(&w);
            let mut peak: Real = start;
            for _ in 0..600 {
                w.step(dt);
                peak = peak.max(energy(&w));
            }
            assert!(!w.diverged, "freq {freq} damping {damping}: diverged");
            // A spring converts kinetic energy to potential and back, so the
            // kinetic peak may exceed the start a little; it may not run away.
            assert!(
                peak < start * 3.0,
                "freq {freq} damping {damping}: energy grew from {start} to {peak}"
            );
        }
    }
}

/// And a damped tendon actually settles the joint rather than leaving it
/// ringing, which is the half of the behaviour that makes it useful.
#[test]
fn a_damped_tendon_brings_a_joint_to_rest() {
    let mut w = hinge_pair(-1.0, 0.0);
    w.joints[0].tendon_frequency = 8.0;
    w.joints[0].tendon_damping = 1.0;
    w.bodies[1].ang_vel = Vec3::Z * 3.0;
    for _ in 0..1200 {
        w.step(1.0 / 120.0);
    }
    let rate = (w.bodies[1].ang_vel - w.bodies[0].ang_vel).length();
    assert!(rate < 0.3, "joint still swinging at {rate} rad/s");
}

#[test]
fn a_hinge_limit_stops_rotation() {
    // cos(0.5 rad) ~ 0.8776
    let mut w = hinge_pair(crate::math::dcos(0.5), 200.0);
    w.joints[0].motor_target = 4.0;
    let dt = 1.0 / 240.0;
    for _ in 0..600 {
        w.step(dt);
    }
    assert!(!w.diverged);
    let (cos_t, _) = w.hinge_angle_cos_sin(0);
    // Allow a little overshoot from the soft constraint, but nothing close to
    // a free spin.
    assert!(cos_t > crate::math::dcos(0.75), "limit breached, cos = {cos_t}");
}

#[test]
fn a_hinge_without_a_motor_stays_where_it_is_put() {
    let mut w = hinge_pair(-1.0, 0.0);
    let dt = 1.0 / 240.0;
    for _ in 0..240 {
        w.step(dt);
    }
    let (cos_t, _) = w.hinge_angle_cos_sin(0);
    assert!(cos_t > 0.999, "drifted to cos = {cos_t}");
}

#[test]
fn stepping_is_bitwise_reproducible() {
    let run = || {
        let mut w = hinge_pair(0.5, 150.0);
        w.params.gravity = vec3(0.0, -9.81, 0.0);
        w.joints[0].motor_target = 2.5;
        for i in 0..500 {
            w.joints[0].motor_target = if i % 100 < 50 { 2.5 } else { -2.5 };
            w.step(1.0 / 120.0);
        }
        (w.bodies[0].pos, w.bodies[1].pos, w.actuation_impulse)
    };
    assert_eq!(run(), run());
}

#[test]
fn motor_effort_is_recorded() {
    let mut w = hinge_pair(-1.0, 100.0);
    assert_eq!(w.actuation_impulse, 0.0);
    w.joints[0].motor_target = 3.0;
    for _ in 0..120 {
        w.step(1.0 / 120.0);
    }
    assert!(w.actuation_impulse > 0.0);
}

#[test]
fn divergence_is_detected_rather_than_propagated() {
    let mut w = drop_box(1.0);
    w.bodies[0].lin_vel = vec3(Real::NAN, 0.0, 0.0);
    w.step(1.0 / 120.0);
    assert!(w.diverged);
    // Once diverged, stepping is a no-op rather than a source of further
    // garbage.
    let before = w.bodies[0].pos;
    w.step(1.0 / 120.0);
    assert!(before.x.is_nan() || before.x == w.bodies[0].pos.x);
}

/// Self-collision must not be a motor.
///
/// This is the shape of the bug that inflated every result in this project
/// for two months, reduced to forty lines: three parts in a chain, folded
/// so the two ends overlap. A pair of loose boxes cannot show it — shoved
/// apart, they separate once and stop. A folded chain is a *cycle*: the
/// joints pull the ends back into each other every step, self-collision
/// shoves them apart again, and when the shove was added straight into
/// `lin_vel` the pair became an engine that never ran down. Evolved
/// champions crossed twenty-six metres with their motors switched off, and
/// no test in the suite objected.
///
/// Nothing here is meant to do work: no motor, no tendon, and the
/// measurement starts after the chain has settled on flat ground, so there
/// is not even potential energy left to spend. On the solver this replaced,
/// it crawls 2.74 m per 7.5 s and keeps doing it indefinitely.
///
/// It is not zero now, and the honest reason is that split impulse stops
/// the correction becoming *momentum* without stopping it becoming
/// *displacement*: the ground is immovable, so a body in an internal cycle
/// can still ratchet against it a fraction of a millimetre at a time. The
/// residue is 0.46 m per 7.5 s here and unmeasurable on real organisms —
/// the champions that exploited the old behaviour now travel nothing at
/// all. The bound below is set to catch a regression toward the old
/// behaviour, not to certify zero.
#[test]
fn self_collision_is_not_a_motor() {
    let dt = 1.0 / 120.0;
    let mut worst: Real = 0.0;
    for fold in 1..10 {
        let reach = 0.30 - fold as Real * 0.028;
        let bodies: Vec<RigidBody> = (0..3)
            .map(|i| {
                let angle = i as Real * 1.9;
                let (s, c) = dsincos(angle);
                RigidBody::box_body(
                    vec3(reach * c, 1.0 + i as Real * 0.05, reach * s),
                    vec3(0.12, 0.12, 0.12),
                    1000.0,
                )
            })
            .collect();
        let joints: Vec<Joint> = (0..2)
            .map(|i| {
                let mid = (bodies[i].pos + bodies[i + 1].pos) * 0.5;
                Joint::fixed(i as u16, i as u16 + 1, mid - bodies[i].pos, mid - bodies[i + 1].pos)
            })
            .collect();
        let params = WorldParams { self_collision: true, ..WorldParams::default() };
        let mut w = World::new(bodies, joints, params);

        // Let it fall and settle; everything after this starts from rest.
        for _ in 0..240 {
            w.step(dt);
        }
        assert!(!w.diverged, "fold {fold}: diverged while settling");
        let (x0, z0) = (w.bodies[0].pos.x, w.bodies[0].pos.z);
        for _ in 0..900 {
            w.step(dt);
        }
        assert!(!w.diverged, "fold {fold}: diverged");
        let (dx, dz) = (w.bodies[0].pos.x - x0, w.bodies[0].pos.z - z0);
        worst = worst.max((dx * dx + dz * dz).sqrt());
    }
    assert!(worst < 1.0, "a motorless folded chain crawled {worst} m in 7.5 s");
}
