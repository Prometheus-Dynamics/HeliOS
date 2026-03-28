use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use helios_peripherals::dto::{I2cInventory, LightingCommand, LightingRuntimeState, SensorData, SensorInventory, SensorKind, SensorScope, SensorSnapshot};
use helios_peripherals::ipc::{SensorCommand, SensorEvent};
use lib_ipc::client::ClientTransportError;
use lib_ipc::client::{Client as GenericClient, Session as GenericSession, TransportConfig};
use lib_ipc::types::CommandId;
use lib_ipc::types::FeatureSet;
use lib_sensors::fan_config::FanConfig as PeripheralFanConfig;
use lib_sensors::fan_config::FanStatus as PeripheralFanStatus;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::debug;

use crate::ipc::{JOURNAL_DIR, PERIPHERALS_SOCKET, command_id_from_context};

const DEV_PERIPHERALS_SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/dev/run/peripherals.sock");
const SENSOR_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

pub type SensorsClient = GenericClient<SensorsClientConfig, SensorCommand, SensorEvent>;
pub type SensorsSession = GenericSession<SensorCommand, SensorEvent>;

pub struct SensorsConnection {
    client: std::sync::Arc<SensorsClient>,
    sessions: std::sync::Arc<Mutex<Vec<IdleSession>>>,
    session_idle_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct SensorsClientConfig {
    socket_path: PathBuf,
    journal_path: PathBuf,
    protocol: lib_ipc::types::ProtocolVersion,
    client_name: String,
    client_version: String,
    features: FeatureSet,
}

impl SensorsClientConfig {
    pub fn new(socket_path: impl Into<PathBuf>, journal_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            journal_path: journal_path.into(),
            protocol: lib_ipc::types::ProtocolVersion::default(),
            client_name: "helios-peripherals-client".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            features: FeatureSet::default(),
        }
    }
}

impl TransportConfig for SensorsClientConfig {
    fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }
    fn journal_path(&self) -> &std::path::Path {
        &self.journal_path
    }
    fn protocol(&self) -> lib_ipc::types::ProtocolVersion {
        self.protocol
    }
    fn client_name(&self) -> &str {
        &self.client_name
    }
    fn client_version(&self) -> &str {
        &self.client_version
    }
    fn features(&self) -> &FeatureSet {
        &self.features
    }
}

pub async fn connect_sensors() -> Result<SensorsConnection, Box<dyn std::error::Error + Send + Sync>> {
    let journal_path = PathBuf::from(JOURNAL_DIR).join("peripherals.journal");
    let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    for socket in resolve_peripherals_socket_candidates() {
        match try_connect_sensors(socket, journal_path.clone()).await {
            Ok(connection) => return Ok(connection),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| "peripherals IPC connect failed".into()))
}

pub async fn connect_sensors_stream() -> Result<SensorsStream, Box<dyn std::error::Error + Send + Sync>> {
    let journal_path = PathBuf::from(JOURNAL_DIR).join("peripherals.journal");
    let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    for socket in resolve_peripherals_socket_candidates() {
        match try_connect_sensors_stream(socket, journal_path.clone()).await {
            Ok(connection) => return Ok(connection),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| "peripherals IPC connect failed".into()))
}

pub struct SensorsStream {
    pub client: std::sync::Arc<SensorsClient>,
    pub session: SensorsSession,
}

#[derive(Debug)]
struct IdleSession {
    session: SensorsSession,
    last_used: Instant,
}

// Command-style peripherals IPC calls (snapshot, fan status, inventory) do not need a live
// broadcast subscription between requests. Reusing idle sessions keeps the server-side broadcast
// receiver attached after the command completes, which turns periodic API polling into an
// effectively always-subscribed sensor stream.
const DEFAULT_SESSION_IDLE_MS: u64 = 0;
const MAX_SESSION_IDLE_MS: u64 = 2_000;
const MAX_IDLE_SESSIONS: usize = 4;

fn session_idle_timeout() -> Duration {
    let raw = std::env::var("HELIOS_PERIPHERALS_SESSION_IDLE_MS").ok().and_then(|value| value.parse::<u64>().ok()).unwrap_or(DEFAULT_SESSION_IDLE_MS);
    Duration::from_millis(raw).clamp(Duration::from_millis(0), Duration::from_millis(MAX_SESSION_IDLE_MS))
}

async fn try_connect_sensors(socket: PathBuf, journal_path: PathBuf) -> Result<SensorsConnection, Box<dyn std::error::Error + Send + Sync>> {
    let client = std::sync::Arc::new(SensorsClient::new(SensorsClientConfig::new(socket.clone(), journal_path))?);
    let session = timeout(Duration::from_secs(5), client.handshake()).await??;
    debug!(
        socket = %socket.display(),
        "peripherals hello: protocol={}, server={}",
        session.server().protocol,
        session.server().server_name
    );
    // Drop the initial session so idle clients do not hold a broadcast receiver open.
    drop(session);
    Ok(SensorsConnection { client, sessions: std::sync::Arc::new(Mutex::new(Vec::new())), session_idle_timeout: session_idle_timeout() })
}

