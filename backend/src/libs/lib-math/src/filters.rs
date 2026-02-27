use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

/// Core trait implemented by all runtime filters.
///
/// Every filter now exposes a serializable configuration payload so that
/// pipelines can be constructed and tuned dynamically without bespoke glue.
pub trait Filter {
    type Input;
    type Output;
    type Config: Clone + Serialize + DeserializeOwned;

    fn update(&mut self, input: Self::Input) -> Self::Output;
    fn value(&self) -> Self::Output;

    fn from_config(config: Self::Config) -> Result<Self, FilterConfigError>
    where
        Self: Sized;

    fn config(&self) -> Self::Config;

    fn reconfigure(&mut self, config: Self::Config) -> Result<(), FilterConfigError>
    where
        Self: Sized,
    {
        *self = Self::from_config(config)?;
        Ok(())
    }
}

/// Error raised when creating or updating a filter from a configuration payload.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum FilterConfigError {
    #[error("{field} must be between {min} and {max}, got {value}")]
    OutOfRange { field: &'static str, min: f32, max: f32, value: f32 },
    #[error("{field} must be positive, got {value}")]
    NotPositive { field: &'static str, value: f32 },
    #[error("{field} must be non-zero")]
    NonZeroRequired { field: &'static str },
    #[error("{field} length mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { field: &'static str, expected: usize, actual: usize },
}

pub mod alpha_beta;
pub mod complementary;
pub mod cv_kalman;
pub mod ekf_cv3d;
pub mod extended_kalman;
pub mod high_pass;
pub mod kalman;
pub mod low_pass;
pub mod moving_average;
pub mod particle;

pub use alpha_beta::{AlphaBetaConfig, AlphaBetaFilter};
pub use complementary::{ComplementaryConfig, ComplementaryFilter};
pub use cv_kalman::{CvKalmanConfig, CvKalmanFilter};
pub use ekf_cv3d::{CvEkf3D, CvEkf3DConfig};
pub use extended_kalman::{ExtendedKalmanConfig, ExtendedKalmanFilter};
pub use high_pass::{HighPassConfig, HighPassFilter};
pub use kalman::{KalmanConfig, KalmanFilter};
pub use low_pass::{LowPassConfig, LowPassFilter};
pub use moving_average::{MovingAverageConfig, MovingAverageFilter};
pub use particle::{ParticleConfig, ParticleFilter};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_pass_filter() {
        let mut f = LowPassFilter::<1>::new(0.5);
        let filt: &mut dyn Filter<Input = [f32; 1], Output = [f32; 1], Config = LowPassConfig> = &mut f;
        assert_eq!(filt.update([10.0]), [10.0]);
        assert_eq!(filt.update([20.0]), [15.0]);
    }

    #[test]
    fn high_pass_filter() {
        let mut f = HighPassFilter::<1>::new(0.5);
        let filt: &mut dyn Filter<Input = [f32; 1], Output = [f32; 1], Config = HighPassConfig> = &mut f;
        assert_eq!(filt.update([10.0]), [0.0]);
        assert!((filt.update([20.0])[0] - 5.0).abs() < 1e-6);
    }

    #[test]
    fn complementary_filter() {
        let mut f = ComplementaryFilter::<1>::new(0.7);
        let filt: &mut dyn Filter<Input = ([f32; 1], [f32; 1]), Output = [f32; 1], Config = ComplementaryConfig> = &mut f;
        assert_eq!(filt.update(([10.0], [0.0])), [7.0]);
        assert_eq!(filt.update(([20.0], [10.0])), [17.0]);
    }

    #[test]
    fn kalman_filter() {
        let mut f = KalmanFilter::<1>::new(0.01, 0.1);
        let filt: &mut dyn Filter<Input = [f32; 1], Output = [f32; 1], Config = KalmanConfig> = &mut f;
        assert_eq!(filt.update([10.0]), [10.0]);
        let v = filt.update([10.5])[0];
        assert!(v > 10.0 && v < 10.5);
    }

    #[test]
    fn extended_kalman_filter() {
        let mut f = ExtendedKalmanFilter::simple(0.01, 0.1);
        let filt: &mut dyn Filter<Input = f32, Output = f32, Config = ExtendedKalmanConfig> = &mut f;
        assert!((filt.update(1.0) - 1.0).abs() < 1e-6);
        let v = filt.update(1.2);
        assert!(v > 1.0 && v < 1.2);
    }

    #[test]
    fn moving_average_filter() {
        let mut f = MovingAverageFilter::<1, 2>::new();
        let filt: &mut dyn Filter<Input = [f32; 1], Output = [f32; 1], Config = MovingAverageConfig> = &mut f;
        assert_eq!(filt.update([1.0]), [1.0]);
        assert_eq!(filt.update([3.0]), [2.0]);
        assert_eq!(filt.update([5.0]), [4.0]);
    }

    #[test]
    fn alpha_beta_filter() {
        let mut f = AlphaBetaFilter::new(0.85, 0.005, 1.0);
        let filt: &mut dyn Filter<Input = f32, Output = (f32, f32), Config = AlphaBetaConfig> = &mut f;
        let (p0, _v0) = filt.update(0.0);
        assert_eq!(p0, 0.0);
        let (p1, _v1) = filt.update(1.0);
        assert!(p1 > 0.0 && p1 < 1.0);
    }

    #[test]
    fn cv_ekf3d_filter() {
        let mut f = CvEkf3D::new(1.0, 0.01, 0.1);
        let filt: &mut dyn Filter<Input = [f32; 3], Output = [f32; 6], Config = CvEkf3DConfig> = &mut f;
        let v0 = filt.update([0.0, 0.0, 0.0]);
        assert_eq!(v0[0], 0.0);
        let v1 = filt.update([1.0, 1.0, 1.0]);
        assert!(v1[0] > 0.0 && v1[0] < 1.0);
    }

    #[test]
    fn particle_filter() {
        let mut pf = ParticleFilter::<16>::new([0.0, 0.0, 0.0]);
        pf.predict([1.0, 0.0, 0.0], 0.0);
        let filt: &mut dyn Filter<Input = ([f32; 2], f32), Output = [f32; 3], Config = ParticleConfig> = &mut pf;
        let est = filt.update(([1.0, 0.0], 0.1));
        assert!((est[0] - 1.0).abs() < 0.1);
    }

    #[test]
    fn cv_kalman_filter_multi_dim() {
        let mut f = CvKalmanFilter::<2>::new(1.0, 0.01, 0.1);
        let filt: &mut dyn Filter<Input = [f32; 2], Output = ([f32; 2], [f32; 2]), Config = CvKalmanConfig> = &mut f;
        let (pos0, vel0) = filt.update([0.0, 0.0]);
        assert_eq!(pos0, [0.0, 0.0]);
        assert_eq!(vel0, [0.0, 0.0]);
        let (pos1, vel1) = filt.update([1.0, -1.0]);
        assert!(pos1[0] > 0.0 && pos1[0] < 1.0);
        assert!(pos1[1] < 0.0 && pos1[1] > -1.0);
        assert!(vel1[0] > 0.0);
        assert!(vel1[1] < 0.0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use quickcheck::TestResult;
    use quickcheck_macros::quickcheck;

    #[quickcheck]
    fn low_pass_output_is_bounded(alpha: f32, start: f32, measurement: f32) -> TestResult {
        if !alpha.is_finite() || !start.is_finite() || !measurement.is_finite() {
            return TestResult::discard();
        }
        let alpha = alpha.clamp(0.0, 1.0);
        let mut filter = LowPassFilter::<1>::new(alpha);
        filter.update([start]);
        let value = filter.update([measurement])[0];
        let (min, max) = if start <= measurement { (start, measurement) } else { (measurement, start) };
        TestResult::from_bool(value >= min - 1e-6 && value <= max + 1e-6)
    }

    #[quickcheck]
    fn config_roundtrip_preserves_alpha(alpha: f32) -> TestResult {
        if !alpha.is_finite() {
            return TestResult::discard();
        }
        let alpha = alpha.clamp(0.0, 1.0);
        let filter = LowPassFilter::<1>::from_config(LowPassConfig { alpha }).unwrap();
        TestResult::from_bool((filter.config().alpha - alpha).abs() < 1e-6)
    }
}
