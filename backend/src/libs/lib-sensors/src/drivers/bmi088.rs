#[cfg(not(test))]
use std::thread;
#[cfg(not(test))]
use std::time::Duration;

use linux_embedded_hal::I2CError;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use utoipa::ToSchema;

use crate::{Accelerometer, AccelerometerBackend, Error, Gyro, GyroBackend, I2c, Result, SensorBackend};

const ACC_DEFAULT_ADDRESS: u8 = 0x18;
pub const ACC_ALTERNATE_ADDRESS: u8 = 0x19;
const GYR_DEFAULT_ADDRESS: u8 = 0x68;
pub const GYR_ALTERNATE_ADDRESS: u8 = 0x69;

const ACC_CHIP_ID_REG: u8 = 0x00;
const ACC_EXPECTED_ID: u8 = 0x1E;
const GYR_CHIP_ID_REG: u8 = 0x00;
const GYR_EXPECTED_ID: u8 = 0x0F;

const ACC_SOFTRESET_REG: u8 = 0x7E;
const GYR_SOFTRESET_REG: u8 = 0x14;
const SOFTRESET_CMD: u8 = 0xB6;

const ACC_PWR_CONF: u8 = 0x7C;
const ACC_PWR_CTRL: u8 = 0x7D;
const ACC_CONF: u8 = 0x40;
const ACC_RANGE: u8 = 0x41;
const ACC_PWR_CONF_ACTIVE: u8 = 0x00;
const ACC_PWR_CTRL_ENABLE: u8 = 0x04;

const GYR_RANGE: u8 = 0x0F;
const GYR_BANDWIDTH: u8 = 0x10;
const GYR_LPM1: u8 = 0x11;
const GYR_POWER_NORMAL: u8 = 0x00;

const ACC_DATA_START: u8 = 0x12;
const GYR_DATA_START: u8 = 0x02;

fn delay_ms(ms: u64) {
    #[cfg(not(test))]
    {
        thread::sleep(Duration::from_millis(ms));
    }
    #[cfg(test)]
    {
        let _ = ms;
    }
}

/// Gyroscope full scale range in degrees per second.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub enum GyroRange {
    Dps125,
    Dps250,
    Dps500,
    Dps1000,
    Dps2000,
}

impl GyroRange {
    fn dps(self) -> f32 {
        match self {
            GyroRange::Dps125 => 125.0,
            GyroRange::Dps250 => 250.0,
            GyroRange::Dps500 => 500.0,
            GyroRange::Dps1000 => 1000.0,
            GyroRange::Dps2000 => 2000.0,
        }
    }

    fn scale(self) -> f32 {
        self.dps() / 32768.0
    }

    fn reg_value(self) -> u8 {
        match self {
            GyroRange::Dps2000 => 0x00,
            GyroRange::Dps1000 => 0x01,
            GyroRange::Dps500 => 0x02,
            GyroRange::Dps250 => 0x03,
            GyroRange::Dps125 => 0x04,
        }
    }

    pub fn max_dps(self) -> f32 {
        self.dps()
    }
}

/// Gyroscope bandwidth configuration. Each variant encodes both ODR and filter bandwidth.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub enum GyroBandwidth {
    Odr2000Hz230,
    Odr1000Hz116,
    Odr400Hz47,
    Odr200Hz23,
    Odr100Hz12,
    Odr50Hz6,
    Odr25Hz3,
    Odr12_5Hz1_5,
}

impl GyroBandwidth {
    fn reg_value(self) -> u8 {
        match self {
            GyroBandwidth::Odr2000Hz230 => 0x01,
            GyroBandwidth::Odr1000Hz116 => 0x02,
            GyroBandwidth::Odr400Hz47 => 0x03,
            GyroBandwidth::Odr200Hz23 => 0x04,
            GyroBandwidth::Odr100Hz12 => 0x05,
            GyroBandwidth::Odr50Hz6 => 0x06,
            GyroBandwidth::Odr25Hz3 => 0x07,
            GyroBandwidth::Odr12_5Hz1_5 => 0x08,
        }
    }

    pub const fn hz(self) -> f32 {
        match self {
            GyroBandwidth::Odr2000Hz230 => 2000.0,
            GyroBandwidth::Odr1000Hz116 => 1000.0,
            GyroBandwidth::Odr400Hz47 => 400.0,
            GyroBandwidth::Odr200Hz23 => 200.0,
            GyroBandwidth::Odr100Hz12 => 100.0,
            GyroBandwidth::Odr50Hz6 => 50.0,
            GyroBandwidth::Odr25Hz3 => 25.0,
            GyroBandwidth::Odr12_5Hz1_5 => 12.5,
        }
    }
}

