//! The ground: height fields, their gradients, and rays cast at them.
//!
//! Separate from [`super::world`] because none of it touches a rigid body. The
//! solver reaches terrain only through `height_at`, `sample`, `normal_at` and
//! `raycast`, so the two can be read, tested and replaced independently.

use crate::math::{dcos, dsin, dsincos, vec3, Real, Vec3, TAU};

use super::noise;

/// Ground model.
///
/// An enum rather than a trait object: dispatch is in the innermost loop, and a
/// second variant is cheaper than an abstraction.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerrainModel {
    Flat {
        height: Real,
    },
    /// Smooth rolling ground: two octaves of a separable sine field.
    ///
    /// Analytic rather than sampled, so the surface and its gradient are exact
    /// everywhere and there is no grid to store, interpolate or get wrong at the
    /// seams. Built from the deterministic [`dsincos`] rather than the standard
    /// library's, because the whole reproducibility argument rests on every
    /// transcendental in the pipeline being ours.
    ///
    /// Why it matters: on flat ground, rolling is optimal and legs are strictly
    /// worse, which is why evolution here keeps rediscovering the wheel. Broken
    /// ground is what makes legs the good answer, without a fitness function
    /// ever mentioning them.
    Rough {
        amplitude: Real,
        /// Distance between crests, metres.
        wavelength: Real,
    },
    /// Seeded fractal landscape: octaves of hashed gradient noise over a warped
    /// domain.
    ///
    /// What [`Rough`](TerrainModel::Rough) is not: it repeats every wavelength,
    /// it has no seed, and it has features at one scale only, so every organism
    /// in every trial meets the same memorisable ripple. This field is
    /// aperiodic, keyed on a seed, moved per trial, and built at four or more
    /// scales at once — which is what makes ground look and behave like ground.
    ///
    /// Still analytic, still exactly differentiable, and now with no
    /// transcendental in it at all: integer hashing and polynomial arithmetic
    /// only. See [`noise`](super::noise) for the construction and
    /// [`Self::sample`] for the chain rule through the warp.
    ///
    /// One honest limit, measured rather than assumed:
    ///
    /// * **It is still a height field**, so it is single-valued and smooth: no
    ///   overhangs, no vertical walls, no gaps. Smooth undulation is exactly
    ///   what a wheel is good at, and raising `amplitude` makes the ground
    ///   *steeper* rather than a different kind of problem. What defeats a
    ///   wheel is a discontinuity at or above its own radius, and that needs
    ///   discrete obstacles, not a better height field.
    Fractal(FractalField),
}