async fn try_connect_sensors_stream(socket: PathBuf, journal_path: PathBuf) -> Result<SensorsStream, Box<dyn std::error::Error + Send + Sync>> {
    let client = std::sync::Arc::new(SensorsClient::new(SensorsClientConfig::new(socket.clone(), journal_path))?);
    let session = timeout(Duration::from_secs(5), client.handshake()).await??;
    debug!(
        socket = %socket.display(),
        "peripherals stream hello: protocol={}, server={}",
        session.server().protocol,
        session.server().server_name
    );
    Ok(SensorsStream { client, session })
}

fn resolve_peripherals_socket_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for name in ["HELIOS_PERIPHERALS_SOCKET", "PERIPHERALS_SOCKET", "SENSORS_SOCKET", "SENSOR_SOCKET"] {
        if let Ok(value) = std::env::var(name) {
            push_unique(&mut candidates, PathBuf::from(value));
        }
    }
    push_unique(&mut candidates, PathBuf::from(DEV_PERIPHERALS_SOCKET));
    push_unique(&mut candidates, PathBuf::from(PERIPHERALS_SOCKET));
    candidates
}

fn push_unique(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}

async fn await_sensor<T, F>(client: &SensorsClient, session: &mut SensorsSession, command: SensorCommand, mut map: F) -> Result<Result<T, String>, ClientTransportError>
where
    F: FnMut(SensorEvent, CommandId) -> Option<Result<T, String>>,
{
    let command_id = command.command_id();
    session.send_command(client.journal(), &command).await?;
    let deadline = tokio::time::Instant::now() + SENSOR_RESPONSE_TIMEOUT;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(Err("peripheral request timed out".into()));
        }
        let remaining = deadline - now;
        match timeout(remaining, session.next_event()).await {
            Ok(Ok(Some(event))) => {
                if let Some(mapped) = map(event, command_id) {
                    return Ok(mapped);
                }
            }
            Ok(Ok(None)) => return Ok(Err("no response".into())),
            Ok(Err(err)) => return Err(err),
            Err(_) => return Ok(Err("peripheral request timed out".into())),
        }
    }
}

impl SensorsConnection {
    async fn checkout_session(&self) -> Result<SensorsSession, ClientTransportError> {
        let idle_timeout = self.session_idle_timeout;
        if idle_timeout > Duration::from_millis(0) {
            let now = Instant::now();
            let mut guard = self.sessions.lock().await;
            guard.retain(|idle| now.duration_since(idle.last_used) <= idle_timeout);
            if let Some(idle) = guard.pop() {
                return Ok(idle.session);
            }
        }
        match timeout(Duration::from_secs(5), self.client.handshake()).await {
            Ok(Ok(session)) => Ok(session),
            Ok(Err(err)) => Err(ClientTransportError::Io(io::Error::other(err.to_string()))),
            Err(_) => Err(ClientTransportError::Io(io::Error::new(io::ErrorKind::TimedOut, "sensors handshake timed out"))),
        }
    }

    async fn recycle_session(&self, session: SensorsSession) {
        let idle_timeout = self.session_idle_timeout;
        if idle_timeout <= Duration::from_millis(0) {
            return;
        }
        let now = Instant::now();
        let idle = IdleSession { session, last_used: now };
        let mut guard = self.sessions.lock().await;
        guard.retain(|entry| now.duration_since(entry.last_used) <= idle_timeout);
        guard.push(idle);
        if guard.len() > MAX_IDLE_SESSIONS
            && let Some((idx, _)) = guard.iter().enumerate().min_by_key(|(_, entry)| entry.last_used)
        {
            guard.swap_remove(idx);
        }
    }

    async fn run_command<T, F>(&self, command: SensorCommand, map: F) -> Result<Result<T, String>, ClientTransportError>
    where
        F: FnMut(SensorEvent, CommandId) -> Option<Result<T, String>>,
    {
        let mut session = self.checkout_session().await?;
        let result = await_sensor(&self.client, &mut session, command, map).await;
        if result.is_ok() {
            self.recycle_session(session).await;
        }
        result
    }

