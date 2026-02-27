use serde::{Deserialize, Serialize};

use crate::filters::{Filter, FilterConfigError};

/// Simple scalar Kalman filter applied independently to each element of a vector.
#[derive(Debug, Clone)]
pub struct KalmanFilter<const N: usize> {
    q: f32,
    r: f32,
    state: [f32; N],
    p: [f32; N],
    initialized: bool,
}

impl<const N: usize> KalmanFilter<N> {
    /// Create a new filter with the given process noise `q` and measurement noise `r`.
    pub const fn new(q: f32, r: f32) -> Self {
        Self { q, r, state: [0.0; N], p: [1.0; N], initialized: false }
    }

    /// Update the filter with a new measurement.
    pub fn update(&mut self, measurement: [f32; N]) -> [f32; N] {
        if !self.initialized {
            self.state = measurement;
            self.initialized = true;
            return self.state;
        }
        for (i, m) in measurement.iter().enumerate() {
            self.p[i] += self.q;
            let k = self.p[i] / (self.p[i] + self.r);
            self.state[i] += k * (*m - self.state[i]);
            self.p[i] *= 1.0 - k;
        }
        self.state
    }

    /// Current estimated value.
    pub fn value(&self) -> [f32; N] {
        self.state
    }
}

impl<const N: usize> Filter for KalmanFilter<N> {
    type Input = [f32; N];
    type Output = [f32; N];
    type Config = KalmanConfig;

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
        if config.q <= 0.0 {
            return Err(FilterConfigError::NotPositive { field: "q", value: config.q });
        }
        if config.r <= 0.0 {
            return Err(FilterConfigError::NotPositive { field: "r", value: config.r });
        }
        Ok(Self::new(config.q, config.r))
    }

    fn config(&self) -> Self::Config {
        KalmanConfig { q: self.q, r: self.r }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct KalmanConfig {
    pub q: f32,
    pub r: f32,
}

impl Default for KalmanConfig {
    fn default() -> Self {
        Self { q: 0.01, r: 0.1 }
    }
}