/// The parameters of a fractal landscape.
///
/// A struct rather than a pile of enum fields because there are now four bands
/// of them, and because a struct can be built with `..Default::default()` —
/// which is what lets every new band default to *off* and keeps a config that
/// does not mention them meaning exactly what it meant before.
///
/// Serialised flat into the `fractal` variant, so a trace still reads
/// `{"kind": "fractal", "amplitude": ..., ...}`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FractalField {
    /// Field identity. Two seeds give unrelated landscapes.
    ///
    /// Written to JSON as a decimal *string*: JavaScript's only number is a
    /// double, so a bare `u64` above 2^53 comes back off by a few — and a seed
    /// off by a few is a different landscape entirely. The viewer has to
    /// reproduce this exactly.
    #[serde(with = "seed_as_string")]
    pub seed: u64,

    // --- Band 1: the landscape itself. -----------------------------------
    /// Scale of the largest band, metres. Relief runs to about 1.8x this.
    pub amplitude: Real,
    /// Size of the largest feature, metres.
    pub wavelength: Real,
    /// How many octaves are summed. Each one is `lacunarity` times finer and
    /// `gain` times shallower than the last.
    pub octaves: u32,
    pub lacunarity: Real,
    pub gain: Real,
    /// Domain warp strength, in units of `wavelength`.
    ///
    /// Bends the field into ridges and basins rather than blobs. It buys the
    /// tail of the slope distribution, and it roughly doubles how much the
    /// ground's character varies from place to place. What it cannot do is
    /// produce a cliff — that is `step` — or concentrate the difficulty, which
    /// is `terrace_mask`.
    pub warp: Real,

    // --- Band 2: detail at organism scale. --------------------------------
    /// Amplitude of a second, finer band laid over the landscape, metres.
    /// Zero leaves the field exactly as it was before this band existed.
    pub detail_amplitude: Real,
    pub detail_wavelength: Real,
    pub detail_octaves: u32,

    // --- Band 3: where the ground is calm and where it is savage. ---------
    /// How strongly a slow field modulates the detail band's amplitude, in
    /// `[0, 1]`. Zero is uniform detail everywhere.
    pub modulation: Real,
    /// Size of the calm and savage regions, metres.
    pub modulation_wavelength: Real,

    // --- Band 4: cliffs. ---------------------------------------------------
    /// Terrace height, metres. Zero is a smooth field.
    ///
    /// Quantising the height to steps is the only thing here that produces a
    /// genuinely sheer face: fractional Brownian motion has one steepness and
    /// applies it everywhere, so scaling it up gives uniformly steep ground
    /// rather than occasional cliffs. A terrace is flat for most of its span
    /// and climbs through the rest, which puts the difficulty in a small
    /// fraction of the area and leaves the rest crossable.
    pub step: Real,
    /// Fraction of a terrace spent on the riser. The riser is steeper than the
    /// underlying slope by exactly `1 / riser`, so this is the cliff knob.
    ///
    /// It has a floor that is physics rather than taste: a wall must be several
    /// integration steps wide or a body crosses it in one and meets it as a
    /// single enormous penetration. `Config::validate` enforces that.
    pub riser: Real,
    /// Terrace only the savage regions, blending smoothly back into untouched
    /// hills elsewhere. Concentrates the cliffs instead of tiling the world
    /// with them.
    pub terrace_mask: bool,

    // --- Per-trial placement. ---------------------------------------------
    /// Rigid motion of the field under the world, so a trial can be run on a
    /// different piece of the same landscape. Translation is in units of
    /// `wavelength`; rotation is stored as its sine and cosine so that no
    /// trigonometry happens per sample. Identity is `(0, 0, 0, 1)`.
    pub offset_x: Real,
    pub offset_z: Real,
    pub rot_sin: Real,
    pub rot_cos: Real,
}

impl Default for FractalField {
    /// Every band but the first is off, so a `FractalField` built with
    /// `..Default::default()` is the field as it was before the bands existed.
    fn default() -> FractalField {
        FractalField {
            seed: 0,
            amplitude: 0.25,
            wavelength: 6.0,
            octaves: 4,
            lacunarity: 2.0,
            gain: 0.5,
            warp: 0.3,
            detail_amplitude: 0.0,
            detail_wavelength: 3.0,
            detail_octaves: 4,
            modulation: 0.0,
            modulation_wavelength: 35.0,
            step: 0.0,
            riser: 0.12,
            terrace_mask: false,
            offset_x: 0.0,
            offset_z: 0.0,
            rot_sin: 0.0,
            rot_cos: 1.0,
        }
    }
}