    pub async fn i2c_inventory(&self) -> Result<Result<I2cInventory, String>, ClientTransportError> {
        let command_id = command_id_from_context("i2c_inventory");
        self.run_command(SensorCommand::I2cInventory { command_id }, |event, command_id| match event {
            SensorEvent::I2cInventory { command_id: event_id, inventory } if event_id == command_id => Some(Ok(inventory)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn inventory(&self) -> Result<Result<SensorInventory, String>, ClientTransportError> {
        let command_id = command_id_from_context("inventory");
        self.run_command(SensorCommand::Inventory { command_id }, |event, command_id| match event {
            SensorEvent::Inventory { command_id: event_id, inventory } if event_id == command_id => Some(Ok(inventory)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn discover(&self, refresh: bool) -> Result<Result<SensorInventory, String>, ClientTransportError> {
        let command_id = command_id_from_context("discover");
        self.run_command(SensorCommand::Discover { command_id, refresh }, |event, command_id| match event {
            SensorEvent::Inventory { command_id: event_id, inventory } if event_id == command_id => Some(Ok(inventory)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn sensor_snapshot(&self, scope: SensorScope) -> Result<Result<SensorSnapshot, String>, ClientTransportError> {
        let command_id = command_id_from_context("snapshot");
        self.run_command(SensorCommand::Snapshot { command_id, scope: scope.clone() }, |event, command_id| match event {
            SensorEvent::Snapshot { command_id: Some(event_id), scope: event_scope, values } if event_id == command_id && event_scope == scope => Some(Ok(values)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn sensor_snapshot_typed(&self, scope: SensorScope) -> Result<Result<helios_peripherals::dto::SensorSnapshotTyped, String>, ClientTransportError> {
        let command_id = command_id_from_context("snapshot_typed");
        self.run_command(SensorCommand::SnapshotTyped { command_id, scope: scope.clone() }, |event, command_id| match event {
            SensorEvent::SnapshotTyped { command_id: Some(event_id), scope: event_scope, values } if event_id == command_id && event_scope == scope => Some(Ok(values)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn fan_status(&self) -> Result<Result<PeripheralFanStatus, String>, ClientTransportError> {
        let command_id = command_id_from_context("fan_status");
        self.run_command(SensorCommand::FanStatus { command_id }, |event, command_id| match event {
            SensorEvent::FanStatus { command_id: event_id, status } if event_id == command_id => Some(Ok(status)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn fan_config(&self) -> Result<Result<PeripheralFanConfig, String>, ClientTransportError> {
        let command_id = command_id_from_context("fan_config");
        self.run_command(SensorCommand::FanConfig { command_id }, |event, command_id| match event {
            SensorEvent::FanConfig { command_id: event_id, config } if event_id == command_id => Some(Ok(config)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn update_fan_config(&self, config: PeripheralFanConfig) -> Result<Result<(), String>, ClientTransportError> {
        let command_id = command_id_from_context("update_fan_config");
        self.run_command(SensorCommand::UpdateFanConfig { command_id, config }, |event, command_id| match event {
            SensorEvent::Ack { command_id: event_id, .. } if event_id == command_id => Some(Ok(())),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn update_sensor(&self, scope: SensorScope, sensor: SensorKind, payload: SensorData) -> Result<Result<SensorSnapshot, String>, ClientTransportError> {
        let command_id = command_id_from_context("update_sensor");
        self.run_command(SensorCommand::Update { command_id, scope: scope.clone(), sensor, payload }, |event, command_id| match event {
            SensorEvent::Snapshot { command_id: Some(event_id), scope: event_scope, values } if event_id == command_id && event_scope == scope => Some(Ok(values)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn lighting_command(&self, command: LightingCommand) -> Result<Result<(), String>, ClientTransportError> {
        let command_id = command_id_from_context("lighting");
        self.run_command(SensorCommand::Lighting { command_id, command }, |event, command_id| match event {
            SensorEvent::Ack { command_id: event_id, .. } if event_id == command_id => Some(Ok(())),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn lighting_state(&self) -> Result<Result<LightingRuntimeState, String>, ClientTransportError> {
        let command_id = command_id_from_context("lighting_state");
        self.run_command(SensorCommand::LightingState { command_id }, |event, command_id| match event {
            SensorEvent::LightingState { command_id: Some(event_id), state } if event_id == command_id => Some(Ok(state)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn configure_firmware(&self, device_id: String, firmware: String) -> Result<Result<(), String>, ClientTransportError> {
        let command_id = command_id_from_context("configure_firmware");
        self.run_command(SensorCommand::ConfigureFirmware { command_id, device_id, firmware }, |event, command_id| match event {
            SensorEvent::Ack { command_id: event_id, .. } if event_id == command_id => Some(Ok(())),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn configure_alias(&self, hardware_key: String, alias: String) -> Result<Result<(), String>, ClientTransportError> {
        let command_id = command_id_from_context("configure_alias");
        self.run_command(SensorCommand::ConfigureAlias { command_id, hardware_key, alias }, |event, command_id| match event {
            SensorEvent::Ack { command_id: event_id, .. } if event_id == command_id => Some(Ok(())),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }
}
