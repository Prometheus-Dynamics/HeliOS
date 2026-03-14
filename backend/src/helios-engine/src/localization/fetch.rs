use std::future::Future;
use std::pin::Pin;

use nalgebra::{Quaternion, UnitQuaternion, Vector3};

use super::config::LocalizationSourceConfig;

pub trait LocalizationSourceFetcher: Send + Sync {
    fn fetch_source_value<'a>(&'a self, source: &'a LocalizationSourceConfig) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send + 'a>>;
}

pub fn imu_backend_to_viewer_basis() -> UnitQuaternion<f64> {
    // Matches frontend imuQuaternionToThree() basis conversion:
    // backend IMU (+X forward, +Y right, +Z up) -> viewer (+X right, +Y up, +Z forward).
    UnitQuaternion::new_normalize(Quaternion::new(0.5, -0.5, -0.5, -0.5))
}

pub fn imu_vec_to_viewer_frame(vector: Vector3<f64>) -> Vector3<f64> {
    imu_backend_to_viewer_basis().transform_vector(&vector)
}
