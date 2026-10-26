//! Pipeline outputs as they are produced: helios-engine's live socket (`helios_engine::live`,
//! `HELIOS_ENGINE_LIVE_SOCKET`: every output of every tick, newline-delimited JSON with the
//! frame's capture time) relayed to clients on `GET /v1/live/ws`, unchanged and in full.
//!
//! One generic stream for everything that reads outputs: a camera view matches each preview
//! frame to the outputs of the same `frame_ts_ns`, tables and charts take what they show, tools
//! take everything. The socket is read only while a client is connected, and the relay never
//! waits on a slow client (it skips to the newest data).

use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::broadcast,
};

use crate::SharedState;

/// Wait between connection attempts while the engine is not there.
const RETRY: Duration = Duration::from_secs(1);
/// Chunks waiting for slow clients; beyond this a client skips ahead.
const FANOUT: usize = 1024;

/// The socket from `HELIOS_ENGINE_LIVE_SOCKET`, or the layout's engine live socket.
pub fn socket_path(layout: &helios_layout::Layout) -> PathBuf {
    std::env::var_os("HELIOS_ENGINE_LIVE_SOCKET").map_or_else(|| layout.engine_live_socket(), PathBuf::from)
}

/// The relay: whole lines read from the engine, fanned out to every client.
pub struct LiveRelay {
    lines: broadcast::Sender<Arc<str>>,
    wanted: tokio::sync::Notify,
}

impl LiveRelay {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { lines: broadcast::channel(FANOUT).0, wanted: tokio::sync::Notify::new() })
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        let rx = self.lines.subscribe();
        self.wanted.notify_one();
        rx
    }

    fn clients(&self) -> usize {
        self.lines.receiver_count()
    }
}

/// Read the engine's socket while anyone is connected to `/v1/live/ws`.
pub fn spawn(relay: Arc<LiveRelay>, socket: PathBuf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            while relay.clients() == 0 {
                relay.wanted.notified().await;
            }
            match UnixStream::connect(&socket).await {
                Ok(stream) => follow(&relay, stream).await,
                Err(_) => tokio::time::sleep(RETRY).await,
            }
        }
    })
}

/// Fan out what the engine sends, in chunks of whole lines, until it closes or nobody is left.
async fn follow(relay: &LiveRelay, mut stream: UnixStream) {
    let mut buffer = Vec::with_capacity(256 * 1024);
    let mut chunk = vec![0_u8; 64 * 1024];
    let mut idle = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            read = stream.read(&mut chunk) => match read {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    buffer.extend_from_slice(&chunk[..n]);
                    // Everything up to the last newline is whole lines; the rest waits.
                    if let Some(end) = buffer.iter().rposition(|b| *b == b'\n') {
                        let lines = String::from_utf8_lossy(&buffer[..=end]).into_owned();
                        buffer.drain(..=end);
                        let _ = relay.lines.send(lines.into());
                    }
                }
            },
            // Disconnecting with nobody left stops the engine serializing for no one.
            _ = idle.tick() => if relay.clients() == 0 {
                let _ = stream.shutdown().await;
                return;
            },
        }
    }
}

/// What a client asks for: only these pipelines and/or ports (all when absent), comma-separated.
#[derive(Debug, Default, Deserialize)]
pub struct LiveFilter {
    pub pipelines: Option<String>,
    pub ports: Option<String>,
}

impl LiveFilter {
    fn parts(text: &Option<String>) -> Vec<String> {
        text.as_deref().unwrap_or_default().split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
    }

    /// The lines of `chunk` this filter keeps (all of them without a filter).
    fn keep(&self, chunk: &str) -> Option<String> {
        let (pipelines, ports) = (Self::parts(&self.pipelines), Self::parts(&self.ports));
        if pipelines.is_empty() && ports.is_empty() {
            return None;
        }
        let mut out = String::new();
        for line in chunk.lines() {
            let Ok(head) = serde_json::from_str::<LineHead>(line) else { continue };
            if (pipelines.is_empty() || pipelines.contains(&head.pipeline)) && (ports.is_empty() || ports.contains(&head.port)) {
                out.push_str(line);
                out.push('\n');
            }
        }
        Some(out)
    }
}

#[derive(Deserialize)]
struct LineHead {
    pipeline: String,
    port: String,
}

/// `GET /v1/live/ws[?pipelines=a,b][&ports=detections,tracks]`: every output of every tick, as
/// text messages of newline-delimited JSON (`{"pipeline", "port", "observed_at_ms",
/// "frame_ts_ns", "value"}` per line).
pub async fn live_ws(State(state): State<SharedState>, Query(filter): Query<LiveFilter>, upgrade: WebSocketUpgrade) -> Response {
    let rx = state.live.subscribe();
    upgrade.on_upgrade(move |socket| relay_to(socket, rx, filter))
}

async fn relay_to(mut socket: WebSocket, mut rx: broadcast::Receiver<Arc<str>>, filter: LiveFilter) {
    loop {
        tokio::select! {
            chunk = rx.recv() => match chunk {
                Ok(chunk) => {
                    let text = match filter.keep(&chunk) {
                        Some(kept) if kept.is_empty() => continue,
                        Some(kept) => kept,
                        None => chunk.to_string(),
                    };
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        return;
                    }
                }
                // Behind: the newer chunks are coming; skip to them.
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            },
            message = socket.next() => match message {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_keeps_the_lines_asked_for() {
        let chunk = concat!(
            r#"{"pipeline":"front","port":"detections","value":{}}"#,
            "\n",
            r#"{"pipeline":"front","port":"tracks","value":{}}"#,
            "\n",
            r#"{"pipeline":"back","port":"detections","value":{}}"#,
            "\n"
        );
        assert!(LiveFilter::default().keep(chunk).is_none(), "no filter: everything as is");
        let ports = LiveFilter { ports: Some("detections".into()), ..LiveFilter::default() }.keep(chunk).expect("filtered");
        assert_eq!(ports.lines().count(), 2);
        let both = LiveFilter { pipelines: Some("front".into()), ports: Some("tracks".into()) }.keep(chunk).expect("filtered");
        assert_eq!(both.lines().count(), 1);
        assert!(both.contains("\"tracks\""));
    }
}