impl FractalField {
    /// Height and its exact gradient, `(h, dh/dx, dh/dz)`, in metres.
    ///
    /// Composed as bands rather than one noise call, because that is what makes
    /// each piece independently measurable — see `examples/terrain_probe.rs`.
    /// The gradient is carried through every band by the product and chain
    /// rules; the places it goes wrong are the warp Jacobian, the `dm` term
    /// where modulation multiplies the detail band, and the `dm * gap` term
    /// where the mask blends terraced ground into smooth. All three are checked
    /// against central differences in the tests.
    pub fn height_and_gradient(&self, x: Real, z: Real) -> (Real, Real, Real) {
        let wavelength = self.wavelength.max(1e-3);
        let inv_w = 1.0 / wavelength;

        // World space to field space, in *metres*: rotate about the origin,
        // then translate by the per-trial offset. Working in metres rather than
        // in units of the base wavelength is what lets the bands below each
        // divide by their own wavelength and still move together per trial.
        let mx = x * self.rot_cos - z * self.rot_sin + self.offset_x * wavelength;
        let mz = x * self.rot_sin + z * self.rot_cos + self.offset_z * wavelength;

        // Domain warp: displace the sample point by a coarse vector field
        // before evaluating anything. Applied in metres, so every band is
        // warped by the same displacement and they stay registered.
        let (gx, gz, jxx, jxz, jzx, jzz) = if self.warp != 0.0 {
            let (px, pz) = (mx * inv_w, mz * inv_w);
            let (wx, wxu, wxv) = noise::perlin_d(
                self.seed ^ WARP_SEED_X,
                px * WARP_FREQUENCY + WARP_OFFSET_X,
                pz * WARP_FREQUENCY + WARP_OFFSET_Z,
            );
            let (wz, wzu, wzv) = noise::perlin_d(
                self.seed ^ WARP_SEED_Z,
                px * WARP_FREQUENCY + WARP_OFFSET_Z,
                pz * WARP_FREQUENCY + WARP_OFFSET_X,
            );
            let g = self.warp * WARP_FREQUENCY;
            // Jacobian of the warped position with respect to the unwarped one.
            // The `wavelength` factors cancel: the displacement is
            // `warp * wavelength * w`, and `w`'s derivative carries `inv_w`.
            (
                mx + self.warp * wavelength * wx,
                mz + self.warp * wavelength * wz,
                1.0 + g * wxu,
                g * wxv,
                g * wzu,
                1.0 + g * wzv,
            )
        } else {
            (mx, mz, 1.0, 0.0, 0.0, 1.0)
        };

        // --- Band 1: the landscape. ---
        let (base, base_dx, base_dz) =
            fbm(self.seed, gx * inv_w, gz * inv_w, self.octaves, self.lacunarity, self.gain);
        let mut h = self.amplitude * base;
        let mut dx = self.amplitude * base_dx * inv_w;
        let mut dz = self.amplitude * base_dz * inv_w;

        // --- Band 3: how savage the ground is here. ---
        // `m` runs 0 (calm) to 1 (savage) and is slow. Computed before the
        // detail band because it scales it, and before the terracing because it
        // can also mask that.
        let (m, mdx, mdz) = if self.modulation > 0.0 {
            let mw = 1.0 / self.modulation_wavelength.max(1e-3);
            let (v, vdx, vdz) = noise::perlin_d(
                self.seed ^ MODULATION_SEED,
                gx * mw + MODULATION_OFFSET_X,
                gz * mw + MODULATION_OFFSET_Z,
            );
            // `smootherstep` of the noise, so the transition between calm and
            // savage is gradual and its derivative vanishes at both ends.
            let (s, ds) = smootherstep(0.5 + 0.5 * v);
            let k = self.modulation;
            (1.0 - k + k * s, k * ds * 0.5 * vdx * mw, k * ds * 0.5 * vdz * mw)
        } else {
            (1.0, 0.0, 0.0)
        };

        // --- Band 2: detail at organism scale, scaled by the modulation. ---
        if self.detail_amplitude > 0.0 {
            let dw = 1.0 / self.detail_wavelength.max(1e-3);
            let (det, det_dx, det_dz) = fbm(
                self.seed ^ DETAIL_SEED,
                gx * dw,
                gz * dw,
                self.detail_octaves,
                self.lacunarity,
                self.gain,
            );
            h += self.detail_amplitude * m * det;
            dx += self.detail_amplitude * (mdx * det + m * det_dx * dw);
            dz += self.detail_amplitude * (mdz * det + m * det_dz * dw);
        }

        // --- Band 4: cliffs. ---
        if self.step > 0.0 {
            let riser = self.riser.clamp(1e-3, 1.0);
            let t = h / self.step;
            let floor = t.floor();
            let (s, ds) = smootherstep((t - floor - 0.5) / riser + 0.5);
            let scale = ds / riser;
            let (terraced, tdx, tdz) = ((floor + s) * self.step, dx * scale, dz * scale);
            if self.terrace_mask {
                // Blend hills into badlands, weighted by the same slow field
                // that drives the detail: terraced where `m` is high, smooth
                // where it is low. `gap * dm` is the blend weight's own
                // contribution, and dropping it is the easiest way to get a
                // normal that does not match the surface.
                let gap = terraced - h;
                dx = dx + m * (tdx - dx) + mdx * gap;
                dz = dz + m * (tdz - dz) + mdz * gap;
                h += m * gap;
            } else {
                h = terraced;
                dx = tdx;
                dz = tdz;
            }
        }

        // Out through the warp and the rotation. `jxx..jzz` is transposed here
        // because a gradient is a covector.
        let wx = dx * jxx + dz * jzx;
        let wz = dx * jxz + dz * jzz;
        (h, wx * self.rot_cos + wz * self.rot_sin, wz * self.rot_cos - wx * self.rot_sin)
    }

