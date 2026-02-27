use serde::{Deserialize, Serialize};

use crate::filters::{Filter, FilterConfigError};

/// Simple high pass filter removing the average component from a signal.
#[derive(Debug, Clone)]
pub struct HighPassFilter<const N: usize> {
    alpha: f32,
    state: [f32; N],
    prev_input: [f32; N],
    initialized: bool,
}

impl<const N: usize> HighPassFilter<N> {
    /// Create a new filter with the given factor `alpha`.
    /// Values close to 1.0 preserve more of the high frequency component.
    pub const fn new(alpha: f32) -> Self {
        Self { alpha, state: [0.0; N], prev_input: [0.0; N], initialized: false }
    }

    /// Update the filter with a new measurement.
    pub fn update(&mut self, input: [f32; N]) -> [f32; N] {
        if !self.initialized {
            self.prev_input = input;
            self.initialized = true;
            return self.state;
        }
        for (i, v) in input.iter().enumerate() {
            self.state[i] = self.alpha * (self.state[i] + *v - self.prev_input[i]);
            self.prev_input[i] = *v;
        }
        self.state
    }

    /// Current filtered value.
    pub fn value(&self) -> [f32; N] {
        self.state
    }
}

impl<const N: usize> Filter for HighPassFilter<N> {
    type Input = [f32; N];
    type Output = [f32; N];
    type Config = HighPassConfig;

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
        HighPassConfig { alpha: self.alpha }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HighPassConfig {
    pub alpha: f32,
}

impl Default for HighPassConfig {
    fn default() -> Self {
        Self { alpha: 0.5 }
    }
}
