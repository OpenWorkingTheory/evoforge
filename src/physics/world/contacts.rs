//! Contact generation and resolution, against the terrain and against itself.

use super::util::*;
use super::*;

impl World {
    // -----------------------------------------------------------------------
    // Self-collision
    // -----------------------------------------------------------------------

    /// Find places where two of the organism's own parts have run into each
    /// other.
    ///
    /// # Why this exists
    ///
    /// Without it, parts pass straight through one another, and a body has no
    /// reason to be a body: limbs can occupy the torso, two legs can share a
    /// space, and a coherent shape with its limbs on the outside has no
    /// advantage over a cloud of overlapping blocks. Occupying a volume is the
    /// most basic thing an animal does, and it is a *constraint*, not a reward.
    ///
    /// # What it costs
    ///
    /// The solver was built on the assumption that this would never exist (see
    /// the module header), so the cheapest honest version is used: parts are
    /// approximated by capsules, pairs joined by a joint are skipped because
    /// they are meant to touch, and only a normal impulse is solved — no
    /// friction between an organism's own parts. Bodies are few, so the pairing
    /// is quadratic and unapologetic, behind a bounding-sphere reject.
    pub(super) fn build_pair_contacts(&mut self) {
        self.pair_contacts.clear();
        if !self.params.self_collision {
            return;
        }
        let n = self.bodies.len();
        for a in 0..n {
            for b in (a + 1)..n {
                if self.jointed[a * n + b] {
                    continue;
                }
                let (a0, a1, ra) = self.bodies[a].collision_segment();
                let (b0, b1, rb) = self.bodies[b].collision_segment();

                // Cheap reject before the closest-point work.
                let gap = self.bodies[a].pos - self.bodies[b].pos;
                let reach = self.bodies[a].bounding_radius() + self.bodies[b].bounding_radius();
                if gap.length_sq() > reach * reach {
                    continue;
                }

                let (pa, pb) = closest_points_on_segments(a0, a1, b0, b1);
                let delta = pb - pa;
                let distance = delta.length();
                let touching = ra + rb;
                if distance >= touching {
                    continue;
                }
                // Coincident centres give no direction to push apart along;
                // any consistent one will do, and the position correction will
                // separate them over the next few steps.
                let normal = if distance > 1e-6 { delta * (1.0 / distance) } else { Vec3::Y };
                let depth = touching - distance;

                let contact_a = pa + normal * ra;
                let contact_b = pb - normal * rb;
                let midpoint = (contact_a + contact_b) * 0.5;
                let r_a = midpoint - self.bodies[a].pos;
                let r_b = midpoint - self.bodies[b].pos;

                let k = pair_effective_mass(
                    &self.bodies[a],
                    &self.bodies[b],
                    &self.inv_inertia[a],
                    &self.inv_inertia[b],
                    r_a,
                    r_b,
                    normal,
                );
                if k <= 0.0 {
                    continue;
                }
                self.pair_contacts.push(PairContact {
                    a: a as u16,
                    b: b as u16,
                    r_a,
                    r_b,
                    normal,
                    depth,
                    k_n: k,
                    pn: 0.0,
                    pn_bias: 0.0,
                });
            }
        }
    }

