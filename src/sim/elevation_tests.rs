use super::ElevationTracker;

/// The gate the deadband exists for.
///
/// An organism bouncing on the spot travels nowhere, and must earn nothing
/// for it. Without a band this is a perpetual-motion fitness source: every
/// upward wobble is ascent, and a gait supplies thousands of them.
#[test]
fn bobbing_on_the_spot_earns_no_climb() {
    let mut t = ElevationTracker::new(0.0, 0.05);
    // 4 cm amplitude at 120 Hz for eight seconds: 960 samples, well inside
    // the band and far more vertical movement than a real gait produces.
    for i in 0..960 {
        let phase = (i % 8) as f32 / 8.0;
        t.observe(0.04 * if phase < 0.5 { 1.0 } else { -1.0 });
    }
    assert_eq!(t.climb, 0.0, "bobbing inside the band accumulated ascent");
    assert_eq!(t.descent, 0.0, "bobbing inside the band accumulated descent");
}

/// A slow drift must still be recorded, or the band would hide real
/// climbing taken in steps smaller than itself.
#[test]
fn a_slow_climb_is_recorded_despite_the_band() {
    let mut t = ElevationTracker::new(0.0, 0.05);
    // One metre, in 1 cm increments — every one of them inside the band.
    for i in 1..=100 {
        t.observe(i as f32 * 0.01);
    }
    assert!(
        (t.climb - 1.0).abs() <= 0.05,
        "a 1 m climb in 1 cm steps registered {} m; the band must not eat it",
        t.climb
    );
    assert_eq!(t.descent, 0.0);
}

/// On a monotone climb, cumulative and net must agree to within one band.
/// If they diverge, the accumulation has a sign or reference bug.
#[test]
fn cumulative_and_net_agree_on_a_monotone_climb() {
    let mut t = ElevationTracker::new(0.0, 0.05);
    for i in 1..=200 {
        t.observe(i as f32 * 0.02);
    }
    let net = 200.0 * 0.02;
    assert!((t.climb - net).abs() <= 0.05, "climb {} against net {net}", t.climb);
}

/// Widening the band can only remove movement, never add it. Catches
/// reference-tracking errors that a single-band test would pass.
#[test]
fn a_wider_band_never_registers_more() {
    let signal: Vec<f32> = (0..600)
        .map(|i| {
            let t = i as f32 * 0.05;
            0.3 * t.sin() + 0.04 * (t * 11.0).sin() + t * 0.002
        })
        .collect();
    let mut previous = f32::INFINITY;
    for band in [0.0, 0.01, 0.02, 0.05, 0.1, 0.25, 0.5] {
        let mut t = ElevationTracker::new(signal[0], band);
        for &y in &signal {
            t.observe(y);
        }
        assert!(
            t.climb <= previous + 1e-4,
            "band {band} registered {} m, more than the narrower band's {previous} m",
            t.climb
        );
        previous = t.climb;
    }
}

/// Descent is the mirror of climb, not a separate rule.
#[test]
fn descent_mirrors_climb() {
    let mut up = ElevationTracker::new(0.0, 0.05);
    let mut down = ElevationTracker::new(0.0, 0.05);
    for i in 1..=100 {
        up.observe(i as f32 * 0.02);
        down.observe(i as f32 * -0.02);
    }
    assert!((up.climb - down.descent).abs() < 1e-5);
    assert_eq!(up.descent, 0.0);
    assert_eq!(down.climb, 0.0);
}
