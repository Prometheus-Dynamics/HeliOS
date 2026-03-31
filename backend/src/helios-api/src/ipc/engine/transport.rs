use super::{EngineClient, EngineClientConfig, EngineConnection, EngineRequest, EngineSession, disconnected_error, dispatcher::run_engine_dispatcher, mark_disconnected};
use crate::ipc::{
    engine::timeouts::{DEV_ENGINE_SOCKET, ENGINE_SOCKET, engine_request_queue_size, timeout_scale_for_streams},
    journal_path,
};
use helios_engine::ipc::{EngineCommand, EngineEvent};
use lib_ipc::types::CommandId;
use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize},
    },
};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Duration, Instant, sleep, timeout};
use tracing::warn;

async fn fetch_stream_count_for_tuning(_client: &EngineClient, mut session: EngineSession) -> Option<usize> {
    let command_id = CommandId::new();
    let command = EngineCommand::List { command_id };
    if session.send_ephemeral_command(&command).await.is_err() {
        return None;
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let remaining = deadline - now;
        match timeout(remaining, session.next_event()).await {
            Ok(Ok(Some(event))) => match event {
                EngineEvent::StreamList { command_id: event_id, streams } if event_id == command_id => return Some(streams.len()),
                EngineEvent::Nack { command_id: event_id, .. } if event_id == command_id => return None,
                _ => continue,
            },
            Ok(Ok(None)) => return None,
            Ok(Err(_)) => return None,
            Err(_) => return None,
        }
    }
}

pub async fn connect_engine() -> Result<EngineConnection, Box<dyn Error + Send + Sync>> {
    let journal_path = journal_path("engine.journal");
    let candidates = resolve_engine_sockets();

    let mut last_err: Option<Box<dyn Error + Send + Sync>> = None;
    for attempt in 0..5 {
        for socket in &candidates {
            match try_connect(socket, journal_path.clone()).await {
                Ok(conn) => return Ok(conn),
                Err(err) => {
                    if attempt == 4 {
                        last_err = Some(err);
                    }
                }
            }
        }
        sleep(Duration::from_millis(200)).await;
    }

    Err(last_err.unwrap_or_else(|| "engine IPC connect failed".into()))
}

pub async fn connect_engine_best_effort() -> EngineConnection {
    match connect_engine().await {
        Ok(conn) => return conn,
        Err(err) => warn!(%err, "engine IPC connect failed; starting in degraded mode"),
    }

    let journal_path = journal_path("engine.journal");
    let candidates = resolve_engine_sockets();
    for socket in &candidates {
        match try_connect_lazy(socket, journal_path.clone()) {
            Ok(conn) => return conn,
            Err(err) => warn!(%err, socket = %socket.display(), "engine IPC client init failed"),
        }
    }

    warn!("engine IPC unavailable; responding with disconnected errors until restart");
    spawn_unavailable_engine()
}

async fn try_connect(socket: &Path, journal_path: PathBuf) -> Result<EngineConnection, Box<dyn Error + Send + Sync>> {
    let config = EngineClientConfig::new(socket.to_path_buf(), journal_path);
    let client = Arc::new(EngineClient::new(config)?);
    let session = timeout(Duration::from_secs(5), client.handshake()).await??;
    let stream_count = fetch_stream_count_for_tuning(&client, session).await.unwrap_or(0);
    Ok(spawn_engine_connection(client, stream_count, true))
}

fn try_connect_lazy(socket: &Path, journal_path: PathBuf) -> Result<EngineConnection, Box<dyn Error + Send + Sync>> {
    let config = EngineClientConfig::new(socket.to_path_buf(), journal_path);
    let client = Arc::new(EngineClient::new(config)?);
    Ok(spawn_engine_connection(client, 0, false))
}

fn spawn_engine_connection(client: Arc<EngineClient>, stream_count: usize, connected_initial: bool) -> EngineConnection {
    let (tx, rx) = mpsc::channel(engine_request_queue_size(stream_count));
    let (events, _) = broadcast::channel(64);
    let (connect_events, _) = broadcast::channel(16);
    let connected = Arc::new(AtomicBool::new(connected_initial));
    let last_disconnect_ms = Arc::new(AtomicU64::new(0));
    let timeout_scale_ppm = Arc::new(AtomicU64::new(timeout_scale_for_streams(stream_count)));
    let active_streams = Arc::new(AtomicUsize::new(stream_count));
    if !connected_initial {
        mark_disconnected(&connected, &last_disconnect_ms);
    }
    tokio::spawn(run_engine_dispatcher(client.clone(), rx, events.clone(), connect_events.clone(), connected.clone(), last_disconnect_ms.clone(), timeout_scale_ppm.clone(), active_streams.clone()));
    EngineConnection { requests: tx, events, connect_events, connected, last_disconnect_ms, timeout_scale_ppm, active_streams }
}

fn spawn_unavailable_engine() -> EngineConnection {
    let (tx, mut rx) = mpsc::channel::<EngineRequest>(engine_request_queue_size(0));
    let (events, _) = broadcast::channel(64);
    let (connect_events, _) = broadcast::channel(16);
    let connected = Arc::new(AtomicBool::new(false));
    let last_disconnect_ms = Arc::new(AtomicU64::new(0));
    let timeout_scale_ppm = Arc::new(AtomicU64::new(timeout_scale_for_streams(0)));
    let active_streams = Arc::new(AtomicUsize::new(0));
    mark_disconnected(&connected, &last_disconnect_ms);
    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            let _ = req.respond_to.send(Err(disconnected_error()));
        }
    });
    EngineConnection { requests: tx, events, connect_events, connected, last_disconnect_ms, timeout_scale_ppm, active_streams }
}

fn resolve_engine_sockets() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(path) = std::env::var("HELIOS_ENGINE_SOCKET").or_else(|_| std::env::var("ENGINE_SOCKET")) {
        paths.push(PathBuf::from(path));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    paths.push(manifest_dir.join("../..").join("target/dev/run/engine.sock"));
    paths.push(PathBuf::from(DEV_ENGINE_SOCKET));
    paths.push(PathBuf::from(ENGINE_SOCKET));

    let mut deduped = Vec::new();
    for p in paths {
        if !deduped.iter().any(|seen: &PathBuf| seen == &p) {
            deduped.push(p);
        }
    }
    deduped
}
