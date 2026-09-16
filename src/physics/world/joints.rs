//! Joint constraints: anchors, hinge limits, tendons and motors.

use super::util::*;
use super::*;

impl World {
    // -----------------------------------------------------------------------
    // Joints
    // -----------------------------------------------------------------------

    pub(super) fn prepare_joints(&mut self, _dt: Real) {
        for (i, j) in self.joints.iter().enumerate() {
            let a = &self.bodies[j.body_a as usize];
            let b = &self.bodies[j.body_b as usize];
            let inv_ia = self.inv_inertia[j.body_a as usize];
            let inv_ib = self.inv_inertia[j.body_b as usize];

            let ra = a.orient.rotate(j.anchor_a);
            let rb = b.orient.rotate(j.anchor_b);

            // K = (ima + imb) I - [ra] Ia [ra] - [rb] Ib [rb]
            let sa = Mat3::skew(ra);
            let sb = Mat3::skew(rb);
            let mass_term = Mat3::diagonal(Vec3::splat(a.inv_mass + b.inv_mass));
            let k_point = mass_term
                .sub(&sa.mul_mat(&inv_ia).mul_mat(&sa))
                .sub(&sb.mul_mat(&inv_ib).mul_mat(&sb));

            let k_ang = inv_ia.add(&inv_ib);

            let axis_w = a.orient.rotate(j.axis_a).normalize_or(Vec3::X);
            let perp1 = axis_w.any_perpendicular();
            let perp2 = axis_w.cross(perp1);

            self.prep[i] = JointPrep {
                ra,
                rb,
                axis_w,
                perp1,
                perp2,
                k_point,
                k_ang,
                k_axis: axis_w.dot(k_ang.mul_vec(axis_w)),
                k_perp1: perp1.dot(k_ang.mul_vec(perp1)),
                k_perp2: perp2.dot(k_ang.mul_vec(perp2)),
                motor_impulse: 0.0,
            };
        }
    }