    /// The largest height this field can produce, in metres.
    ///
    /// Exact rather than measured: each octave of gradient noise is bounded by
    /// one, so each band is bounded by the sum of its octave weights.
    /// Terracing cannot exceed it either — quantising a value moves it by less
    /// than one step, and the bound already covers that.
    pub fn height_bound(&self) -> Real {
        let band = |octaves: u32, gain: Real| {
            let mut total = 0.0;
            let mut weight = 1.0;
            for _ in 0..octaves.min(MAX_TERRAIN_OCTAVES) {
                total += weight;
                weight *= gain.abs();
            }
            total
        };
        let g = self.gain.abs();
        self.amplitude.abs() * band(self.octaves, g)
            + self.detail_amplitude.abs() * band(self.detail_octaves, g)
            + self.step.abs()
    }

    /// The gradient of this field with terracing switched off, at the 90th
    /// percentile of a sample of it.
    ///
    /// Ninetieth rather than median because that is where the thin walls are:
    /// a riser compresses a terrace's whole height change into `riser` of its
    /// span, so it is steepest, and therefore narrowest, exactly where the
    /// underlying ground was already steep. Measured rather than derived — an
    /// analytic estimate summing the octaves' contributions came out five times
    /// wrong against the field it was estimating, because the gradient of
    /// fractional Brownian motion is dominated by its finest octave in a way
    /// that is easy to get backwards.
    ///
    /// Sampled on a coarse, deliberately awkward grid. This runs once per config
    /// load, not per step.
    pub fn smooth_gradient(&self) -> Real {
        let smooth = FractalField { step: 0.0, ..*self };
        let mut g = Vec::with_capacity(21 * 21);
        for a in -10..11 {
            for b in -10..11 {
                let (_, dx, dz) =
                    smooth.height_and_gradient(a as Real * 3.7 + 0.31, b as Real * 4.3 - 0.17);
                g.push((dx * dx + dz * dz).sqrt());
            }
        }
        g.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        g[(g.len() * 9) / 10]
    }

    /// How wide this field's terrace risers are, in metres.
    ///
    /// This is the number that decides whether a cliff is physics or a bug. A
    /// body moving at `v` covers `v * dt` per step, and a wall it crosses in one
    /// step is a wall the solver meets as a single enormous penetration — or
    /// tunnels through entirely. `Config::validate` uses this.
    ///
    /// The riser occupies `step * riser` of height, so on ground of gradient `g`
    /// it occupies `step * riser / g` of horizontal distance. Checked against
    /// `examples/terrain_probe.rs`, which measures the walls the field actually
    /// contains: this predicts 150 mm for the shipped settings and the probe
    /// finds a median of 160.
    pub fn riser_width(&self) -> Real {
        if self.step <= 0.0 {
            return Real::INFINITY;
        }
        self.step * self.riser / self.smooth_gradient().max(1e-3)
    }
}

