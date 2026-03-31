mod commands;
mod config;
mod connect;
mod session;

#[cfg(test)]
mod tests;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use helios_peripherals::ipc::{SensorCommand, SensorEvent};
use lib_ipc::client::{Client as GenericClient, Session as GenericSession};
use tokio::sync::Mutex;

pub type SensorsClient = GenericClient<config::SensorsClientConfig, SensorCommand, SensorEvent>;
pub type SensorsSession = GenericSession<SensorCommand, SensorEvent>;

pub struct SensorsConnection {
    client: Arc<SensorsClient>,
    sessions: Arc<Mutex<Vec<IdleSession>>>,
    session_idle_timeout: Duration,
}

pub struct SensorsStream {
    pub client: Arc<SensorsClient>,
    pub session: SensorsSession,
}

#[derive(Debug)]
struct IdleSession {
    session: SensorsSession,
    last_used: Instant,
}

pub(crate) use connect::{connect_sensors, connect_sensors_stream};
