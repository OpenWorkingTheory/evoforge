use super::*;

/// The ray against an independent brute-force march, which is how the
/// terrain gradient was validated and for the same reason: a root find that
/// agrees with itself proves nothing.
#[test]
fn a_ray_finds_the_same_ground_a_dense_march_does() {
    let fields = [
        TerrainModel::Flat { height: 0.0 },
        TerrainModel::Rough { amplitude: 0.25, wavelength: 2.0 },
        TerrainModel::Fractal(FractalField {
            seed: 7,
            amplitude: 3.0,
            wavelength: 25.0,
            octaves: 5,
            lacunarity: 2.0,
            gain: 0.5,
            warp: 0.6,
            detail_amplitude: 0.35,
            detail_wavelength: 3.0,
            detail_octaves: 4,
            modulation: 0.9,
            modulation_wavelength: 35.0,
            step: 0.8,
            riser: 0.12,
            terrace_mask: true,
            ..Default::default()
        }),
    ];
    let range = 6.0;
    // A stride, refined by four bisections, is all the march can resolve.
    let tol = 0.1 / 16.0 + 1e-3;
    let mut checked = 0;
    let mut missed = 0;
    for terrain in fields {
        for i in 0..400 {
            // Deliberately awkward origins and bearings: on a lattice point
            // every octave of gradient noise is exactly zero, so round
            // numbers would agree between two different implementations.
            let a = i as Real * 0.37;
            let x = (a * 1.7) % 23.0 - 11.5;
            let z = (a * 2.9) % 19.0 - 9.5;
            let origin = vec3(x, terrain.height_at(x, z) + 0.6, z);
            let dir =
                vec3(((a * 0.61) % 2.0) - 1.0, ((a * 0.29) % 1.4) - 1.2, ((a * 0.83) % 2.0) - 1.0)
                    .normalize_or(vec3(0.0, -1.0, 0.0));

            let got = terrain.raycast(origin, dir, range);

            // Brute force: step finely and take the first crossing.
            const FINE: u32 = 4000;
            let mut expected = None;
            for k in 1..=FINE {
                let t = k as Real * (range / FINE as Real);
                let p = origin + dir * t;
                if p.y - terrain.height_at(p.x, p.z) <= 0.0 {
                    expected = Some(t);
                    break;
                }
            }

            match (got, expected) {
                (Some(g), Some(e)) if (g - e).abs() < tol => checked += 1,
                (None, None) => checked += 1,
                // Terrain that rises above the ray and drops back within one
                // stride is invisible by construction, and the march then
                // either finds a later crossing or none. That is the stated
                // resolution limit rather than a bug, so it is counted and
                // bounded rather than forgiven case by case.
                (Some(_), Some(_)) | (None, Some(_)) => missed += 1,
                (Some(g), None) => panic!("ray {i}: reported a hit at {g} that is not there"),
            }
        }
    }
    // Sub-stride features are missed by construction; the contract is that
    // they are rare. A regression that broke the march outright, or coarsened
    // the stride, shows up here immediately.
    assert!(
        missed * 50 < checked,
        "{missed} of {} rays disagreed with a dense march; the resolution limit              should account for far fewer than 2%",
        checked + missed
    );
    assert!(checked > 1000, "only {checked} rays actually agreed");
}

/// A sensor already under the surface sees the surface, rather than
/// reporting nothing and letting a buried organism believe it is in clear
/// air.
#[test]
fn a_ray_from_below_the_ground_hits_immediately() {
    let t = TerrainModel::Rough { amplitude: 0.3, wavelength: 2.0 };
    let below = vec3(0.4, t.height_at(0.4, 0.2) - 0.1, 0.2);
    assert_eq!(t.raycast(below, vec3(1.0, 0.0, 0.0), 4.0), Some(0.0));
}

/// A ray pointing away from the ground finds nothing, and says so.
#[test]
fn a_ray_into_the_sky_finds_nothing() {
    let t = TerrainModel::Flat { height: 0.0 };
    assert_eq!(t.raycast(vec3(0.0, 0.5, 0.0), Vec3::Y, 10.0), None);
}