    pub(super) fn solve_joints(&mut self, dt: Real) {
        let inv_dt = 1.0 / dt;
        let beta = self.params.baumgarte;
        let max_corr = self.params.max_correction_speed;

        for i in 0..self.joints.len() {
            let j = self.joints[i];
            if j.broken {
                continue;
            }
            let p = self.prep[i];
            let ia = j.body_a as usize;
            let ib = j.body_b as usize;
            let inv_ia = self.inv_inertia[ia];
            let inv_ib = self.inv_inertia[ib];

            // --- Point-to-point: the anchors must coincide. ---
            // Velocity half: hold the anchors moving together.
            let v_rel = self.bodies[ib].point_velocity(p.rb) - self.bodies[ia].point_velocity(p.ra);
            let impulse = p.k_point.solve(-v_rel);
            self.bodies[ia].apply_impulse(p.ra, -impulse, &inv_ia);
            self.bodies[ib].apply_impulse(p.rb, impulse, &inv_ib);

            // Position half: pull apart anchors that have already drifted. A
            // joint stretched every step by a limb it cannot hold would
            // otherwise be a motor in exactly the way self-collision was.
            let anchor_a = self.bodies[ia].pos + p.ra;
            let anchor_b = self.bodies[ib].pos + p.rb;
            let error = anchor_b - anchor_a;
            let mut bias = error * (-beta * inv_dt);
            clamp_speed(&mut bias, max_corr);
            let vb_rel = self.bias_point_velocity(ib, p.rb) - self.bias_point_velocity(ia, p.ra);
            let bias_impulse = p.k_point.solve(bias - vb_rel);
            self.apply_bias_impulse(ia, p.ra, -bias_impulse);
            self.apply_bias_impulse(ib, p.rb, bias_impulse);

            // --- Angular. ---
            match j.kind {
                JointKind::Fixed => {
                    // Drive the relative rotation back to the rest pose. Bodies
                    // start axis-aligned, so the rest relative rotation is the
                    // identity and the error is just the relative quaternion.
                    let w_rel = self.bodies[ib].ang_vel - self.bodies[ia].ang_vel;
                    let ang_impulse = p.k_ang.solve(-w_rel);
                    self.bodies[ia].apply_angular_impulse(-ang_impulse, &inv_ia);
                    self.bodies[ib].apply_angular_impulse(ang_impulse, &inv_ib);

                    let q_rel = self.bodies[ib].orient.mul(self.bodies[ia].orient.conjugate());
                    let sign = if q_rel.w < 0.0 { -1.0 } else { 1.0 };
                    let err = q_rel.vec() * (2.0 * sign);
                    let mut ang_bias = err * (-beta * inv_dt);
                    clamp_speed(&mut ang_bias, max_corr * 4.0);
                    let wb_rel = self.bias_ang[ib] - self.bias_ang[ia];
                    let bias_impulse = p.k_ang.solve(ang_bias - wb_rel);
                    self.apply_bias_angular_impulse(ia, -bias_impulse);
                    self.apply_bias_angular_impulse(ib, bias_impulse);
                }
                JointKind::Hinge => {
                    // Remove the two rotational degrees of freedom that are not
                    // about the hinge axis, leaving exactly one free.
                    let axis_b_w = self.bodies[ib].orient.rotate(j.axis_b);
                    let misalign = p.axis_w.cross(axis_b_w);
                    for (t, k) in [(p.perp1, p.k_perp1), (p.perp2, p.k_perp2)] {
                        if k <= 0.0 {
                            continue;
                        }
                        let w_rel = self.bodies[ib].ang_vel - self.bodies[ia].ang_vel;
                        let lambda = -w_rel.dot(t) / k;
                        let imp = t * lambda;
                        self.bodies[ia].apply_angular_impulse(-imp, &inv_ia);
                        self.bodies[ib].apply_angular_impulse(imp, &inv_ib);

                        let target = clamp(
                            -beta * inv_dt * misalign.dot(t),
                            -max_corr * 4.0,
                            max_corr * 4.0,
                        );
                        let wb_rel = self.bias_ang[ib] - self.bias_ang[ia];
                        let lb = (target - wb_rel.dot(t)) / k;
                        let imp_b = t * lb;
                        self.apply_bias_angular_impulse(ia, -imp_b);
                        self.apply_bias_angular_impulse(ib, imp_b);
                    }

                    self.solve_hinge_limit(i, dt);
                    self.solve_hinge_motor(i, dt);
                }
            }
        }
    }

    pub(super) fn solve_hinge_limit(&mut self, i: usize, dt: Real) {
        let j = self.joints[i];
        let p = self.prep[i];
        if p.k_axis <= 0.0 {
            return;
        }
        let (cos_theta, sin_theta) = self.hinge_angle_cos_sin(i);
        if cos_theta >= j.cos_limit {
            return; // inside the allowed range
        }

        let ia = j.body_a as usize;
        let ib = j.body_b as usize;
        let inv_ia = self.inv_inertia[ia];
        let inv_ib = self.inv_inertia[ib];

        // Sign of the direction in which the joint is over-rotated.
        let dir = if sin_theta >= 0.0 { 1.0 } else { -1.0 };

        // Velocity half: stop the joint rotating further past its limit. It is
        // allowed to come back on its own; it is not allowed to keep going.
        let w_rel = self.bodies[ib].ang_vel - self.bodies[ia].ang_vel;
        let rate = dir * w_rel.dot(p.axis_w);
        if rate > 0.0 {
            let lambda = -rate / p.k_axis;
            let imp = p.axis_w * (lambda * dir);
            self.bodies[ia].apply_angular_impulse(-imp, &inv_ia);
            self.bodies[ib].apply_angular_impulse(imp, &inv_ib);
        }

        // Position half: unwind the overshoot that already happened. Overshoot
        // is measured in cosine rather than radians — monotone in |theta| over
        // the half-turn a hinge limit can occupy, and free of `acos`.
        let overshoot = j.cos_limit - cos_theta;
        let push_back = -clamp(
            self.params.baumgarte * overshoot / dt,
            0.0,
            self.params.max_correction_speed * 4.0,
        );
        let wb_rel = self.bias_ang[ib] - self.bias_ang[ia];
        let bias_rate = dir * wb_rel.dot(p.axis_w);
        if bias_rate > push_back {
            let lb = (push_back - bias_rate) / p.k_axis;
            let imp = p.axis_w * (lb * dir);
            self.apply_bias_angular_impulse(ia, -imp);
            self.apply_bias_angular_impulse(ib, imp);
        }
    }

