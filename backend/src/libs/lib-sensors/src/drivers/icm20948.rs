use linux_embedded_hal::I2CError;
use serde::{Deserialize, Serialize};
use tracing::info;
use utoipa::ToSchema;

use crate::{Accelerometer, AccelerometerBackend, Error, Gyro, GyroBackend, I2c, Result, SensorBackend};

pub struct Icm20948<T: I2c<Error = I2CError> + Send> {
    dev: T,
    addr: u8,
    config: Icm20948Config,
}

/// Gyroscope full scale range in degrees per second.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub enum GyroRange {
    Dps250,
    Dps500,
    Dps1000,
    Dps2000,
}

impl GyroRange {
    fn dps(self) -> f32 {
        match self {
            GyroRange::Dps250 => 250.0,
            GyroRange::Dps500 => 500.0,
            GyroRange::Dps1000 => 1000.0,
            GyroRange::Dps2000 => 2000.0,
        }
    }

    fn scale(self) -> f32 {
        self.dps() / 32768.0
    }

    pub fn max_dps(self) -> f32 {
        self.dps()
    }
}

/// Accelerometer full scale range in g.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub enum AccelRange {
    G2,
    G4,
    G8,
    G16,
}

impl AccelRange {
    fn g(self) -> f32 {
        match self {
            AccelRange::G2 => 2.0,
            AccelRange::G4 => 4.0,
            AccelRange::G8 => 8.0,
            AccelRange::G16 => 16.0,
        }
    }

    fn scale(self) -> f32 {
        self.g() / 32768.0
    }

    pub fn max_g(self) -> f32 {
        self.g()
    }
}

/// Configuration options applied during initialization.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct Icm20948Config {
    pub address: u8,
    pub gyro: GyroRange,
    pub accel: AccelRange,
}

impl Default for Icm20948Config {
    fn default() -> Self {
        Self { address: 0x68, gyro: GyroRange::Dps2000, accel: AccelRange::G16 }
    }
}

impl Icm20948Config {
    /// Creates a configuration from numeric ranges.
    /// Unrecognized values fall back to the closest supported range.
    pub fn new(accel_g: u16, gyro_dps: u16) -> Self {
        let accel = match accel_g {
            2 => AccelRange::G2,
            4 => AccelRange::G4,
            8 => AccelRange::G8,
            _ => AccelRange::G16,
        };
        let gyro = match gyro_dps {
            250 => GyroRange::Dps250,
            500 => GyroRange::Dps500,
            1000 => GyroRange::Dps1000,
            _ => GyroRange::Dps2000,
        };
        Self { address: 0x68, gyro, accel }
    }
}

