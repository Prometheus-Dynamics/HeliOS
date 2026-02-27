use linux_embedded_hal::I2CError;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{Error, I2c, PowerBackend, Result, SensorBackend};

/// Driver for the INA238 current, voltage and power monitor.
pub struct Ina238<T: I2c<Error = I2CError> + Send> {
    dev: T,
    addr: u8,
    shunt_resistance: f32,
}

/// Configuration parameters for the INA238.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct Ina238Config {
    /// I²C address of the device.
    pub address: u8,
    /// Shunt resistor value in ohms.
    pub shunt_resistance: f32,
}

impl Default for Ina238Config {
    fn default() -> Self {
        Self { address: 0x40, shunt_resistance: 0.003 }
    }
}

impl<T> Ina238<T>
where
    T: I2c<Error = I2CError> + Send,
{
    const REG_CONFIG: u8 = 0x00;
    const REG_ADC_CONFIG: u8 = 0x01;
    const REG_SHUNT_CALIBRATION: u8 = 0x02;
    const REG_SHUNT_VOLTAGE: u8 = 0x04;
    const REG_BUS_VOLTAGE: u8 = 0x05;
    const REG_DIE_TEMP: u8 = 0x06;
    const REG_CURRENT: u8 = 0x07;
    const REG_POWER: u8 = 0x08;
    const REG_DIAG_ALERT: u8 = 0x0B;

    const CALIBRATION_VALUE: u16 = 0x4000;
    const ADC_CONFIG_DEFAULT: u16 = 0xFB6A;
    const DIAG_ALERT_DEFAULT: u16 = 0x2000;
    const GAIN: f32 = 4.0;

    /// Creates a new device with default configuration.
    pub fn new(dev: T) -> Result<Self> {
        Self::new_with_config(dev, Ina238Config::default())
    }

    /// Creates a new device with the provided configuration.
    pub fn new_with_config(mut dev: T, config: Ina238Config) -> Result<Self> {
        dev.write(config.address, &[Self::REG_CONFIG, 0x00, 0x00]).map_err(Error::I2c)?;
        dev.write(config.address, &[Self::REG_ADC_CONFIG, (Self::ADC_CONFIG_DEFAULT >> 8) as u8, Self::ADC_CONFIG_DEFAULT as u8]).map_err(Error::I2c)?;
        dev.write(config.address, &[Self::REG_SHUNT_CALIBRATION, (Self::CALIBRATION_VALUE >> 8) as u8, Self::CALIBRATION_VALUE as u8]).map_err(Error::I2c)?;
        dev.write(config.address, &[Self::REG_DIAG_ALERT, (Self::DIAG_ALERT_DEFAULT >> 8) as u8, Self::DIAG_ALERT_DEFAULT as u8]).map_err(Error::I2c)?;

        Ok(Self { dev, addr: config.address, shunt_resistance: config.shunt_resistance })
    }

    fn read_u16(&mut self, reg: u8) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.dev.write_read(self.addr, &[reg], &mut buf).map_err(Error::I2c)?;
        Ok(u16::from_be_bytes(buf))
    }

    fn read_u24(&mut self, reg: u8) -> Result<u32> {
        let mut buf = [0u8; 3];
        self.dev.write_read(self.addr, &[reg], &mut buf).map_err(Error::I2c)?;
        Ok(((buf[0] as u32) << 16) | ((buf[1] as u32) << 8) | buf[2] as u32)
    }

    /// Reads the bus voltage in volts.
    pub fn bus_voltage(&mut self) -> Result<f32> {
        let raw = self.read_u16(Self::REG_BUS_VOLTAGE)?;
        Ok(raw as f32 * 3.125e-3)
    }

    /// Reads the shunt voltage in volts.
    pub fn shunt_voltage(&mut self) -> Result<f32> {
        let raw = self.read_u16(Self::REG_SHUNT_VOLTAGE)?;
        Ok(i16::from_be_bytes(raw.to_be_bytes()) as f32 * 5.0e-6)
    }

    /// Reads the current in amperes.
    pub fn current(&mut self) -> Result<f32> {
        let raw = self.read_u16(Self::REG_CURRENT)?;
        let val = i16::from_be_bytes(raw.to_be_bytes()) as f32;
        Ok(val * Self::GAIN * 5.0e-6 / self.shunt_resistance)
    }

    /// Reads the power in watts.
    pub fn power(&mut self) -> Result<f32> {
        let raw = self.read_u24(Self::REG_POWER)?;
        Ok(raw as f32 * Self::GAIN * 1.0e-6 / self.shunt_resistance)
    }

    /// Reads the die temperature in degrees Celsius.
    pub fn die_temperature(&mut self) -> Result<f32> {
        let raw = self.read_u16(Self::REG_DIE_TEMP)?;
        let val = i16::from_be_bytes(raw.to_be_bytes()) >> 4;
        Ok(val as f32 * 0.125)
    }
}

impl<T> SensorBackend for Ina238<T>
where
    T: crate::I2c<Error = linux_embedded_hal::I2CError> + Send,
{
    fn name(&self) -> &'static str {
        "INA238"
    }
}

impl<T> PowerBackend for Ina238<T>
where
    T: crate::I2c<Error = linux_embedded_hal::I2CError> + Send,
{
    fn bus_voltage(&mut self) -> Result<f32> {
        Ina238::<T>::bus_voltage(self)
    }
    fn shunt_voltage(&mut self) -> Result<f32> {
        Ina238::<T>::shunt_voltage(self)
    }
    fn current(&mut self) -> Result<f32> {
        Ina238::<T>::current(self)
    }
    fn power(&mut self) -> Result<f32> {
        Ina238::<T>::power(self)
    }
}
