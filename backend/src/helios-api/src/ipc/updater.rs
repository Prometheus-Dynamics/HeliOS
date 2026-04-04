use std::{path::PathBuf, sync::Arc};

use helios_updater::client::Error as UpdaterError;
use helios_updater::client::{UpdaterClient, UpdaterClientConfig, UpdaterSession};
use helios_updater::ipc::{UpdaterCommand, UpdaterEvent};
use lib_runtime_policy::HELIOS_UPDATER_FILESYSTEM_POLICY;
use tokio::time::{Duration, timeout};
use tracing::{error, info};

use crate::ipc::journal_path;

const DEV_UPDATER_SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/dev/run/updater.sock");

pub struct UpdaterConnection {
    pub client: Arc<UpdaterClient>,
}

pub async fn connect_updater() -> Result<UpdaterConnection, Box<dyn std::error::Error + Send + Sync>> {
    let journal_path = journal_path("updater.journal");
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
    let journal_entry = session.send_command(client.journal(), &command).await?;
    match session.next_event().await {
        Ok(Some(UpdaterEvent::Ack { command_id: ack_id, .. })) if ack_id == command_id => {
            info!("updater acked QueryState (journal offset {})", journal_entry.offset)
        }
        Ok(Some(other)) => info!("updater initial event: {:?}", other),
        Ok(None) => info!("updater closed connection after QueryState"),
        Err(err) => error!(%err, "failed waiting for updater event"),
    }

    drop(session);
    Ok(UpdaterConnection { client })
}

impl UpdaterConnection {
    pub async fn checkout_session(&self) -> Result<UpdaterSession, UpdaterError> {
        handshake_with_timeout(&self.client).await
    }

    pub async fn recycle_session(&self, session: UpdaterSession) {
        drop(session);
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
    let filesystem_policy = HELIOS_UPDATER_FILESYSTEM_POLICY.resolve();
    let default_socket = HELIOS_UPDATER_FILESYSTEM_POLICY.default_socket_path();
    if filesystem_policy.socket_path != default_socket {
        push_unique(&mut candidates, filesystem_policy.socket_path);
    }
    push_unique(&mut candidates, PathBuf::from(DEV_UPDATER_SOCKET));
    push_unique(&mut candidates, default_socket);
    candidates
}

fn push_unique(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::{DEV_UPDATER_SOCKET, resolve_updater_socket_candidates};
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().expect("env lock poisoned")
    }

    #[test]
    fn updater_candidates_prefer_canonical_socket_override() {
        let _lock = env_lock();
        unsafe {
            std::env::set_var("HELIOS_UPDATER_SOCKET", "/tmp/policy-updater.sock");
        }

        let candidates = resolve_updater_socket_candidates();
        assert_eq!(candidates, vec![PathBuf::from("/tmp/policy-updater.sock"), PathBuf::from(DEV_UPDATER_SOCKET), PathBuf::from("/run/helios/updater.sock"),]);

        unsafe {
            std::env::remove_var("HELIOS_UPDATER_SOCKET");
        }
    }

    #[test]
    fn updater_candidates_ignore_removed_socket_alias() {
        let _lock = env_lock();
        unsafe {
            std::env::remove_var("HELIOS_UPDATER_SOCKET");
            std::env::set_var("UPDATER_SOCKET", "/tmp/legacy-updater.sock");
        }

        let candidates = resolve_updater_socket_candidates();
        assert_eq!(candidates, vec![PathBuf::from(DEV_UPDATER_SOCKET), PathBuf::from("/run/helios/updater.sock")]);

        unsafe {
            std::env::remove_var("UPDATER_SOCKET");
        }
    }
}
