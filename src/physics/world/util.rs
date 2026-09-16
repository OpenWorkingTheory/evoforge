//! Small geometric and mass-matrix helpers shared by the solver stages.

use super::*;

#[inline]
pub(super) fn project_out(v: Vec3, axis: Vec3) -> Vec3 {
    v - axis * v.dot(axis)
}

#[inline]
pub(super) fn clamp_speed(v: &mut Vec3, max: Real) {
    let len_sq = v.length_sq();
    if len_sq > max * max {
        *v = *v * (max / len_sq.sqrt());
    }
}

/// Scalar effective mass for a single dynamic body constrained along `dir` at
/// world offset `r`: `1 / (m^-1 + dir . ((I^-1 (r x dir)) x r))`.
#[inline]
/// Effective mass of a contact between two moving bodies along `dir`.
pub(super) fn pair_effective_mass(
    a: &RigidBody,
    b: &RigidBody,
    inv_ia: &Mat3,
    inv_ib: &Mat3,
    ra: Vec3,
    rb: Vec3,
    dir: Vec3,
) -> Real {
    let ta = ra.cross(dir);
    let tb = rb.cross(dir);
    a.inv_mass + b.inv_mass + ta.dot(inv_ia.mul_vec(ta)) + tb.dot(inv_ib.mul_vec(tb))
}

/// Closest points on two segments, one on each.
///
/// The standard clamped-parameter solution: solve the unconstrained least
/// squares for the two line parameters, then clamp each to its segment and
/// re-solve the other against the clamped value. Degenerate segments — a sphere
/// stands in as a zero-length one — fall out of the same arithmetic.
pub(super) fn closest_points_on_segments(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> (Vec3, Vec3) {
    let da = a1 - a0;
    let db = b1 - b0;
    let r = a0 - b0;
    let aa = da.dot(da);
    let bb = db.dot(db);
    let f = db.dot(r);

    const EPS: Real = 1e-12;
    let (mut s, mut t);
    if aa <= EPS && bb <= EPS {
        return (a0, b0);
    }
    if aa <= EPS {
        s = 0.0;
        t = clamp(f / bb, 0.0, 1.0);
    } else {
        let c = da.dot(r);
        if bb <= EPS {
            t = 0.0;
            s = clamp(-c / aa, 0.0, 1.0);
        } else {
            let d = da.dot(db);
            let denom = aa * bb - d * d;
            s = if denom > EPS { clamp((d * f - c * bb) / denom, 0.0, 1.0) } else { 0.0 };
            t = (d * s + f) / bb;
            if t < 0.0 {
                t = 0.0;
                s = clamp(-c / aa, 0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = clamp((d - c) / aa, 0.0, 1.0);
            }
        }
    }
    (a0 + da * s, b0 + db * t)
}

pub(super) fn effective_mass(inv_mass: Real, inv_inertia: &Mat3, r: Vec3, dir: Vec3) -> Real {
    let rn = r.cross(dir);
    let term = inv_inertia.mul_vec(rn).cross(r).dot(dir);
    let k = inv_mass + term;
    if k > 1e-12 {
        k
    } else {
        1e-12
    }
}
