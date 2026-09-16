//! `evo replay` — re-simulate a recorded organism, optionally at higher fidelity.

use anyhow::{bail, Context, Result};

use evoforge::evolution;
use evoforge::record::Run;
use evoforge::sim;

use super::ReplayArgs;

pub(super) fn cmd_replay(args: ReplayArgs) -> Result<()> {
    let run =
        Run::open(&args.run).with_context(|| format!("opening run {}", args.run.display()))?;
    let mut cfg = run.config()?;

    let stored = match (args.organism, args.best) {
        (Some(id), _) => {
            run.find_genome(id)?.with_context(|| format!("no stored genome for organism {id}"))?
        }
        (None, true) => run.best_stored_genome()?.context("no genomes were stored for this run")?,
        (None, false) => bail!("specify --organism <id> or --best"),
    };

    // The run's own dynamics, before any override. `--hz` only changes how often
    // the trajectory is sampled and leaves this untouched, which is the whole
    // point of the command; `--duration` changes the simulation itself.
    let run_digest = cfg.evolution_digest();
    if let Some(hz) = args.hz {
        cfg.recording.record_hz = hz;
    }
    if let Some(d) = args.duration {
        cfg.simulation.duration = d;
    }
    cfg.validate()?;
    let dynamics_changed = cfg.evolution_digest() != run_digest;

    println!(
        "organism {} from generation {} (parents {:?}), recorded fitness {:.4}",
        stored.id, stored.generation, stored.parents, stored.fitness
    );
    describe_genome(&stored.genome);

    let result = sim::evaluate(&stored.genome, &cfg, true);
    println!();
    println!("re-simulated fitness {:.4}", result.fitness);
    if (result.fitness - stored.fitness).abs() > 1e-3 && args.duration.is_none() {
        println!(
            "  note: differs from the recorded fitness by {:.4}; the run's config may \
             have changed since",
            result.fitness - stored.fitness
        );
    }
    let m = &result.metrics;
    println!(
        "  displacement {:.3} m ({:.3} along x), path {:.3} m, mean speed {:.3} m/s",
        m.displacement,
        m.displacement_x,
        m.path_length,
        m.mean_speed()
    );
    println!(
        "  upright {:.2}s of {:.2}s, mean height {:.3} m, actuation {:.1}",
        m.upright_seconds, m.duration, m.mean_height, m.actuation
    );

    let Some(trace) = result.trace else {
        bail!("the organism diverged when re-simulated; nothing to record");
    };
    let frames = trace.frames.len();

    // The re-simulated organism, carrying the fitness it just earned rather than
    // the one it was recorded with.
    let individual = evolution::Individual {
        id: stored.id,
        generation: stored.generation,
        parents: stored.parents,
        genome: stored.genome.clone(),
        fitness: result.fitness,
        metrics: result.metrics,
    };

    let path = match args.out {
        Some(p) => {
            run.write_replay_to(&p, &individual, trace, &cfg)?;
            p
        }
        // Re-simulating under different dynamics must not overwrite the trajectory
        // the run itself recorded: that file is the run's own evidence, and this
        // one was produced by a different experiment.
        None if dynamics_changed => {
            let p = run.variant_replay_path(
                individual.generation,
                individual.id,
                cfg.evolution_digest(),
            );
            run.write_replay_to(&p, &individual, trace, &cfg)?;
            println!();
            println!(
                "note: these dynamics differ from the run's own, so the run's \
                 recording of organism {} was left untouched",
                individual.id
            );
            p
        }
        None => run.write_replay(&individual, trace, &cfg)?,
    };

    println!();
    println!("wrote {frames} frames at {}Hz to {}", cfg.recording.record_hz, path.display());
    Ok(())
}

fn describe_genome(g: &evoforge::genome::Genome) {
    println!(
        "  {} parts, {} joints ({} hinges), {} controller weights",
        g.part_count(),
        g.joint_count(),
        g.hinge_count(),
        g.weights.len()
    );
    for (i, p) in g.parts.iter().enumerate() {
        if i == 0 {
            println!(
                "    part 0 (slot {}): root, {:?} {:.2} x {:.2} x {:.2} m",
                p.slot,
                p.shape,
                p.half_extents.x * 2.0,
                p.half_extents.y * 2.0,
                p.half_extents.z * 2.0
            );
        } else {
            println!(
                "    part {} (slot {}): {:?} {:.2} x {:.2} x {:.2} m, {:?} on face {} of part {}, \
                 limit {:.2} rad, motor {:.1} rad/s / {:.0} N m",
                i,
                p.slot,
                p.shape,
                p.half_extents.x * 2.0,
                p.half_extents.y * 2.0,
                p.half_extents.z * 2.0,
                p.joint.kind,
                p.attach_face,
                p.parent,
                p.joint.limit,
                p.joint.motor_speed,
                p.joint.motor_torque
            );
        }
    }
}