/// Fractional Brownian motion: octaves of gradient noise, each finer and
/// shallower than the last. Returns the value and its gradient in the
/// coordinates it was given.
#[inline]
fn fbm(
    seed: u64,
    x: Real,
    z: Real,
    octaves: u32,
    lacunarity: Real,
    gain: Real,
) -> (Real, Real, Real) {
    let mut sum = 0.0;
    let mut dx = 0.0;
    let mut dz = 0.0;
    let mut frequency = 1.0;
    let mut weight = 1.0;
    for o in 0..octaves.min(MAX_TERRAIN_OCTAVES) {
        let step = (o + 1) as Real;
        let (n, nu, nv) = noise::perlin_d(
            seed.wrapping_add((o as u64).wrapping_mul(OCTAVE_SEED_STRIDE)),
            x * frequency + step * OCTAVE_OFFSET_X,
            z * frequency + step * OCTAVE_OFFSET_Z,
        );
        sum += weight * n;
        dx += weight * frequency * nu;
        dz += weight * frequency * nv;
        frequency *= lacunarity;
        weight *= gain;
    }
    (sum, dx, dz)
}

/// `6t^5 - 15t^4 + 10t^3` clamped to `[0, 1]`, and its derivative.
///
/// Zero first derivative at both ends, so anything built by blending with it
/// stays continuously differentiable where the pieces meet — which is what
/// keeps the terrace risers and the badlands mask from putting creases in the
/// surface normal.
#[inline]
fn smootherstep(t: Real) -> (Real, Real) {
    if t <= 0.0 {
        return (0.0, 0.0);
    }
    if t >= 1.0 {
        return (1.0, 0.0);
    }
    let t2 = t * t;
    (t2 * t * (t * (t * 6.0 - 15.0) + 10.0), 30.0 * t2 * (t - 1.0) * (t - 1.0))
}

/// A `u64` that survives a round trip through JavaScript. See
/// [`FractalField::seed`].
mod seed_as_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(v)
    }

    /// Accepts a number as well as a string, so that a trace hand-edited into
    /// the obvious shape still loads.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Text(String),
            Number(u64),
        }
        match Either::deserialize(d)? {
            Either::Number(n) => Ok(n),
            Either::Text(s) => s.parse().map_err(serde::de::Error::custom),
        }
    }
}

/// Hardest limit on the octave loop, and therefore on the cost of one sample.
/// Beyond about six octaves the finest is far below the size of any body part
/// and only costs time. `Config::validate` enforces the same bound.
pub const MAX_TERRAIN_OCTAVES: u32 = 8;

/// Per-octave displacement in field space.
///
/// Perlin noise is exactly zero at every lattice point, and with an integer
/// `lacunarity` every octave shares a lattice point at the origin — which is
/// where every organism spawns. Without these offsets the spawn point would sit
/// in a dead flat dimple whatever the seed, and the flatness would not show up
/// in any aggregate statistic.
const OCTAVE_OFFSET_X: Real = 0.513_7;
const OCTAVE_OFFSET_Z: Real = 0.942_1;

/// The warp field is sampled at half the base frequency: it has to be coarser
/// than what it is warping, or it merely adds noise instead of shaping it.
const WARP_FREQUENCY: Real = 0.5;
const WARP_OFFSET_X: Real = 3.311;
const WARP_OFFSET_Z: Real = -1.749;

/// Sub-seeds for the two warp components, mixed with the field seed so that
/// changing the seed moves the warp too.
const WARP_SEED_X: u64 = 0x5741_5250_5f58_0001;
const WARP_SEED_Z: u64 = 0x5741_5250_5f5a_0001;
/// Sub-seeds for the bands that were added after the first. Distinct from the
/// base seed so that a band is not a rescaled copy of the landscape under it.
const DETAIL_SEED: u64 = 0x4445_5441_494c_0001;
const MODULATION_SEED: u64 = 0x4d4f_4455_4c41_5445;
/// Offsets keeping the modulation field off the lattice at the origin, for the
/// same reason as [`OCTAVE_OFFSET_X`].
const MODULATION_OFFSET_X: Real = 7.13;
const MODULATION_OFFSET_Z: Real = -2.71;
/// Separation between octaves, so no two octaves are the same field rescaled.
const OCTAVE_SEED_STRIDE: u64 = 0x4f43_5441_5645_0001;

