use linux_embedded_hal::I2CError;

use crate::backends::RangeBackend;
use crate::{Error, I2c, Result, SensorBackend};

pub struct Vl53l1x<T: I2c<Error = I2CError> + Send> {
    dev: T,
    max_range_mm: u16,
}

impl<T> Vl53l1x<T>
where
    T: I2c<Error = I2CError> + Send,
{
    const ADDRESS: u8 = 0x29;
    const REG_IDENTIFICATION_MODEL_ID: u16 = 0x010F;
    const REG_GPIO_STATUS: u16 = 0x0031;
    const REG_SYSTEM_INTERRUPT_CLEAR: u16 = 0x0086;
    const REG_SYSTEM_MODE_START: u16 = 0x0087;
    const REG_RESULT_DISTANCE: u16 = 0x0096;

    pub fn new(dev: T) -> Result<Self> {
        let mut sensor = Self { dev, max_range_mm: 4000 };
        sensor.verify_device()?;
        sensor.start_ranging()?;
        Ok(sensor)
    }

    fn verify_device(&mut self) -> Result<()> {
        let id = self.read_u16(Self::REG_IDENTIFICATION_MODEL_ID)?;
        if id == 0 {
            return Err(Error::InvalidDevice(id));
        }
        Ok(())
    }

    fn start_ranging(&mut self) -> Result<()> {
        self.write_u8(Self::REG_SYSTEM_MODE_START, 0x40)
    }

    fn write_u8(&mut self, reg: u16, value: u8) -> Result<()> {
        let buffer = [((reg >> 8) & 0xFF) as u8, (reg & 0xFF) as u8, value];
        self.dev.write(Self::ADDRESS, &buffer).map_err(Error::I2c)
    }

    fn read_u16(&mut self, reg: u16) -> Result<u16> {
        let mut buf = [0u8; 2];
        let addr = [((reg >> 8) & 0xFF) as u8, (reg & 0xFF) as u8];
        self.dev.write_read(Self::ADDRESS, &addr, &mut buf).map_err(Error::I2c)?;
        Ok(u16::from_be_bytes(buf))
    }

    fn data_ready(&mut self) -> Result<bool> {
        let status = self.read_u16(Self::REG_GPIO_STATUS)?;
        Ok((status & 0x0001) != 0)
    }

    fn clear_interrupt(&mut self) -> Result<()> {
        self.write_u8(Self::REG_SYSTEM_INTERRUPT_CLEAR, 0x01)
    }
}

impl<T> SensorBackend for Vl53l1x<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn name(&self) -> &'static str {
        "VL53L1X"
    }
}

impl<T> RangeBackend for Vl53l1x<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn distance_mm(&mut self) -> Result<u16> {
        if !self.data_ready()? {
            return Err(Error::DataNotReady("vl53l1x"));
        }
        let distance = self.read_u16(Self::REG_RESULT_DISTANCE)?;
        self.clear_interrupt()?;
        Ok(distance)
    }

    fn max_range_mm(&self) -> u16 {
        self.max_range_mm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct MockI2c {
        responses: VecDeque<(Vec<u8>, [u8; 2])>,
        writes: Vec<(u8, Vec<u8>)>,
    }

    impl MockI2c {
        fn new(responses: Vec<(Vec<u8>, [u8; 2])>) -> Self {
            Self { responses: responses.into(), writes: Vec::new() }
        }
    }

    impl I2c for MockI2c {
        type Error = I2CError;

        fn write(&mut self, addr: u8, bytes: &[u8]) -> core::result::Result<(), Self::Error> {
            self.writes.push((addr, bytes.to_vec()));
            Ok(())
        }

        fn write_read(&mut self, addr: u8, bytes: &[u8], buffer: &mut [u8]) -> core::result::Result<(), Self::Error> {
            assert_eq!(addr, Vl53l1x::<MockI2c>::ADDRESS);
            let (expected, data) = self.responses.pop_front().expect("unexpected read");
            assert_eq!(expected, bytes);
            buffer.copy_from_slice(&data);
            Ok(())
        }
    }

    #[test]
    fn reads_distance_when_ready() {
        let responses = vec![(vec![0x01, 0x0F], 0xEACCu16.to_be_bytes()), (vec![0x00, 0x31], 0x0001u16.to_be_bytes()), (vec![0x00, 0x96], 0x012Cu16.to_be_bytes())];
        let mut sensor = Vl53l1x::new(MockI2c::new(responses)).expect("init ok");
        let distance = sensor.distance_mm().unwrap();
        assert_eq!(distance, 300);
    }

    #[test]
    fn returns_not_ready_error() {
        let responses = vec![(vec![0x01, 0x0F], 0xEACCu16.to_be_bytes()), (vec![0x00, 0x31], 0x0000u16.to_be_bytes())];
        let mut sensor = Vl53l1x::new(MockI2c::new(responses)).expect("init ok");
        let err = sensor.distance_mm().unwrap_err();
        assert!(matches!(err, Error::DataNotReady(_)));
    }
}