/// Straight down onto flat ground is the one case with an exact answer.
#[test]
fn a_vertical_ray_measures_its_own_height() {
    let t = TerrainModel::Flat { height: 0.0 };
    let d = t.raycast(vec3(1.0, 2.0, -3.0), vec3(0.0, -1.0, 0.0), 8.0).expect("hit");
    // Resolved to a stride refined by `BISECTIONS` halvings, and no finer;
    // the march does not claim more precision than that.
    let tol = MARCH_STRIDE / (1 << BISECTIONS) as Real;
    assert!((d - 2.0).abs() <= tol, "expected 2 m within {tol}, got {d}");
}

/// The shipped landscape band with one seed changed. Everything after band
/// one is off in `FractalField::default()`, so a case that varies one field
/// with `..` varies exactly that.
fn frac(seed: u64) -> FractalField {
    FractalField { seed, wavelength: 6.0, ..FractalField::default() }
}

fn fractal(seed: u64) -> TerrainModel {
    TerrainModel::Fractal(frac(seed))
}

/// `sample` exists to halve the terrain work on the contact path. It is only
/// allowed to do that if it is the *same* arithmetic, bit for bit — the
/// goldens depend on that for `Rough`, and the gradient test below depends
/// on it for `Fractal`.
#[test]
fn sample_agrees_bitwise_with_height_and_normal_taken_separately() {
    let models = [
        TerrainModel::Flat { height: 0.0 },
        TerrainModel::Flat { height: -0.7 },
        TerrainModel::Rough { amplitude: 0.05, wavelength: 1.6 },
        TerrainModel::Rough { amplitude: 0.25, wavelength: 6.0 },
        fractal(0),
        fractal(0xABCD_EF01),
        TerrainModel::Fractal(FractalField { warp: 0.0, ..frac(9) }),
        TerrainModel::Fractal(FractalField { octaves: 1, ..frac(9) }),
    ];
    for m in models {
        for a in -40..40 {
            for b in -40..40 {
                let (x, z) = (a as Real * 0.313, b as Real * 0.271);
                let (h, n) = m.sample(x, z);
                assert_eq!(h.to_bits(), m.height_at(x, z).to_bits(), "height at {x},{z}");
                let want = m.normal_at(x, z);
                assert_eq!(
                    (n.x.to_bits(), n.y.to_bits(), n.z.to_bits()),
                    (want.x.to_bits(), want.y.to_bits(), want.z.to_bits()),
                    "normal at {x},{z}"
                );
            }
        }
    }
}

/// The normal is the analytic gradient of the height, and the contact solver
/// trusts it completely. Central differences over the *assembled* field —
/// warp, octaves, rotation and all — are the independent check that the
/// chain rule was carried through correctly.
#[test]
fn the_fractal_normal_is_the_gradient_of_its_own_height() {
    let h = 5e-3;
    let variants = [
        fractal(0),
        fractal(1),
        fractal(0xDEAD_BEEF),
        TerrainModel::Fractal(FractalField { warp: 0.0, ..frac(2) }),
        TerrainModel::Fractal(FractalField { warp: 0.9, ..frac(3) }),
        TerrainModel::Fractal(FractalField { octaves: 1, ..frac(4) }),
        TerrainModel::Fractal(FractalField { octaves: 6, ..frac(5) }),
        TerrainModel::Fractal(FractalField { lacunarity: 2.7, gain: 0.65, ..frac(6) }),
        TerrainModel::Fractal(FractalField { wavelength: 1.5, amplitude: 0.05, ..frac(7) }),
        TerrainModel::Fractal(FractalField { rot_sin: 0.6, rot_cos: 0.8, ..frac(8) }),
        TerrainModel::Fractal(FractalField { offset_x: 12.5, offset_z: -7.25, ..frac(9) }),
    ];
    let mut worst = 0.0f64;
    for m in variants {
        for a in -25..25 {
            for b in -25..25 {
                let (x, z) = (a as Real * 0.731, b as Real * 0.917);
                let n = m.normal_at(x, z);
                // Recover the gradient the normal encodes.
                let (dhdx, dhdz) = (-n.x / n.y, -n.z / n.y);
                let fdx = (m.height_at(x + h, z) - m.height_at(x - h, z)) / (2.0 * h);
                let fdz = (m.height_at(x, z + h) - m.height_at(x, z - h)) / (2.0 * h);
                worst = worst.max((dhdx - fdx).abs() as f64);
                worst = worst.max((dhdz - fdz).abs() as f64);
            }
        }
    }
    // What is left is finite-difference truncation over a field whose finest
    // octave is a fraction of a metre, not a wrong derivative. A sign error
    // or a dropped warp term lands orders of magnitude above this.
    assert!(worst < 2e-2, "worst gradient error {worst}");
}

