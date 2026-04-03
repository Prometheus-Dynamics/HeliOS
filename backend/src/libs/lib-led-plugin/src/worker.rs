use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use daedalus::runtime::NodeError;
use helios_peripherals::dto::{LightingAnimation, LightingCommand};
use helios_peripherals::ipc::{SensorCommand, SensorEvent};
use lib_ipc::client::{Client as GenericClient, Session as GenericSession, TransportConfig};
use lib_ipc::types::{CommandId, FeatureSet};
use lib_ipc::wire::ServiceKind;
use lib_led_animations::LedAnimationEntry;
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};
use tracing::warn;

const PERIPHERALS_SOCKET: &str = "/run/helios/peripherals.sock";
const DEV_PERIPHERALS_SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/dev/run/peripherals.sock");
const DEFAULT_JOURNAL_DIR: &str = "/var/lib/helios/journal/ipc";
const IPC_JOURNAL_DIR_ENV: &str = "HELIOS_IPC_JOURNAL_DIR";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const TICK_INTERVAL: Duration = Duration::from_millis(50);
const CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_millis(500);

pub type SensorsClient = GenericClient<SensorsClientConfig, SensorCommand, SensorEvent>;
pub type SensorsSession = GenericSession<SensorCommand, SensorEvent>;

pub struct LedWorkerHandle {
    tx: mpsc::UnboundedSender<WorkerCommand>,
}

#[derive(Clone, Copy, Debug)]
pub enum PlayMode {
    Immediate,
    Queue,
}

impl LedWorkerHandle {
    pub fn play(&self, entry: LedAnimationEntry, mode: PlayMode) -> bool {
        self.tx.send(WorkerCommand::Play { entry, mode }).is_ok()
    }

    pub fn stop(&self) -> bool {
        self.tx.send(WorkerCommand::Stop).is_ok()
    }
}

pub fn spawn() -> Result<LedWorkerHandle, NodeError> {
    tokio::runtime::Handle::try_current().map_err(|_| NodeError::Handler("LED nodes require a tokio runtime".into()))?;

    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        run_worker(rx).await;
    });

    Ok(LedWorkerHandle { tx })
}

#[derive(Debug)]
enum WorkerCommand {
    Play { entry: LedAnimationEntry, mode: PlayMode },
    Stop,
}

#[derive(Debug)]
struct QueuedEntry {
    entry: LedAnimationEntry,
}

#[derive(Debug)]
struct ActiveEntry {
    deadline: Option<Instant>,
    pending: VecDeque<FrameStep>,
}

#[derive(Debug)]
struct FrameStep {
    command: LightingCommand,
    duration_ms: Option<u32>,
}

async fn run_worker(mut rx: mpsc::UnboundedReceiver<WorkerCommand>) {
    let mut conn: Option<Connection> = None;
    let mut queue: VecDeque<QueuedEntry> = VecDeque::new();
    let mut active: Option<ActiveEntry> = None;
    let mut last_command_at: Option<Instant> = None;
    let mut tick = tokio::time::interval(TICK_INTERVAL);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let now = Instant::now();
                if conn.is_some()
                    && last_command_at.is_some_and(|at| now.saturating_duration_since(at) >= CONNECTION_IDLE_TIMEOUT)
                {
                    conn = None;
                    last_command_at = None;
                }
                if let Some(current) = &mut active
                    && let Some(deadline) = current.deadline
                        && now >= deadline {
                            if let Some(next_step) = current.pending.pop_front() {
                                if apply_command(&mut conn, next_step.command, &mut last_command_at).await {
                                    current.deadline = next_step
                                        .duration_ms
                                        .map(|ms| Instant::now() + Duration::from_millis(ms as u64));
                                } else {
                                    active = None;
                                }
                            } else {
                                active = None;
                            }
                        }
                if active.is_none()
                    && let Some(next) = queue.pop_front()
                        && let Some(new_active) = apply_entry(&mut conn, &next.entry, &mut last_command_at).await {
                            active = Some(new_active);
                        }
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break; };
                match cmd {
                    WorkerCommand::Play { entry, mode } => {
                        match mode {
                            PlayMode::Immediate => {
                                queue.clear();
                                active = apply_entry(&mut conn, &entry, &mut last_command_at).await;
                            }
                            PlayMode::Queue => {
                                if active.is_none() {
                                    active = apply_entry(&mut conn, &entry, &mut last_command_at).await;
                                } else {
                                    queue.push_back(QueuedEntry { entry });
                                }
                            }
                        }
                    }
                    WorkerCommand::Stop => {
                        queue.clear();
                        active = None;
                        let _ = apply_command(&mut conn, LightingCommand { frame: None, brightness: None, animation: Some(LightingAnimation::Off) }, &mut last_command_at).await;
                    }
                }
            }
        }
    }
}