/// Spacing between samples along a sensor ray, metres.
///
/// The resolution of every range sensor in the simulator, and the width of the
/// narrowest feature one can see: terrain that rises above the ray and drops
/// back within a stride is stepped over. 100 mm is chosen against the terrace
/// risers the fractal field produces, which measure about 152 mm at the shipped
/// settings and are the single most important thing for an organism to notice.
const MARCH_STRIDE: Real = 0.1;
/// Ceiling on samples per ray, so a large `range` cannot make evaluation
/// arbitrarily expensive.
const MAX_MARCH_STEPS: u32 = 128;
/// Halvings used to refine the crossing once a stride containing it is found.
const BISECTIONS: u32 = 4;

impl TerrainModel {
    #[inline]
    pub fn height_at(&self, x: Real, z: Real) -> Real {
        match *self {
            TerrainModel::Flat { height } => height,
            TerrainModel::Rough { amplitude, wavelength } => {
                let k = TAU / wavelength.max(1e-3);
                amplitude * (dsin(k * x) * dcos(k * z))
                    + 0.5 * amplitude * (dsin(2.0 * k * x + 1.7) * dcos(2.0 * k * z + 0.9))
            }
            TerrainModel::Fractal { .. } => self.sample(x, z).0,
        }
    }

    /// Distance from `origin` along `dir` to the first point where the ray meets
    /// the ground, or `None` if it does not within `range`.
    ///
    /// `dir` must be normalised. An origin already below the surface returns
    /// `Some(0.0)`: a sensor buried in a hillside sees the hillside, which is
    /// both true and the reading that keeps a controller's input bounded.
    ///
    /// # Why a fixed march rather than a root find
    ///
    /// The height field is cheap and everywhere-defined but not Lipschitz-bounded
    /// in any form the solver knows, so sphere tracing has nothing to step by.
    /// Marching at a fixed stride until `ray.y - h(ray.x, ray.z)` changes sign and
    /// then bisecting is simple, has no failure mode worse than missing a feature
    /// narrower than the stride, and — the part that matters here — costs the
    /// *same* number of samples for every ray.
    ///
    /// That last property is not an optimisation. A loop that stopped early would
    /// make evaluation cost a function of what evolved and of where an organism
    /// happened to be standing, which turns a benchmark into a measurement of the
    /// population. It also keeps the work identical on every thread, which is
    /// what the determinism contract needs.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, range: Real) -> Option<Real> {
        let above = |p: Vec3| p.y - self.height_at(p.x, p.z);
        if above(origin) <= 0.0 {
            return Some(0.0);
        }

        // Steps follow from the stride, not the other way round. A fixed *count*
        // would make the sensor's resolution depend on its range, so a
        // longer-sighted experiment would quietly become blind to terrace
        // risers — which at the shipped settings are 152 mm wide and are
        // precisely the feature worth seeing. Fixing the stride instead makes
        // resolution a stated physical property, and keeps the sample count
        // identical for every ray in an experiment.
        let steps = ((range / MARCH_STRIDE).ceil() as u32).clamp(1, MAX_MARCH_STEPS);
        let stride = range / steps as Real;
        let mut near = 0.0;
        let mut hit = false;
        for i in 1..=steps {
            let far = i as Real * stride;
            if above(origin + dir * far) <= 0.0 {
                hit = true;
                break;
            }
            near = far;
        }
        if !hit {
            return None;
        }

        // The crossing is somewhere in `(near, near + stride]`. Bisection rather
        // than a secant step because it cannot diverge on a discontinuous-looking
        // terrace riser, and four halvings of a stride this size are already
        // finer than the contact solver resolves.
        let mut lo = near;
        let mut hi = near + stride;
        for _ in 0..BISECTIONS {
            let mid = 0.5 * (lo + hi);
            if above(origin + dir * mid) <= 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        Some(0.5 * (lo + hi))
    }

    #[inline]
    pub fn normal_at(&self, x: Real, z: Real) -> Vec3 {
        match *self {
            TerrainModel::Flat { .. } => Vec3::Y,
            TerrainModel::Rough { amplitude, wavelength } => {
                // Exact gradient of `height_at`, so contacts on a slope get the
                // slope's own normal rather than a finite-difference guess.
                let k = TAU / wavelength.max(1e-3);
                let dhdx = amplitude * k * dcos(k * x) * dcos(k * z)
                    + amplitude * k * dcos(2.0 * k * x + 1.7) * dcos(2.0 * k * z + 0.9);
                let dhdz = -amplitude * k * dsin(k * x) * dsin(k * z)
                    - amplitude * k * dsin(2.0 * k * x + 1.7) * dsin(2.0 * k * z + 0.9);
                vec3(-dhdx, 1.0, -dhdz).normalize_or(Vec3::Y)
            }
            TerrainModel::Fractal { .. } => self.sample(x, z).1,
        }
    }

