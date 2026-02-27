use crate::console_protocol::ConsoleServerEvent;
use chrono::{TimeZone, Utc};
use nix::{
    pty::{Winsize, openpty},
    sys::signal::{Signal, kill},
    sys::termios::{ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg, tcgetattr, tcsetattr},
    unistd::Pid,
};
use once_cell::sync::Lazy;
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    os::fd::AsFd,
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicU16, AtomicUsize, Ordering},
    },
};
use thiserror::Error;
use tokio::{
    process::Command,
    sync::{Mutex, broadcast, mpsc},
};
use tracing::debug;
use uuid::Uuid;

const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 34;
const MIN_COLS: u16 = 40;
const MAX_COLS: u16 = 320;
const MIN_ROWS: u16 = 12;
const MAX_ROWS: u16 = 160;
const OUTPUT_CHUNK: usize = 4096;
const OUTPUT_BUFFER_MAX_BYTES: usize = 512 * 1024;
const DEFAULT_CONSOLE_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Debug, Error)]
pub enum ConsoleSessionError {
    #[error("terminal unavailable: {0}")]
    Terminal(String),
    #[error("failed to allocate pseudo-terminal: {0}")]
    Pty(#[from] nix::Error),
    #[error("failed to spawn shell: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("session not found")]
    NotFound,
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct ConsoleSessionSummaryPayload {
    pub session_id: Uuid,
    pub shell: String,
    pub created_at: String,
    pub last_activity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub client_count: usize,
    pub cols: u16,
    pub rows: u16,
    pub closed: bool,
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct ConsoleSessionListPayload {
    #[serde(default)]
    pub sessions: Vec<ConsoleSessionSummaryPayload>,
}

pub struct ConsoleSession {
    id: Uuid,
    shell: String,
    cols: AtomicU16,
    rows: AtomicU16,
    created_at_ms: i64,
    last_activity_ms: AtomicI64,
    client_count: AtomicUsize,
    closed: AtomicBool,
    exit_code: Mutex<Option<i32>>,
    output_buffer: Mutex<OutputBuffer>,
    events_tx: broadcast::Sender<ConsoleServerEvent>,
    input_tx: mpsc::UnboundedSender<Vec<u8>>,
    child_pid: Option<i32>,
    pty_master: std::fs::File,
}

#[derive(Default)]
struct OutputBuffer {
    chunks: VecDeque<String>,
    bytes: usize,
}

impl ConsoleSession {
    pub fn summary(&self) -> ConsoleSessionSummaryPayload {
        let created_at = Utc.timestamp_millis_opt(self.created_at_ms).single().map(|dt| dt.to_rfc3339()).unwrap_or_default();
        let last_ms = self.last_activity_ms.load(Ordering::Relaxed);
        let last_activity = Utc.timestamp_millis_opt(last_ms).single().map(|dt| dt.to_rfc3339()).unwrap_or(created_at.clone());
        let exit_code = self.exit_code.try_lock().ok().and_then(|guard| *guard);
        ConsoleSessionSummaryPayload {
            session_id: self.id,
            shell: self.shell.clone(),
            created_at,
            last_activity,
            exit_code,
            client_count: self.client_count.load(Ordering::Relaxed),
            cols: self.cols.load(Ordering::Relaxed),
            rows: self.rows.load(Ordering::Relaxed),
            closed: self.closed.load(Ordering::Relaxed),
        }
    }

    pub fn ready_event(&self) -> ConsoleServerEvent {
        ConsoleServerEvent::Ready { session_id: self.id, shell: self.shell.clone(), cols: self.cols.load(Ordering::Relaxed), rows: self.rows.load(Ordering::Relaxed) }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConsoleServerEvent> {
        self.events_tx.subscribe()
    }

    pub async fn output_snapshot(&self) -> Vec<String> {
        let guard = self.output_buffer.lock().await;
        guard.chunks.iter().cloned().collect()
    }

    pub fn input_sender(&self) -> mpsc::UnboundedSender<Vec<u8>> {
        self.input_tx.clone()
    }

    pub fn connect_client(&self) -> ConsoleSessionClientGuard<'_> {
        self.client_count.fetch_add(1, Ordering::Relaxed);
        self.touch();
        ConsoleSessionClientGuard { session: self }
    }

    pub fn touch(&self) {
        self.last_activity_ms.store(Utc::now().timestamp_millis(), Ordering::Relaxed);
    }

    pub fn mark_closed(&self, exit_code: Option<i32>) {
        self.closed.store(true, Ordering::Relaxed);
        self.last_activity_ms.store(Utc::now().timestamp_millis(), Ordering::Relaxed);
        if let Ok(mut guard) = self.exit_code.try_lock() {
            *guard = exit_code;
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        let cols = clamp_dimension(cols, MIN_COLS, MAX_COLS);
        let rows = clamp_dimension(rows, MIN_ROWS, MAX_ROWS);
        let winsize = rustix::termios::Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        rustix::termios::tcsetwinsize(&self.pty_master, winsize)?;
        self.cols.store(cols, Ordering::Relaxed);
        self.rows.store(rows, Ordering::Relaxed);
        Ok(())
    }

    fn terminate(&self) {
        let Some(pid_raw) = self.child_pid else {
            return;
        };
        let pid = Pid::from_raw(pid_raw);
        let _ = kill(pid, Signal::SIGTERM);
    }
}

pub struct ConsoleSessionClientGuard<'a> {
    session: &'a ConsoleSession,
}

impl Drop for ConsoleSessionClientGuard<'_> {
    fn drop(&mut self) {
        self.session.client_count.fetch_sub(1, Ordering::Relaxed);
        self.session.touch();
    }
}

#[derive(Default)]
pub struct ConsoleSessionManager {
    sessions: Mutex<HashMap<Uuid, Arc<ConsoleSession>>>,
}

pub static CONSOLE_SESSIONS: Lazy<ConsoleSessionManager> = Lazy::new(ConsoleSessionManager::default);

impl ConsoleSessionManager {
    pub async fn create(&self, cols: Option<u16>, rows: Option<u16>) -> Result<Arc<ConsoleSession>, ConsoleSessionError> {
        let session = spawn_session(cols, rows).await?;
        let id = session.id;
        self.sessions.lock().await.insert(id, session.clone());
        Ok(session)
    }

    pub async fn get(&self, session_id: Uuid) -> Option<Arc<ConsoleSession>> {
        self.sessions.lock().await.get(&session_id).cloned()
    }

    pub async fn list(&self) -> Vec<ConsoleSessionSummaryPayload> {
        let sessions: Vec<Arc<ConsoleSession>> = self.sessions.lock().await.values().cloned().collect();
        let mut summaries = sessions.into_iter().map(|session| session.summary()).collect::<Vec<_>>();
        summaries.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        summaries
    }

    pub async fn close(&self, session_id: Uuid) -> Result<(), ConsoleSessionError> {
        let session = self.sessions.lock().await.remove(&session_id).ok_or(ConsoleSessionError::NotFound)?;
        session.terminate();
        Ok(())
    }
}

async fn spawn_session(cols: Option<u16>, rows: Option<u16>) -> Result<Arc<ConsoleSession>, ConsoleSessionError> {
    let cols = clamp_dimension(cols.unwrap_or(DEFAULT_COLS), MIN_COLS, MAX_COLS);
    let rows = clamp_dimension(rows.unwrap_or(DEFAULT_ROWS), MIN_ROWS, MAX_ROWS);
    let winsize = Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };

    let result = openpty(Some(&winsize), None)?;
    if let Ok(mut termios) = tcgetattr(result.slave.as_fd()) {
        termios.local_flags.insert(LocalFlags::ECHO | LocalFlags::ECHONL | LocalFlags::ICANON | LocalFlags::ISIG | LocalFlags::IEXTEN);
        termios.input_flags.insert(InputFlags::ICRNL | InputFlags::IXON);
        termios.output_flags.insert(OutputFlags::OPOST | OutputFlags::ONLCR);
        termios.control_flags.insert(ControlFlags::CREAD | ControlFlags::CS8);
        let _ = tcsetattr(result.slave.as_fd(), SetArg::TCSANOW, &termios);
    }

    let slave_file = std::fs::File::from(result.slave);
    let stdin = slave_file.try_clone()?;
    let stdout = slave_file.try_clone()?;
    let stderr = slave_file;

    let shell = resolve_shell().ok_or_else(|| ConsoleSessionError::Terminal("shell executable not found".into()))?;

    let wrapper = std::env::current_exe()?;
    let mut command = Command::new(wrapper);
    command.stdin(Stdio::from(stdin)).stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr)).env("TERM", "xterm-256color").env("PATH", resolve_console_path()).kill_on_drop(true);
    command.arg("console-child").arg(&shell);
    if shell.ends_with("bash") {
        command.args(["--login", "-i"]);
    } else if shell.ends_with("/sh") {
        command.arg("-i");
    }
    let mut child = command.spawn()?;
    let child_pid = child.id().map(|pid| pid as i32);

    let master_file = std::fs::File::from(result.master);
    // PTY IO: Use dedicated blocking reads/writes on duplicated master fds.
    //
    // We intentionally avoid `tokio::io::split` and `tokio::fs::File` here. PTY fds can
    // block in ways that starve the async runtime (and `split` serializes IO behind a mutex),
    // which can prevent input writes from ever reaching the shell.
    let mut master_reader = master_file.try_clone()?;
    let mut master_writer = master_file.try_clone()?;

    let session_id = Uuid::new_v4();
    let now_ms = Utc::now().timestamp_millis();

    let (events_tx, _) = broadcast::channel::<ConsoleServerEvent>(256);
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let session = Arc::new(ConsoleSession {
        id: session_id,
        shell,
        cols: AtomicU16::new(cols),
        rows: AtomicU16::new(rows),
        created_at_ms: now_ms,
        last_activity_ms: AtomicI64::new(now_ms),
        client_count: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
        exit_code: Mutex::new(None),
        output_buffer: Mutex::new(OutputBuffer::default()),
        events_tx,
        input_tx,
        child_pid,
        pty_master: master_file,
    });

    let session_writer = session.clone();
    tokio::spawn(async move {
        while let Some(chunk) = input_rx.recv().await {
            if chunk.is_empty() {
                continue;
            }
            session_writer.touch();
            let write_result = tokio::task::block_in_place(|| master_writer.write_all(&chunk));
            if write_result.is_err() {
                let _ = session_writer.events_tx.send(ConsoleServerEvent::Error { message: "failed to write to console".into() });
                break;
            }
        }
    });

    let session_reader = session.clone();
    let runtime_handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; OUTPUT_CHUNK];
        loop {
            match master_reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    session_reader.touch();
                    let data = String::from_utf8_lossy(&buf[..n]).to_string();
                    runtime_handle.block_on(async {
                        let mut guard = session_reader.output_buffer.lock().await;
                        guard.bytes = guard.bytes.saturating_add(data.len());
                        guard.chunks.push_back(data.clone());
                        while guard.bytes > OUTPUT_BUFFER_MAX_BYTES {
                            if let Some(front) = guard.chunks.pop_front() {
                                guard.bytes = guard.bytes.saturating_sub(front.len());
                            } else {
                                guard.bytes = 0;
                                break;
                            }
                        }
                    });
                    // If there are no subscribers yet, broadcast returns an error; keep reading anyway.
                    let _ = session_reader.events_tx.send(ConsoleServerEvent::Output { data });
                }
                Err(err) => {
                    debug!(error = %err, "console reader halted");
                    break;
                }
            }
        }
    });

    let session_exit = session.clone();
    tokio::spawn(async move {
        let status = child.wait().await.ok();
        let code = status.and_then(|s| s.code());
        session_exit.mark_closed(code);
        let _ = session_exit.events_tx.send(ConsoleServerEvent::Exit { code });
    });

    Ok(session)
}