async fn apply_entry(conn: &mut Option<Connection>, entry: &LedAnimationEntry, last_command_at: &mut Option<Instant>) -> Option<ActiveEntry> {
    if !entry.sequence.is_empty() {
        let mut steps = VecDeque::new();
        for frame in &entry.sequence {
            steps.push_back(FrameStep {
                command: LightingCommand { frame: Some(frame.frame.clone()), brightness: entry.command.brightness, animation: None },
                duration_ms: Some(frame.duration_ms.max(50)),
            });
        }
        let first = steps.pop_front()?;
        if apply_command(conn, first.command, last_command_at).await {
            return Some(ActiveEntry { deadline: first.duration_ms.map(|ms| Instant::now() + Duration::from_millis(ms as u64)), pending: steps });
        }
        return None;
    }

    if apply_command(conn, entry.command.clone(), last_command_at).await {
        Some(ActiveEntry { deadline: entry.duration_ms.map(|ms| Instant::now() + Duration::from_millis(ms as u64)), pending: VecDeque::new() })
    } else {
        None
    }
}

async fn apply_command(conn: &mut Option<Connection>, command: LightingCommand, last_command_at: &mut Option<Instant>) -> bool {
    if conn.is_none() {
        *conn = connect_peripherals().await.ok();
    }
    let Some(connection) = conn.as_mut() else {
        return false;
    };
    match send_lighting_command(connection, command).await {
        Ok(()) => {
            *last_command_at = Some(Instant::now());
            true
        }
        Err(err) => {
            warn!(%err, "LED IPC command failed");
            *conn = None;
            false
        }
    }
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
            client_name: "helios-led-plugin".to_string(),
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

    fn service_kind(&self) -> ServiceKind {
        ServiceKind::Peripherals
    }
}

struct Connection {
    client: Arc<SensorsClient>,
    session: SensorsSession,
}

async fn connect_peripherals() -> Result<Connection, String> {
    let journal_path = journal_path("peripherals.journal");
    let mut last_err = None;
    for socket in resolve_peripherals_socket_candidates() {
        match try_connect(socket, journal_path.clone()).await {
            Ok(conn) => return Ok(conn),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| "peripherals IPC connect failed".to_string()))
}

async fn try_connect(socket: PathBuf, journal_path: PathBuf) -> Result<Connection, String> {
    let client = Arc::new(SensorsClient::new(SensorsClientConfig::new(socket, journal_path)).map_err(|e| e.to_string())?);
    let session = match timeout(Duration::from_secs(3), client.handshake()).await {
        Ok(Ok(session)) => session,
        Ok(Err(err)) => return Err(err.to_string()),
        Err(_) => return Err("peripherals handshake timed out".to_string()),
    };
    Ok(Connection { client, session })
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

fn journal_path(file_name: &str) -> PathBuf {
    std::env::var_os(IPC_JOURNAL_DIR_ENV).filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_JOURNAL_DIR)).join(file_name)
}

fn push_unique(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}

async fn send_lighting_command(conn: &mut Connection, command: LightingCommand) -> Result<(), String> {
    let command_id = CommandId::new();
    let cmd = SensorCommand::Lighting { command_id, command };
    conn.session.send_command(conn.client.journal(), &cmd).await.map_err(|e| e.to_string())?;

    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err("peripherals lighting command timed out".into());
        }
        let remaining = deadline - now;
        match timeout(remaining, conn.session.next_event()).await {
            Ok(Ok(Some(event))) => match event {
                SensorEvent::Ack { command_id: event_id, .. } if event_id == command_id => return Ok(()),
                SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => return Err(reason),
                _ => {}
            },
            Ok(Ok(None)) => return Err("peripherals session closed".into()),
            Ok(Err(err)) => return Err(err.to_string()),
            Err(_) => return Err("peripherals lighting command timed out".into()),
        }
    }
}
