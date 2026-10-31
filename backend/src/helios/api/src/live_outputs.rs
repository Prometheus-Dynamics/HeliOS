//! Pipeline outputs as they are produced: helios-engine's live socket (`helios_engine::live`,
//! `HELIOS_ENGINE_LIVE_SOCKET`: every output of every tick, newline-delimited JSON with the
//! frame's capture time) relayed to clients on `GET /v1/live/ws`, unchanged and in full.
//!
//! One generic stream for everything that reads outputs: a camera view matches each preview
//! frame to the outputs of the same `frame_ts_ns`, tables and charts take what they show, tools
//! take everything. The socket is read only while a client is connected, and the relay never
//! waits on a slow client (it skips to the newest data).
//!
//! What waits for slow clients is bounded in bytes, not chunks (`HELIOS_API_LIVE_BUFFER_BYTES`,
//! [`DEFAULT_BUFFER_BYTES`]): the engine writes as fast as it is read, so chunks are up to a whole
//! read each, and one stalled reader (a slow link, or a self-calibration store waiting on Orion
//! while the engine restarts) used to pin a thousand of them, ~64 MiB of the brk heap that glibc
//! never gave back. The buffer is let go once nobody is left.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

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
    sync::{Notify, broadcast::error::RecvError, watch},
};

use crate::SharedState;

/// Wait between connection attempts while the engine is not there.
const RETRY: Duration = Duration::from_secs(1);
/// Bytes of chunks kept for slow readers; beyond this a reader skips to the newest chunk.
pub const DEFAULT_BUFFER_BYTES: usize = 4 << 20;
/// The longest line read from the engine (a saved map is base64 of up to 16 MiB); a longer one is
/// dropped rather than buffered.
const MAX_LINE_BYTES: usize = 32 << 20;
/// What the line buffer keeps between reads; a long line's growth is given back after it.
const LINE_BUFFER_BYTES: usize = 256 * 1024;

/// The socket from `HELIOS_ENGINE_LIVE_SOCKET`, or the layout's engine live socket.
pub fn socket_path(layout: &helios_layout::Layout) -> PathBuf {
    std::env::var_os("HELIOS_ENGINE_LIVE_SOCKET").map_or_else(|| layout.engine_live_socket(), PathBuf::from)
}

/// The newest chunks, `first` the sequence number of the oldest one kept.
#[derive(Default)]
struct Ring {
    chunks: VecDeque<Arc<str>>,
    first: u64,
    bytes: usize,
}

impl Ring {
    /// The sequence number the next chunk gets.
    fn end(&self) -> u64 {
        self.first + self.chunks.len() as u64
    }
}

/// The relay: whole lines read from the engine, fanned out to every client.
pub struct LiveRelay {
    ring: Mutex<Ring>,
    budget: usize,
    /// The ring's end after each chunk: readers wait on it.
    published: watch::Sender<u64>,
    clients: AtomicUsize,
    wanted: Notify,
}

impl LiveRelay {
    /// `budget`: bytes kept for slow readers (the newest chunk is always kept, whatever its size).
    pub fn new(budget: usize) -> Arc<Self> {
        Arc::new(Self { ring: Mutex::default(), budget, published: watch::channel(0).0, clients: AtomicUsize::new(0), wanted: Notify::new() })
    }

    fn ring(&self) -> std::sync::MutexGuard<'_, Ring> {
        // Every update leaves the ring consistent.
        self.ring.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn subscribe(self: &Arc<Self>) -> LiveReceiver {
        self.clients.fetch_add(1, Ordering::AcqRel);
        let changes = self.published.subscribe();
        let next = self.ring().end();
        self.wanted.notify_one();
        LiveReceiver { relay: Arc::clone(self), next, changes }
    }

    fn clients(&self) -> usize {
        self.clients.load(Ordering::Acquire)
    }