fn clamp_dimension(value: u16, min: u16, max: u16) -> u16 {
    value.clamp(min, max)
}

fn resolve_shell() -> Option<String> {
    let env_shell = std::env::var("SHELL").ok().filter(|value| !value.trim().is_empty());
    let mut candidates = Vec::new();
    if let Some(shell) = env_shell {
        candidates.push(shell);
    }
    candidates.extend(["/bin/bash", "/bin/sh"].iter().map(|entry| entry.to_string()));
    candidates.into_iter().find(|candidate| Path::new(candidate).exists())
}

fn resolve_console_path() -> String {
    if let Ok(value) = std::env::var("HELIOS_CONSOLE_PATH") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let mut entries: Vec<String> = std::env::var("PATH").unwrap_or_default().split(':').map(str::trim).filter(|entry| !entry.is_empty()).map(|entry| entry.to_string()).collect();
    if entries.is_empty() {
        return DEFAULT_CONSOLE_PATH.to_string();
    }
    let mut has_bin = false;
    let mut has_sbin = false;
    for entry in &entries {
        if entry == "/bin" {
            has_bin = true;
        } else if entry == "/sbin" {
            has_sbin = true;
        }
    }
    if !has_sbin {
        entries.push("/sbin".to_string());
    }
    if !has_bin {
        entries.push("/bin".to_string());
    }
    entries.join(":")
}
