use std::{path::PathBuf, sync::Arc, time::Duration};

use tokio::time::timeout;
use tracing::debug;

use crate::ipc::journal_path;

use super::config::{SensorsClientConfig, resolve_peripherals_socket_candidates, session_idle_timeout};
use super::{SensorsClient, SensorsConnection, SensorsStream};

pub(crate) async fn connect_sensors() -> Result<SensorsConnection, Box<dyn std::error::Error + Send + Sync>> {
    let journal_path = journal_path("peripherals.journal");
    let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    for socket in resolve_peripherals_socket_candidates() {
        match try_connect_sensors(socket, journal_path.clone()).await {
            Ok(connection) => return Ok(connection),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| "peripherals IPC connect failed".into()))
}

pub(crate) async fn connect_sensors_stream() -> Result<SensorsStream, Box<dyn std::error::Error + Send + Sync>> {
    let journal_path = journal_path("peripherals.journal");
    let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    for socket in resolve_peripherals_socket_candidates() {
        match try_connect_sensors_stream(socket, journal_path.clone()).await {
            Ok(connection) => return Ok(connection),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| "peripherals IPC connect failed".into()))
}

async fn try_connect_sensors(socket: PathBuf, journal_path: PathBuf) -> Result<SensorsConnection, Box<dyn std::error::Error + Send + Sync>> {
    let client = Arc::new(SensorsClient::new(SensorsClientConfig::new(socket.clone(), journal_path))?);
    let session = timeout(Duration::from_secs(5), client.handshake()).await??;
    debug!(
        socket = %socket.display(),
        "peripherals hello: protocol={}, server={}",
        session.server().protocol,
        session.server().server_name
    );
    drop(session);
    Ok(SensorsConnection { client, sessions: Arc::new(tokio::sync::Mutex::new(Vec::new())), session_idle_timeout: session_idle_timeout() })
}

async fn try_connect_sensors_stream(socket: PathBuf, journal_path: PathBuf) -> Result<SensorsStream, Box<dyn std::error::Error + Send + Sync>> {
    let client = Arc::new(SensorsClient::new(SensorsClientConfig::new(socket.clone(), journal_path))?);
    let session = timeout(Duration::from_secs(5), client.handshake()).await??;
    debug!(
        socket = %socket.display(),
        "peripherals stream hello: protocol={}, server={}",
        session.server().protocol,
        session.server().server_name
    );
    Ok(SensorsStream { client, session })
}
