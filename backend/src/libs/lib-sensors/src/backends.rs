pub mod accelerometer;
pub mod gyro;
pub mod magnetometer;
pub mod power;
pub mod range;

pub use accelerometer::{Accelerometer, AccelerometerBackend, AccelerometerDevice, AccelerometerSensor};
pub use gyro::{Gyro, GyroBackend, GyroDevice, GyroSensor};
pub use magnetometer::{Magnetometer, MagnetometerBackend, MagnetometerDevice, MagnetometerSensor};
pub use power::{PowerBackend, PowerDevice, PowerSensor, ScaledPower};
pub use range::{RangeBackend, RangeDevice, RangeSensor};

use std::sync::{Arc, Mutex, MutexGuard};

/// Common trait implemented by all blocking sensor backends.
pub trait SensorBackend: Send {
    /// Human readable device identifier.
    fn name(&self) -> &'static str;
}

/// Thread-safe handle for sharing sensor backends across tasks.
pub struct BackendHandle<B: SensorBackend> {
    inner: Arc<Mutex<B>>,
}

impl<B: SensorBackend> BackendHandle<B> {
    #[inline]
    pub fn new(backend: B) -> Self {
        Self { inner: Arc::new(Mutex::new(backend)) }
    }

    #[inline]
    pub fn lock(&self) -> MutexGuard<'_, B> {
        self.inner.lock().expect("sensor backend poisoned")
    }
}

impl<B: SensorBackend> Clone for BackendHandle<B> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}
