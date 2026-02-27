use std::{path::PathBuf, sync::Arc};

use helios_updater::client::Error as UpdaterError;
use helios_updater::client::{UpdaterClient, UpdaterClientConfig, UpdaterSession};
use helios_updater::ipc::{UpdaterCommand, UpdaterEvent};
use lib_ipc::protocol::ControlEvent;
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};
use tracing::{error, info};

use crate::ipc::{JOURNAL_DIR, UPDATER_SOCKET};

const DEV_UPDATER_SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/dev/run/updater.sock");

pub struct UpdaterConnection {
    pub client: Arc<UpdaterClient>,
    sessions: Mutex<Vec<UpdaterSession>>,
}

pub async fn connect_updater() -> Result<UpdaterConnection, Box<dyn std::error::Error + Send + Sync>> {
    let journal_path = PathBuf::from(JOURNAL_DIR).join("updater.journal");
    let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    for socket in resolve_updater_socket_candidates() {
        match try_connect_updater(socket, journal_path.clone()).await {
            Ok(conn) => return Ok(conn),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| "updater IPC connect failed".into()))
}

async fn try_connect_updater(socket: PathBuf, journal_path: PathBuf) -> Result<UpdaterConnection, Box<dyn std::error::Error + Send + Sync>> {
    let config = UpdaterClientConfig::new(socket.clone(), journal_path);
    let client = Arc::new(UpdaterClient::new(config)?);
    let mut session = handshake_with_timeout(&client).await?;
    info!(
        socket = %socket.display(),
        "updater hello: protocol={}, server={}",
        session.server().protocol,
        session.server().server_name
    );

    // Send a no-op QueryState to verify round-trip.
    let command_id = lib_ipc::types::CommandId::new();
    let command = UpdaterCommand::QueryState { command_id };
    let journal_entry = client.journal().append(&command)?;
    session.send_command(client.journal(), &command).await?;
    match session.next_event().await {
        Ok(Some(UpdaterEvent::Control(ControlEvent::Ack(ack)))) if ack.command_id == command_id => {
            info!("updater acked QueryState (journal offset {})", journal_entry.offset)
        }
        Ok(Some(other)) => info!("updater initial event: {:?}", other),
        Ok(None) => info!("updater closed connection after QueryState"),
        Err(err) => error!(%err, "failed waiting for updater event"),
    }

    Ok(UpdaterConnection { client, sessions: Mutex::new(vec![session]) })
}

impl UpdaterConnection {
    pub async fn checkout_session(&self) -> Result<UpdaterSession, UpdaterError> {
        if let Some(session) = self.sessions.lock().await.pop() {
            return Ok(session);
        }
        handshake_with_timeout(&self.client).await
    }

    pub async fn recycle_session(&self, session: UpdaterSession) {
        const MAX_SESSIONS: usize = 4;
        let mut guard = self.sessions.lock().await;
        if guard.len() < MAX_SESSIONS {
            guard.push(session);
        }
    }
}

async fn handshake_with_timeout(client: &UpdaterClient) -> Result<UpdaterSession, UpdaterError> {
    match timeout(Duration::from_secs(5), client.handshake()).await {
        Ok(result) => result,
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "updater handshake timed out").into()),
    }
}

fn resolve_updater_socket_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for name in ["HELIOS_UPDATER_SOCKET", "UPDATER_SOCKET"] {
        if let Ok(value) = std::env::var(name) {
            push_unique(&mut candidates, PathBuf::from(value));
        }
    }
    push_unique(&mut candidates, PathBuf::from(DEV_UPDATER_SOCKET));
    push_unique(&mut candidates, PathBuf::from(UPDATER_SOCKET));
    candidates
}

fn push_unique(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}
