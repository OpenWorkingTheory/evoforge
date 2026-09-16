use super::*;
use crate::rng::Rng;

fn cfg() -> Config {
    Config::default()
}

/// A configuration with every shape available, so the tests below exercise
/// the carving rules rather than only the box path.
fn shaped_cfg() -> Config {
    let mut cfg = Config::default();
    cfg.body.shapes = vec![
        ShapeKind::Box,
        ShapeKind::Taper,
        ShapeKind::Sphere,
        ShapeKind::Capsule,
        ShapeKind::Cylinder,
    ];
    cfg
}

#[test]
fn faces_are_orthonormal() {
    for f in FACES.iter() {
        assert!((f.normal.length() - 1.0).abs() < 1e-6);
        for t in f.tangents {
            assert!((t.length() - 1.0).abs() < 1e-6);
            assert!(f.normal.dot(t).abs() < 1e-6);
        }
        assert!(f.tangents[0].dot(f.tangents[1]).abs() < 1e-6);
    }
}

/// Every carved shape has to fit inside the box its genome asked for, or the
/// attachment rules and the spawn drop are working from a bound that is not
/// a bound.
#[test]
fn every_shape_is_inscribed_in_its_box() {
    let cfg = shaped_cfg();
    let layout = cfg.brain_layout();
    for seed in 0..200 {
        let mut rng = Rng::new(seed);
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        for p in &g.parts {
            let shape = carve(p.shape, p.half_extents, cfg.body.taper_top_scale);
            let b = shape.bounds();
            assert!(
                b.x <= p.half_extents.x + 1e-6
                    && b.y <= p.half_extents.y + 1e-6
                    && b.z <= p.half_extents.z + 1e-6,
                "seed {seed}: {:?} bounds {b:?} escape {:?}",
                p.shape,
                p.half_extents
            );
            assert!(b.x > 0.0 && b.y > 0.0 && b.z > 0.0, "seed {seed}: degenerate {b:?}");
        }
    }
}

/// The same drop test as for boxes, but over organisms made of every shape.
/// A curved part reports a different lowest point than its bounding box
/// would, so this is what catches a shape whose ground points disagree with
/// its own bounds.
#[test]
fn a_shaped_organism_also_sits_on_the_ground() {
    let cfg = shaped_cfg();
    let layout = cfg.brain_layout();
    for seed in 0..100 {
        let mut rng = Rng::new(seed);
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        let p = build(&g, &cfg);
        let lowest = p.world.bodies.iter().fold(Real::INFINITY, |m, b| m.min(b.lowest_point_y()));
        assert!((lowest - SPAWN_CLEARANCE).abs() < 1e-4, "seed {seed}: lowest point at {lowest}");
    }
}

/// A taper is the only shape whose centre of mass is not its geometric
/// centre, and getting that offset backwards would put the body half a part
/// away from where the attachment rules placed it.
#[test]
fn a_tapered_part_is_placed_by_its_centre_of_mass() {
    let mut cfg = cfg();
    cfg.body.shapes = vec![ShapeKind::Taper];
    let layout = cfg.brain_layout();
    let mut rng = Rng::new(7);
    let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
    let p = build(&g, &cfg);
    for (body, gene) in p.world.bodies.iter().zip(&g.parts) {
        let shape = carve(gene.shape, gene.half_extents, cfg.body.taper_top_scale);
        assert!(shape.com_offset().length() > 0.0, "a taper should be off-centre");
        // The body sits at the centre of mass, so backing the offset out has
        // to land on the geometric centre the bounds are measured about.
        let centre = body.pos - shape.com_offset();
        assert!((body.lowest_point_y() - (centre.y - shape.bounds().y)).abs() < 1e-5);
    }
}

fn symmetric_cfg() -> Config {
    let mut cfg = shaped_cfg();
    cfg.body.pair_probability = 1.0; // every part a pair
    cfg
}

/// A body built from paired parts must be its own mirror image.
///
/// This is the whole claim of bilateral symmetry: for every body off the
/// midline there is a twin at the same place on the other side, of the same
/// shape and mass. If the mirroring is wrong anywhere — the anchor, the
/// child offset, the chain of a repeated segment — the two halves drift
/// apart and this fails.
#[test]
fn a_paired_body_is_its_own_mirror_image() {
    let cfg = symmetric_cfg();
    let layout = cfg.brain_layout();
    for seed in 0..60 {
        let mut rng = Rng::new(seed);
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        let p = build(&g, &cfg);

        for (i, body) in p.world.bodies.iter().enumerate() {
            if body.pos.z.abs() < 1e-6 {
                continue; // on the midline; it is its own reflection
            }
            let twin = p.world.bodies.iter().enumerate().find(|(k, other)| {
                *k != i
                    && (other.pos.x - body.pos.x).abs() < 1e-4
                    && (other.pos.y - body.pos.y).abs() < 1e-4
                    && (other.pos.z + body.pos.z).abs() < 1e-4
            });
            let (_, twin) = twin.unwrap_or_else(|| {
                panic!("seed {seed}: body {i} at {:?} has no mirror twin", body.pos)
            });
            // The twin's shape is the *reflection* of this one, which for
            // everything but a taper along Z is the same shape.
            assert_eq!(
                twin.shape,
                reflect_shape(body.shape),
                "seed {seed}: twins are not reflections"
            );
            assert!((twin.mass() - body.mass()).abs() < 1e-3);
        }
    }
}

