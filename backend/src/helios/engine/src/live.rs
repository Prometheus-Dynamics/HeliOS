//! Pipeline outputs, live, for helios-api (and through it the UI): every graph tick's host outputs
//! on a local Unix socket as they are produced, instead of waiting for the next Orion publish
//! (`HELIOS_ENGINE_PUBLISH_INTERVAL_MS`, 250 ms). Tag overlays and values then follow the frames.
//!
//! The socket (`HELIOS_ENGINE_LIVE_SOCKET`, the layout's `engine_live_socket`) carries newline-delimited JSON,
//! one line per output and tick: `{"pipeline", "port", "observed_at_ms", "frame_ts_ns",
//! "value"}`, the value being the same JSON `GET /v1/outputs` shows and `frame_ts_ns` the capture
//! time of the frame the tick ran on (the preview's frames carry the same clock), so a viewer can
//! draw each frame with its own detections. Every tick is sent; a reader that falls behind skips
//! to the newest lines instead of holding the engine back.
//!
//! Graph threads only hand the tick's payloads over (shared, no copy), and only while someone is
//! connected; serializing happens here, once per output whatever the number of subscribers.
//!
//! A subscriber may name the ports it reads by writing a line `{"ports":["pose","map_pose"]}`
//! (`{"ports":null}`: every port, the default), at any time; the latest line counts. Only ports
//! some subscriber wants are serialized: a self-calibration watching two pose ports no longer has
//! every output of every frame turned into JSON for it.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use daedalus::{runtime::host_bridge::ValueSerializerMap, transport::Payload};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, unix::OwnedReadHalf},
    sync::{broadcast, mpsc, watch},
};
use tracing::{info, warn};

use crate::execution::outputs::payload_message;

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
    /// The capture time of the frame the tick ran on (`FrameMeta::timestamp`), when it had one.
    pub frame_ts_ns: Option<u64>,
}

/// Where graph threads hand their ticks over. Cheap to clone; a no-op with nobody connected.
#[derive(Clone, Default)]
pub struct LiveSink {
    inner: Option<Arc<SinkInner>>,
}

struct SinkInner {
    tx: mpsc::Sender<LiveTick>,
    subscribers: AtomicUsize,
    /// Each subscriber's ports, by subscriber.
    filters: Mutex<BTreeMap<u64, Ports>>,
    /// The ports any subscriber wants (their union).
    wanted: RwLock<Ports>,
    next_id: AtomicU64,
}

impl SinkInner {
    fn set_filter(&self, id: u64, ports: Option<Ports>) {
        let mut filters = self.filters.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match ports {
            Some(ports) => filters.insert(id, ports),
            None => filters.remove(&id),
        };
        let union = filters.values().fold(Ports::none(), |union, ports| union.union(ports));
        *self.wanted.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = union;
    }
}

/// The ports a subscriber reads: all of them, or only these.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ports {
    All,
    Only(BTreeSet<String>),
}

impl Ports {
    fn none() -> Ports {
        Ports::Only(BTreeSet::new())
    }

    fn wants(&self, port: &str) -> bool {
        match self {
            Ports::All => true,
            Ports::Only(ports) => ports.contains(port),
        }
    }

    fn union(self, other: &Ports) -> Ports {
        match (self, other) {
            (Ports::Only(mut mine), Ports::Only(theirs)) => {
                mine.extend(theirs.iter().cloned());
                Ports::Only(mine)
            }
            _ => Ports::All,
        }
    }

    /// A subscriber's request line, `{"ports": [...] | null}` (`None`: not a request).
    fn parse(line: &str) -> Option<Ports> {
        #[derive(serde::Deserialize)]
        struct Request {
            ports: Option<BTreeSet<String>>,
        }
        let request: Request = serde_json::from_str(line).ok()?;
        Some(request.ports.map_or(Ports::All, Ports::Only))
    }
}