    /// A passive spring and damper across the hinge: a tendon.
    ///
    /// Animals do not move by servo. A great deal of what makes running and
    /// hopping efficient is elastic: tendons store energy on landing and return
    /// it on push-off, so the muscle does not have to pay for the whole stride.
    /// Without any passive element, every joule of a gait has to come out of the
    /// motor, which is why evolved gaits here look so unlike animal ones.
    ///
    /// The restoring torque uses `sin(angle)` rather than the angle itself. It
    /// is monotone over the whole legal range — [`crate::config`] refuses a
    /// joint limit at or beyond a quarter turn — costs no `atan2`, and is
    /// already computed for the controller's benefit.
    pub(super) fn apply_tendons(&mut self, dt: Real) {
        for i in 0..self.joints.len() {
            let j = self.joints[i];
            if j.broken || j.kind != JointKind::Hinge || j.tendon_frequency <= 0.0 {
                continue;
            }
            let k = self.prep[i].k_axis;
            if k <= 0.0 {
                continue;
            }
            let axis = self.prep[i].axis_w;
            let (_, sin_a) = self.hinge_angle_cos_sin(i);
            let ia = j.body_a as usize;
            let ib = j.body_b as usize;
            let rate = (self.bodies[ib].ang_vel - self.bodies[ia].ang_vel).dot(axis);

            // The change in relative rate a spring of this frequency asks for
            // over one step. Working in rate rather than torque is what makes it
            // independent of the limb's inertia, and therefore stable.
            let w = j.tendon_frequency;
            let spring = -w * w * sin_a * dt;
            // The damper may remove the joint's motion but never reverse it,
            // which is the difference between damping and driving.
            let bleed = clamp(2.0 * j.tendon_damping * w * dt, 0.0, 1.0);
            let delta_rate = spring - bleed * rate;

            let lambda = delta_rate / k;
            if lambda == 0.0 {
                continue;
            }
            let inv_ia = self.inv_inertia[ia];
            let inv_ib = self.inv_inertia[ib];
            let imp = axis * lambda;
            self.bodies[ia].apply_angular_impulse(-imp, &inv_ia);
            self.bodies[ib].apply_angular_impulse(imp, &inv_ib);
        }
    }

    pub(super) fn solve_hinge_motor(&mut self, i: usize, dt: Real) {
        let j = self.joints[i];
        let p = self.prep[i];
        if p.k_axis <= 0.0 || j.motor_torque_max <= 0.0 {
            return;
        }
        let ia = j.body_a as usize;
        let ib = j.body_b as usize;
        let inv_ia = self.inv_inertia[ia];
        let inv_ib = self.inv_inertia[ib];

        let target = clamp(j.motor_target, -j.motor_speed_max, j.motor_speed_max);
        let w_rel = self.bodies[ib].ang_vel - self.bodies[ia].ang_vel;
        let current = w_rel.dot(p.axis_w);
        let desired = (target - current) / p.k_axis;

        // Accumulate so the torque budget applies to the whole step rather than
        // being granted afresh on every solver iteration.
        let max_impulse = j.motor_torque_max * dt;
        let old = p.motor_impulse;
        let new = clamp(old + desired, -max_impulse, max_impulse);
        let lambda = new - old;
        self.prep[i].motor_impulse = new;
        if lambda == 0.0 {
            return;
        }
        let imp = p.axis_w * lambda;
        self.bodies[ia].apply_angular_impulse(-imp, &inv_ia);
        self.bodies[ib].apply_angular_impulse(imp, &inv_ib);
        self.actuation_impulse += lambda.abs();
    }
}
