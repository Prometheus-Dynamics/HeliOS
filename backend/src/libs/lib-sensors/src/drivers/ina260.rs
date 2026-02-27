use linux_embedded_hal::I2CError;

use crate::{Error, I2c, PowerBackend, Result, SensorBackend};

pub struct Ina260<T: I2c<Error = I2CError> + Send> {
    dev: T,
}

impl<T> Ina260<T>
where
    T: I2c<Error = I2CError> + Send,
{
    const ADDRESS: u8 = 0x40;
    const REG_MANUFACTURER_ID: u8 = 0xFE;
    const REG_DIE_ID: u8 = 0xFF;
    const REG_BUS_VOLTAGE: u8 = 0x02;
    const REG_CURRENT: u8 = 0x01;
    const REG_POWER: u8 = 0x03;

    const MANUFACTURER_ID_TI: u16 = 0x5449; // ASCII "TI"
    const MAX_CURRENT_A: f32 = 15.0;
    const INTERNAL_SHUNT_OHMS: f32 = 0.002;
    const BUS_VOLTAGE_LSB: f32 = 0.00125; // 1.25 mV
    const CURRENT_LSB: f32 = 0.00125; // 1.25 mA
    const POWER_LSB: f32 = 0.01; // 10 mW

    pub fn new(mut dev: T) -> Result<Self> {
        let mut buf = [0u8; 2];
        dev.write_read(Self::ADDRESS, &[Self::REG_MANUFACTURER_ID], &mut buf).map_err(Error::I2c)?;
        let manufacturer = u16::from_be_bytes(buf);
        if manufacturer != Self::MANUFACTURER_ID_TI {
            return Err(Error::InvalidDevice(manufacturer));
        }
        dev.write_read(Self::ADDRESS, &[Self::REG_DIE_ID], &mut buf).map_err(Error::I2c)?;
        Ok(Self { dev })
    }

    fn read_register(&mut self, reg: u8) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.dev.write_read(Self::ADDRESS, &[reg], &mut buf).map_err(Error::I2c)?;
        Ok(u16::from_be_bytes(buf))
    }

    pub fn bus_voltage(&mut self) -> Result<f32> {
        let raw = self.read_register(Self::REG_BUS_VOLTAGE)?;
        Ok(raw as f32 * Self::BUS_VOLTAGE_LSB)
    }

    pub fn current(&mut self) -> Result<f32> {
        let raw = self.read_register(Self::REG_CURRENT)? as i16;
        let current = raw as f32 * Self::CURRENT_LSB;
        Ok(current.clamp(-Self::MAX_CURRENT_A, Self::MAX_CURRENT_A))
    }

    pub fn power(&mut self) -> Result<f32> {
        let raw = self.read_register(Self::REG_POWER)?;
        Ok(raw as f32 * Self::POWER_LSB)
    }

    pub fn shunt_voltage(&mut self) -> Result<f32> {
        let current = self.current()?;
        Ok(current * Self::INTERNAL_SHUNT_OHMS)
    }
}

impl<T> SensorBackend for Ina260<T>
where
    T: crate::I2c<Error = linux_embedded_hal::I2CError> + Send,
{
    fn name(&self) -> &'static str {
        "INA260"
    }
}

impl<T> PowerBackend for Ina260<T>
where
    T: crate::I2c<Error = linux_embedded_hal::I2CError> + Send,
{
    fn bus_voltage(&mut self) -> Result<f32> {
        Ina260::<T>::bus_voltage(self)
    }
    fn shunt_voltage(&mut self) -> Result<f32> {
        Ina260::<T>::shunt_voltage(self)
    }
    fn current(&mut self) -> Result<f32> {
        Ina260::<T>::current(self)
    }
    fn power(&mut self) -> Result<f32> {
        Ina260::<T>::power(self)
    }
}
