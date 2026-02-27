use super::{BackendHandle, SensorBackend};
use crate::Result;

/// Blocking accelerometer interface implemented by device drivers.
pub trait Accelerometer: SensorBackend {
    /// Reads the three-axis acceleration values in g-forces.
    fn read_accel(&mut self) -> Result<[f32; 3]>;
}

/// Backend abstraction implemented by concrete accelerometer devices.
pub trait AccelerometerBackend: Accelerometer {
    /// Optional maximum measurable acceleration in g-forces.
    fn max_g(&self) -> Option<f32> {
        None
    }
}

/// Thread-safe accelerometer facade exposed to the application layer.
pub trait AccelerometerSensor: Send + Sync {
    /// Reads the current acceleration vector.
    fn acceleration(&self) -> Result<[f32; 3]>;
    /// Optional maximum measurable acceleration in g-forces.
    fn max_g(&self) -> Option<f32> {
        None
    }
}

/// Generic wrapper turning a blocking accelerometer into a thread-safe `Sensor`.
pub struct AccelerometerDevice<B: AccelerometerBackend> {
    backend: BackendHandle<B>,
}

impl<B: AccelerometerBackend> AccelerometerDevice<B> {
    pub fn new(backend: B) -> Self {
        Self { backend: BackendHandle::new(backend) }
    }
}

impl<B: AccelerometerBackend> AccelerometerSensor for AccelerometerDevice<B> {
    fn acceleration(&self) -> Result<[f32; 3]> {
        let mut backend = self.backend.lock();
        backend.read_accel()
    }

    fn max_g(&self) -> Option<f32> {
        let backend = self.backend.lock();
        backend.max_g()
    }
}
