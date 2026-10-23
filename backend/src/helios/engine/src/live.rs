//! Pipeline outputs, live, for helios-api (and through it the UI): every graph tick's host outputs
//! on a local Unix socket as they are produced, instead of waiting for the next Orion publish
//! (`HELIOS_ENGINE_PUBLISH_INTERVAL_MS`, 250 ms). Tag overlays and values then follow the frames.
//!
//! The socket (`HELIOS_ENGINE_LIVE_SOCKET`, [`DEFAULT_SOCKET`]) carries newline-delimited JSON,
//! one line per output: `{"pipeline", "port", "observed_at_ms", "value"}`, the value being the
//! same JSON `GET /v1/outputs` shows. Each subscriber gets the latest value of every output at
//! most every [`FLUSH_INTERVAL`] (a 120 fps pipeline is coalesced to twice the preview's 15 fps),
//! so a slow reader never holds the engine back.
//!
//! Graph threads only hand the tick's payloads over (shared, no copy), and only while someone is
//! connected; serializing happens here, once per output whatever the number of subscribers.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use daedalus::{runtime::host_bridge::ValueSerializerMap, transport::Payload};
use tokio::{
    io::AsyncWriteExt,
    net::{UnixListener, UnixStream},
    sync::{broadcast, mpsc},
};
use tracing::{info, warn};

use crate::execution::outputs::payload_message;

/// Where helios-api finds the live outputs.
pub const DEFAULT_SOCKET: &str = "/run/helios/engine-live.sock";
/// How often a subscriber is sent what changed: 30 Hz, twice the camera preview's frame rate, so
/// overlays keep up with the video without sending feature tracks or maps 120 times a second.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(33);
/// Ticks waiting to be serialized; more are dropped (the newest tick matters, not every tick).
const QUEUE: usize = 32;
/// Serialized outputs waiting for the subscribers' flushes.
const FANOUT: usize = 512;
const SOCKET_MODE: u32 = 0o660;

/// One graph tick's outputs.
pub struct LiveTick {
    pub pipeline: String,
    pub outputs: Vec<(String, Payload)>,
    pub serializers: ValueSerializerMap,
}

/// Where graph threads hand their ticks over. Cheap to clone; a no-op with nobody connected.
#[derive(Clone, Default)]
pub struct LiveSink {
    inner: Option<Arc<SinkInner>>,
}

struct SinkInner {
    tx: mpsc::Sender<LiveTick>,
    subscribers: AtomicUsize,
}

impl LiveSink {
    /// Whether a tick handed over now would reach anyone.
    pub fn active(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.subscribers.load(Ordering::Acquire) > 0)
    }

    /// Hand a tick over without waiting; dropped when the serializer is behind.
    pub fn offer(&self, tick: LiveTick) {
        if let Some(inner) = &self.inner {
            let _ = inner.tx.try_send(tick);
        }
    }
}

/// One output, serialized: its key and its line.
#[derive(Clone)]
struct Line {
    key: Arc<str>,
    text: Arc<str>,
}

/// Start serving `socket` on the current Tokio runtime; returns the sink graph threads use. A
/// socket that cannot be bound is logged and the sink stays inactive.
pub fn spawn(socket: PathBuf) -> LiveSink {
    let listener = match bind(&socket) {
        Ok(listener) => listener,
        Err(error) => {
            warn!(socket = %socket.display(), %error, "live outputs not served");
            return LiveSink::default();
        }
    };
    info!(socket = %socket.display(), "live outputs served");
    let (tx, rx) = mpsc::channel(QUEUE);
    let inner = Arc::new(SinkInner { tx, subscribers: AtomicUsize::new(0) });
    let (lines, _) = broadcast::channel(FANOUT);
    tokio::spawn(serialize(rx, lines.clone()));
    tokio::spawn(accept(listener, lines, inner.clone()));
    LiveSink { inner: Some(inner) }
}

fn bind(socket: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // A socket left by an earlier run (a stale file refuses the bind).
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(SOCKET_MODE))?;
    Ok(listener)
}

