use linux_embedded_hal::I2CError;
use serde::{Deserialize, Serialize};
use tracing::info;
use utoipa::ToSchema;

use crate::{Accelerometer, AccelerometerBackend, Error, Gyro, GyroBackend, I2c, Result, SensorBackend};

pub struct Icm42688p<T: I2c<Error = I2CError> + Send> {
    dev: T,
    config: IcmConfig,
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

    pub fn variants() -> Vec<Self> {
        vec![Self::Dps250, Self::Dps500, Self::Dps1000, Self::Dps2000]
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

    pub fn variants() -> Vec<Self> {
        vec![Self::G2, Self::G4, Self::G8, Self::G16]
    }

    pub fn max_g(self) -> f32 {
        self.g()
    }
}

/// Configuration options applied during initialization.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct IcmConfig {
    pub gyro: GyroRange,
    pub accel: AccelRange,
}

impl Default for IcmConfig {
    fn default() -> Self {
        Self { gyro: GyroRange::Dps2000, accel: AccelRange::G16 }
    }
}

impl IcmConfig {
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
        Self { gyro, accel }
    }
}

impl<T> Icm42688p<T>
where
    T: I2c<Error = I2CError> + Send,
{
    const WHO_AM_I: u8 = 0x75;
    const EXPECTED_ID: u8 = 0x47;
    const ADDRESS: u8 = 0x68;

    const PWR_MGMT0: u8 = 0x4E;
    const GYRO_CONFIG0: u8 = 0x4F;
    const ACCEL_CONFIG0: u8 = 0x50;

    pub fn new(dev: T) -> Result<Self> {
        Self::new_with_config(dev, IcmConfig::default())
    }

    /// Creates a new device with the provided configuration.
    pub fn new_with_config(mut dev: T, config: IcmConfig) -> Result<Self> {
        let mut buf = [0u8];
        dev.write_read(Self::ADDRESS, &[Self::WHO_AM_I], &mut buf).map_err(Error::I2c)?;
        if buf[0] != Self::EXPECTED_ID {
            return Err(Error::InvalidId(buf[0]));
        }
        info!("ICM-42688P detected with id 0x{:X}", buf[0]);
        let mut this = Self { dev, config };
        this.configure(config)?;
        Ok(this)
    }

    /// Returns the current configuration.
    pub fn config(&self) -> IcmConfig {
        self.config
    }

    /// Applies configuration registers.
    pub fn configure(&mut self, config: IcmConfig) -> Result<()> {
        let gyro_bits = match config.gyro {
            GyroRange::Dps2000 => 0b00,
            GyroRange::Dps1000 => 0b01,
            GyroRange::Dps500 => 0b10,
            GyroRange::Dps250 => 0b11,
        };
        let accel_bits = match config.accel {
            AccelRange::G16 => 0b00,
            AccelRange::G8 => 0b01,
            AccelRange::G4 => 0b10,
            AccelRange::G2 => 0b11,
        };

        // 1kHz ODR = 0b0110 << 3
        let gyro_cfg = (0b0110 << 3) | gyro_bits;
        let accel_cfg = (0b0110 << 3) | accel_bits;

        self.dev.write(Self::ADDRESS, &[Self::PWR_MGMT0, 0x0F]).map_err(Error::I2c)?;
        self.dev.write(Self::ADDRESS, &[Self::GYRO_CONFIG0, gyro_cfg]).map_err(Error::I2c)?;
        self.dev.write(Self::ADDRESS, &[Self::ACCEL_CONFIG0, accel_cfg]).map_err(Error::I2c)?;
        self.config = config;
        Ok(())
    }
}

impl<T> Gyro for Icm42688p<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn read_gyro(&mut self) -> Result<[f32; 3]> {
        let mut buf = [0u8; 6];
        // starting gyroscope registers GYRO_DATA_X1 = 0x25
        self.dev.write_read(Self::ADDRESS, &[0x25], &mut buf).map_err(Error::I2c)?;
        let gx = i16::from_be_bytes([buf[0], buf[1]]) as f32 * self.config.gyro.scale();
        let gy = i16::from_be_bytes([buf[2], buf[3]]) as f32 * self.config.gyro.scale();
        let gz = i16::from_be_bytes([buf[4], buf[5]]) as f32 * self.config.gyro.scale();
        Ok([gx, gy, gz])
    }
}

impl<T> SensorBackend for Icm42688p<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn name(&self) -> &'static str {
        "ICM-42688P"
    }
}

impl<T> GyroBackend for Icm42688p<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn max_dps(&self) -> Option<f32> {
        Some(self.config.gyro.max_dps())
    }
}

impl<T> Accelerometer for Icm42688p<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn read_accel(&mut self) -> Result<[f32; 3]> {
        let mut buf = [0u8; 6];
        // starting accelerometer registers ACCEL_DATA_X1 = 0x1F
        self.dev.write_read(Self::ADDRESS, &[0x1F], &mut buf).map_err(Error::I2c)?;
        let ax = i16::from_be_bytes([buf[0], buf[1]]) as f32 * self.config.accel.scale();
        let ay = i16::from_be_bytes([buf[2], buf[3]]) as f32 * self.config.accel.scale();
        let az = i16::from_be_bytes([buf[4], buf[5]]) as f32 * self.config.accel.scale();
        Ok([ax, ay, az])
    }
}

impl<T> AccelerometerBackend for Icm42688p<T>
where
    T: I2c<Error = I2CError> + Send,
{
    fn max_g(&self) -> Option<f32> {
        Some(self.config.accel.max_g())
    }
}
