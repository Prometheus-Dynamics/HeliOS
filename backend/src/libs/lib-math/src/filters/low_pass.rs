use serde::{Deserialize, Serialize};

use crate::filters::{Filter, FilterConfigError};

/// Simple low pass filter using an exponential moving average.
#[derive(Debug, Clone)]
pub struct LowPassFilter<const N: usize> {
    alpha: f32,
    state: [f32; N],
    initialized: bool,
}

impl<const N: usize> LowPassFilter<N> {
    /// Create a new filter with the given smoothing factor `alpha`.
    /// `alpha` should be in the range `0.0..=1.0` with higher values giving
    /// more weight to new samples.
    pub const fn new(alpha: f32) -> Self {
        Self { alpha, state: [0.0; N], initialized: false }
    }

    /// Update the filter state with a new measurement and return the filtered value.
    pub fn update(&mut self, input: [f32; N]) -> [f32; N] {
        if !self.initialized {
            self.state = input;
            self.initialized = true;
            return self.state;
        }
        for (i, v) in input.iter().enumerate() {
            self.state[i] = self.alpha * *v + (1.0 - self.alpha) * self.state[i];
        }
        self.state
    }

    /// Current filtered value.
    pub fn value(&self) -> [f32; N] {
        self.state
    }
}

impl<const N: usize> Filter for LowPassFilter<N> {
    type Input = [f32; N];
    type Output = [f32; N];
    type Config = LowPassConfig;

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
        Ok(Self::new(config.alpha))
    }

    fn config(&self) -> Self::Config {
        LowPassConfig { alpha: self.alpha }
    }
}

/// Serializable configuration for [`LowPassFilter`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LowPassConfig {
    pub alpha: f32,
}

impl Default for LowPassConfig {
    fn default() -> Self {
        Self { alpha: 0.5 }
    }
}
