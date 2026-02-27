use serde::{Deserialize, Serialize};
use std::vec::Vec;

use crate::filters::{Filter, FilterConfigError};

/// Simple moving average filter over the last `M` samples.
#[derive(Debug, Clone)]
pub struct MovingAverageFilter<const N: usize, const M: usize> {
    window: [[f32; N]; M],
    sum: [f32; N],
    index: usize,
    count: usize,
}

impl<const N: usize, const M: usize> MovingAverageFilter<N, M> {
    /// Create a new moving average filter.
    pub const fn new() -> Self {
        Self { window: [[0.0; N]; M], sum: [0.0; N], index: 0, count: 0 }
    }

    /// Update the filter with a new measurement and return the averaged value.
    pub fn update(&mut self, input: [f32; N]) -> [f32; N] {
        if self.count < M {
            self.window[self.index] = input;
            for (i, v) in input.iter().enumerate() {
                self.sum[i] += *v;
            }
            self.count += 1;
        } else {
            for (i, v) in input.iter().enumerate() {
                self.sum[i] += *v - self.window[self.index][i];
            }
            self.window[self.index] = input;
        }
        self.index = (self.index + 1) % M;
        self.value()
    }

    /// Current averaged value.
    pub fn value(&self) -> [f32; N] {
        let mut out = [0.0; N];
        if self.count == 0 {
            return out;
        }
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.sum[i] / self.count as f32;
        }
        out
    }
}

impl<const N: usize, const M: usize> Filter for MovingAverageFilter<N, M> {
    type Input = [f32; N];
    type Output = [f32; N];
    type Config = MovingAverageConfig;

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
        if M == 0 {
            return Err(FilterConfigError::NonZeroRequired { field: "window" });
        }
        let mut filter = Self::new();
        if let Some(initial) = config.initial {
            if initial.len() != N {
                return Err(FilterConfigError::DimensionMismatch { field: "initial", expected: N, actual: initial.len() });
            }
            for (dst, src) in filter.window[0].iter_mut().zip(initial.iter()) {
                *dst = *src;
            }
            filter.sum = filter.window[0];
            filter.count = 1;
            filter.index = 1 % M;
        }
        Ok(filter)
    }

    fn config(&self) -> Self::Config {
        let initial = if self.count > 0 { Some(self.value().to_vec()) } else { None };
        MovingAverageConfig { initial }
    }
}

impl<const N: usize, const M: usize> Default for MovingAverageFilter<N, M> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MovingAverageConfig {
    pub initial: Option<Vec<f32>>,
}
