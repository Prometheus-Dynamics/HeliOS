use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::config::SensorsConfig;
use crate::error::{Error, Result};
use crate::ipc::{SensorCommand, SensorEvent};
use crate::service::SensorsService;
use lib_ipc::handshake::{ClientHello, ServerHello};
use lib_ipc::server::{self, RetryableError, ServerHandshakeError, ServerLoopError, ServerTransportError};
use lib_ipc::types::CommandId;
use lib_ipc::wire::ServiceKind;
use tokio::net::UnixStream;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
pub(super) async fn handle_connection(stream: UnixStream, config: Arc<SensorsConfig>, shutdown: CancellationToken, service: Arc<SensorsService>, runtime_start: Instant) -> Result<()> {
    let server_config = server::ServerConfig::new(config.protocol(), config.server_name(), config.server_version(), config.features().clone(), ServiceKind::Peripherals).with_snapshot_required(false);
    let runtime_origin = runtime_start;
    let heartbeat_sequence = Arc::new(AtomicU64::new(0));

    let service_for_subscribe = Arc::clone(&service);
    let service_for_snapshot = Arc::clone(&service);
    let service_for_command = Arc::clone(&service);

    let result = server::run_snapshot_server(
        stream,
        shutdown,
        server_config,
        move || service_for_subscribe.subscribe(),
        move || {
            let service = Arc::clone(&service_for_snapshot);
            async move {
                let inventory = service.inventory().await;
                let event = SensorEvent::Inventory { command_id: CommandId::new(), inventory };
                Ok::<_, Error>(Some(event))
            }
        },
        move |command| {
            let service = Arc::clone(&service_for_command);
            async move {
                handle_command(&service, command).await?;
                Ok(None)
            }
        },
        move || {
            let heartbeat_sequence = Arc::clone(&heartbeat_sequence);
            async move {
                let _uptime = runtime_origin.elapsed().as_millis() as u64;
                let _ = heartbeat_sequence.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
                Ok::<_, Error>(None)
            }
        },
        move |client: &ClientHello, server: &ServerHello| {
            debug!(
                client_name = %client.client_name,
                client_version = %client.client_version,
                protocol = %client.protocol,
                server_name = %server.server_name,
                "sensor IPC client connected"
            );
        },
    )
    .await;

    let peer_disconnect = result.as_ref().err().is_some_and(is_peer_disconnect);

    match &result {
        Ok(()) => debug!("sensor IPC client disconnected"),
        Err(_) if peer_disconnect => debug!("sensor IPC client disconnected"),
        Err(err) => warn!(error = %err, "sensor IPC session terminated with error"),
    }

    match result {
        Ok(()) => Ok(()),
        Err(_) if peer_disconnect => Ok(()),
        Err(err) => Err(Error::from(err)),
    }
}

fn is_peer_disconnect(err: &ServerLoopError<Error>) -> bool {
    let io_err = match err {
        ServerLoopError::Handshake(ServerHandshakeError::Io(err)) => Some(err),
        ServerLoopError::Handshake(ServerHandshakeError::Closed) => return true,
        ServerLoopError::Transport(ServerTransportError::Io(err)) => Some(err),
        _ => None,
    };

    io_err.map(|err| matches!(err.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted | io::ErrorKind::UnexpectedEof)).unwrap_or(false)
}

async fn handle_command(service: &Arc<SensorsService>, command: SensorCommand) -> Result<()> {
    let command_id = command.command_id();
    if let Err(err) = handle_command_inner(service, command).await {
        service.publish_nack(command_id, err.to_string(), err.retryable());
    }
    Ok(())
}

async fn handle_command_inner(service: &Arc<SensorsService>, command: SensorCommand) -> Result<()> {
    match command {
        SensorCommand::Discover { command_id, refresh } => {
            let inventory = service.discover(refresh).await?;
            service.publish_inventory(command_id, inventory);
        }
        SensorCommand::Inventory { command_id } => {
            let inventory = service.inventory().await;
            service.publish_inventory(command_id, inventory);
        }
        SensorCommand::Snapshot { command_id, scope } => {
            let values = service.snapshot(&scope).await?;
            service.publish_event(SensorEvent::Snapshot { command_id: Some(command_id), scope, values });
        }
        SensorCommand::SnapshotTyped { command_id, scope } => {
            let values = service.snapshot_typed(&scope).await?;
            service.publish_event(SensorEvent::SnapshotTyped { command_id: Some(command_id), scope, values });
        }
        SensorCommand::Update { command_id, scope, sensor, payload } => {
            service.update_sensor(&scope, sensor, payload).await?;
            service.publish_snapshot(Some(command_id), &scope).await;
        }
        SensorCommand::Subscribe { command_id, scope } => {
            service.subscribe_scope(scope.clone()).await?;
            service.publish_event(SensorEvent::Subscribed { command_id, scope: scope.clone() });
            service.publish_snapshot(None, &scope).await;
            if matches!(scope, crate::dto::SensorScope::Device) {
                let lighting = service.lighting_state().await;
                service.publish_event(SensorEvent::LightingState { command_id: None, state: lighting });
            }
        }
        SensorCommand::Unsubscribe { command_id: _, scope } => match service.unsubscribe_scope(&scope).await? {
            true => service.publish_event(SensorEvent::Unsubscribed { scope }),
            false => return Err(Error::InvalidState(format!("scope {scope:?} is not registered"))),
        },
        SensorCommand::ConfigureFirmware { command_id, device_id, firmware } => {
            service.configure_firmware(&device_id, firmware).await?;
            service.publish_ack(command_id);
        }
        SensorCommand::ConfigureAlias { command_id, hardware_key, alias } => {
            service.configure_alias(&hardware_key, alias).await?;
            service.publish_ack(command_id);
        }
        SensorCommand::AiListModels { command_id } => {
            let inventory = service.ai_inventory().await?;
            service.publish_event(SensorEvent::AiModelInventory { command_id, inventory });
        }
        SensorCommand::AiUploadModel { command_id, model } => {
            let descriptor = service.upload_ai_model(model).await?;
            service.publish_event(SensorEvent::AiModelUploaded { command_id, model: Box::new(descriptor) });
        }
        SensorCommand::AiDeleteModel { command_id, model_id } => {
            service.delete_ai_model(model_id.clone()).await?;
            service.publish_event(SensorEvent::AiModelDeleted { command_id, model_id });
        }
        SensorCommand::I2cInventory { command_id } => {
            let inventory = service.i2c_inventory().await?;
            service.publish_event(SensorEvent::I2cInventory { command_id, inventory });
        }
        SensorCommand::Lighting { command_id, command } => {
            service.lighting_command(command).await?;
            service.publish_ack(command_id);
        }
        SensorCommand::LightingState { command_id } => {
            let state = service.lighting_state().await;
            service.publish_event(SensorEvent::LightingState { command_id: Some(command_id), state });
            service.publish_ack(command_id);
        }
        SensorCommand::FanStatus { command_id } => {
            let status = service.fan_status().await?;
            service.publish_event(SensorEvent::FanStatus { command_id, status });
        }
        SensorCommand::FanConfig { command_id } => {
            let config = service.fan_config().await?;
            service.publish_event(SensorEvent::FanConfig { command_id, config });
        }
        SensorCommand::UpdateFanConfig { command_id, config } => {
            service.update_fan_config(config).await?;
            service.publish_ack(command_id);
        }
    }

    Ok(())
}
