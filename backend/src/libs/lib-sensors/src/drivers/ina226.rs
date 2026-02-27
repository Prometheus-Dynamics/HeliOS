use linux_embedded_hal::I2CError;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{Error, I2c, PowerBackend, Result, SensorBackend};

/// Driver for the INA226 current and power monitor.
pub struct Ina226<T: I2c<Error = I2CError> + Send> {
    dev: T,
    addr: u8,
    current_lsb: f32,
}

/// Configuration parameters for the INA226.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct Ina226Config {
    /// I²C address of the device.
    pub address: u8,
    /// Shunt resistor value in ohms.
    pub shunt_resistance: f32,
    /// Maximum expected current in amperes.
    pub max_current: f32,
}

impl Default for Ina226Config {
    fn default() -> Self {
        Self { address: 0x40, shunt_resistance: 0.1, max_current: 10.0 }
    }
}

impl<T> Ina226<T>
where
    T: I2c<Error = I2CError> + Send,
{
    const REG_CONFIG: u8 = 0x00;
    const REG_SHUNT_VOLTAGE: u8 = 0x01;
    const REG_BUS_VOLTAGE: u8 = 0x02;
    const REG_POWER: u8 = 0x03;
    const REG_CURRENT: u8 = 0x04;
    const REG_CALIBRATION: u8 = 0x05;

    /// Creates a new device with default configuration.
    pub fn new(dev: T) -> Result<Self> {
        Self::new_with_config(dev, Ina226Config::default())
    }

    /// Creates a new device with the provided configuration.
    pub fn new_with_config(mut dev: T, config: Ina226Config) -> Result<Self> {
        let current_lsb = config.max_current / 32768.0;
        let calibration = (0.00512 / (current_lsb * config.shunt_resistance)) as u16;
        dev.write(config.address, &[Self::REG_CALIBRATION, (calibration >> 8) as u8, calibration as u8]).map_err(Error::I2c)?;
        // Average 16 samples, 1.1ms conversion time, continuous mode
        dev.write(config.address, &[Self::REG_CONFIG, 0x45, 0x27]).map_err(Error::I2c)?;
        Ok(Self { dev, addr: config.address, current_lsb })
    }

    fn read_u16(&mut self, reg: u8) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.dev.write_read(self.addr, &[reg], &mut buf).map_err(Error::I2c)?;
        Ok(u16::from_be_bytes(buf))
    }

    /// Reads the bus voltage in volts.
    pub fn bus_voltage(&mut self) -> Result<f32> {
        let raw = self.read_u16(Self::REG_BUS_VOLTAGE)?;
        Ok(raw as f32 * 1.25e-3)
    }

    /// Reads the shunt voltage in volts.
    pub fn shunt_voltage(&mut self) -> Result<f32> {
        let raw = self.read_u16(Self::REG_SHUNT_VOLTAGE)?;
        Ok(i16::from_be_bytes(raw.to_be_bytes()) as f32 * 2.5e-6)
    }

    /// Reads the current in amperes.
    pub fn current(&mut self) -> Result<f32> {
        let raw = self.read_u16(Self::REG_CURRENT)?;
        Ok(i16::from_be_bytes(raw.to_be_bytes()) as f32 * self.current_lsb)
    }

    /// Reads the power in watts.
    pub fn power(&mut self) -> Result<f32> {
        let raw = self.read_u16(Self::REG_POWER)?;
        Ok(raw as f32 * self.current_lsb * 25.0)
    }
}

impl<T> SensorBackend for Ina226<T>
where
    T: crate::I2c<Error = linux_embedded_hal::I2CError> + Send,
{
    fn name(&self) -> &'static str {
        "INA226"
    }
}

impl<T> PowerBackend for Ina226<T>
where
    T: crate::I2c<Error = linux_embedded_hal::I2CError> + Send,
{
    fn bus_voltage(&mut self) -> Result<f32> {
        Ina226::<T>::bus_voltage(self)
    }
    fn shunt_voltage(&mut self) -> Result<f32> {
        Ina226::<T>::shunt_voltage(self)
    }
    fn current(&mut self) -> Result<f32> {
        Ina226::<T>::current(self)
    }
    fn power(&mut self) -> Result<f32> {
        Ina226::<T>::power(self)
    }
}
