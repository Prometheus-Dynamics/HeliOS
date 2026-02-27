use thiserror::Error;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    I2c(#[from] linux_embedded_hal::I2CError),
    #[error("invalid sensor id: {0}")]
    InvalidId(u8),
    #[error("invalid device id: {0}")]
    InvalidDevice(u16),
    #[error("{0} data not ready")]
    DataNotReady(&'static str),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}