/// Accelerometer full scale range in g.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub enum AccelRange {
    G3,
    G6,
    G12,
    G24,
}

impl AccelRange {
    fn g(self) -> f32 {
        match self {
            AccelRange::G3 => 3.0,
            AccelRange::G6 => 6.0,
            AccelRange::G12 => 12.0,
            AccelRange::G24 => 24.0,
        }
    }

    fn scale(self) -> f32 {
        self.g() / 32768.0
    }

    fn reg_value(self) -> u8 {
        match self {
            AccelRange::G3 => 0x00,
            AccelRange::G6 => 0x01,
            AccelRange::G12 => 0x02,
            AccelRange::G24 => 0x03,
        }
    }

    pub fn max_g(self) -> f32 {
        self.g()
    }
}

/// Accelerometer output data rate selections.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub enum AccelOdr {
    Hz12_5,
    Hz25,
    Hz50,
    Hz100,
    Hz200,
    Hz400,
    Hz800,
    Hz1600,
}

impl AccelOdr {
    fn reg_value(self) -> u8 {
        match self {
            AccelOdr::Hz12_5 => 0x05,
            AccelOdr::Hz25 => 0x06,
            AccelOdr::Hz50 => 0x07,
            AccelOdr::Hz100 => 0x08,
            AccelOdr::Hz200 => 0x09,
            AccelOdr::Hz400 => 0x0A,
            AccelOdr::Hz800 => 0x0B,
            AccelOdr::Hz1600 => 0x0C,
        }
    }

    pub const fn hz(self) -> f32 {
        match self {
            AccelOdr::Hz12_5 => 12.5,
            AccelOdr::Hz25 => 25.0,
            AccelOdr::Hz50 => 50.0,
            AccelOdr::Hz100 => 100.0,
            AccelOdr::Hz200 => 200.0,
            AccelOdr::Hz400 => 400.0,
            AccelOdr::Hz800 => 800.0,
            AccelOdr::Hz1600 => 1600.0,
        }
    }
}

/// Accelerometer bandwidth filter configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub enum AccelBandwidth {
    Osr4,
    Osr2,
    Normal,
}

impl AccelBandwidth {
    fn reg_value(self) -> u8 {
        match self {
            AccelBandwidth::Osr4 => 0x08,
            AccelBandwidth::Osr2 => 0x09,
            AccelBandwidth::Normal => 0x0A,
        }
    }
}

/// Runtime configuration for the BMI088 sensors.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct BmiSettings {
    pub gyro_range: GyroRange,
    pub gyro_bandwidth: GyroBandwidth,
    pub accel_range: AccelRange,
    pub accel_odr: AccelOdr,
    pub accel_bandwidth: AccelBandwidth,
}

impl Default for BmiSettings {
    fn default() -> Self {
        Self { gyro_range: GyroRange::Dps2000, gyro_bandwidth: GyroBandwidth::Odr1000Hz116, accel_range: AccelRange::G24, accel_odr: AccelOdr::Hz800, accel_bandwidth: AccelBandwidth::Normal }
    }
}

/// Configuration including sensor addresses and runtime settings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct BmiConfig {
    pub accel_address: u8,
    pub gyro_address: u8,
    pub settings: BmiSettings,
}

impl Default for BmiConfig {
    fn default() -> Self {
        Self { accel_address: ACC_DEFAULT_ADDRESS, gyro_address: GYR_DEFAULT_ADDRESS, settings: BmiSettings::default() }
    }
}

pub struct Bmi088<T: I2c<Error = I2CError> + Send> {
    dev: T,
    accel_addr: u8,
    gyro_addr: u8,
    settings: BmiSettings,
}