/// Pairing must not put both halves in the same place.
#[test]
fn a_pair_is_actually_separated() {
    let cfg = symmetric_cfg();
    let layout = cfg.brain_layout();
    for seed in 0..60 {
        let mut rng = Rng::new(seed);
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        let p = build(&g, &cfg);
        for (i, a) in p.world.bodies.iter().enumerate() {
            for (k, b) in p.world.bodies.iter().enumerate().skip(i + 1) {
                assert!(
                    (a.pos - b.pos).length() > 1e-4,
                    "seed {seed}: bodies {i} and {k} occupy the same point"
                );
            }
        }
    }
}

/// Both halves of a pair answer to one controller slot, and a chain of
/// segments likewise. That sharing is what turns two limbs into a gait.
#[test]
fn mirrored_and_repeated_parts_share_a_controller_slot() {
    let mut cfg = symmetric_cfg();
    cfg.body.max_repeat = 3;
    let layout = cfg.brain_layout();
    let mut shared = 0;
    for seed in 0..40 {
        let mut rng = Rng::new(seed);
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        let p = build(&g, &cfg);
        assert_eq!(p.joint_drive.len(), p.world.joints.len());
        assert_eq!(p.body_slots.len(), p.world.bodies.len());
        for slot in 0..layout.max_slots {
            let n = p.body_slots.iter().filter(|&&s| s as usize == slot).count();
            assert_eq!(n, p.slot_bodies[slot] as usize, "slot {slot} miscounted");
            if n > 1 {
                shared += 1;
            }
        }
    }
    assert!(shared > 0, "no slot ever owned more than one body");
}

/// Segmentation extends a part into a chain, and children hang off its end
/// rather than sprouting from every segment.
#[test]
fn repetition_lengthens_the_body_without_branching() {
    let mut cfg = cfg();
    cfg.body.max_repeat = 4;
    let layout = cfg.brain_layout();
    let mut grew = false;
    for seed in 0..40 {
        let mut rng = Rng::new(seed);
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        let want: usize = g.parts.iter().map(|p| p.repeat.max(1) as usize).sum();
        let p = build(&g, &cfg);
        // The root is never repeated, so it contributes exactly one body.
        let expect = want - (g.parts[0].repeat.max(1) as usize) + 1;
        assert_eq!(p.world.bodies.len(), expect, "seed {seed}");
        grew |= p.world.bodies.len() > g.parts.len();
    }
    assert!(grew, "repetition never produced a longer body");
}

#[test]
fn built_organism_sits_on_the_ground() {
    let cfg = cfg();
    let layout = cfg.brain_layout();
    for seed in 0..100 {
        let mut rng = Rng::new(seed);
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        let p = build(&g, &cfg);
        let lowest = p.world.bodies.iter().fold(Real::INFINITY, |m, b| m.min(b.lowest_point_y()));
        assert!((lowest - SPAWN_CLEARANCE).abs() < 1e-4, "seed {seed}: lowest corner at {lowest}");
    }
}

/// The same drop test again, over ground that is neither flat nor level.
///
/// `build` samples the terrain under *every* corner rather than once under
/// the root, and on a fractal field that is the difference between resting
/// on the surface and being buried in the next hill. Sitting the wrong way
/// up would be free fitness or an instant faceplant, and neither is a fair
/// test of a gait.
#[test]
fn an_organism_sits_on_fractal_ground_too() {
    let mut cfg = shaped_cfg();
    cfg.environment.terrain = crate::config::Terrain::Fractal;
    cfg.environment.terrain_amplitude = 0.25;
    cfg.environment.terrain_wavelength = 6.0;
    let layout = cfg.brain_layout();
    for seed in 0..100 {
        let mut rng = Rng::new(seed);
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        // Include the perturbed path, which moves the organism *and* the
        // ground under it.
        let start = StartPerturbation {
            yaw: 0.3,
            tilt: -0.15,
            offset: vec3(0.4, 0.0, -0.3),
            terrain: TerrainShift { offset_x: 7.5, offset_z: -3.25, sin: 0.6, cos: 0.8 },
        };
        for p in [build(&g, &cfg), build_with_start(&g, &cfg, Some(start))] {
            let gap = p.world.ground_clearance();
            assert!(
                (gap - SPAWN_CLEARANCE).abs() < 1e-4,
                "seed {seed}: closest point {gap} above the ground"
            );
        }
    }
}

