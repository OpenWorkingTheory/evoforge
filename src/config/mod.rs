//! Experiment configuration.
//!
//! Everything that defines an experiment lives in a single TOML file. Every
//! field has a default so that a minimal config is legal, but unknown fields are
//! rejected: a typo that silently reverts a setting to its default would quietly
//! invalidate a comparison between runs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::brain::BrainLayout;
use crate::genome::ShapeKind;
use crate::math::Real;

mod fingerprint;
mod sections;
mod validate;

#[cfg(test)]
mod tests;

use fingerprint::fingerprint;

pub use fingerprint::fnv1a;
pub use sections::*;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub experiment: ExperimentCfg,
    pub evolution: EvolutionCfg,
    pub mutation: MutationParams,
    pub body: BodyLimits,
    pub brain: BrainCfg,
    pub simulation: SimulationCfg,
    pub environment: EnvironmentCfg,
    pub fitness: FitnessCfg,
    pub sensor: SensorCfg,
    pub recording: RecordingCfg,
    pub checkpoint: CheckpointCfg,
}

impl Config {
    pub fn from_toml_str(text: &str) -> Result<Config, ConfigError> {
        let cfg: Config = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(path.display().to_string(), e.to_string()))?;
        Config::from_toml_str(&text)
    }

    pub fn to_toml_string(&self) -> String {
        toml::to_string_pretty(self).expect("config is always serialisable")
    }

    /// The controller shape implied by this configuration.
    pub fn brain_layout(&self) -> BrainLayout {
        BrainLayout::new_full(
            self.body.max_parts,
            self.brain.hidden,
            self.joints_can_break(),
            self.simulation.steer,
            self.sensor_channels(),
        )
    }

    /// Whether organisms in this experiment may carry range sensors at all.
    ///
    /// Zero probability means no part ever draws a sensor gene, which is what
    /// makes the feature exactly off rather than approximately off: the random
    /// stream and the controller layout are both untouched.
    #[inline]
    pub fn uses_sensors(&self) -> bool {
        self.body.sensor_probability > 0.0
    }

    /// Controller inputs each slot contributes for sensing, which is one range
    /// reading per ray, or none at all.
    #[inline]
    pub fn sensor_channels(&self) -> usize {
        if self.uses_sensors() {
            self.sensor.rays
        } else {
            0
        }
    }

    /// Whether this experiment lets joints wear out and limbs detach.
    #[inline]
    pub fn joints_can_break(&self) -> bool {
        self.body.joint_endurance > 0.0
    }

    /// Number of physics steps in one evaluation, including the settling period.
    pub fn total_steps(&self) -> u32 {
        ((self.simulation.settle_time + self.simulation.duration) / self.simulation.timestep).ceil()
            as u32
    }

    /// Number of physics steps to run before the controller is enabled and
    /// fitness measurement begins.
    pub fn settle_steps(&self) -> u32 {
        (self.simulation.settle_time / self.simulation.timestep).ceil() as u32
    }

    /// Fingerprint of the whole configuration, recorded in a run's manifest.
    pub fn digest(&self) -> u64 {
        fingerprint(self, true)
    }

    /// Fingerprint of only those settings that change what evolution *does*.
    ///
    /// Where the results go, how long the run lasts, and what gets recorded do
    /// not affect the trajectory, so they are excluded. That distinction is what
    /// lets a finished run be extended — resume with a larger `generations` and
    /// the checkpoint still matches — while still refusing to resume a run whose
    /// physics, mutation rates or seed have changed underneath it.
    ///
    /// Built from a versioned, field-by-field byte stream rather than pretty
    /// TOML, so a serializer upgrade or a comment cannot silently invalidate
    /// resume.
    pub fn evolution_digest(&self) -> u64 {
        fingerprint(self, false)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(String, String),
    Parse(String),
    Invalid(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(path, e) => write!(f, "could not read config {path}: {e}"),
            ConfigError::Parse(e) => write!(f, "could not parse config: {e}"),
            ConfigError::Invalid(m) => write!(f, "invalid config: {m}"),
        }
    }
}

impl std::error::Error for ConfigError {}
