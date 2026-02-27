use super::{BackendHandle, SensorBackend};
use crate::Result;

/// Blocking gyroscope interface implemented by device drivers.
pub trait Gyro: SensorBackend {
    /// Reads the three-axis angular velocity values in degrees per second.
    fn read_gyro(&mut self) -> Result<[f32; 3]>;
}

/// Backend abstraction implemented by concrete gyroscope devices.
pub trait GyroBackend: Gyro {
    /// Optional maximum angular velocity in degrees per second.
    fn max_dps(&self) -> Option<f32> {
        None
    }
}

/// Thread-safe gyroscope facade exposed to the application layer.
pub trait GyroSensor: Send + Sync {
    /// Reads the three-axis angular velocity.
    fn angular_velocity(&self) -> Result<[f32; 3]>;
    /// Optional maximum angular velocity in degrees per second.
    fn max_dps(&self) -> Option<f32> {
        None
    }
}

/// Generic wrapper turning a blocking gyroscope into a thread-safe `Sensor`.
pub struct GyroDevice<B: GyroBackend> {
    backend: BackendHandle<B>,
}

impl<B: GyroBackend> GyroDevice<B> {
    pub fn new(backend: B) -> Self {
        Self { backend: BackendHandle::new(backend) }
    }
}

impl<B: GyroBackend> GyroSensor for GyroDevice<B> {
    fn angular_velocity(&self) -> Result<[f32; 3]> {
        let mut backend = self.backend.lock();
        backend.read_gyro()
    }

    fn max_dps(&self) -> Option<f32> {
        let backend = self.backend.lock();
        backend.max_dps()
    }
}
