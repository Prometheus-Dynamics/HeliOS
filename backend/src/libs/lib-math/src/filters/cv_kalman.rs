use serde::{Deserialize, Serialize};

use crate::filters::{Filter, FilterConfigError};

#[derive(Debug, Clone, Copy)]
pub struct CvKalman1D {
    dt: f32,
    q: f32,
    r: f32,
    state: [f32; 2],
    p: [[f32; 2]; 2],
    initialized: bool,
}

impl CvKalman1D {
    pub const fn new(dt: f32, q: f32, r: f32) -> Self {
        Self { dt, q, r, state: [0.0; 2], p: [[1.0, 0.0], [0.0, 1.0]], initialized: false }
    }

    pub fn update(&mut self, z: f32) -> [f32; 2] {
        if !self.initialized {
            self.state[0] = z;
            self.initialized = true;
            return self.state;
        }
        let dt = self.dt;
        // Predict
        let m0 = self.state[0] + dt * self.state[1];
        let m1 = self.state[1];
        let mut p00 = self.p[0][0] + dt * (self.p[0][1] + self.p[1][0]) + dt * dt * self.p[1][1] + self.q;
        let mut p01 = self.p[0][1] + dt * self.p[1][1];
        let mut p10 = self.p[1][0] + dt * self.p[1][1];
        let mut p11 = self.p[1][1] + self.q;

        // Update
        let s = p00 + self.r;
        let k0 = p00 / s;
        let k1 = p10 / s;
        let y = z - m0;
        self.state[0] = m0 + k0 * y;
        self.state[1] = m1 + k1 * y;
        p00 *= 1.0 - k0;
        p01 *= 1.0 - k0;
        p10 = p01;
        p11 -= k1 * p01;
        self.p = [[p00, p01], [p10, p11]];
        self.state
    }

    pub fn value(&self) -> [f32; 2] {
        self.state
    }
}

#[derive(Debug, Clone)]
pub struct CvKalmanFilter<const N: usize> {
    axis: [CvKalman1D; N],
    dt: f32,
    q: f32,
    r: f32,
}

impl<const N: usize> CvKalmanFilter<N> {
    pub const fn new(dt: f32, q: f32, r: f32) -> Self {
        Self { axis: [CvKalman1D::new(dt, q, r); N], dt, q, r }
    }

    pub fn update(&mut self, measurement: [f32; N]) -> ([f32; N], [f32; N]) {
        let mut pos = [0.0; N];
        let mut vel = [0.0; N];
        let mut i = 0;
        while i < N {
            let s = self.axis[i].update(measurement[i]);
            pos[i] = s[0];
            vel[i] = s[1];
            i += 1;
        }
        (pos, vel)
    }

    pub fn value(&self) -> ([f32; N], [f32; N]) {
        let mut pos = [0.0; N];
        let mut vel = [0.0; N];
        let mut i = 0;
        while i < N {
            let s = self.axis[i].value();
            pos[i] = s[0];
            vel[i] = s[1];
            i += 1;
        }
        (pos, vel)
    }
}

impl<const N: usize> Filter for CvKalmanFilter<N> {
    type Input = [f32; N];
    type Output = ([f32; N], [f32; N]);
    type Config = CvKalmanConfig;

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
        if config.dt <= 0.0 {
            return Err(FilterConfigError::NotPositive { field: "dt", value: config.dt });
        }
        if config.q <= 0.0 {
            return Err(FilterConfigError::NotPositive { field: "q", value: config.q });
        }
        if config.r <= 0.0 {
            return Err(FilterConfigError::NotPositive { field: "r", value: config.r });
        }
        Ok(Self::new(config.dt, config.q, config.r))
    }

    fn config(&self) -> Self::Config {
        CvKalmanConfig { dt: self.dt, q: self.q, r: self.r }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CvKalmanConfig {
    pub dt: f32,
    pub q: f32,
    pub r: f32,
}

impl Default for CvKalmanConfig {
    fn default() -> Self {
        Self { dt: 1.0, q: 0.01, r: 0.1 }
    }
}
