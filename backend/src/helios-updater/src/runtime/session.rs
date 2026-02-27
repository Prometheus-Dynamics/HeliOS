use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::config::UpdaterConfig;
use crate::error::{Error, Result};
use crate::ipc::{UpdaterCommand, UpdaterEvent};
use crate::service::UpdaterService;
use chrono::Utc;
use futures::{FutureExt, future::BoxFuture};
use lib_ipc::frame::Frame;
use lib_ipc::handshake::{ClientHello, ServerHello};
use lib_ipc::journal::JournalWriter;
use lib_ipc::protocol::{AckEvent, ControlEvent, NackEvent};
use lib_ipc::server::{self, BroadcastHandler};
use lib_ipc::types::CommandId;
use tokio::net::UnixStream;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub(super) async fn handle_connection(
    stream: UnixStream,
    config: Arc<UpdaterConfig>,
    journal: Arc<JournalWriter<UpdaterCommand>>,
    shutdown: CancellationToken,
    service: Arc<UpdaterService>,
    runtime_start: Instant,
) -> Result<()> {
    let handler = UpdaterSessionHandler::new(config, journal, service, runtime_start);
    server::run_broadcast(stream, shutdown, &handler).await.map_err(Error::from)
}

struct UpdaterSessionHandler {
    config: Arc<UpdaterConfig>,
    journal: Arc<JournalWriter<UpdaterCommand>>,
    service: Arc<UpdaterService>,
    runtime_origin: Instant,
    heartbeat_sequence: AtomicU64,
}

impl UpdaterSessionHandler {
    fn new(config: Arc<UpdaterConfig>, journal: Arc<JournalWriter<UpdaterCommand>>, service: Arc<UpdaterService>, runtime_origin: Instant) -> Self {
        Self { config, journal, service, runtime_origin, heartbeat_sequence: AtomicU64::new(0) }
    }
}

impl BroadcastHandler for UpdaterSessionHandler {
    type Command = UpdaterCommand;
    type Event = UpdaterEvent;
    type Error = Error;

    fn server_config(&self) -> server::ServerConfig {
        server::ServerConfig::new(self.config.protocol(), self.config.server_name(), self.config.server_version(), self.config.features().clone()).with_snapshot_required(false)
    }

    fn event_receiver(&self) -> tokio::sync::broadcast::Receiver<Self::Event> {
        self.service.subscribe()
    }

    fn initial_event(&self) -> BoxFuture<'_, Result<Option<Self::Event>>> {
        let service = Arc::clone(&self.service);
        async move {
            let snapshot = service.snapshot_event().await;
            Ok(Some(snapshot))
        }
        .boxed()
    }

    fn handle_command(&self, command: Self::Command) -> BoxFuture<'_, Result<()>> {
        let journal = Arc::clone(&self.journal);
        let service = Arc::clone(&self.service);
        async move { handle_command(&journal, &service, command).await }.boxed()
    }

    fn handle_heartbeat(&self) -> BoxFuture<'_, Result<Option<Self::Event>>> {
        let service = Arc::clone(&self.service);
        let runtime_origin = self.runtime_origin;
        let sequence = self.heartbeat_sequence.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        async move {
            let uptime = runtime_origin.elapsed().as_millis() as u64;
            service.publish_event(UpdaterEvent::Heartbeat { uptime_ms: uptime, sequence, stage_queue_depth: 0 });
            service.publish_snapshot().await;
            Ok(None)
        }
        .boxed()
    }

    fn handle_lagged(&self, skipped: u64) {
        warn!(skipped, "updater client lagged; dropped events");
    }

    fn handle_other(&self, frame: Frame) {
        warn!(kind = ?frame.header.message_kind, "unexpected message kind from client");
    }

    fn on_accept(&self, _client: &ClientHello, _server: &ServerHello) {}
}

async fn handle_command(journal: &Arc<JournalWriter<UpdaterCommand>>, service: &Arc<UpdaterService>, command: UpdaterCommand) -> Result<()> {
    let command_id = command_id(&command);
    let command_name = command_name(&command);
    match journal.append(&command) {
        Ok(_) => match service.handle_command(command).await {
            Ok(()) => {
                service.publish_event(UpdaterEvent::Control(ControlEvent::Ack(AckEvent { command_id, processed_at: Utc::now() })));
                info!(%command_id, command = command_name, "updater command acknowledged");
            }
            Err(err) => {
                warn!(%err, %command_id, command = command_name, "updater command execution failed");
                service.publish_event(UpdaterEvent::Control(ControlEvent::Nack(NackEvent { command_id, reason: err.to_string(), retryable: matches!(err, Error::Http(_) | Error::Io(_)) })));
            }
        },
        Err(err) => {
            warn!(%err, %command_id, command = command_name, "failed to append updater command to journal");
            service.publish_event(UpdaterEvent::Control(ControlEvent::Nack(NackEvent { command_id, reason: err.to_string(), retryable: false })));
        }
    }

    Ok(())
}

fn command_id(command: &UpdaterCommand) -> CommandId {
    match command {
        UpdaterCommand::StageRelease { command_id, .. }
        | UpdaterCommand::Cancel { command_id, .. }
        | UpdaterCommand::ApplyRelease { command_id, .. }
        | UpdaterCommand::Rollback { command_id, .. }
        | UpdaterCommand::QueryState { command_id } => *command_id,
    }
}

fn command_name(command: &UpdaterCommand) -> &'static str {
    match command {
        UpdaterCommand::StageRelease { .. } => "stage_release",
        UpdaterCommand::Cancel { .. } => "cancel",
        UpdaterCommand::ApplyRelease { .. } => "apply_release",
        UpdaterCommand::Rollback { .. } => "rollback",
        UpdaterCommand::QueryState { .. } => "query_state",
    }
}