impl<T> Bmi088<T>
where
    T: I2c<Error = I2CError> + Send,
{
    pub fn new(dev: T) -> Result<Self> {
        Self::new_with_config(dev, BmiConfig::default())
    }

    pub fn new_with_config(mut dev: T, config: BmiConfig) -> Result<Self> {
        let mut buf = [0u8];
        dev.write_read(config.accel_address, &[ACC_CHIP_ID_REG], &mut buf).map_err(Error::I2c)?;
        if buf[0] != ACC_EXPECTED_ID {
            return Err(Error::InvalidId(buf[0]));
        }

        dev.write_read(config.gyro_address, &[GYR_CHIP_ID_REG], &mut buf).map_err(Error::I2c)?;
        if buf[0] != GYR_EXPECTED_ID {
            return Err(Error::InvalidId(buf[0]));
        }

        let mut this = Self { dev, accel_addr: config.accel_address, gyro_addr: config.gyro_address, settings: config.settings };
        this.reset()?;
        this.apply_settings(config.settings)?;
        info!("BMI088 detected at accel 0x{:02X}, gyro 0x{:02X}", this.accel_addr, this.gyro_addr);
        Ok(this)
    }

    fn reset(&mut self) -> Result<()> {
        // Some firmware/bridge configurations reject soft-reset writes (EIO), while still
        // supporting normal register configuration. Treat soft-reset as best-effort.
        if let Err(err) = self.dev.write(self.accel_addr, &[ACC_SOFTRESET_REG, SOFTRESET_CMD]).map_err(Error::I2c) {
            warn!(error = %err, addr = format_args!("{:#04x}", self.accel_addr), "BMI088 accel soft reset failed; continuing");
        } else {
            delay_ms(2);
        }

        if let Err(err) = self.dev.write(self.gyro_addr, &[GYR_SOFTRESET_REG, SOFTRESET_CMD]).map_err(Error::I2c) {
            warn!(error = %err, addr = format_args!("{:#04x}", self.gyro_addr), "BMI088 gyro soft reset failed; continuing");
        } else {
            delay_ms(30);
        }
        Ok(())
    }

    /// Returns the current runtime settings.
    pub fn settings(&self) -> BmiSettings {
        self.settings
    }

    pub fn configure(&mut self, settings: BmiSettings) -> Result<()> {
        self.apply_settings(settings)
    }

    fn apply_settings(&mut self, settings: BmiSettings) -> Result<()> {
        self.dev.write(self.accel_addr, &[ACC_PWR_CONF, ACC_PWR_CONF_ACTIVE]).map_err(Error::I2c)?;
        self.dev.write(self.accel_addr, &[ACC_PWR_CTRL, ACC_PWR_CTRL_ENABLE]).map_err(Error::I2c)?;
        delay_ms(2);
        self.dev.write(self.accel_addr, &[ACC_RANGE, settings.accel_range.reg_value()]).map_err(Error::I2c)?;
        let acc_conf = (settings.accel_bandwidth.reg_value() << 4) | settings.accel_odr.reg_value();
        self.dev.write(self.accel_addr, &[ACC_CONF, acc_conf]).map_err(Error::I2c)?;

        self.dev.write(self.gyro_addr, &[GYR_RANGE, settings.gyro_range.reg_value()]).map_err(Error::I2c)?;
        self.dev.write(self.gyro_addr, &[GYR_BANDWIDTH, settings.gyro_bandwidth.reg_value()]).map_err(Error::I2c)?;
        self.dev.write(self.gyro_addr, &[GYR_LPM1, GYR_POWER_NORMAL]).map_err(Error::I2c)?;

        self.settings = settings;
        Ok(())
    }

    fn read_accel_raw(&mut self) -> Result<[i16; 3]> {
        let mut buf = [0u8; 6];
        self.dev.write_read(self.accel_addr, &[ACC_DATA_START], &mut buf).map_err(Error::I2c)?;
        Ok([i16::from_le_bytes([buf[0], buf[1]]), i16::from_le_bytes([buf[2], buf[3]]), i16::from_le_bytes([buf[4], buf[5]])])
    }

    fn read_gyro_raw(&mut self) -> Result<[i16; 3]> {
        let mut buf = [0u8; 6];
        self.dev.write_read(self.gyro_addr, &[GYR_DATA_START], &mut buf).map_err(Error::I2c)?;
        Ok([i16::from_le_bytes([buf[0], buf[1]]), i16::from_le_bytes([buf[2], buf[3]]), i16::from_le_bytes([buf[4], buf[5]])])
    }
}

impl<T> Gyro for Bmi088<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn read_gyro(&mut self) -> Result<[f32; 3]> {
        let raw = self.read_gyro_raw()?;
        let scale = self.settings.gyro_range.scale();
        Ok([raw[0] as f32 * scale, raw[1] as f32 * scale, raw[2] as f32 * scale])
    }
}

impl<T> SensorBackend for Bmi088<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn name(&self) -> &'static str {
        "BMI088"
    }
}

