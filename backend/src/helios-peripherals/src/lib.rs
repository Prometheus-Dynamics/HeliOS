//! Sensor service abstractions and IPC protocol definitions.

#[cfg(feature = "runtime")]
mod ai;
#[cfg(feature = "runtime")]
pub mod config;
#[cfg(feature = "runtime")]
mod config_store;
#[cfg(feature = "dto")]
pub mod dto;
#[cfg(feature = "runtime")]
mod fan;
#[cfg(feature = "runtime")]
pub mod imu;
#[cfg(feature = "runtime")]
pub mod inventory;
#[cfg(feature = "dto")]
pub mod ipc;
#[cfg(feature = "runtime")]
mod lighting;
#[cfg(feature = "runtime")]
pub mod mappers;
#[cfg(feature = "runtime")]
mod power;
#[cfg(feature = "runtime")]
pub mod runtime;
#[cfg(feature = "runtime")]
pub mod service;
#[cfg(feature = "runtime")]
mod usb_proxy;

#[cfg(feature = "runtime")]
mod error;
#[cfg(feature = "runtime")]
pub mod orchestrator;

#[cfg(feature = "runtime")]
pub use config::SensorsConfig;
#[cfg(feature = "dto")]
pub use dto::{AiModelDescriptor, AiModelHealth, AiModelHealthStatus, AiModelInventory, AiModelUpload, SensorData, SensorDescriptor, SensorInventory, SensorKind, SensorScope};
#[cfg(feature = "dto")]
pub use dto::{I2cBusInfo, I2cDeviceInfo, I2cInventory};
#[cfg(feature = "runtime")]
pub use error::{Error, Result};
#[cfg(feature = "runtime")]
pub use imu::{ImuFusionMethod, ImuRange};
#[cfg(feature = "dto")]
pub use ipc::{SensorCommand, SensorEvent};
#[cfg(feature = "runtime")]
pub use runtime::SensorsRuntime;
#[cfg(feature = "runtime")]
pub use service::SensorsService;
