use std::sync::Arc;

use super::{BackendHandle, SensorBackend};
use crate::Result;

/// Blocking magnetometer interface implemented by device drivers.
pub trait Magnetometer: SensorBackend {
    /// Reads the three-axis magnetic field values.
    fn read_mag(&mut self) -> Result<[f32; 3]>;
}

/// Backend abstraction implemented by concrete magnetometer devices.
pub trait MagnetometerBackend: Magnetometer {
    /// Optional magnetic field range (min, max) in microtesla.
    fn range_microtesla(&self) -> Option<(f32, f32)> {
        None
    }
}

/// Thread-safe magnetometer provider used by higher level components.
pub trait MagnetometerSensor: Send + Sync {
    /// Reads the three-axis magnetic field values. Absolute scale is optional.
    fn magnetic_field(&self) -> Result<[f32; 3]>;
    /// Optional magnetic field range (min, max) in microtesla.
    fn range_microtesla(&self) -> Option<(f32, f32)> {
        None
    }
}

struct MagnetometerProvider<B: MagnetometerBackend> {
    backend: BackendHandle<B>,
}

impl<B: MagnetometerBackend> MagnetometerProvider<B> {
    fn new(backend: BackendHandle<B>) -> Self {
        Self { backend }
    }
}

impl<B: MagnetometerBackend + 'static> MagnetometerSensor for MagnetometerProvider<B> {
    fn magnetic_field(&self) -> Result<[f32; 3]> {
        let mut backend = self.backend.lock();
        backend.read_mag()
    }

    fn range_microtesla(&self) -> Option<(f32, f32)> {
        let backend = self.backend.lock();
        backend.range_microtesla()
    }
}

/// Generic wrapper turning a blocking magnetometer into a thread-safe `Sensor`.
pub struct MagnetometerDevice<B: MagnetometerBackend> {
    backend: BackendHandle<B>,
}

impl<B: MagnetometerBackend> MagnetometerDevice<B> {
    pub fn new(backend: B) -> Self {
        Self { backend: BackendHandle::new(backend) }
    }

    /// Returns a thread-safe provider clone for fusion pairing.
    pub fn provider(&self) -> Arc<dyn MagnetometerSensor>
    where
        B: 'static,
    {
        Arc::new(MagnetometerProvider::new(self.backend.clone()))
    }
}

impl<B: MagnetometerBackend> MagnetometerSensor for MagnetometerDevice<B> {
    fn magnetic_field(&self) -> Result<[f32; 3]> {
        let mut backend = self.backend.lock();
        backend.read_mag()
    }

    fn range_microtesla(&self) -> Option<(f32, f32)> {
        let backend = self.backend.lock();
        backend.range_microtesla()
    }
}