/// Turn each tick's outputs into lines, once, for every subscriber.
async fn serialize(mut rx: mpsc::Receiver<LiveTick>, lines: broadcast::Sender<Line>) {
    while let Some(tick) = rx.recv().await {
        let observed_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        for (port, payload) in &tick.outputs {
            let message = payload_message(payload, &tick.serializers);
            let _ = lines.send(Line { key: format!("{}/{port}", tick.pipeline).into(), text: line(&tick.pipeline, port, observed_at_ms, &message).into() });
        }
    }
}

/// One output as a line: its value inline when the message is JSON, else as a JSON string (as
/// `GET /v1/outputs` parses it).
fn line(pipeline: &str, port: &str, observed_at_ms: u64, message: &str) -> String {
    let value = if serde_json::from_str::<serde::de::IgnoredAny>(message).is_ok() { message.to_string() } else { serde_json::Value::String(message.to_string()).to_string() };
    let pipeline = serde_json::Value::String(pipeline.to_string());
    let port = serde_json::Value::String(port.to_string());
    format!("{{\"pipeline\":{pipeline},\"port\":{port},\"observed_at_ms\":{observed_at_ms},\"value\":{value}}}\n")
}

async fn accept(listener: UnixListener, lines: broadcast::Sender<Line>, inner: Arc<SinkInner>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                inner.subscribers.fetch_add(1, Ordering::AcqRel);
                let (rx, inner) = (lines.subscribe(), inner.clone());
                tokio::spawn(async move {
                    serve(stream, rx).await;
                    inner.subscribers.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(error) => {
                warn!(%error, "live outputs: accept failed");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

/// Send one subscriber the latest line of every output that changed, every [`FLUSH_INTERVAL`].
async fn serve(mut stream: UnixStream, mut rx: broadcast::Receiver<Line>) {
    let mut pending: BTreeMap<Arc<str>, Arc<str>> = BTreeMap::new();
    let mut flush = tokio::time::interval(FLUSH_INTERVAL);
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut buffer = Vec::with_capacity(16 * 1024);
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Ok(line) => { pending.insert(line.key, line.text); }
                // Behind: the newer lines are still coming, and only the latest of each matters.
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            },
            _ = flush.tick(), if !pending.is_empty() => {
                buffer.clear();
                for (_, text) in std::mem::take(&mut pending) {
                    buffer.extend_from_slice(text.as_bytes());
                }
                if stream.write_all(&buffer).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[test]
    fn lines_carry_json_values_inline_and_text_as_strings() {
        let json: serde_json::Value = serde_json::from_str(line("front", "detections", 7, r#"{"a":1}"#).trim()).expect("json");
        assert_eq!(json, serde_json::json!({ "pipeline": "front", "port": "detections", "observed_at_ms": 7, "value": { "a": 1 } }));
        let text: serde_json::Value = serde_json::from_str(line("front", "note", 7, "not json").trim()).expect("json");
        assert_eq!(text["value"], "not json");
    }

    #[tokio::test]
    async fn a_subscriber_gets_the_latest_value_of_each_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("live.sock");
        let sink = spawn(socket.clone());
        assert!(!sink.active(), "nobody connected yet");
        let stream = UnixStream::connect(&socket).await.expect("connect");
        let mut reader = BufReader::new(stream).lines();
        for _ in 0..100 {
            if sink.active() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(sink.active());
        for n in 0..5 {
            sink.offer(LiveTick { pipeline: "front".into(), outputs: vec![("count".into(), Payload::owned("test:count", format!("{{\"n\":{n}}}")))], serializers: ValueSerializerMap::default() });
        }
        let first = tokio::time::timeout(Duration::from_secs(2), reader.next_line()).await.expect("in time").expect("read").expect("a line");
        let json: serde_json::Value = serde_json::from_str(&first).expect("json");
        assert_eq!(json["pipeline"], "front");
        assert_eq!(json["port"], "count");
        assert!(json["value"]["n"].as_i64().is_some(), "{json}");
    }
}
