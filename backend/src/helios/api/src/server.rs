//! The HTTP server's lifetime. A stop (SIGTERM or SIGINT, what `systemctl stop` sends) cancels
//! `AppState::shutdown`, which ends the long-lived streams (events, logs, updates, previews and
//! the live sockets) at once. The server then finishes what is left within `GRACE`, so a client
//! that never lets go cannot hold a stop (an upgrade waits on it, and systemd would SIGKILL it).

use std::{net::SocketAddr, time::Duration};

use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{SharedState, router};

/// How long connections may take to finish once a stop is asked for.
pub const GRACE: Duration = Duration::from_secs(2);
/// How long blocking work (a camera read, a recording's write) may run on after the runtime is
/// asked to stop. Its threads end with the process.
pub const BLOCKING_GRACE: Duration = Duration::from_secs(2);

/// Cancels `shutdown` on SIGINT or SIGTERM.
pub fn stop_on_signal(shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut stream) => {
                    stream.recv().await;
                }
                Err(_) => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate => {}
        }
        shutdown.cancel();
    })
}

/// Serves `state`'s router on `listener` until `state.shutdown` is cancelled, then returns once
/// the connections have finished or `GRACE` has passed.
pub async fn serve(listener: TcpListener, state: SharedState) -> std::io::Result<()> {
    let stop = state.shutdown.clone();
    let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
    let server = axum::serve(listener, app).with_graceful_shutdown(stop.clone().cancelled_owned());
    tokio::select! {
        served = server => served,
        _ = async { stop.cancelled().await; tokio::time::sleep(GRACE).await } => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::{ApiConfig, AppState};

    #[tokio::test]
    async fn a_stop_ends_an_open_event_stream_within_the_grace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::new(ApiConfig { state_dir: dir.path().join("state"), ui_dir: None, ..ApiConfig::default() });
        let stop = state.shutdown.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(serve(listener, state));

        // A client that subscribes to the event stream and keeps the connection open.
        let mut client = tokio::net::TcpStream::connect(address).await.expect("connect");
        client.write_all(b"GET /v1/events HTTP/1.1\r\nHost: helios\r\nAccept: text/event-stream\r\n\r\n").await.expect("request");
        let mut seen = Vec::new();
        while !String::from_utf8_lossy(&seen).contains("event: hello") {
            let mut chunk = [0u8; 512];
            let read = client.read(&mut chunk).await.expect("read");
            assert!(read > 0, "the stream closed before its first event");
            seen.extend_from_slice(&chunk[..read]);
        }

        let asked = Instant::now();
        stop.cancel();
        let served = tokio::time::timeout(GRACE * 5, server).await.expect("the server stopped in time").expect("joined");
        assert!(served.is_ok());
        // The stream ends at once; only the grace period (not the stream) would take GRACE.
        assert!(asked.elapsed() < GRACE / 2, "stopping took {:?}", asked.elapsed());
    }
}