    /// Height and surface normal at one point, together.
    ///
    /// Every contact needs both, and for all three models they share nearly all
    /// of their arithmetic — so asking for them separately does the work twice
    /// on the hottest path in the simulator. Callers with both in hand should
    /// prefer this to a `height_at` followed by a `normal_at`.
    ///
    /// Bit-identical to calling the two separately, for every model. That is
    /// what keeps the goldens where they are.
    #[inline]
    pub fn sample(&self, x: Real, z: Real) -> (Real, Vec3) {
        match *self {
            TerrainModel::Flat { height } => (height, Vec3::Y),
            TerrainModel::Rough { amplitude, wavelength } => {
                // `dsin` and `dcos` are both projections of `dsincos`, so taking
                // the pairs once is the same arithmetic in the same order — and
                // half the trig.
                let k = TAU / wavelength.max(1e-3);
                let (sx, cx) = dsincos(k * x);
                let (sz, cz) = dsincos(k * z);
                let (sx2, cx2) = dsincos(2.0 * k * x + 1.7);
                let (sz2, cz2) = dsincos(2.0 * k * z + 0.9);
                let h = amplitude * (sx * cz) + 0.5 * amplitude * (sx2 * cz2);
                let dhdx = amplitude * k * cx * cz + amplitude * k * cx2 * cz2;
                let dhdz = -amplitude * k * sx * sz - amplitude * k * sx2 * sz2;
                (h, vec3(-dhdx, 1.0, -dhdz).normalize_or(Vec3::Y))
            }
            TerrainModel::Fractal(f) => {
                let (h, dhdx, dhdz) = f.height_and_gradient(x, z);
                (h, vec3(-dhdx, 1.0, -dhdz).normalize_or(Vec3::Y))
            }
        }
    }

    /// How level the ground is within `radius` of `(x, z)`, as the smallest `y`
    /// component of the surface normal found there: 1 is a flat plateau, 0 is a
    /// vertical face.
    ///
    /// Used to choose somewhere fair to set an organism down. On smooth ground
    /// this is a formality; on terraced ground it is not, because an organism
    /// spawned straddling a riser starts half inside a wall, and what the solver
    /// does about that is not a fair test of a gait.
    ///
    /// Nine samples — the centre and a ring of eight — which is enough to catch
    /// a riser crossing the footprint and cheap enough to run a dozen times per
    /// trial.
    pub fn levelness_near(&self, x: Real, z: Real, radius: Real) -> Real {
        const RING: [(Real, Real); 8] = [
            (1.0, 0.0),
            (0.707_106_77, 0.707_106_77),
            (0.0, 1.0),
            (-0.707_106_77, 0.707_106_77),
            (-1.0, 0.0),
            (-0.707_106_77, -0.707_106_77),
            (0.0, -1.0),
            (0.707_106_77, -0.707_106_77),
        ];
        let mut worst = self.normal_at(x, z).y;
        for (dx, dz) in RING {
            worst = worst.min(self.normal_at(x + dx * radius, z + dz * radius).y);
        }
        worst
    }

    /// The largest height this model can produce, in metres.
    ///
    /// Exact rather than measured: each octave of gradient noise is bounded by
    /// one, so the sum is bounded by the sum of the octave weights. Used to
    /// document what an `amplitude` setting actually buys, and asserted against
    /// in the tests.
    pub fn height_bound(&self) -> Real {
        match *self {
            TerrainModel::Flat { height } => height.abs(),
            TerrainModel::Rough { amplitude, .. } => 1.5 * amplitude.abs(),
            TerrainModel::Fractal(f) => f.height_bound(),
        }
    }
}

#[cfg(test)]
mod tests;
