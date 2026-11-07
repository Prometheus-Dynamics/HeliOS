//! The HTTP server's lifetime. A stop (SIGTERM or SIGINT, what `systemctl stop` sends; see
//! `helios_shutdown`) cancels `AppState::shutdown`, which ends the long-lived streams (events,
//! logs, updates, previews and the live sockets) at once. The server then finishes what is left
//! within `GRACE`, so a client that never lets go cannot hold a stop (an upgrade waits on it, and
//! systemd would SIGKILL it).

use std::{
    net::SocketAddr,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use tokio::net::TcpListener;

use crate::{SharedState, router};

/// How long connections may take to finish once a stop is asked for.
pub const GRACE: Duration = Duration::from_secs(2);
/// How long blocking work (a camera read, a recording's write) may run on after the runtime is
/// asked to stop. Its threads end with the process.
pub const BLOCKING_GRACE: Duration = Duration::from_secs(2);

/// Requests the router has taken (the outermost layer counts them), for `release_after_requests`.
pub static REQUESTS: AtomicU64 = AtomicU64::new(0);
/// How often the heap is given back after requests.
pub const RELEASE_EVERY: Duration = Duration::from_secs(5);

/// Every `RELEASE_EVERY` in which requests were answered, gives the heap's free pages back:
/// a burst of requests (a UI opening, several viewers at once) leaves its freed buffers spread
/// over the heap, where glibc keeps them (it only returns the top), so without this the burst's
/// peak stays the process's size.
async fn release_after_requests(stop: tokio_util::sync::CancellationToken) {
    let mut seen = REQUESTS.load(Ordering::Relaxed);
    let mut quiet_once = false;
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            () = tokio::time::sleep(RELEASE_EVERY) => {}
        }
        // Once more on the first quiet tick after them: a stream (events, a preview) frees its
        // buffers when it ends, after the request that opened it.
        let now = REQUESTS.load(Ordering::Relaxed);
        if now != seen || quiet_once {
            quiet_once = now != seen;
            seen = now;
            let _ = tokio::task::spawn_blocking(crate::host::release_heap).await;
        }
    }
}

/// Serves `state`'s router on `listener` until `state.shutdown` is cancelled, then returns once
/// the connections have finished or `GRACE` has passed.
pub async fn serve(listener: TcpListener, state: SharedState) -> std::io::Result<()> {
    let stop = state.shutdown.clone();
    let _release = tokio::spawn(release_after_requests(stop.clone()));
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