    fn publish(&self, chunk: Arc<str>) {
        let end = {
            let mut ring = self.ring();
            ring.bytes += chunk.len();
            ring.chunks.push_back(chunk);
            while ring.bytes > self.budget && ring.chunks.len() > 1 {
                let oldest = ring.chunks.pop_front().expect("more than one chunk");
                ring.bytes -= oldest.len();
                ring.first += 1;
            }
            ring.end()
        };
        self.published.send_replace(end);
    }

    /// Lets the kept chunks go (nobody is left to read them).
    fn release(&self) {
        let mut ring = self.ring();
        ring.first = ring.end();
        ring.chunks = VecDeque::new();
        ring.bytes = 0;
    }

    /// Bytes kept for readers now.
    pub fn buffered_bytes(&self) -> usize {
        self.ring().bytes
    }
}

/// One reader of the relay: every chunk from its subscription on, or, when it falls behind by more
/// than the relay keeps, [`RecvError::Lagged`] and then the newest chunk.
pub struct LiveReceiver {
    relay: Arc<LiveRelay>,
    next: u64,
    changes: watch::Receiver<u64>,
}

impl LiveReceiver {
    pub async fn recv(&mut self) -> Result<Arc<str>, RecvError> {
        loop {
            // Marked seen before the ring is read: a chunk published after this wakes the wait.
            self.changes.borrow_and_update();
            {
                let ring = self.relay.ring();
                if self.next < ring.first && ring.chunks.is_empty() {
                    // Released while subscribing: nothing was kept to miss.
                    self.next = ring.end();
                } else if self.next < ring.first {
                    let newest = ring.end() - 1;
                    let missed = newest - self.next;
                    self.next = newest;
                    return Err(RecvError::Lagged(missed));
                }
                if let Some(chunk) = ring.chunks.get((self.next - ring.first) as usize) {
                    self.next += 1;
                    return Ok(Arc::clone(chunk));
                }
            }
            // The sender lives in the relay this receiver holds: it is never closed.
            self.changes.changed().await.map_err(|_| RecvError::Closed)?;
        }
    }
}

impl Drop for LiveReceiver {
    fn drop(&mut self) {
        if self.relay.clients.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.relay.release();
        }
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
    let mut buffer = Vec::with_capacity(LINE_BUFFER_BYTES);
    let mut chunk = vec![0_u8; 64 * 1024];
    // Inside a line too long to keep: dropped up to its newline.
    let mut skipping = false;
    let mut idle = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            read = stream.read(&mut chunk) => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut read = &chunk[..n];
                    if skipping {
                        let Some(newline) = read.iter().position(|b| *b == b'\n') else { continue };
                        read = &read[newline + 1..];
                        skipping = false;
                    }
                    buffer.extend_from_slice(read);
                    // Everything up to the last newline is whole lines; the rest waits.
                    if let Some(end) = buffer.iter().rposition(|b| *b == b'\n') {
                        relay.publish(Arc::from(String::from_utf8_lossy(&buffer[..=end]).as_ref()));
                        buffer.drain(..=end);
                    }
                    if buffer.len() > MAX_LINE_BYTES {
                        tracing::warn!(bytes = buffer.len(), "live outputs: a line longer than {MAX_LINE_BYTES} bytes, dropped");
                        buffer.clear();
                        skipping = true;
                    }
                    if buffer.capacity() > LINE_BUFFER_BYTES && buffer.len() <= LINE_BUFFER_BYTES {
                        buffer.shrink_to(LINE_BUFFER_BYTES);
                    }
                }
            },
            // Disconnecting with nobody left stops the engine serializing for no one.
            _ = idle.tick() => if relay.clients() == 0 {
                let _ = stream.shutdown().await;
                break;
            },
        }
    }
    if relay.clients() == 0 {
        relay.release();
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
    let shutdown = state.shutdown.clone();
    upgrade.on_upgrade(move |socket| relay_to(socket, rx, filter, shutdown))
}

