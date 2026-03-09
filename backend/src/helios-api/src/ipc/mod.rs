pub mod engine;
pub mod peripherals;
pub mod updater;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, sync::Arc};

use serde::Serialize;
use tokio::sync::{Mutex, RwLock, broadcast};
use tracing::{error, info, warn};
use uuid::Uuid;

use self::engine::EngineConnection;
use self::peripherals::SensorsConnection;
use self::updater::UpdaterConnection;

/// Default IPC socket locations for each runtime.
pub const PERIPHERALS_SOCKET: &str = "/run/helios/peripherals.sock";
pub const UPDATER_SOCKET: &str = "/run/helios/updater.sock";

/// Journals are stored under /tmp for now; rotate later if needed.
pub const JOURNAL_DIR: &str = "/tmp/helios-ipc";

/// Live handles into each service connection.
pub struct IpcHandles {
    pub engine: EngineConnection,
    sensors: RwLock<Option<Arc<SensorsConnection>>>,
    sensors_connect: Mutex<()>,
    pub updater: Mutex<Option<Arc<UpdaterConnection>>>,
    pub updates: RealtimeUpdateBus,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeUpdateOrigin {
    Http,
    Ws,
}

#[derive(Debug, Clone, Serialize)]
pub struct RealtimeUpdateEvent {
    pub seq: u64,
    pub timestamp_ms: u64,
    pub origin: RealtimeUpdateOrigin,
    pub kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

pub struct RealtimeUpdateBus {
    tx: broadcast::Sender<RealtimeUpdateEvent>,
    seq: AtomicU64,
}

impl Default for RealtimeUpdateBus {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx, seq: AtomicU64::new(0) }
    }
}

impl RealtimeUpdateBus {
    pub fn subscribe(&self) -> broadcast::Receiver<RealtimeUpdateEvent> {
        self.tx.subscribe()
    }

    pub fn publish(&self, origin: RealtimeUpdateOrigin, kind: impl Into<String>, path: impl Into<String>, method: Option<String>, request_id: Option<String>) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let event = RealtimeUpdateEvent { seq, timestamp_ms: now_ms(), origin, kind: kind.into(), path: path.into(), method, request_id };
        let _ = self.tx.send(event);
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u64::MAX as u128) as u64
}

fn is_transient_peripherals_connect_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("no such file or directory") || msg.contains("connection refused") || msg.contains("connection reset") || msg.contains("timed out") || msg.contains("handshake io error")
}

fn log_peripherals_connect_failure(err: &str, phase: &'static str) {
    if is_transient_peripherals_connect_error(err) {
        warn!(error = %err, phase, "peripherals IPC unavailable (transient); will retry");
    } else {
        error!(error = %err, phase, "failed to connect to peripherals IPC");
    }
}

/// Entry point for wiring all IPC endpoints needed by the control plane.
pub async fn connect_all() -> IpcHandles {
    let engine = engine::connect_engine_best_effort().await;

    let sensors = match peripherals::connect_sensors().await {
        Ok(conn) => {
            info!("connected to peripherals IPC");
            Some(conn)
        }
        Err(err) => {
            let detail = err.to_string();
            log_peripherals_connect_failure(&detail, "startup");
            None
        }
    };

    let updater = match updater::connect_updater().await {
        Ok(conn) => {
            info!("connected to updater IPC");
            Some(Arc::new(conn))
        }
        Err(err) => {
            error!(%err, "failed to connect to updater IPC");
            None
        }
    };

    IpcHandles { engine, sensors: RwLock::new(sensors.map(Arc::new)), sensors_connect: Mutex::new(()), updater: Mutex::new(updater), updates: RealtimeUpdateBus::default() }
}

impl IpcHandles {
    /// Ensure a live peripherals connection, attempting to reconnect on demand.
    pub async fn ensure_sensors(&self) -> Option<Arc<SensorsConnection>> {
        if let Some(conn) = self.sensors.read().await.as_ref().cloned() {
            return Some(conn);
        }

        let _connect_guard = self.sensors_connect.lock().await;
        if let Some(conn) = self.sensors.read().await.as_ref().cloned() {
            return Some(conn);
        }
        match peripherals::connect_sensors().await {
            Ok(conn) => {
                let conn = Arc::new(conn);
                let mut guard = self.sensors.write().await;
                *guard = Some(conn.clone());
                info!("connected to peripherals IPC");
                Some(conn)
            }
            Err(err) => {
                let detail = err.to_string();
                log_peripherals_connect_failure(&detail, "on_demand");
                None
            }
        }
    }

    pub async fn invalidate_sensors(&self) {
        let mut guard = self.sensors.write().await;
        *guard = None;
    }

    pub fn subscribe_realtime_updates(&self) -> broadcast::Receiver<RealtimeUpdateEvent> {
        self.updates.subscribe()
    }

    pub fn publish_realtime_update(&self, origin: RealtimeUpdateOrigin, kind: impl Into<String>, path: impl Into<String>, method: Option<String>, request_id: Option<String>) {
        self.updates.publish(origin, kind, path, method, request_id);
    }
}

/// Ensure journal directory exists.
pub fn ensure_journal_dir() -> std::io::Result<()> {
    fs::create_dir_all(JOURNAL_DIR)
}

pub fn command_id_from_context(label: &str) -> lib_ipc::types::CommandId {
    let context = crate::http::error::current_error_context();
    if let Some(request_id) = context.request_id.as_deref()
        && let Ok(uuid) = Uuid::parse_str(request_id)
    {
        let seq = context.next_sequence();
        let name = format!("{label}:{seq}");
        let derived = Uuid::new_v5(&uuid, name.as_bytes());
        return lib_ipc::types::CommandId::from_uuid(derived);
    }
    lib_ipc::types::CommandId::new()
}