impl<T> Icm20948<T>
where
    T: I2c<Error = I2CError> + Send,
{
    // Bank 0
    const REG_WHO_AM_I: u8 = 0x00;
    const REG_PWR_MGMT_1: u8 = 0x06;
    const REG_PWR_MGMT_2: u8 = 0x07;
    const REG_BANK_SEL: u8 = 0x7F;
    const REG_ACCEL_XOUT_H: u8 = 0x2D;
    const REG_GYRO_XOUT_H: u8 = 0x33;

    // Bank 2
    const REG_GYRO_CONFIG_1: u8 = 0x01; // bank 2
    const REG_ACCEL_CONFIG: u8 = 0x14; // bank 2

    const WHO_AM_I_VAL: u8 = 0xEA;

    pub fn new(dev: T) -> Result<Self> {
        Self::new_with_config(dev, Icm20948Config::default())
    }

    fn bank_select(&mut self, bank: u8) -> Result<()> {
        // write bank value into REG_BANK_SEL (upper bits)
        self.dev.write(self.addr, &[Self::REG_BANK_SEL, (bank & 0x03) << 4]).map_err(Error::I2c)
    }

    /// Creates a new device with the provided configuration.
    pub fn new_with_config(mut dev: T, config: Icm20948Config) -> Result<Self> {
        // Verify identity
        let mut buf = [0u8];
        dev.write_read(config.address, &[Self::REG_WHO_AM_I], &mut buf).map_err(Error::I2c)?;
        if buf[0] != Self::WHO_AM_I_VAL {
            return Err(Error::InvalidId(buf[0]));
        }
        info!("ICM-20948 detected with id 0x{:X}", buf[0]);
        // Wake up device
        // Auto-select best clock (CLKSEL=1) and ensure SLEEP=0
        dev.write(config.address, &[Self::REG_PWR_MGMT_1, 0x01]).map_err(Error::I2c)?;
        // Ensure accel and gyro are enabled (PWR_MGMT_2 bitmask disables axes when set)
        // 0x00 enables all accel+gyro axes.
        dev.write(config.address, &[Self::REG_PWR_MGMT_2, 0x00]).map_err(Error::I2c)?;
        // Configure ranges in BANK 2
        let mut this = Self { dev, addr: config.address, config };
        this.configure(config)?;
        Ok(this)
    }

    pub fn address(&self) -> u8 {
        self.addr
    }

    /// Applies configuration registers.
    pub fn configure(&mut self, config: Icm20948Config) -> Result<()> {
        self.bank_select(2)?;
        let gyro_bits = match config.gyro {
            GyroRange::Dps2000 => 0b11,
            GyroRange::Dps1000 => 0b10,
            GyroRange::Dps500 => 0b01,
            GyroRange::Dps250 => 0b00,
        };
        let accel_bits = match config.accel {
            AccelRange::G16 => 0b11,
            AccelRange::G8 => 0b10,
            AccelRange::G4 => 0b01,
            AccelRange::G2 => 0b00,
        };
        // Set ODR defaults with range bits
        // GYRO_CONFIG_1: [GYRO_DLPFCFG(3bits)|GYRO_FS_SEL(2bits)|GYRO_FCHOICE(1)|RESERVED(2)]
        let gyro_cfg = (0b001 << 3) | (gyro_bits << 1);
        // ACCEL_CONFIG: [ACCEL_DLPFCFG(3)|ACCEL_FS_SEL(2)|ACCEL_FCHOICE(1)|RESERVED(2)]
        let accel_cfg = (0b001 << 3) | (accel_bits << 1);
        self.dev.write(self.addr, &[Self::REG_GYRO_CONFIG_1, gyro_cfg]).map_err(Error::I2c)?;
        self.dev.write(self.addr, &[Self::REG_ACCEL_CONFIG, accel_cfg]).map_err(Error::I2c)?;
        self.bank_select(0)?;
        self.config = config;
        Ok(())
    }
}

impl<T> Gyro for Icm20948<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn read_gyro(&mut self) -> Result<[f32; 3]> {
        let mut buf = [0u8; 6];
        self.dev.write_read(self.addr, &[Self::REG_GYRO_XOUT_H], &mut buf).map_err(Error::I2c)?;
        let gx = i16::from_be_bytes([buf[0], buf[1]]) as f32 * self.config.gyro.scale();
        let gy = i16::from_be_bytes([buf[2], buf[3]]) as f32 * self.config.gyro.scale();
        let gz = i16::from_be_bytes([buf[4], buf[5]]) as f32 * self.config.gyro.scale();
        Ok([gx, gy, gz])
    }
}

impl<T> SensorBackend for Icm20948<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn name(&self) -> &'static str {
        "ICM-20948"
    }
}

impl<T> GyroBackend for Icm20948<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn max_dps(&self) -> Option<f32> {
        Some(self.config.gyro.max_dps())
    }
}

impl<T> Accelerometer for Icm20948<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn read_accel(&mut self) -> Result<[f32; 3]> {
        let mut buf = [0u8; 6];
        self.dev.write_read(self.addr, &[Self::REG_ACCEL_XOUT_H], &mut buf).map_err(Error::I2c)?;
        let ax = i16::from_be_bytes([buf[0], buf[1]]) as f32 * self.config.accel.scale();
        let ay = i16::from_be_bytes([buf[2], buf[3]]) as f32 * self.config.accel.scale();
        let az = i16::from_be_bytes([buf[4], buf[5]]) as f32 * self.config.accel.scale();
        Ok([ax, ay, az])
    }
}

impl<T> AccelerometerBackend for Icm20948<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn max_g(&self) -> Option<f32> {
        Some(self.config.accel.max_g())
    }
}