impl<T> GyroBackend for Bmi088<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn max_dps(&self) -> Option<f32> {
        Some(self.settings.gyro_range.max_dps())
    }
}

impl<T> Accelerometer for Bmi088<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn read_accel(&mut self) -> Result<[f32; 3]> {
        let raw = self.read_accel_raw()?;
        let scale = self.settings.accel_range.scale();
        Ok([raw[0] as f32 * scale, raw[1] as f32 * scale, raw[2] as f32 * scale])
    }
}

impl<T> AccelerometerBackend for Bmi088<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn max_g(&self) -> Option<f32> {
        Some(self.settings.accel_range.max_g())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug)]
    enum Interaction {
        Write { addr: u8, bytes: Vec<u8> },
        WriteRead { addr: u8, write: Vec<u8>, read: Vec<u8> },
    }

    struct MockI2c {
        interactions: VecDeque<Interaction>,
    }

    impl MockI2c {
        fn new(interactions: Vec<Interaction>) -> Self {
            Self { interactions: interactions.into() }
        }
    }

    impl Drop for MockI2c {
        fn drop(&mut self) {
            if !self.interactions.is_empty() {
                panic!("unconsumed interactions: {}", self.interactions.len());
            }
        }
    }

    impl I2c for MockI2c {
        type Error = I2CError;

        fn write(&mut self, addr: u8, bytes: &[u8]) -> core::result::Result<(), Self::Error> {
            match self.interactions.pop_front() {
                Some(Interaction::Write { addr: expected_addr, bytes: expected }) => {
                    assert_eq!(addr, expected_addr);
                    assert_eq!(bytes, expected.as_slice());
                    Ok(())
                }
                other => panic!("unexpected write: addr={addr:#X}, bytes={bytes:?}, expected={other:?}"),
            }
        }

        fn write_read(&mut self, addr: u8, bytes: &[u8], buffer: &mut [u8]) -> core::result::Result<(), Self::Error> {
            match self.interactions.pop_front() {
                Some(Interaction::WriteRead { addr: expected_addr, write, read }) => {
                    assert_eq!(addr, expected_addr);
                    assert_eq!(bytes, write.as_slice());
                    assert_eq!(buffer.len(), read.len());
                    buffer.copy_from_slice(&read);
                    Ok(())
                }
                other => panic!("unexpected write_read: addr={addr:#X}, bytes={bytes:?}, expected={other:?}"),
            }
        }
    }

    fn default_init_sequence(settings: BmiSettings) -> Vec<Interaction> {
        // Keep this consistent with the driver init path (bandwidth in the high nibble, ODR in low).
        let acc_conf = (settings.accel_bandwidth.reg_value() << 4) | settings.accel_odr.reg_value();
        vec![
            Interaction::WriteRead { addr: ACC_DEFAULT_ADDRESS, write: vec![ACC_CHIP_ID_REG], read: vec![ACC_EXPECTED_ID] },
            Interaction::WriteRead { addr: GYR_DEFAULT_ADDRESS, write: vec![GYR_CHIP_ID_REG], read: vec![GYR_EXPECTED_ID] },
            Interaction::Write { addr: ACC_DEFAULT_ADDRESS, bytes: vec![ACC_SOFTRESET_REG, SOFTRESET_CMD] },
            Interaction::Write { addr: GYR_DEFAULT_ADDRESS, bytes: vec![GYR_SOFTRESET_REG, SOFTRESET_CMD] },
            Interaction::Write { addr: ACC_DEFAULT_ADDRESS, bytes: vec![ACC_PWR_CONF, ACC_PWR_CONF_ACTIVE] },
            Interaction::Write { addr: ACC_DEFAULT_ADDRESS, bytes: vec![ACC_PWR_CTRL, ACC_PWR_CTRL_ENABLE] },
            Interaction::Write { addr: ACC_DEFAULT_ADDRESS, bytes: vec![ACC_RANGE, settings.accel_range.reg_value()] },
            Interaction::Write { addr: ACC_DEFAULT_ADDRESS, bytes: vec![ACC_CONF, acc_conf] },
            Interaction::Write { addr: GYR_DEFAULT_ADDRESS, bytes: vec![GYR_RANGE, settings.gyro_range.reg_value()] },
            Interaction::Write { addr: GYR_DEFAULT_ADDRESS, bytes: vec![GYR_BANDWIDTH, settings.gyro_bandwidth.reg_value()] },
            Interaction::Write { addr: GYR_DEFAULT_ADDRESS, bytes: vec![GYR_LPM1, GYR_POWER_NORMAL] },
        ]
    }

    #[test]
    fn initializes_and_reads_scaled_values() {
        let settings = BmiSettings::default();
        let mut interactions = default_init_sequence(settings);
        interactions.push(Interaction::WriteRead { addr: ACC_DEFAULT_ADDRESS, write: vec![ACC_DATA_START], read: vec![0x00, 0x08, 0x00, 0xFC, 0x00, 0x10] });
        interactions.push(Interaction::WriteRead { addr: GYR_DEFAULT_ADDRESS, write: vec![GYR_DATA_START], read: vec![0x00, 0x02, 0x00, 0xFF, 0x00, 0x04] });
        let mock = MockI2c::new(interactions);
        let mut imu = Bmi088::new(mock).expect("init ok");
        let accel = imu.read_accel().expect("accel read ok");
        let gyro = imu.read_gyro().expect("gyro read ok");
        assert!((accel[0] - 1.5).abs() < 1e-6);
        assert!((accel[1] + 0.75).abs() < 1e-6);
        assert!((accel[2] - 3.0).abs() < 1e-6);
        assert!((gyro[0] - 31.25).abs() < 1e-2);
        assert!((gyro[1] + 15.625).abs() < 1e-3);
        assert!((gyro[2] - 62.5).abs() < 1e-2);
    }

    #[test]
    fn returns_error_on_invalid_accel_id() {
        let interactions = vec![Interaction::WriteRead { addr: ACC_DEFAULT_ADDRESS, write: vec![ACC_CHIP_ID_REG], read: vec![0xFF] }];
        let mock = MockI2c::new(interactions);
        match Bmi088::new(mock) {
            Err(Error::InvalidId(id)) => assert_eq!(id, 0xFF),
            Err(other) => panic!("unexpected error: {other:?}"),
            Ok(_) => panic!("expected invalid id error"),
        }
    }

    #[test]
    fn reconfigure_writes_new_settings() {
        let mut interactions = default_init_sequence(BmiSettings::default());
        let new_settings =
            BmiSettings { gyro_range: GyroRange::Dps500, gyro_bandwidth: GyroBandwidth::Odr200Hz23, accel_range: AccelRange::G12, accel_odr: AccelOdr::Hz200, accel_bandwidth: AccelBandwidth::Osr2 };
        // Keep this consistent with the driver init path (bandwidth in the high nibble, ODR in low).
        let acc_conf = (new_settings.accel_bandwidth.reg_value() << 4) | new_settings.accel_odr.reg_value();
        interactions.extend([
            Interaction::WriteRead { addr: ACC_DEFAULT_ADDRESS, write: vec![ACC_DATA_START], read: vec![0; 6] },
            Interaction::WriteRead { addr: GYR_DEFAULT_ADDRESS, write: vec![GYR_DATA_START], read: vec![0; 6] },
        ]);
        // Reconfigure sequence
        interactions.push(Interaction::Write { addr: ACC_DEFAULT_ADDRESS, bytes: vec![ACC_PWR_CONF, ACC_PWR_CONF_ACTIVE] });
        interactions.push(Interaction::Write { addr: ACC_DEFAULT_ADDRESS, bytes: vec![ACC_PWR_CTRL, ACC_PWR_CTRL_ENABLE] });
        interactions.push(Interaction::Write { addr: ACC_DEFAULT_ADDRESS, bytes: vec![ACC_RANGE, new_settings.accel_range.reg_value()] });
        interactions.push(Interaction::Write { addr: ACC_DEFAULT_ADDRESS, bytes: vec![ACC_CONF, acc_conf] });
        interactions.push(Interaction::Write { addr: GYR_DEFAULT_ADDRESS, bytes: vec![GYR_RANGE, new_settings.gyro_range.reg_value()] });
        interactions.push(Interaction::Write { addr: GYR_DEFAULT_ADDRESS, bytes: vec![GYR_BANDWIDTH, new_settings.gyro_bandwidth.reg_value()] });
        interactions.push(Interaction::Write { addr: GYR_DEFAULT_ADDRESS, bytes: vec![GYR_LPM1, GYR_POWER_NORMAL] });

        let mock = MockI2c::new(interactions);
        let mut imu = Bmi088::new(mock).expect("init ok");
        imu.read_accel().expect("drain init accel read");
        imu.read_gyro().expect("drain init gyro read");
        imu.configure(new_settings).expect("reconfigure ok");
    }
}
