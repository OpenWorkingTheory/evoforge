//! Rejecting configurations that would not mean what they say.

use super::*;

/// The speed a terrace wall has to survive being hit at, m/s.
///
/// Not `max_linear_speed`, which is a divergence clamp at 60 m/s rather than a
/// speed anything reaches. Measured honest gaits in this project run at one to
/// two metres a second; three leaves room for a faster one without demanding
/// walls so wide they stop being walls.
const WALL_CROSSING_SPEED: Real = 3.0;

/// How many integration steps a body must take to cross a wall.
///
/// Below about four the contact solver meets the wall as one huge penetration
/// rather than a surface; below one, the body tunnels straight through.
const MIN_WALL_STEPS: Real = 4.0;

impl Config {
    /// Refuse terraces whose walls are too thin for the timestep to resolve.
    ///
    /// A cliff in a height field is only a cliff if a body meets it over
    /// several integration steps. Cross it in one and the solver sees a single
    /// enormous penetration and responds accordingly; cross it in less than one
    /// and the body tunnels through as though it were not there. Neither is
    /// terrain, and both are the kind of thing evolution finds and lives on.
    fn validate_terrace_walls(&self) -> Result<(), ConfigError> {
        if self.environment.terrain != Terrain::Fractal || self.environment.terrain_step <= 0.0 {
            return Ok(());
        }
        // Built with an arbitrary seed: wall width is a property of the band
        // structure, not of which landscape the seed picks out.
        let field = crate::physics::FractalField {
            seed: 1,
            amplitude: self.environment.terrain_amplitude,
            wavelength: self.environment.terrain_wavelength,
            octaves: self.environment.terrain_octaves,
            lacunarity: self.environment.terrain_lacunarity,
            gain: self.environment.terrain_gain,
            warp: self.environment.terrain_warp,
            detail_amplitude: self.environment.terrain_detail_amplitude,
            detail_wavelength: self.environment.terrain_detail_wavelength,
            detail_octaves: self.environment.terrain_detail_octaves,
            modulation: self.environment.terrain_modulation,
            modulation_wavelength: self.environment.terrain_modulation_wavelength,
            step: self.environment.terrain_step,
            riser: self.environment.terrain_riser,
            terrace_mask: self.environment.terrain_terrace_mask,
            ..Default::default()
        };
        let width = field.riser_width();
        let per_step = WALL_CROSSING_SPEED * self.simulation.timestep;
        if width < MIN_WALL_STEPS * per_step {
            return Err(ConfigError::Invalid(format!(
                "environment.terrain_riser is too small for this timestep: the terrace \
                 walls would be {:.0} mm wide, which a body at {WALL_CROSSING_SPEED} m/s \
                 crosses in {:.1} steps. Raise terrain_riser or terrain_step, lower \
                 terrain_amplitude, or lower simulation.timestep, until the walls are at \
                 least {:.0} mm.",
                width * 1000.0,
                width / per_step,
                MIN_WALL_STEPS * per_step * 1000.0,
            )));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let bad = |m: &str| -> Result<(), ConfigError> { Err(ConfigError::Invalid(m.into())) };
        let rate = |v: Real, name: &str| -> Result<(), ConfigError> {
            if (0.0..=1.0).contains(&v) {
                Ok(())
            } else {
                bad(&format!("{name} must be within [0, 1]"))
            }
        };

        if self.evolution.population_size < 2 {
            bad("evolution.population_size must be at least 2")?;
        }
        if self.evolution.elite_count >= self.evolution.population_size {
            bad("evolution.elite_count must be smaller than population_size")?;
        }
        if self.evolution.tournament_size < 1 {
            bad("evolution.tournament_size must be at least 1")?;
        }
        rate(self.evolution.crossover_rate, "evolution.crossover_rate")?;
        rate(self.evolution.immigrant_rate, "evolution.immigrant_rate")?;
        if self.body.min_parts < 1 || self.body.max_parts < self.body.min_parts {
            bad("body part limits must satisfy 1 <= min_parts <= max_parts")?;
        }
        if self.body.max_parts > 255 {
            bad("body.max_parts must be at most 255 (slots are u8)")?;
        }
        if self.body.min_half_extent <= 0.0 || self.body.max_half_extent < self.body.min_half_extent
        {
            bad("body half-extent limits must satisfy 0 < min <= max")?;
        }
        if self.body.min_joint_limit <= 0.0
            || self.body.max_joint_limit < self.body.min_joint_limit
            || self.body.max_joint_limit >= crate::math::FRAC_PI_2
        {
            bad("body joint limits must satisfy 0 < min <= max < pi/2 (cosine limit test)")?;
        }
        if self.body.max_motor_speed < 0.0 || self.body.max_motor_torque < 0.0 {
            bad("body motor limits must be non-negative")?;
        }
        rate(self.body.hinge_probability, "body.hinge_probability")?;
        if self.body.shapes.is_empty() {
            bad("body.shapes must list at least one shape")?;
        }
        // A taper wider at the top than the base would put its bounding box
        // somewhere other than where `Shape::bounds` says it is, and every
        // attachment and spawn calculation trusts that bound.
        if !(0.0..=1.0).contains(&self.body.taper_top_scale) {
            bad("body.taper_top_scale must be within [0, 1]")?;
        }
        if self.simulation.trials < 1 {
            bad("simulation.trials must be at least 1")?;
        }
        rate(self.simulation.start_jitter, "simulation.start_jitter")?;
        if self.environment.terrain_amplitude < 0.0 {
            bad("environment.terrain_amplitude must not be negative")?;
        }
        if self.environment.terrain_wavelength <= 0.0 {
            bad("environment.terrain_wavelength must be positive")?;
        }
        if self.environment.terrain_octaves < 1
            || self.environment.terrain_octaves > crate::physics::terrain::MAX_TERRAIN_OCTAVES
        {
            bad("environment.terrain_octaves must be within [1, 8]")?;
        }
        // Below one, an "octave" would be *coarser* than the one before it, and
        // `terrain_wavelength` would stop describing the largest feature.
        if self.environment.terrain_lacunarity < 1.0 {
            bad("environment.terrain_lacunarity must be at least 1")?;
        }
        // At a gain of one every octave contributes its full amplitude and the
        // field is dominated by its finest, which is noise rather than terrain.
        if !(0.0..=1.0).contains(&self.environment.terrain_gain) {
            bad("environment.terrain_gain must be within [0, 1]")?;
        }
        if self.environment.terrain_warp < 0.0 {
            bad("environment.terrain_warp must not be negative")?;
        }
        if self.environment.terrain_detail_amplitude < 0.0 {
            bad("environment.terrain_detail_amplitude must not be negative")?;
        }
        if self.environment.terrain_detail_wavelength <= 0.0 {
            bad("environment.terrain_detail_wavelength must be positive")?;
        }
        if self.environment.terrain_detail_octaves < 1
            || self.environment.terrain_detail_octaves
                > crate::physics::terrain::MAX_TERRAIN_OCTAVES
        {
            bad("environment.terrain_detail_octaves must be within [1, 8]")?;
        }
        rate(self.environment.terrain_modulation, "environment.terrain_modulation")?;
        if self.environment.terrain_modulation_wavelength <= 0.0 {
            bad("environment.terrain_modulation_wavelength must be positive")?;
        }
        if self.environment.terrain_step < 0.0 {
            bad("environment.terrain_step must not be negative (0 is smooth ground)")?;
        }
        if !(0.0..=1.0).contains(&self.environment.terrain_riser)
            || (self.environment.terrain_step > 0.0 && self.environment.terrain_riser <= 0.0)
        {
            bad("environment.terrain_riser must be within (0, 1] when terracing is on")?;
        }
        self.validate_terrace_walls()?;
        if self.body.tendon_frequency < 0.0 || self.body.tendon_damping < 0.0 {
            bad("body.tendon_frequency and body.tendon_damping must not be negative")?;
        }
        // Beyond this the explicit spring stops being stable within one step.
        if self.body.tendon_frequency * self.simulation.timestep > 0.5 {
            bad("body.tendon_frequency is too high for this timestep")?;
        }
        if self.body.muscle_stress < 0.0 {
            bad("body.muscle_stress must not be negative (0 disables the cap)")?;
        }
        if self.body.joint_endurance < 0.0 {
            bad("body.joint_endurance must not be negative (0 disables joint damage)")?;
        }
        if !(0.0..=1.0).contains(&self.body.min_drive) {
            bad("body.min_drive must be within [0, 1]")?;
        }
        if self.body.density <= 0.0 {
            bad("body.density must be positive")?;
        }
        if self.brain.hidden == 0 {
            bad("brain.hidden must be at least 1")?;
        }
        if self.brain.init_sigma < 0.0 || self.brain.weight_limit <= 0.0 {
            bad("brain.init_sigma must be non-negative and weight_limit positive")?;
        }
        if self.simulation.timestep <= 0.0 {
            bad("simulation.timestep must be positive")?;
        }
        if self.simulation.duration <= 0.0 {
            bad("simulation.duration must be positive")?;
        }
        if self.simulation.control_hz <= 0.0 {
            bad("simulation.control_hz must be positive")?;
        }
        if self.simulation.solver_iterations == 0 {
            bad("simulation.solver_iterations must be at least 1")?;
        }
        if self.simulation.settle_time < 0.0 {
            bad("simulation.settle_time must be non-negative")?;
        }
        if !(0.0..=1.0).contains(&self.simulation.baumgarte) {
            bad("simulation.baumgarte must be within [0, 1]")?;
        }
        if self.simulation.slop < 0.0 {
            bad("simulation.slop must be non-negative")?;
        }
        if self.simulation.max_correction_speed <= 0.0
            || self.simulation.max_linear_speed <= 0.0
            || self.simulation.max_angular_speed <= 0.0
        {
            bad("simulation speed clamps must be positive")?;
        }
        if !(0.0..=2.0).contains(&self.environment.friction) {
            bad("environment.friction must be within [0, 2]")?;
        }
        if self.environment.restitution < 0.0 || self.environment.restitution > 1.0 {
            bad("environment.restitution must be within [0, 1]")?;
        }
        if self.environment.linear_damping < 0.0 || self.environment.angular_damping < 0.0 {
            bad("environment damping must be non-negative")?;
        }
        if self.environment.gravity < 0.0 {
            bad("environment.gravity must be non-negative")?;
        }
        if self.recording.record_hz <= 0.0 {
            bad("recording.record_hz must be positive")?;
        }
        rate(self.mutation.weight_rate, "mutation.weight_rate")?;
        rate(self.mutation.weight_reset_rate, "mutation.weight_reset_rate")?;
        rate(self.mutation.size_rate, "mutation.size_rate")?;
        rate(self.mutation.shape_rate, "mutation.shape_rate")?;
        rate(self.mutation.caution_rate, "mutation.caution_rate")?;
        rate(self.mutation.pair_rate, "mutation.pair_rate")?;
        rate(self.mutation.repeat_rate, "mutation.repeat_rate")?;
        rate(self.body.pair_probability, "body.pair_probability")?;
        if self.body.max_repeat < 1 {
            bad("body.max_repeat must be at least 1")?;
        }
        rate(self.mutation.attach_rate, "mutation.attach_rate")?;
        rate(self.mutation.joint_limit_rate, "mutation.joint_limit_rate")?;
        rate(self.mutation.joint_kind_rate, "mutation.joint_kind_rate")?;
        rate(self.mutation.joint_axis_rate, "mutation.joint_axis_rate")?;
        rate(self.mutation.motor_rate, "mutation.motor_rate")?;
        rate(self.mutation.add_part_rate, "mutation.add_part_rate")?;
        rate(self.mutation.remove_part_rate, "mutation.remove_part_rate")?;
        if self.mutation.weight_sigma < 0.0
            || self.mutation.size_sigma < 0.0
            || self.mutation.attach_sigma < 0.0
            || self.mutation.joint_limit_sigma < 0.0
            || self.mutation.motor_sigma < 0.0
        {
            bad("mutation step sizes must be non-negative")?;
        }
        if !(0.0..=1.0).contains(&self.body.sensor_probability) {
            bad("body.sensor_probability must be within [0, 1]")?;
        }
        if self.uses_sensors() {
            if self.sensor.rays == 0 || self.sensor.rays > 8 {
                bad("sensor.rays must be between 1 and 8")?;
            }
            if self.sensor.range <= 0.0 {
                bad("sensor.range must be positive")?;
            }
            if self.sensor.spread < 0.0 {
                bad("sensor.spread must be non-negative")?;
            }
        }
        if self.fitness.energy_penalty < 0.0 || self.fitness.upright_bonus < 0.0 {
            bad("fitness energy_penalty and upright_bonus must be non-negative")?;
        }
        // Penalties are subtracted, so a negative one is a bonus wearing the
        // wrong name — and a negative `descent_penalty` pays an organism to fall,
        // which is precisely the behaviour these terms exist to stop rewarding by
        // accident.
        if self.fitness.descent_penalty < 0.0
            || self.fitness.cumulative_descent_penalty < 0.0
            || self.fitness.fall_penalty < 0.0
        {
            bad("fitness descent penalties must be non-negative")?;
        }
        if self.fitness.climb_deadband < 0.0 {
            bad("fitness climb_deadband must be non-negative")?;
        }
        // A cumulative term with no band pays per unit of vertical wobble, and a
        // gait produces plenty. Refusing this outright is cheaper than
        // discovering it forty generations into a run.
        if (self.fitness.cumulative_climb_bonus != 0.0
            || self.fitness.cumulative_descent_penalty != 0.0)
            && self.fitness.climb_deadband <= 0.0
        {
            bad(
                "fitness climb_deadband must be greater than zero when a cumulative                  elevation term is used: without a band, an organism bobbing on the                  spot accumulates ascent without travelling",
            )?;
        }
        Ok(())
    }
}
