use serde::{Deserialize, Serialize};

use crate::filters::{Filter, FilterConfigError};

/// Combines two measurements using a weighted average.
#[derive(Debug, Clone)]
pub struct ComplementaryFilter<const N: usize> {
    alpha: f32,
    state: [f32; N],
    initialized: bool,
}

impl<const N: usize> ComplementaryFilter<N> {
    /// Create a new filter with the given weight `alpha` applied to `fast` values.
    pub const fn new(alpha: f32) -> Self {
        Self { alpha, state: [0.0; N], initialized: false }
    }

    /// Update the filter combining `fast` and `slow` measurements.
    pub fn update(&mut self, fast: [f32; N], slow: [f32; N]) -> [f32; N] {
        if !self.initialized {
            for (i, v) in fast.iter().enumerate() {
                self.state[i] = self.alpha * *v + (1.0 - self.alpha) * slow[i];
            }
            self.initialized = true;
            return self.state;
        }
        for (i, v) in fast.iter().enumerate() {
            self.state[i] = self.alpha * *v + (1.0 - self.alpha) * slow[i];
        }
        self.state
    }

    /// Current fused value.
    pub fn value(&self) -> [f32; N] {
        self.state
    }
}

impl<const N: usize> Filter for ComplementaryFilter<N> {
    type Input = ([f32; N], [f32; N]);
    type Output = [f32; N];
    type Config = ComplementaryConfig;

    fn update(&mut self, input: Self::Input) -> Self::Output {
        Self::update(self, input.0, input.1)
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
        ComplementaryConfig { alpha: self.alpha }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComplementaryConfig {
    pub alpha: f32,
}

impl Default for ComplementaryConfig {
    fn default() -> Self {
        Self { alpha: 0.5 }
    }
}
