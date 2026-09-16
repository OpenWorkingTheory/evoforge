//! Read-only questions asked of a world: attachment, centre of mass, clearance.

use super::util::*;
use super::*;

impl World {
    /// Whether `body` is still part of the organism rather than debris.
    #[inline]
    pub fn is_attached(&self, body: usize) -> bool {
        !self.detached[body]
    }

    /// Centre of mass of the whole organism.
    /// Centre of mass of the organism, counting only what is still attached.
    ///
    /// A shed limb keeps falling and tumbling in the world, but it stops being
    /// part of *you*: fitness should charge an organism for losing a limb's
    /// usefulness, not for where the wreckage happens to land.
    ///
    /// # Why the correction
    ///
    /// Dropping a body out of an average moves that average, instantly and for
    /// free. Shed a limb that trails behind you and the mean of what is left
    /// lurches forward — displacement the organism never travelled. Measured on
    /// a real run, most organisms lost a little distance this way and a few
    /// gained a great deal: one collected 3.34 m, a third of its recorded
    /// distance, by discarding a part at the right moment.
    ///
    /// So the discontinuity is cancelled. At the instant a joint fails the shift
    /// is measured and folded into `com_correction`, which every later reading
    /// subtracts. The reported centre of mass is therefore continuous across a
    /// breakage while still tracking only the attached parts afterwards — the
    /// wreckage stops counting, but detaching it is worth exactly zero metres.
    pub fn centre_of_mass(&self) -> Vec3 {
        self.attached_centre_of_mass() - self.com_correction
    }

    /// The raw mean position of everything still attached, before the
    /// continuity correction. This is the quantity that jumps.
    pub(super) fn attached_centre_of_mass(&self) -> Vec3 {
        let mut total = 0.0;
        let mut acc = Vec3::ZERO;
        for (i, b) in self.bodies.iter().enumerate() {
            if self.detached[i] {
                continue;
            }
            let m = b.mass();
            total += m;
            acc += b.pos * m;
        }
        if total > 0.0 {
            acc * (1.0 / total)
        } else {
            Vec3::ZERO
        }
    }

    /// `(cos, sin)` of the hinge angle, measured from the reference vectors.
    ///
    /// Returned as a pair rather than an angle: it costs no transcendentals, and
    /// it is a better controller input because it has no discontinuity at the
    /// wrap-around.
    pub fn hinge_angle_cos_sin(&self, joint_index: usize) -> (Real, Real) {
        let j = &self.joints[joint_index];
        let a = &self.bodies[j.body_a as usize];
        let b = &self.bodies[j.body_b as usize];
        let axis = a.orient.rotate(j.axis_a).normalize_or(Vec3::X);
        let ra = project_out(a.orient.rotate(j.ref_a), axis).normalize_or(axis.any_perpendicular());
        let rb = project_out(b.orient.rotate(j.ref_b), axis).normalize_or(ra);
        (clamp(ra.dot(rb), -1.0, 1.0), ra.cross(rb).dot(axis))
    }

    /// Smallest gap between any still-attached part and the terrain below it.
    ///
    /// Negative while something is penetrating, zero while resting, positive
    /// only when the whole organism is genuinely off the ground.
    ///
    /// This exists because "is anything in contact?" is not the same question.
    /// A contact is only generated once a point is *below* the terrain, so a
    /// body skimming a millimetre above it registers no contact at all. Asked
    /// for hang time on that basis, evolution promptly produced organisms that
    /// spent half the trial "airborne" while never rising above the grass.
    pub fn ground_clearance(&self) -> Real {
        let mut gap = Real::INFINITY;
        for (i, body) in self.bodies.iter().enumerate() {
            if self.detached[i] {
                continue;
            }
            let (points, count) = body.ground_points(Vec3::Y);
            for p in &points[..count] {
                gap = gap.min(p.y - self.params.terrain.height_at(p.x, p.z));
            }
        }
        if gap.is_finite() {
            gap
        } else {
            0.0
        }
    }

    /// Whether any corner of `body` is touching the terrain.
    pub fn body_in_contact(&self, body: usize) -> bool {
        self.contacts.iter().any(|c| c.body as usize == body)
    }
}
