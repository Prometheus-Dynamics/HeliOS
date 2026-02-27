pub mod backends;
pub mod drivers;
pub mod dto;
pub mod fan_config;
pub mod imu;
pub mod led_config;
pub mod model;
pub mod sensor_config;

pub use backends::{
    BackendHandle, SensorBackend,
    accelerometer::{self, Accelerometer, AccelerometerBackend, AccelerometerDevice, AccelerometerSensor},
    gyro::{self, Gyro, GyroBackend, GyroDevice, GyroSensor},
    magnetometer::{self, Magnetometer, MagnetometerBackend, MagnetometerDevice, MagnetometerSensor},
    power::{self, PowerBackend, PowerDevice, PowerSensor, ScaledPower},
    range::{self, RangeBackend, RangeDevice, RangeSensor},
};

pub use lib_math::filters::{AlphaBetaFilter, ComplementaryFilter, CvKalmanFilter, ExtendedKalmanFilter, Filter, HighPassFilter, KalmanFilter, LowPassFilter, MovingAverageFilter, ParticleFilter};

mod error;
pub use error::{Error, Result};

/// Minimal I²C interface used by this crate.
pub trait I2c {
    /// Error type produced by the implementation.
    type Error;

    /// Writes the provided bytes to the device at `addr`.
    fn write(&mut self, addr: u8, bytes: &[u8]) -> core::result::Result<(), Self::Error>;

    /// Writes the provided bytes to the device at `addr` and then reads data
    /// into `buffer` within the same transaction.
    fn write_read(&mut self, addr: u8, bytes: &[u8], buffer: &mut [u8]) -> core::result::Result<(), Self::Error>;
}

impl I2c for linux_embedded_hal::I2cdev {
    type Error = linux_embedded_hal::I2CError;

    fn write(&mut self, addr: u8, bytes: &[u8]) -> core::result::Result<(), Self::Error> {
        <Self as embedded_hal::i2c::I2c>::write(self, addr, bytes)
    }

    fn write_read(&mut self, addr: u8, bytes: &[u8], buffer: &mut [u8]) -> core::result::Result<(), Self::Error> {
        <Self as embedded_hal::i2c::I2c>::write_read(self, addr, bytes, buffer)
    }
}