    /// Push interpenetrating parts apart. Normal impulse only: friction between
    /// an organism's own limbs would be a second-order effect on top of a
    /// first-order approximation.
    pub(super) fn solve_pair_contacts(&mut self, dt: Real) {
        let inv_dt = 1.0 / dt;
        let beta = self.params.baumgarte;
        let slop = self.params.slop;
        let max_corr = self.params.max_correction_speed;

        for i in 0..self.pair_contacts.len() {
            let c = self.pair_contacts[i];
            let ia = c.a as usize;
            let ib = c.b as usize;
            let inv_ia = self.inv_inertia[ia];
            let inv_ib = self.inv_inertia[ib];

            // Velocity half: stop the parts driving further into each other.
            // The normal points from a to b, so separating means vn > 0.
            let relative =
                self.bodies[ib].point_velocity(c.r_b) - self.bodies[ia].point_velocity(c.r_a);
            let vn = relative.dot(c.normal);
            let mut lambda = -vn / c.k_n;
            let old = c.pn;
            let new = (old + lambda).max(0.0);
            lambda = new - old;
            self.pair_contacts[i].pn = new;
            if lambda != 0.0 {
                let impulse = c.normal * lambda;
                self.bodies[ia].apply_impulse(c.r_a, -impulse, &inv_ia);
                self.bodies[ib].apply_impulse(c.r_b, impulse, &inv_ib);
            }

            // Position half. This is the one that mattered: shoving overlapping
            // limbs apart at up to `max_correction_speed` and *keeping* the
            // velocity was a rocket any sprawling body could fire, and the
            // whole population found it.
            let bias = clamp((c.depth - slop).max(0.0) * beta * inv_dt, 0.0, max_corr);
            if bias > 0.0 {
                let vb = (self.bias_point_velocity(ib, c.r_b)
                    - self.bias_point_velocity(ia, c.r_a))
                .dot(c.normal);
                let mut lb = (bias - vb) / c.k_n;
                let old_b = c.pn_bias;
                let new_b = (old_b + lb).max(0.0);
                lb = new_b - old_b;
                self.pair_contacts[i].pn_bias = new_b;
                if lb != 0.0 {
                    let impulse = c.normal * lb;
                    self.apply_bias_impulse(ia, c.r_a, -impulse);
                    self.apply_bias_impulse(ib, c.r_b, impulse);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Contacts
    // -----------------------------------------------------------------------

    pub(super) fn build_contacts(&mut self) {
        self.contacts.clear();
        let terrain = self.params.terrain;
        let restitution = self.params.restitution;

        for (bi, body) in self.bodies.iter().enumerate() {
            let inv_i = &self.inv_inertia[bi];
            // Which points of a curved shape are candidates depends on the
            // ground normal, so ask the terrain first. Under the body's centre
            // is close enough: a shape is small relative to any terrain feature
            // we intend to support, and the per-point height below is still
            // sampled exactly.
            let under = terrain.normal_at(body.pos.x, body.pos.z);
            let (points, count) = body.ground_points(under);
            for &corner in &points[..count] {
                // One sample for both: height and normal share nearly all of
                // their arithmetic, and this is the innermost loop in the
                // simulator — up to eight points per body per step.
                let (ground, normal) = terrain.sample(corner.x, corner.z);
                let drop = ground - corner.y;
                if drop <= 0.0 {
                    continue;
                }
                // `drop` is how far the point is below the surface *vertically*,
                // but the contact is resolved along the surface normal, and on a
                // slope those are not the same distance: the vertical measure
                // overstates the true perpendicular penetration by 1/cos(slope)
                // — 1.4x at 45 degrees, 7x at 82. Since the positional
                // correction is proportional to depth, leaving it uncorrected
                // makes every steep face push about seven times too hard, and
                // the steeper the ground the harder it shoves. `normal.y` is
                // exactly that cosine, and the conversion is exact for a
                // locally flat surface.
                let depth = drop * normal.y;
                let tangent1 = normal.any_perpendicular();
                let tangent2 = normal.cross(tangent1);
                let r = corner - body.pos;

                let vn = body.point_velocity(r).dot(normal);
                // Only meaningful impacts bounce; otherwise resting contacts
                // would jitter forever.
                let bounce = if vn < -1.0 { -restitution * vn } else { 0.0 };

                self.contacts.push(Contact {
                    body: bi as u16,
                    r,
                    normal,
                    tangent1,
                    tangent2,
                    depth,
                    k_n: effective_mass(body.inv_mass, inv_i, r, normal),
                    k_t1: effective_mass(body.inv_mass, inv_i, r, tangent1),
                    k_t2: effective_mass(body.inv_mass, inv_i, r, tangent2),
                    pn: 0.0,
                    pt1: 0.0,
                    pt2: 0.0,
                    pn_bias: 0.0,
                    bounce,
                });
            }
        }
    }

    pub(super) fn solve_contacts(&mut self, dt: Real) {
        let inv_dt = 1.0 / dt;
        let beta = self.params.baumgarte;
        let slop = self.params.slop;
        let max_corr = self.params.max_correction_speed;
        let mu = self.params.friction;

        for ci in 0..self.contacts.len() {
            let c = self.contacts[ci];
            let bi = c.body as usize;
            let inv_i = self.inv_inertia[bi];

            // Normal, velocity half: stop the body moving into the ground, and
            // bounce it if the impact was hard enough. No positional term —
            // that is the job of the bias half below, and mixing the two is
            // what used to make the ground a motor.
            let vn = self.bodies[bi].point_velocity(c.r).dot(c.normal);
            let mut lambda = (c.bounce - vn) / c.k_n;
            let new_pn = (c.pn + lambda).max(0.0);
            lambda = new_pn - c.pn;
            self.contacts[ci].pn = new_pn;
            if lambda != 0.0 {
                let p = c.normal * lambda;
                self.bodies[bi].apply_impulse(c.r, p, &inv_i);
            }

            // Normal, position half: separate what is already overlapping,
            // into a velocity that only ever displaces.
            let correction = clamp(beta * (c.depth - slop).max(0.0) * inv_dt, 0.0, max_corr);
            if correction > 0.0 {
                let vb = self.bias_point_velocity(bi, c.r).dot(c.normal);
                let mut lb = (correction - vb) / c.k_n;
                let new_pb = (c.pn_bias + lb).max(0.0);
                lb = new_pb - c.pn_bias;
                self.contacts[ci].pn_bias = new_pb;
                if lb != 0.0 {
                    self.apply_bias_impulse(bi, c.r, c.normal * lb);
                }
            }

            // Friction, clamped to the Coulomb cone around the normal impulse
            // accumulated so far.
            let limit = mu * new_pn;
            for (tangent, k, stored) in [(c.tangent1, c.k_t1, 1usize), (c.tangent2, c.k_t2, 2usize)]
            {
                let old = if stored == 1 { self.contacts[ci].pt1 } else { self.contacts[ci].pt2 };
                let vt = self.bodies[bi].point_velocity(c.r).dot(tangent);
                let new = clamp(old - vt / k, -limit, limit);
                let delta = new - old;
                if stored == 1 {
                    self.contacts[ci].pt1 = new;
                } else {
                    self.contacts[ci].pt2 = new;
                }
                if delta != 0.0 {
                    self.bodies[bi].apply_impulse(c.r, tangent * delta, &inv_i);
                }
            }
        }
    }
}
