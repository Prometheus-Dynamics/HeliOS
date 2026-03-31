mod spawn;

use crate::console_protocol::ConsoleServerEvent;
use chrono::{TimeZone, Utc};
use once_cell::sync::Lazy;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicU16, AtomicUsize, Ordering},
    },
};
use thiserror::Error;
use tokio::sync::{Mutex, broadcast, mpsc};
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
        let pid = nix::unistd::Pid::from_raw(pid_raw);
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);
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
        let session = spawn::spawn_session(cols, rows).await?;
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

fn clamp_dimension(value: u16, min: u16, max: u16) -> u16 {
    value.clamp(min, max)
}