async fn relay_to(mut socket: WebSocket, mut rx: LiveReceiver, filter: LiveFilter, shutdown: tokio_util::sync::CancellationToken) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
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
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
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

    fn chunk(n: usize, bytes: usize) -> Arc<str> {
        let line = format!(r#"{{"pipeline":"p","port":"map_snapshot","value":{n}}}"#);
        let mut text = line.repeat(bytes / (line.len() + 1) + 1).replace("}{", "}\n{");
        text.push('\n');
        text.into()
    }

    #[tokio::test]
    async fn a_stalled_reader_pins_no_more_than_the_budget() {
        let budget = 1 << 20;
        let relay = LiveRelay::new(budget);
        let mut stalled = relay.subscribe();
        let mut largest = 0;
        // 600 MiB through the relay while the reader reads nothing: the old 1024-chunk ring held
        // ~64 MiB of these.
        for n in 0..10_000 {
            let chunk = chunk(n, 60 * 1024);
            largest = largest.max(chunk.len());
            relay.publish(chunk);
            assert!(relay.buffered_bytes() <= budget, "{} bytes kept at chunk {n}", relay.buffered_bytes());
        }
        assert!(relay.buffered_bytes() + largest > budget, "the budget is used");
        // Behind: told so, then the newest chunk.
        assert!(matches!(stalled.recv().await, Err(RecvError::Lagged(_))));
        assert!(stalled.recv().await.expect("newest").contains(r#""value":9999"#));
        // Nobody left: nothing kept.
        drop(stalled);
        assert_eq!(relay.buffered_bytes(), 0);
    }

    #[tokio::test]
    async fn the_newest_chunk_is_kept_whatever_its_size() {
        let relay = LiveRelay::new(64 << 10);
        let mut reader = relay.subscribe();
        relay.publish(chunk(0, 1 << 20));
        assert!(relay.buffered_bytes() >= 1 << 20);
        assert!(reader.recv().await.expect("big chunk").contains(r#""value":0"#));
        relay.publish(chunk(1, 100));
        assert!(relay.buffered_bytes() < 64 << 10, "the big one goes once a newer chunk is there");
    }

    #[tokio::test]
    async fn a_reader_that_keeps_up_gets_every_chunk() {
        let relay = LiveRelay::new(DEFAULT_BUFFER_BYTES);
        let mut reader = relay.subscribe();
        let writer = {
            let relay = relay.clone();
            tokio::spawn(async move {
                for n in 0..2_000 {
                    relay.publish(chunk(n, 100));
                    tokio::task::yield_now().await;
                }
            })
        };
        for n in 0..2_000 {
            assert!(reader.recv().await.expect("chunk").contains(&format!(r#""value":{n}}}"#)));
        }
        writer.await.expect("writer");
    }

    /// The device's case end to end: the engine's socket at full speed, one reader stalled.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_engine_socket_with_a_stalled_reader_stays_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("live.sock");
        let listener = tokio::net::UnixListener::bind(&socket).expect("bind");
        let total: usize = 256 << 20;
        let engine = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let lines = chunk(7, 256 * 1024);
            let mut sent = 0;
            while sent < total {
                if stream.write_all(lines.as_bytes()).await.is_err() {
                    return sent;
                }
                sent += lines.len();
            }
            sent
        });
        let budget = 2 << 20;
        let relay = LiveRelay::new(budget);
        let mut stalled = relay.subscribe();
        let follower = spawn(relay.clone(), socket);
        let mut peak = 0;
        while !engine.is_finished() {
            peak = peak.max(relay.buffered_bytes());
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(engine.await.expect("engine") >= total, "the engine never waits on the stalled reader");
        tokio::time::sleep(Duration::from_millis(50)).await;
        peak = peak.max(relay.buffered_bytes());
        // A chunk is at most one read (64 KiB) plus the line it finishes.
        assert!(peak <= budget + 64 * 1024 + 256 * 1024, "peak {peak} bytes kept for a stalled reader");
        assert!(matches!(stalled.recv().await, Err(RecvError::Lagged(_))));
        assert!(stalled.recv().await.expect("newest").ends_with('\n'));
        drop(stalled);
        assert_eq!(relay.buffered_bytes(), 0, "let go once nobody is left");
        follower.abort();
    }
}
