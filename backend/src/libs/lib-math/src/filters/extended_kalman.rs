use serde::{Deserialize, Serialize};

use crate::filters::{Filter, FilterConfigError};

/// One-dimensional extended Kalman filter with user provided model functions.
#[derive(Clone)]
pub struct ExtendedKalmanFilter<F, H, DF, DH>
where
    F: Fn(f32) -> f32,
    H: Fn(f32) -> f32,
    DF: Fn(f32) -> f32,
    DH: Fn(f32) -> f32,
{
    q: f32,
    r: f32,
    state: f32,
    p: f32,
    f: F,
    h: H,
    df: DF,
    dh: DH,
    initialized: bool,
}

const fn id(x: f32) -> f32 {
    x
}

const fn one(_: f32) -> f32 {
    1.0
}

impl<F, H, DF, DH> ExtendedKalmanFilter<F, H, DF, DH>
where
    F: Fn(f32) -> f32,
    H: Fn(f32) -> f32,
    DF: Fn(f32) -> f32,
    DH: Fn(f32) -> f32,
{
    /// Construct a new filter with the given noise parameters and model functions.
    pub const fn new(q: f32, r: f32, f: F, h: H, df: DF, dh: DH) -> Self {
        Self { q, r, state: 0.0, p: 1.0, f, h, df, dh, initialized: false }
    }

    /// Run a single prediction/update cycle with `measurement`.
    pub fn update(&mut self, measurement: f32) -> f32 {
        if !self.initialized {
            self.state = measurement;
            self.initialized = true;
        }

        let predicted = (self.f)(self.state);
        let df = (self.df)(self.state);
        let mut p = df * self.p * df + self.q;

        let h_pred = (self.h)(predicted);
        let dh = (self.dh)(predicted);
        let innovation = measurement - h_pred;
        let s = dh * p * dh + self.r;
        let k = p * dh / s;
        self.state = predicted + k * innovation;
        p *= 1.0 - k * dh;
        self.p = p;
        self.state
    }

    /// Current estimated value.
    pub fn value(&self) -> f32 {
        self.state
    }
}

impl ExtendedKalmanFilter<fn(f32) -> f32, fn(f32) -> f32, fn(f32) -> f32, fn(f32) -> f32> {
    /// Construct a filter using identity model functions.
    pub const fn simple(q: f32, r: f32) -> Self {
        Self::new(q, r, id, id, one, one)
    }
}

impl Filter for ExtendedKalmanFilter<fn(f32) -> f32, fn(f32) -> f32, fn(f32) -> f32, fn(f32) -> f32> {
    type Input = f32;
    type Output = f32;
    type Config = ExtendedKalmanConfig;

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
        Ok(Self::simple(config.q, config.r))
    }

    fn config(&self) -> Self::Config {
        ExtendedKalmanConfig { q: self.q, r: self.r }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ExtendedKalmanConfig {
    pub q: f32,
    pub r: f32,
}

impl Default for ExtendedKalmanConfig {
    fn default() -> Self {
        Self { q: 0.01, r: 0.1 }
    }
}
