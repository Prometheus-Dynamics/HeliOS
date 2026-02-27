use super::{BackendHandle, SensorBackend};
use crate::Result;

/// Power monitoring trait exposed by sensor wrappers.
pub trait PowerSensor: Send + Sync {
    /// Bus voltage (input/common-mode), typically the supply rail.
    fn voltage(&self) -> Result<f32>;
    /// Explicit bus voltage accessor.
    fn bus_voltage(&self) -> Result<f32>;
    /// Shunt voltage (drop across the sense resistor).
    fn shunt_voltage(&self) -> Result<f32>;
    fn current(&self) -> Result<f32>;
    fn power(&self) -> Result<f32>;
}

/// Backend abstraction implemented by concrete power monitor devices.
pub trait PowerBackend: SensorBackend {
    fn bus_voltage(&mut self) -> Result<f32>;
    fn shunt_voltage(&mut self) -> Result<f32>;
    fn current(&mut self) -> Result<f32>;
    fn power(&mut self) -> Result<f32>;
}

/// Generic wrapper turning a blocking power monitor into a thread-safe `Sensor`.
pub struct PowerDevice<D: PowerBackend> {
    backend: BackendHandle<D>,
}

impl<D: PowerBackend> PowerDevice<D> {
    pub fn new(dev: D) -> Self {
        Self { backend: BackendHandle::new(dev) }
    }
}

impl<D: PowerBackend> PowerSensor for PowerDevice<D> {
    fn voltage(&self) -> Result<f32> {
        let mut backend = self.backend.lock();
        backend.bus_voltage()
    }
    fn bus_voltage(&self) -> Result<f32> {
        let mut backend = self.backend.lock();
        backend.bus_voltage()
    }
    fn shunt_voltage(&self) -> Result<f32> {
        let mut backend = self.backend.lock();
        backend.shunt_voltage()
    }
    fn current(&self) -> Result<f32> {
        let mut backend = self.backend.lock();
        backend.current()
    }
    fn power(&self) -> Result<f32> {
        let mut backend = self.backend.lock();
        backend.power()
    }
}

/// Wrapper backend applying an affine transform to the reported bus voltage
/// and recomputing power from (scaled_bus_voltage * current).
pub struct ScaledPower<D: PowerBackend> {
    inner: D,
    bus_scale: f32,
    bus_offset: f32,
}

impl<D: PowerBackend> ScaledPower<D> {
    pub fn new(inner: D, bus_scale: f32, bus_offset: f32) -> Self {
        Self { inner, bus_scale, bus_offset }
    }
}

impl<D: PowerBackend> SensorBackend for ScaledPower<D> {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
}

impl<D: PowerBackend> PowerBackend for ScaledPower<D> {
    fn bus_voltage(&mut self) -> Result<f32> {
        let v = self.inner.bus_voltage()?;
        Ok(v * self.bus_scale + self.bus_offset)
    }
    fn shunt_voltage(&mut self) -> Result<f32> {
        self.inner.shunt_voltage()
    }
    fn current(&mut self) -> Result<f32> {
        self.inner.current()
    }
    fn power(&mut self) -> Result<f32> {
        // Compute using scaled bus voltage for consistency
        let v = self.bus_voltage()?;
        let i = self.inner.current()?;
        Ok(v * i)
    }
}
