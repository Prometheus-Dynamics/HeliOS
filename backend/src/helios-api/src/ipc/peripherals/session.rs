use std::{
    io,
    time::{Duration, Instant},
};

use lib_ipc::client::ClientTransportError;
use lib_ipc::types::CommandId;
use tokio::time::timeout;

use super::config::MAX_IDLE_SESSIONS;
use super::{IdleSession, SensorsClient, SensorsConnection, SensorsSession};
use helios_peripherals::ipc::{SensorCommand, SensorEvent};

async fn await_sensor<T, F>(client: &SensorsClient, session: &mut SensorsSession, command: SensorCommand, mut map: F) -> Result<Result<T, String>, ClientTransportError>
where
    F: FnMut(SensorEvent, CommandId) -> Option<Result<T, String>>,
{
    const SENSOR_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

    let command_id = command.command_id();
    session.send_command(client.journal(), &command).await?;
    let deadline = tokio::time::Instant::now() + SENSOR_RESPONSE_TIMEOUT;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(Err("peripheral request timed out".into()));
        }
        let remaining = deadline - now;
        match timeout(remaining, session.next_event()).await {
            Ok(Ok(Some(event))) => {
                if let Some(mapped) = map(event, command_id) {
                    return Ok(mapped);
                }
            }
            Ok(Ok(None)) => return Ok(Err("no response".into())),
            Ok(Err(err)) => return Err(err),
            Err(_) => return Ok(Err("peripheral request timed out".into())),
        }
    }
}

impl SensorsConnection {
    pub(super) async fn checkout_session(&self) -> Result<SensorsSession, ClientTransportError> {
        let idle_timeout = self.session_idle_timeout;
        if idle_timeout > Duration::from_millis(0) {
            let now = Instant::now();
            let mut guard = self.sessions.lock().await;
            guard.retain(|idle| now.duration_since(idle.last_used) <= idle_timeout);
            if let Some(idle) = guard.pop() {
                return Ok(idle.session);
            }
        }
        match timeout(Duration::from_secs(5), self.client.handshake()).await {
            Ok(Ok(session)) => Ok(session),
            Ok(Err(err)) => Err(ClientTransportError::Io(io::Error::other(err.to_string()))),
            Err(_) => Err(ClientTransportError::Io(io::Error::new(io::ErrorKind::TimedOut, "sensors handshake timed out"))),
        }
    }

    pub(super) async fn recycle_session(&self, session: SensorsSession) {
        let idle_timeout = self.session_idle_timeout;
        if idle_timeout <= Duration::from_millis(0) {
            return;
        }
        let now = Instant::now();
        let idle = IdleSession { session, last_used: now };
        let mut guard = self.sessions.lock().await;
        guard.retain(|entry| now.duration_since(entry.last_used) <= idle_timeout);
        guard.push(idle);
        if guard.len() > MAX_IDLE_SESSIONS
            && let Some((idx, _)) = guard.iter().enumerate().min_by_key(|(_, entry)| entry.last_used)
        {
            guard.swap_remove(idx);
        }
    }

    pub(super) async fn run_command<T, F>(&self, command: SensorCommand, map: F) -> Result<Result<T, String>, ClientTransportError>
    where
        F: FnMut(SensorEvent, CommandId) -> Option<Result<T, String>>,
    {
        let mut session = self.checkout_session().await?;
        let result = await_sensor(&self.client, &mut session, command, map).await;
        if result.is_ok() {
            self.recycle_session(session).await;
        }
        result
    }
}
