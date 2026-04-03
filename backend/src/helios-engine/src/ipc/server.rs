use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::UnixListener;
use tokio::time::{Duration, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::error::Error;
use crate::ipc::{EngineCommand, EngineEvent};
use crate::runtime::EngineRuntime;

use lib_ipc::server;
use lib_ipc::types::{FeatureSet, ProtocolVersion};
use lib_ipc::wire::ServiceKind;
use tokio::sync::broadcast;

/// Default engine IPC socket path.
pub const ENGINE_SOCKET: &str = "/run/helios/engine.sock";

fn resolve_engine_socket() -> PathBuf {
    match std::env::var("HELIOS_ENGINE_SOCKET").or_else(|_| std::env::var("ENGINE_SOCKET")) {
        Ok(value) => PathBuf::from(value),
        Err(_) => PathBuf::from(ENGINE_SOCKET),
    }
}

pub struct EngineIpcServer {
    pub runtime: Arc<EngineRuntime>,
    shutdown: CancellationToken,
    tx: broadcast::Sender<EngineEvent>,
}

impl EngineIpcServer {
    pub fn new(runtime: Arc<EngineRuntime>, shutdown: CancellationToken) -> Self {
        let (tx, _rx) = broadcast::channel(engine_ipc_event_buffer());
        let _ = _rx;
        Self { runtime, shutdown, tx }
    }

    pub async fn run(&self) -> Result<(), Error> {
        let socket_path = resolve_engine_socket();
        if let Some(parent) = socket_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).map_err(|_| Error::InvalidState("engine socket bind failed"))?;
        info!(socket = %socket_path.display(), "engine IPC listening");

        tokio::spawn(run_metrics_broadcaster(self.runtime.clone(), self.tx.clone(), self.shutdown.clone()));

        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => {
                    info!("engine IPC shutdown requested");
                    break;
                }
                accept_res = listener.accept() => {
                    let (stream, _addr) = accept_res.map_err(|_| Error::InvalidState("engine socket accept failed"))?;
                    let server_config = server::ServerConfig::new(
                        ProtocolVersion::default(),
                        "helios-engine",
                        crate::VERSION.to_string(),
                        FeatureSet::default(),
                        ServiceKind::Engine,
                    )
                    .with_snapshot_required(false);
                    let shutdown = self.shutdown.clone();
                    let runtime = self.runtime.clone();
                    let tx = self.tx.clone();
                    let subscribe = {
                        let tx = self.tx.clone();
                        move || tx.subscribe()
                    };

                    tokio::spawn(async move {
                        let handle_command = move |command: EngineCommand| {
                            let runtime = runtime.clone();
                            let tx = tx.clone();
                            async move {
                                let event = runtime.handle_command(command).await;
                                let _ = tx.send(event.clone());
                                Ok(Some(event))
                            }
                        };

                        let snapshot = || async { Ok::<_, Error>(None) };
                        let heartbeat = server::no_heartbeat();

                        let on_accept = server::log_accept;

                        let result = server::run_snapshot_server(
                            stream,
                            shutdown,
                            server_config,
                            subscribe,
                            snapshot,
                            handle_command,
                            heartbeat,
                            on_accept,
                        ).await;
                        if let Err(err) = result {
                            warn!(error = %err, "engine IPC session terminated");
                        }
                    });
                }
            }
        }
        let _ = std::fs::remove_file(&socket_path);
        Ok(())
    }
}

async fn run_metrics_broadcaster(runtime: Arc<EngineRuntime>, tx: broadcast::Sender<EngineEvent>, shutdown: CancellationToken) {
    let mut ticker = tokio::time::interval(metrics_broadcast_interval());
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = ticker.tick() => {
                if tx.receiver_count() == 0 {
                    continue;
                }
                let summaries = runtime.services.list_streams().await;
                for summary in summaries {
                    match runtime.services.get_metrics(summary.stream_id).await {
                        Ok(metrics) => {
                            let _ = tx.send(EngineEvent::MetricsUpdate { stream_id: summary.stream_id, metrics });
                        }
                        Err(err) => {
                            warn!(stream_id = %summary.stream_id, error = %err, "failed to collect metrics for broadcast");
                        }
                    }
                }
            }
        }
    }
}

fn engine_ipc_event_buffer() -> usize {
    std::env::var("HELIOS_ENGINE_IPC_EVENT_BUFFER").ok().and_then(|raw| raw.parse::<usize>().ok()).unwrap_or(4096).clamp(128, 16384)
}

fn metrics_broadcast_interval() -> Duration {
    let ms = std::env::var("HELIOS_ENGINE_METRICS_BROADCAST_MS").ok().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or(5000);
    Duration::from_millis(ms.clamp(250, 10_000))
}
