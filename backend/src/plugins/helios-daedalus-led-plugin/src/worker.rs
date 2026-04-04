use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use daedalus::runtime::NodeError;
use helios_peripherals::dto::{LightingAnimation, LightingCommand};
use helios_peripherals::ipc::{SensorCommand, SensorEvent};
use lib_ipc::client::{Client as GenericClient, Session as GenericSession, TransportConfig};
use lib_ipc::types::{CommandId, FeatureSet};
use lib_ipc::wire::ServiceKind;
use lib_led_animations::LedAnimationEntry;
use lib_runtime_policy::{HELIOS_IPC_JOURNAL_POLICY, HELIOS_PERIPHERALS_SERVICE_POLICY};
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};
use tracing::warn;

const DEV_PERIPHERALS_SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/dev/run/peripherals.sock");
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

    fn service_kind(&self) -> ServiceKind {
        ServiceKind::Peripherals
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

struct Connection {
    session: SensorsSession,
}

async fn connect_peripherals() -> Result<Connection, NodeError> {
    let peripherals_policy = HELIOS_PERIPHERALS_SERVICE_POLICY.resolve();
    let default_socket = HELIOS_PERIPHERALS_SERVICE_POLICY.default_socket_path();
    let socket_path = if PathBuf::from(DEV_PERIPHERALS_SOCKET).exists() {
        PathBuf::from(DEV_PERIPHERALS_SOCKET)
    } else if peripherals_policy.socket_path != default_socket {
        peripherals_policy.socket_path
    } else {
        default_socket
    };
    let journal_path = HELIOS_IPC_JOURNAL_POLICY.resolve().dir;
    let client = SensorsClient::new(SensorsClientConfig::new(socket_path, journal_path)).map_err(|err| NodeError::Handler(format!("connect peripherals failed: {err}")))?;
    let session = client.handshake().await.map_err(|err| NodeError::Handler(format!("open peripherals session failed: {err}")))?;
    Ok(Connection { session })
}

async fn send_lighting_command(connection: &mut Connection, command: LightingCommand) -> Result<(), NodeError> {
    let command_id = CommandId::new();
    connection
        .session
        .send_ephemeral_command(&SensorCommand::Lighting { command_id, command: command.into() })
        .await
        .map_err(|err| NodeError::Handler(format!("send lighting command failed: {err}")))?;

    let event = timeout(COMMAND_TIMEOUT, connection.session.next_event())
        .await
        .map_err(|_| NodeError::Handler("timed out waiting for lighting ack".into()))?
        .map_err(|err| NodeError::Handler(format!("receive lighting ack failed: {err}")))?
        .ok_or_else(|| NodeError::Handler("lighting IPC closed".into()))?;

    match event {
        SensorEvent::Ack { command_id: acked, .. } if acked == command_id => Ok(()),
        SensorEvent::Nack { command_id: failed, reason, .. } if failed == command_id => Err(NodeError::Handler(format!("lighting command rejected: {reason}"))),
        other => Err(NodeError::Handler(format!("unexpected lighting response: {other:?}"))),
    }
}
