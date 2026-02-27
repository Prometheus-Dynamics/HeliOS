use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};

use crate::filters::{Filter, FilterConfigError};

#[derive(Clone)]
struct Particle {
    pose: [f32; 3],
    weight: f32,
}

#[derive(Clone)]
pub struct ParticleFilter<const N: usize> {
    particles: [Particle; N],
    rng: StdRng,
}

impl<const N: usize> ParticleFilter<N> {
    pub fn new(initial: [f32; 3]) -> Self {
        // Seed a reproducible StdRng from the OS RNG once
        let rng = StdRng::from_os_rng();
        Self { particles: core::array::from_fn(|_| Particle { pose: initial, weight: 1.0 / N as f32 }), rng }
    }

    pub fn with_seed(initial: [f32; 3], seed: u64) -> Self {
        let rng = StdRng::seed_from_u64(seed);
        Self { particles: core::array::from_fn(|_| Particle { pose: initial, weight: 1.0 / N as f32 }), rng }
    }

    pub fn predict(&mut self, motion: [f32; 3], noise: f32) {
        for p in &mut self.particles {
            p.pose[0] += motion[0] + self.rng.random_range(-noise..=noise);
            p.pose[1] += motion[1] + self.rng.random_range(-noise..=noise);
            p.pose[2] += motion[2] + self.rng.random_range(-noise..=noise);
        }
    }

    pub fn update(&mut self, meas: [f32; 2], noise: f32) -> [f32; 3] {
        let mut sum = 0.0;
        for p in &mut self.particles {
            let dx = meas[0] - p.pose[0];
            let dy = meas[1] - p.pose[1];
            let dist2 = dx * dx + dy * dy;
            p.weight = (-dist2 / (2.0 * noise * noise)).exp();
            sum += p.weight;
        }
        for p in &mut self.particles {
            p.weight /= sum;
        }
        self.estimate()
    }

    pub fn estimate(&self) -> [f32; 3] {
        let mut est = [0.0; 3];
        for p in &self.particles {
            for (v, comp) in est.iter_mut().zip(p.pose) {
                *v += comp * p.weight;
            }
        }
        est
    }
}

impl<const N: usize> Filter for ParticleFilter<N> {
    type Input = ([f32; 2], f32);
    type Output = [f32; 3];
    type Config = ParticleConfig;

    fn update(&mut self, input: Self::Input) -> Self::Output {
        Self::update(self, input.0, input.1)
    }

    fn value(&self) -> Self::Output {
        self.estimate()
    }

    fn from_config(config: Self::Config) -> Result<Self, FilterConfigError>
    where
        Self: Sized,
    {
        Ok(match config.seed {
            Some(seed) => Self::with_seed(config.initial, seed),
            None => Self::new(config.initial),
        })
    }

    fn config(&self) -> Self::Config {
        ParticleConfig { initial: self.estimate(), seed: None }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ParticleConfig {
    pub initial: [f32; 3],
    pub seed: Option<u64>,
}

impl Default for ParticleConfig {
    fn default() -> Self {
        Self { initial: [0.0, 0.0, 0.0], seed: None }
    }
}
