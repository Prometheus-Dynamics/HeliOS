use super::{BackendHandle, SensorBackend};
use crate::Result;

pub trait RangeSensor: Send + Sync {
    fn distance_mm(&self) -> Result<u16>;
    fn max_range_mm(&self) -> u16;
}

pub trait RangeBackend: SensorBackend {
    fn distance_mm(&mut self) -> Result<u16>;
    fn max_range_mm(&self) -> u16;
}

pub struct RangeDevice<B: RangeBackend> {
    backend: BackendHandle<B>,
}

impl<B: RangeBackend> RangeDevice<B> {
    pub fn new(backend: B) -> Self {
        Self { backend: BackendHandle::new(backend) }
    }
}

impl<B: RangeBackend> RangeSensor for RangeDevice<B> {
    fn distance_mm(&self) -> Result<u16> {
        let mut backend = self.backend.lock();
        backend.distance_mm()
    }

    fn max_range_mm(&self) -> u16 {
        let backend = self.backend.lock();
        backend.max_range_mm()
    }
}