/// The bands that were added after the first, each varied on its own, and
/// then all at once. Terracing is the one that will break: it multiplies
/// the gradient by `1/riser`, so a factor dropped there is a normal that
/// disagrees with the surface by a factor of eight.
fn banded() -> FractalField {
    FractalField {
        seed: 0x5EED_0001,
        amplitude: 3.0,
        wavelength: 25.0,
        octaves: 5,
        detail_amplitude: 0.35,
        detail_wavelength: 3.0,
        modulation: 0.9,
        step: 0.8,
        riser: 0.12,
        terrace_mask: true,
        ..FractalField::default()
    }
}

#[test]
fn every_band_has_an_exact_gradient() {
    let h = 1e-3;
    let variants: [(&str, FractalField); 9] = [
        ("landscape only", FractalField { detail_amplitude: 0.0, ..banded() }),
        ("detail, unmodulated", FractalField { modulation: 0.0, step: 0.0, ..banded() }),
        ("detail, modulated", FractalField { step: 0.0, ..banded() }),
        ("terraced, unmasked", FractalField { terrace_mask: false, ..banded() }),
        ("terraced, masked", banded()),
        ("terraced, sheer", FractalField { riser: 0.05, ..banded() }),
        ("terraced, shallow", FractalField { riser: 0.9, ..banded() }),
        ("terraced, no warp", FractalField { warp: 0.0, ..banded() }),
        (
            "everything, moved",
            FractalField { offset_x: 3.5, offset_z: -7.25, rot_sin: 0.6, rot_cos: 0.8, ..banded() },
        ),
    ];
    for (label, f) in variants {
        let mut worst = 0.0f64;
        for a in -60..60 {
            for b in -60..60 {
                let (x, z) = (a as Real * 0.317, b as Real * 0.211);
                let (_, dx, dz) = f.height_and_gradient(x, z);
                let fdx = (f.height_and_gradient(x + h, z).0 - f.height_and_gradient(x - h, z).0)
                    / (2.0 * h);
                let fdz = (f.height_and_gradient(x, z + h).0 - f.height_and_gradient(x, z - h).0)
                    / (2.0 * h);
                // A riser is a genuinely steep, genuinely narrow feature, so
                // a central difference straddling one is measuring the
                // secant of a cliff rather than its tangent. Compare
                // relative to the local scale instead of absolutely.
                let scale = 1.0 + dx.abs().max(dz.abs()) as f64;
                worst = worst.max((dx - fdx).abs() as f64 / scale);
                worst = worst.max((dz - fdz).abs() as f64 / scale);
            }
        }
        assert!(worst < 0.05, "{label}: worst relative gradient error {worst}");
    }
}

