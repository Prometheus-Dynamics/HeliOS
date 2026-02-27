use serde::{Deserialize, Serialize};

use crate::filters::{Filter, FilterConfigError};

/// Alpha-beta filter for estimating position and velocity from noisy measurements.
#[derive(Debug, Clone)]
pub struct AlphaBetaFilter {
    alpha: f32,
    beta: f32,
    dt: f32,
    position: f32,
    velocity: f32,
    initialized: bool,
}

impl AlphaBetaFilter {
    /// Create a new filter with given gains `alpha` and `beta` and sample period `dt`.
    pub const fn new(alpha: f32, beta: f32, dt: f32) -> Self {
        Self { alpha, beta, dt, position: 0.0, velocity: 0.0, initialized: false }
    }

    /// Update the filter with a new position measurement.
    pub fn update(&mut self, measurement: f32) -> (f32, f32) {
        if !self.initialized {
            self.position = measurement;
            self.initialized = true;
            return (self.position, self.velocity);
        }

        let predicted = self.position + self.velocity * self.dt;
        let residual = measurement - predicted;
        self.position = predicted + self.alpha * residual;
        self.velocity += self.beta * residual / self.dt;
        (self.position, self.velocity)
    }

    /// Current estimated position and velocity.
    pub fn value(&self) -> (f32, f32) {
        (self.position, self.velocity)
    }
}

impl Filter for AlphaBetaFilter {
    type Input = f32;
    type Output = (f32, f32);
    type Config = AlphaBetaConfig;

    fn update(&mut self, input: Self::Input) -> Self::Output {
        Self::update(self, input)
    }

    fn value(&self) -> Self::Output {
        Self::value(self)
    }

    fn from_config(config: Self::Config) -> Result<Self, FilterConfigError>
    where
        Self: Sized,
    {
        if !(0.0..=1.0).contains(&config.alpha) {
            return Err(FilterConfigError::OutOfRange { field: "alpha", min: 0.0, max: 1.0, value: config.alpha });
        }
        if !(0.0..=1.0).contains(&config.beta) {
            return Err(FilterConfigError::OutOfRange { field: "beta", min: 0.0, max: 1.0, value: config.beta });
        }
        if config.beta <= 0.0 {
            return Err(FilterConfigError::NotPositive { field: "beta", value: config.beta });
        }
        if config.dt <= 0.0 {
            return Err(FilterConfigError::NotPositive { field: "dt", value: config.dt });
        }
        Ok(Self::new(config.alpha, config.beta, config.dt))
    }

    fn config(&self) -> Self::Config {
        AlphaBetaConfig { alpha: self.alpha, beta: self.beta, dt: self.dt }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlphaBetaConfig {
    pub alpha: f32,
    pub beta: f32,
    pub dt: f32,
}

impl Default for AlphaBetaConfig {
    fn default() -> Self {
        Self { alpha: 0.85, beta: 0.005, dt: 1.0 }
    }
}
