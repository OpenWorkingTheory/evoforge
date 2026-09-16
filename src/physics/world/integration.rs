//! Advancing and maintaining state: velocities, positions, joint wear, detachment.

use super::util::*;
use super::*;

impl World {
    /// Charge each saturated motor for the rotation it failed to deliver.
    ///
    /// A joint is only harmed while its motor is at the torque ceiling its
    /// genome set — that is exactly the state of being asked for more than it
    /// can give. The damage is the shortfall in the rotation actually achieved,
    /// in radians, which makes endurance a quantity with a physical meaning
    /// rather than an arbitrary point score, and makes it independent of the
    /// solver's iteration count.
    pub(super) fn wear_joints(&mut self, dt: Real) {
        for i in 0..self.joints.len() {
            let j = self.joints[i];
            if j.broken || j.endurance <= 0.0 || j.motor_torque_max <= 0.0 {
                continue;
            }
            // Saturated means the accumulated motor impulse hit its budget.
            let budget = j.motor_torque_max * dt;
            if self.prep[i].motor_impulse.abs() < budget * 0.999 {
                continue;
            }
            let target = clamp(j.motor_target, -j.motor_speed_max, j.motor_speed_max);
            let axis = self.prep[i].axis_w;
            let achieved = (self.bodies[j.body_b as usize].ang_vel
                - self.bodies[j.body_a as usize].ang_vel)
                .dot(axis);
            let shortfall = (target - achieved).abs();
            if shortfall <= 0.0 {
                continue;
            }
            let joint = &mut self.joints[i];
            joint.health -= shortfall * dt;
            if joint.health <= 0.0 {
                joint.health = 0.0;
                joint.broken = true;
                self.breaks.push((i as u16, self.elapsed));
                // Take the shift this detachment causes and cancel it, so
                // shedding a limb neither teleports the organism forward nor
                // drags it back.
                let before = self.attached_centre_of_mass();
                self.refresh_detached();
                let after = self.attached_centre_of_mass();
                self.com_correction += after - before;
            }
        }
        self.elapsed += dt;
    }

    /// Recompute which bodies are still connected to the root.
    ///
    /// Relies on the invariant [`crate::phenotype::build`] maintains: joints are
    /// stored parent-before-child, so one forward pass propagates a break down
    /// the whole subtree hanging off it.
    pub(super) fn refresh_detached(&mut self) {
        for d in self.detached.iter_mut() {
            *d = false;
        }
        for j in &self.joints {
            let cut = j.broken || self.detached[j.body_a as usize];
            if cut {
                self.detached[j.body_b as usize] = true;
            }
        }
    }
    // -----------------------------------------------------------------------
    // Integration
    // -----------------------------------------------------------------------

    pub(super) fn integrate_velocities(&mut self, dt: Real) {
        let g = self.params.gravity;
        let lin_scale = 1.0 - clamp(self.params.linear_damping * dt, 0.0, 1.0);
        let ang_scale = 1.0 - clamp(self.params.angular_damping * dt, 0.0, 1.0);
        for b in self.bodies.iter_mut() {
            b.lin_vel += g * dt;
            b.lin_vel = b.lin_vel * lin_scale;
            b.ang_vel = b.ang_vel * ang_scale;
        }
    }

    /// Move bodies by their velocity *plus* the step's positional correction.
    ///
    /// The correction displaces and is then dropped: it never reaches
    /// `lin_vel`, so a body that has been pushed out of an overlap is left
    /// exactly as fast as it was before. It is clamped on the same terms as the
    /// individual constraint biases that produced it, because a pathological
    /// mutated body can pile up dozens of overlapping contacts and their
    /// corrections all point the same way.
    pub(super) fn integrate_positions(&mut self, dt: Real) {
        let max_v = self.params.max_linear_speed;
        let max_w = self.params.max_angular_speed;
        let max_corr = self.params.max_correction_speed;
        for (i, b) in self.bodies.iter_mut().enumerate() {
            clamp_speed(&mut b.lin_vel, max_v);
            clamp_speed(&mut b.ang_vel, max_w);
            let mut bias_lin = self.bias_lin[i];
            let mut bias_ang = self.bias_ang[i];
            clamp_speed(&mut bias_lin, max_corr);
            clamp_speed(&mut bias_ang, max_corr * 4.0);
            b.pos += (b.lin_vel + bias_lin) * dt;
            b.orient = b.orient.integrate(b.ang_vel + bias_ang, dt);
        }
    }

    /// Velocity of a body-fixed point under the positional correction alone.
    #[inline]
    pub(super) fn bias_point_velocity(&self, i: usize, r: Vec3) -> Vec3 {
        self.bias_lin[i] + self.bias_ang[i].cross(r)
    }

    /// The positional-correction counterpart of [`RigidBody::apply_impulse`].
    #[inline]
    pub(super) fn apply_bias_impulse(&mut self, i: usize, r: Vec3, impulse: Vec3) {
        self.bias_lin[i] += impulse * self.bodies[i].inv_mass;
        self.bias_ang[i] += self.inv_inertia[i].mul_vec(r.cross(impulse));
    }

    #[inline]
    pub(super) fn apply_bias_angular_impulse(&mut self, i: usize, impulse: Vec3) {
        self.bias_ang[i] += self.inv_inertia[i].mul_vec(impulse);
    }

    pub(super) fn refresh_inertia(&mut self) {
        for (i, b) in self.bodies.iter().enumerate() {
            self.inv_inertia[i] = b.inv_inertia_world();
        }
    }

    pub(super) fn check_finite(&mut self) {
        for b in &self.bodies {
            if !b.is_finite() || b.pos.length_sq() > 1.0e8 {
                self.diverged = true;
                return;
            }
        }
    }
}