/// A per-trial shift has to reach the world the organism is actually built
/// into, not just the copy the spawn drop consulted.
#[test]
fn a_terrain_shift_reaches_the_built_world() {
    let mut cfg = cfg();
    cfg.environment.terrain = crate::config::Terrain::Fractal;
    let g = Genome::random(&mut Rng::new(4), &cfg.body, &cfg.brain, &cfg.brain_layout());
    let shift = TerrainShift { offset_x: 11.0, offset_z: -6.0, sin: 0.6, cos: 0.8 };
    let moved = build_with_start(
        &g,
        &cfg,
        Some(StartPerturbation { terrain: shift, ..Default::default() }),
    );
    assert_eq!(moved.world.params.terrain, cfg_terrain(&cfg, shift));
    assert_ne!(moved.world.params.terrain, cfg_terrain(&cfg, TerrainShift::NONE));
    // And the unperturbed build is still the unmoved field.
    assert_eq!(build(&g, &cfg).world.params.terrain, cfg_terrain(&cfg, TerrainShift::NONE));
}

/// `terrain_seed = 0` means "a landscape for this experiment", so two seeds
/// must not get the same one; any other value names a specific landscape,
/// which is what makes two experiments comparable on identical ground.
#[test]
fn a_zero_terrain_seed_derives_from_the_experiment_seed() {
    let mut a = cfg();
    a.environment.terrain = crate::config::Terrain::Fractal;
    a.experiment.seed = 1;
    let mut b = a.clone();
    b.experiment.seed = 2;
    assert_ne!(cfg_terrain(&a, TerrainShift::NONE), cfg_terrain(&b, TerrainShift::NONE));

    a.environment.terrain_seed = 77;
    b.environment.terrain_seed = 77;
    assert_eq!(cfg_terrain(&a, TerrainShift::NONE), cfg_terrain(&b, TerrainShift::NONE));
}

#[test]
fn body_and_joint_counts_match_the_genome() {
    let cfg = cfg();
    let layout = cfg.brain_layout();
    let mut rng = Rng::new(3);
    for _ in 0..50 {
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        let p = build(&g, &cfg);
        assert_eq!(p.world.bodies.len(), g.part_count());
        assert_eq!(p.world.joints.len(), g.joint_count());
        assert_eq!(p.body_slots.len(), g.part_count());
        assert_eq!(p.joint_slots.len(), g.joint_count());
    }
}

#[test]
fn joint_anchors_start_coincident() {
    let cfg = cfg();
    let layout = cfg.brain_layout();
    let mut rng = Rng::new(9);
    for _ in 0..100 {
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        let p = build(&g, &cfg);
        for j in &p.world.joints {
            let a = &p.world.bodies[j.body_a as usize];
            let b = &p.world.bodies[j.body_b as usize];
            let pa = a.pos + a.orient.rotate(j.anchor_a);
            let pb = b.pos + b.orient.rotate(j.anchor_b);
            assert!(
                (pa - pb).length() < 1e-4,
                "anchors {} apart at construction",
                (pa - pb).length()
            );
        }
    }
}

#[test]
fn hinge_axes_are_perpendicular_to_their_reference() {
    let cfg = cfg();
    let layout = cfg.brain_layout();
    let mut rng = Rng::new(15);
    let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
    let p = build(&g, &cfg);
    for j in &p.world.joints {
        assert!(j.axis_a.dot(j.ref_a).abs() < 1e-6);
        assert!(j.axis_b.dot(j.ref_b).abs() < 1e-6);
    }
}

#[test]
fn expression_is_deterministic() {
    let cfg = cfg();
    let layout = cfg.brain_layout();
    let g = Genome::random(&mut Rng::new(21), &cfg.body, &cfg.brain, &layout);
    let a = build(&g, &cfg);
    let b = build(&g, &cfg);
    for (x, y) in a.world.bodies.iter().zip(&b.world.bodies) {
        assert_eq!(x.pos, y.pos);
        assert_eq!(x.shape, y.shape);
    }
    assert_eq!(a.total_mass, b.total_mass);
}

#[test]
fn world_params_carry_solver_knobs_from_config() {
    let mut cfg = cfg();
    cfg.simulation.baumgarte = 0.4;
    cfg.simulation.slop = 0.01;
    cfg.simulation.max_linear_speed = 12.0;
    let p = world_params(&cfg);
    assert!((p.baumgarte - 0.4).abs() < 1e-6);
    assert!((p.slop - 0.01).abs() < 1e-6);
    assert!((p.max_linear_speed - 12.0).abs() < 1e-6);
    assert_eq!(p.iterations, cfg.simulation.solver_iterations);
}

#[test]
fn a_built_organism_settles_without_diverging() {
    let cfg = cfg();
    let layout = cfg.brain_layout();
    for seed in 0..60 {
        let mut rng = Rng::new(1000 + seed);
        let g = Genome::random(&mut rng, &cfg.body, &cfg.brain, &layout);
        let mut p = build(&g, &cfg);
        for _ in 0..240 {
            p.world.step(cfg.simulation.timestep);
        }
        assert!(!p.world.diverged, "seed {seed} diverged while settling");
        for b in &p.world.bodies {
            assert!(b.pos.y > -1.0, "seed {seed} fell through the floor");
        }
    }
}
