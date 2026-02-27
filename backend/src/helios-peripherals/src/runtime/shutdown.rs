use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};

pub(super) async fn wait_for_shutdown(shutdown: CancellationToken) -> Result<&'static str> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{self, SignalKind};

        let mut sigterm = unix::signal(SignalKind::terminate()).map_err(|err| Error::InvalidState(format!("failed to install SIGTERM handler: {err}")))?;
        let mut sigint = unix::signal(SignalKind::interrupt()).map_err(|err| Error::InvalidState(format!("failed to install SIGINT handler: {err}")))?;

        tokio::select! {
            _ = shutdown.cancelled() => Ok("cancellation token"),
            _ = sigterm.recv() => Ok("SIGTERM"),
            _ = sigint.recv() => Ok("SIGINT"),
            result = tokio::signal::ctrl_c() => result.map(|_| "ctrl_c").map_err(|err| Error::InvalidState(format!("failed to listen for ctrl_c: {err}"))),
        }
    }

    #[cfg(not(unix))]
    {
        tokio::select! {
            _ = shutdown.cancelled() => Ok("cancellation token"),
            result = tokio::signal::ctrl_c() => result.map(|_| "ctrl_c").map_err(|err| Error::InvalidState(format!("failed to listen for ctrl_c: {err}"))),
        }
    }
}