impl LiveSink {
    /// Whether a tick handed over now would reach anyone.
    pub fn active(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.subscribers.load(Ordering::Acquire) > 0)
    }

    /// Hand a tick over without waiting, only the ports someone wants; dropped when the serializer
    /// is behind.
    pub fn offer(&self, mut tick: LiveTick) {
        if let Some(inner) = &self.inner {
            {
                let wanted = inner.wanted.read().unwrap_or_else(|poisoned| poisoned.into_inner());
                if *wanted != Ports::All {
                    tick.outputs.retain(|(port, _)| wanted.wants(port));
                }
            }
            if !tick.outputs.is_empty() {
                let _ = inner.tx.try_send(tick);
            }
        }
    }
}

/// One output, serialized: its port and its line.
#[derive(Clone)]
struct Line {
    port: Arc<str>,
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
    let inner = Arc::new(SinkInner { tx, subscribers: AtomicUsize::new(0), filters: Mutex::default(), wanted: RwLock::new(Ports::none()), next_id: AtomicU64::new(0) });
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
            let _ = lines.send(Line { port: port.as_str().into(), text: line(&tick.pipeline, port, observed_at_ms, tick.frame_ts_ns, &message).into() });
        }
    }
}

/// One output as a line: its value inline when the message is JSON, else as a JSON string (as
/// `GET /v1/outputs` parses it).
fn line(pipeline: &str, port: &str, observed_at_ms: u64, frame_ts_ns: Option<u64>, message: &str) -> String {
    let value = if serde_json::from_str::<serde::de::IgnoredAny>(message).is_ok() { message.to_string() } else { serde_json::Value::String(message.to_string()).to_string() };
    let pipeline = serde_json::Value::String(pipeline.to_string());
    let port = serde_json::Value::String(port.to_string());
    let frame = frame_ts_ns.map_or_else(|| "null".to_string(), |ts| ts.to_string());
    format!("{{\"pipeline\":{pipeline},\"port\":{port},\"observed_at_ms\":{observed_at_ms},\"frame_ts_ns\":{frame},\"value\":{value}}}\n")
}

async fn accept(listener: UnixListener, lines: broadcast::Sender<Line>, inner: Arc<SinkInner>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let id = inner.next_id.fetch_add(1, Ordering::Relaxed);
                inner.set_filter(id, Some(Ports::All));
                inner.subscribers.fetch_add(1, Ordering::AcqRel);
                let (rx, inner) = (lines.subscribe(), inner.clone());
                tokio::spawn(async move {
                    let (read, write) = stream.into_split();
                    let (filter_tx, filter) = watch::channel(Ports::All);
                    // A subscriber that closes (or shuts its side down) is gone: its reads end.
                    tokio::select! {
                        () = serve(write, rx, filter) => {}
                        () = read_requests(read, id, &inner, filter_tx) => {}
                    }
                    inner.subscribers.fetch_sub(1, Ordering::AcqRel);
                    inner.set_filter(id, None);
                });
            }
            Err(error) => {
                warn!(%error, "live outputs: accept failed");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

/// A subscriber's port requests, one JSON line each; other lines are ignored.
async fn read_requests(read: OwnedReadHalf, id: u64, inner: &SinkInner, filter: watch::Sender<Ports>) {
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(ports) = Ports::parse(&line) {
            inner.set_filter(id, Some(ports.clone()));
            filter.send_replace(ports);
        }
    }
}