/// Terracing exists to make cliffs, and the mask exists to put them
/// somewhere rather than everywhere. Both claims are measurable.
#[test]
fn terracing_makes_cliffs_and_leaves_the_ground_crossable() {
    let smooth = FractalField { step: 0.0, ..banded() };
    let terraced = FractalField { terrace_mask: false, ..banded() };

    let slopes = |f: &FractalField| {
        let mut v: Vec<Real> = Vec::with_capacity(240 * 240);
        for a in -120..120 {
            for b in -120..120 {
                let (_, dx, dz) = f.height_and_gradient(a as Real * 0.19, b as Real * 0.23);
                v.push((dx * dx + dz * dz).sqrt().atan().to_degrees());
            }
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    };

    let s = slopes(&smooth);
    let t = slopes(&terraced);
    let at = |v: &[Real], q: f64| v[((v.len() - 1) as f64 * q) as usize];

    // Smooth fractional Brownian motion has one steepness and applies it
    // everywhere: scaling it up cannot make a cliff.
    assert!(at(&s, 1.0) < 65.0, "smooth fBm reached {} degrees", at(&s, 1.0));
    // Terracing puts most of the plane flat and the difficulty in the rest.
    assert!(at(&t, 0.5) < 5.0, "terraced median slope {} is not a plateau", at(&t, 0.5));
    assert!(at(&t, 0.99) > 70.0, "terraced p99 slope {} is not a cliff", at(&t, 0.99));
    // And it stays crossable: the ground must not become a wall everywhere.
    let walkable = t.iter().filter(|v| **v < 40.0).count() as Real / t.len() as Real;
    assert!(walkable > 0.80, "only {:.0}% of terraced ground is walkable", walkable * 100.0);
}

/// How much the *character* of the ground varies from place to place.
///
/// Measured as the spread of mean slope across 12 m tiles, which is the
/// metric that matters: a tile on the flank of a big hill has enormous
/// relief and may still be billiard-smooth, so relief per tile answers a
/// different question and answers it misleadingly. Measuring relief instead
/// is what produced the claim — repeated in this file's history, the README
/// and both plan documents — that domain warping does nothing for
/// heterogeneity. It does; the metric could not see it.
///
/// Measured on this field, warp 0 to 1 takes the spread from 0.09 to 0.18,
/// and on the shipped landscape band from 0.04 to 0.11. The mask is still
/// the strongest single lever and they compose.
#[test]
fn the_bands_each_make_the_ground_more_heterogeneous() {
    let spread = |f: &FractalField| {
        let mut tiles = Vec::new();
        for tx in -5..5 {
            for tz in -5..5 {
                let mut sum = 0.0;
                for a in 0..24 {
                    for b in 0..24 {
                        let (_, dx, dz) = f.height_and_gradient(
                            tx as Real * 12.0 + a as Real * 0.5,
                            tz as Real * 12.0 + b as Real * 0.5,
                        );
                        sum += (dx * dx + dz * dz).sqrt().atan().to_degrees();
                    }
                }
                tiles.push(sum / 576.0);
            }
        }
        let mean = tiles.iter().sum::<Real>() / tiles.len() as Real;
        let var = tiles.iter().map(|t| (t - mean).powi(2)).sum::<Real>() / tiles.len() as Real;
        var.sqrt() / mean
    };

    let plain = spread(&FractalField { modulation: 0.0, step: 0.0, warp: 0.0, ..banded() });
    let warped = spread(&FractalField { modulation: 0.0, step: 0.0, warp: 1.0, ..banded() });
    let modulated = spread(&FractalField { step: 0.0, ..banded() });
    let masked = spread(&banded());

    assert!(warped > plain * 1.5, "warp: {plain} -> {warped}");
    assert!(modulated > plain * 1.2, "modulation: {plain} -> {modulated}");
    assert!(masked > plain * 2.0, "mask: {plain} -> {masked}");
    // The mask is the strongest single lever, which is why it is the one
    // the shipped experiment turns on.
    assert!(masked > warped, "the mask ({masked}) should beat warp ({warped}) alone");
}

#[test]
fn fractal_heights_respect_the_amplitude_bound() {
    for seed in [0u64, 3, 0x1234_5678_9ABC_DEF0] {
        let m = fractal(seed);
        let bound = m.height_bound();
        let mut peak = Real::NEG_INFINITY;
        let mut trough = Real::INFINITY;
        for a in -150..150 {
            for b in -150..150 {
                let y = m.height_at(a as Real * 0.41, b as Real * 0.37);
                assert!(y.is_finite(), "not finite");
                assert!(y.abs() <= bound, "{y} exceeds the bound {bound}");
                peak = peak.max(y);
                trough = trough.min(y);
            }
        }
        // Relief runs to roughly 2.5x amplitude in practice, well inside the
        // 3.75x the bound allows. If this ever fails low, the field has gone
        // flat and every organism is on a plain.
        let relief = peak - trough;
        assert!(relief > 0.5 * 0.25, "suspiciously flat: {relief}");
    }
}

/// Every octave of Perlin noise is exactly zero at every lattice point, and
/// with an integer lacunarity they all share one at the origin. Organisms
/// spawn at the origin, so a dead flat dimple there would be present in
/// every trial and invisible in every aggregate.
#[test]
fn the_origin_is_not_a_flat_spot() {
    for seed in [0u64, 1, 2, 3, 99] {
        let m = fractal(seed);
        let n = m.normal_at(0.0, 0.0);
        assert!(n.y < 0.9999, "origin is flat for seed {seed}: {n:?}");
    }
}

#[test]
fn a_fractal_field_is_deterministic_and_seed_dependent() {
    let a = fractal(11);
    let b = fractal(12);
    let mut differ = 0;
    for i in 0..500 {
        let (x, z) = (i as Real * 0.19, i as Real * -0.07);
        assert_eq!(a.height_at(x, z).to_bits(), a.height_at(x, z).to_bits());
        if a.height_at(x, z) != b.height_at(x, z) {
            differ += 1;
        }
    }
    assert!(differ > 490, "seeds barely differ: {differ}/500");
}

/// Layer 2: a per-trial rigid motion has to move the ground the organism
/// meets, or it is not closing the memorisation hole it exists to close.
#[test]
fn a_rigid_motion_moves_the_field() {
    let base = fractal(5);
    let shifted = TerrainModel::Fractal(FractalField { offset_x: 3.7, offset_z: -2.1, ..frac(5) });
    let turned = TerrainModel::Fractal(FractalField { rot_sin: 0.6, rot_cos: 0.8, ..frac(5) });
    let mut moved = 0;
    for i in 1..200 {
        let (x, z) = (i as Real * 0.23, i as Real * 0.17);
        if base.height_at(x, z) != shifted.height_at(x, z)
            && base.height_at(x, z) != turned.height_at(x, z)
        {
            moved += 1;
        }
    }
    assert!(moved > 190, "field did not move: {moved}/199");
    // A rotation about the origin leaves the origin where it was.
    assert_eq!(base.height_at(0.0, 0.0), turned.height_at(0.0, 0.0));
}

/// Aperiodicity is the whole point of replacing the sine field. Sampling a
/// full wavelength apart should find different ground.
#[test]
fn the_fractal_field_does_not_repeat() {
    let m = fractal(21);
    let mut same = 0;
    for i in 1..300 {
        let x = i as Real * 0.3;
        if (m.height_at(x, 0.0) - m.height_at(x + 6.0, 0.0)).abs() < 1e-4 {
            same += 1;
        }
    }
    assert!(same < 15, "field looks periodic: {same}/299 samples coincide");
}

#[test]
fn a_degenerate_fractal_field_stays_finite() {
    let m = TerrainModel::Fractal(FractalField {
        seed: 0,
        amplitude: 0.0,
        wavelength: 0.0,
        octaves: 0,
        lacunarity: 0.0,
        gain: 0.0,
        warp: 0.0,
        detail_amplitude: 0.0,
        detail_wavelength: 0.0,
        detail_octaves: 0,
        modulation: 0.0,
        modulation_wavelength: 0.0,
        step: 0.0,
        riser: 0.0,
        terrace_mask: true,
        offset_x: 0.0,
        offset_z: 0.0,
        rot_sin: 0.0,
        rot_cos: 0.0,
    });
    let (h, n) = m.sample(1.0, -1.0);
    assert_eq!(h, 0.0);
    assert_eq!(n, Vec3::Y);
    // Nor a long way from the origin, where the lattice index saturates.
    for x in [1e4, -1e4, 1e12, -1e12] {
        let (h, n) = fractal(4).sample(x, x);
        assert!(h.is_finite() && n.y.is_finite(), "blew up at {x}");
    }
}
