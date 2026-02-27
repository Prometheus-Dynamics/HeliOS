#![allow(clippy::needless_range_loop)]

use serde::{Deserialize, Serialize};

use crate::filters::{Filter, FilterConfigError};

#[derive(Debug, Clone)]
pub struct CvEkf3D {
    dt: f32,
    q: f32,
    r: f32,
    state: [f32; 6],
    p: [[f32; 6]; 6],
    initialized: bool,
}

impl CvEkf3D {
    pub const fn new(dt: f32, q: f32, r: f32) -> Self {
        Self { dt, q, r, state: [0.0; 6], p: [[0.0; 6]; 6], initialized: false }
    }

    pub fn update(&mut self, meas: [f32; 3]) -> [f32; 6] {
        if !self.initialized {
            self.state[0] = meas[0];
            self.state[1] = meas[1];
            self.state[2] = meas[2];
            for i in 0..6 {
                self.p[i][i] = 1.0;
            }
            self.initialized = true;
            return self.state;
        }

        let dt = self.dt;
        // state prediction
        self.state[0] += dt * self.state[3];
        self.state[1] += dt * self.state[4];
        self.state[2] += dt * self.state[5];
        let mut f = [[0.0; 6]; 6];
        for i in 0..6 {
            f[i][i] = 1.0;
        }
        f[0][3] = dt;
        f[1][4] = dt;
        f[2][5] = dt;
        // P = F * P * F^T + Q
        let mut temp = [[0.0; 6]; 6];
        for i in 0..6 {
            for k in 0..6 {
                let mut sum = 0.0;
                for j in 0..6 {
                    sum += f[i][j] * self.p[j][k];
                }
                temp[i][k] = sum;
            }
        }
        let mut p = [[0.0; 6]; 6];
        for i in 0..6 {
            for k in 0..6 {
                let mut sum = 0.0;
                for j in 0..6 {
                    sum += temp[i][j] * f[k][j];
                }
                p[i][k] = sum;
            }
        }
        for i in 0..6 {
            p[i][i] += self.q;
        }
        // measurement update
        let mut s = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                s[i][j] = p[i][j];
            }
        }
        for i in 0..3 {
            s[i][i] += self.r;
        }
        // compute Kalman gain K = P * H^T * S^{-1}
        // Because H selects first 3 states, we can compute simpler
        let mut k = [[0.0; 3]; 6];
        for i in 0..6 {
            for j in 0..3 {
                k[i][j] = p[i][j];
            }
        }
        // invert S (3x3) assuming diagonal
        let mut inv_s = [[0.0; 3]; 3];
        for i in 0..3 {
            inv_s[i][i] = 1.0 / s[i][i];
        }
        // multiply k by inv_s
        for i in 0..6 {
            for j in 0..3 {
                k[i][j] *= inv_s[j][j];
            }
        }
        // update state = state + K * innovation
        let mut innovation = [0.0; 3];
        innovation[0] = meas[0] - self.state[0];
        innovation[1] = meas[1] - self.state[1];
        innovation[2] = meas[2] - self.state[2];
        for i in 0..6 {
            for j in 0..3 {
                self.state[i] += k[i][j] * innovation[j];
            }
        }
        // update covariance
        for i in 0..6 {
            for j in 0..6 {
                let mut sum = p[i][j];
                for m in 0..3 {
                    sum -= k[i][m] * p[m][j];
                }
                self.p[i][j] = sum;
            }
        }
        self.state
    }

    pub fn value(&self) -> [f32; 6] {
        self.state
    }
}

impl Filter for CvEkf3D {
    type Input = [f32; 3];
    type Output = [f32; 6];
    type Config = CvEkf3DConfig;

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
        CvEkf3DConfig { dt: self.dt, q: self.q, r: self.r }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CvEkf3DConfig {
    pub dt: f32,
    pub q: f32,
    pub r: f32,
}

impl Default for CvEkf3DConfig {
    fn default() -> Self {
        Self { dt: 1.0, q: 0.01, r: 0.1 }
    }
}