/// Send one subscriber every line of its ports as it comes, whatever is queued written at once.
async fn serve(mut stream: impl AsyncWriteExt + Unpin, mut rx: broadcast::Receiver<Line>, filter: watch::Receiver<Ports>) {
    let mut buffer = Vec::with_capacity(16 * 1024);
    loop {
        let first = match rx.recv().await {
            Ok(line) => line,
            // Behind: skip to the newer lines; the latest of each is what a viewer draws.
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        };
        buffer.clear();
        {
            let ports = filter.borrow();
            for line in std::iter::once(first).chain(std::iter::from_fn(|| rx.try_recv().ok())) {
                if ports.wants(&line.port) {
                    buffer.extend_from_slice(line.text.as_bytes());
                }
            }
        }
        if !buffer.is_empty() && stream.write_all(&buffer).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixStream;

    #[test]
    fn lines_carry_json_values_inline_and_text_as_strings() {
        let json: serde_json::Value = serde_json::from_str(line("front", "detections", 7, Some(42), r#"{"a":1}"#).trim()).expect("json");
        assert_eq!(json, serde_json::json!({ "pipeline": "front", "port": "detections", "observed_at_ms": 7, "frame_ts_ns": 42, "value": { "a": 1 } }));
        let text: serde_json::Value = serde_json::from_str(line("front", "note", 7, None, "not json").trim()).expect("json");
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
            sink.offer(LiveTick {
                pipeline: "front".into(),
                outputs: vec![("count".into(), Payload::owned("test:count", format!("{{\"n\":{n}}}")))],
                serializers: ValueSerializerMap::default(),
                frame_ts_ns: Some(n),
            });
        }
        let first = tokio::time::timeout(Duration::from_secs(2), reader.next_line()).await.expect("in time").expect("read").expect("a line");
        let json: serde_json::Value = serde_json::from_str(&first).expect("json");
        assert_eq!(json["pipeline"], "front");
        assert_eq!(json["port"], "count");
        assert!(json["value"]["n"].as_i64().is_some(), "{json}");
    }

    #[tokio::test]
    async fn a_subscriber_naming_its_ports_gets_only_those_and_the_rest_are_not_serialized() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("live.sock");
        let sink = spawn(socket.clone());
        let mut stream = UnixStream::connect(&socket).await.expect("connect");
        stream.write_all(b"{\"ports\":[\"pose\"]}\n").await.expect("request");
        let inner = sink.inner.clone().expect("served");
        for _ in 0..200 {
            if *inner.wanted.read().unwrap() == Ports::Only(BTreeSet::from(["pose".to_string()])) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(*inner.wanted.read().unwrap(), Ports::Only(BTreeSet::from(["pose".to_string()])));
        let tick = |n: u64| LiveTick {
            pipeline: "front".into(),
            outputs: vec![("detections".into(), Payload::owned("test:json", "{}".to_string())), ("pose".into(), Payload::owned("test:json", format!("{{\"n\":{n}}}")))],
            serializers: ValueSerializerMap::default(),
            frame_ts_ns: Some(n),
        };
        sink.offer(tick(1));
        let mut reader = BufReader::new(stream).lines();
        let first = tokio::time::timeout(Duration::from_secs(2), reader.next_line()).await.expect("in time").expect("read").expect("a line");
        let json: serde_json::Value = serde_json::from_str(&first).expect("json");
        assert_eq!(json["port"], "pose");
        assert_eq!(json["value"]["n"], 1);
        drop(reader);
        for _ in 0..200 {
            if !sink.active() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(*inner.wanted.read().unwrap(), Ports::none(), "a gone subscriber wants nothing");
    }

    #[test]
    fn requests_parse_and_unions_widen() {
        assert_eq!(Ports::parse(r#"{"ports":null}"#), Some(Ports::All));
        assert_eq!(Ports::parse(r#"{"ports":["a"]}"#), Some(Ports::Only(BTreeSet::from(["a".to_string()]))));
        assert_eq!(Ports::parse("hello"), None);
        let a = Ports::Only(BTreeSet::from(["a".to_string()]));
        assert_eq!(a.clone().union(&Ports::Only(BTreeSet::from(["b".to_string()]))), Ports::Only(BTreeSet::from(["a".to_string(), "b".to_string()])));
        assert_eq!(a.union(&Ports::All), Ports::All);
    }
}
