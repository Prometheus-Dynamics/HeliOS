//! Live streams to the UI (`GET /v1/streams/ws`, see `routes/streams.rs`).
//!
//! The node keeps a stream of typed series a publisher samples at full rate (Orion
//! `docs/streams.md`): by default `board.hardware`, lemnosd's devices, a series per channel. This
//! module holds **one** subscription to it, owned by the node, shared by every client, and fans
//! what arrives out to the clients:
//!
//! - the subscription starts with the first client and ends with the last (its unsubscribe, and
//!   the stream connection, go with it), so nothing is subscribed for nobody;
//! - each client gets the catalog (text), the recent history (the last `backfill_ms`, binary, see
//!   [`codec`]), then every batch as it arrives, from one broadcast channel that never blocks the
//!   publisher: a client that falls [`QUEUE_FRAMES`] behind loses its oldest frames, is told how
//!   many (`lagged`) and gets the catalog again;
//! - nothing is polled: a batch goes out when Orion delivers it.
//!
//! Frames are generation-tagged: a follower that outlived its subscribers cannot publish into
//! the next one's history.

pub mod codec;
mod follow;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::{broadcast, oneshot};

use crate::orion::Orion;

/// The stream on the node that the viewers show: lemnosd's devices, as board-agent names it.
pub const DEFAULT_STREAM_ID: &str = "board.hardware";
/// The history a new client starts with, ms.
pub const DEFAULT_BACKFILL_MS: u64 = 5_000;
/// Frames a client may fall behind before its oldest are dropped.
pub const QUEUE_FRAMES: usize = 256;
/// The most history frames kept, whatever their span (the backfill window caps them first).
const HISTORY_FRAMES: usize = 4_096;

/// Where the stream is and how much history a client starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveStreamSettings {
    pub stream_id: String,
    pub backfill_ms: u64,
}

impl Default for LiveStreamSettings {
    fn default() -> Self {
        Self { stream_id: DEFAULT_STREAM_ID.into(), backfill_ms: DEFAULT_BACKFILL_MS }
    }
}

/// What a client is sent: a text frame (JSON) or a binary frame (samples).
#[derive(Debug, Clone, PartialEq)]
pub enum LiveMessage {
    Text(Arc<str>),
    Binary(Arc<[u8]>),
}

/// A samples frame kept for clients that connect later.
struct HistoryFrame {
    newest_us: u64,
    bytes: Arc<[u8]>,
}

#[derive(Default)]
struct Inner {
    /// Clients subscribed now.
    clients: usize,
    /// Bumped whenever the follower changes (started, or the last client left).
    generation: u64,
    /// Stops the follower of the current generation.
    stop: Option<oneshot::Sender<()>>,
    /// The catalog, as sent (text), while the stream is subscribed.
    catalog: Option<Arc<str>>,
    /// The follower's last status (text), so a client that connects sees where it stands.
    status: Option<Arc<str>>,
    history: VecDeque<HistoryFrame>,
}

pub struct LiveStreams {
    settings: LiveStreamSettings,
    orion: Orion,
    inner: Mutex<Inner>,
    messages: broadcast::Sender<LiveMessage>,
}

/// One client's view: what it is sent first, then `messages`. Dropping it releases the client,
/// and the last one out ends the Orion subscription.
pub struct Subscription {
    live: Arc<LiveStreams>,
    /// The status, the catalog and the recent history, in the order they are sent.
    pub initial: Vec<LiveMessage>,
    pub messages: broadcast::Receiver<LiveMessage>,
}

impl Subscription {
    /// The catalog and status again, for a client that fell behind: its series are the same
    /// unless the stream changed while it was away.
    pub fn resync(&self) -> Vec<LiveMessage> {
        self.live.resync()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.live.release();
    }
}

impl LiveStreams {
    pub fn new(settings: LiveStreamSettings, orion: Orion) -> Arc<Self> {
        let (messages, _) = broadcast::channel(QUEUE_FRAMES);
        Arc::new(Self { settings, orion, inner: Mutex::new(Inner::default()), messages })
    }

    pub fn stream_id(&self) -> &str {
        &self.settings.stream_id
    }

    pub fn backfill_ms(&self) -> u64 {
        self.settings.backfill_ms
    }

    pub(crate) fn orion(&self) -> &Orion {
        &self.orion
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A poisoned lock still holds consistent data (every update is one assignment or push).
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Adds a client. The first one starts the follower. Must run inside the Tokio runtime.
    pub fn subscribe(self: &Arc<Self>) -> Subscription {
        let mut inner = self.lock();
        inner.clients += 1;
        // Taken under the lock: every frame sent after this point is in `messages`, and every one
        // before it is in `initial`.
        let messages = self.messages.subscribe();
        if inner.clients == 1 {
            inner.generation += 1;
            let (stop, stopped) = oneshot::channel();
            inner.stop = Some(stop);
            tokio::spawn(follow::run(Arc::clone(self), inner.generation, stopped));
        }
        let mut initial = Vec::with_capacity(inner.history.len() + 2);
        initial.extend(inner.status.clone().map(LiveMessage::Text));
        initial.extend(inner.catalog.clone().map(LiveMessage::Text));
        initial.extend(inner.history.iter().map(|frame| LiveMessage::Binary(Arc::clone(&frame.bytes))));
        Subscription { live: Arc::clone(self), initial, messages }
    }

    fn release(&self) {
        let mut inner = self.lock();
        inner.clients = inner.clients.saturating_sub(1);
        if inner.clients == 0 {
            inner.generation += 1;
            if let Some(stop) = inner.stop.take() {
                // The follower unsubscribes and ends; a send that finds it gone is no matter.
                let _ = stop.send(());
            }
            inner.catalog = None;
            inner.status = None;
            inner.history.clear();
        }
    }

    fn resync(&self) -> Vec<LiveMessage> {
        let inner = self.lock();
        inner.status.clone().map(LiveMessage::Text).into_iter().chain(inner.catalog.clone().map(LiveMessage::Text)).collect()
    }

    /// Sends `message` to the clients. Called with the lock held, so every client's queue is in
    /// the order the follower produced. No receivers is fine: the history covers the next one.
    fn send(&self, message: LiveMessage) {
        let _ = self.messages.send(message);
    }

    /// The follower's status: `connecting`, `unavailable` (Orion or the stream socket is not
    /// there, and why), `waiting` (subscribed, no batch yet), `live`, or `stale` (the publisher
    /// is gone, its history still served).
    pub(crate) fn report(&self, generation: u64, state: &str, reason: &str) {
        let mut inner = self.lock();
        if inner.generation != generation {
            return;
        }
        let text: Arc<str> = status_text(state, reason).into();
        inner.status = Some(Arc::clone(&text));
        self.send(LiveMessage::Text(text));
    }

    /// A new catalog: the series are indexed by it from now on, so the history before it is
    /// dropped (the batches after it come with the backfill).
    pub(crate) fn publish_catalog(&self, generation: u64, text: String, live: bool) {
        let mut inner = self.lock();
        if inner.generation != generation {
            return;
        }
        let text: Arc<str> = text.into();
        inner.catalog = Some(Arc::clone(&text));
        inner.history.clear();
        let state = if live { "live" } else { "stale" };
        let reason = if live { "" } else { "the publisher is gone; its history is still served" };
        let status: Arc<str> = status_text(state, reason).into();
        inner.status = Some(Arc::clone(&status));
        self.send(LiveMessage::Text(text));
        self.send(LiveMessage::Text(status));
    }

    /// One batch, for the clients and the history.
    pub(crate) fn publish_samples(&self, generation: u64, frame: codec::SamplesFrame) {
        let mut inner = self.lock();
        if inner.generation != generation {
            return;
        }
        let bytes: Arc<[u8]> = frame.bytes.into();
        let horizon = frame.newest_us.saturating_sub(self.settings.backfill_ms.saturating_mul(1_000));
        inner.history.push_back(HistoryFrame { newest_us: frame.newest_us, bytes: Arc::clone(&bytes) });
        while inner.history.front().is_some_and(|oldest| oldest.newest_us < horizon || inner.history.len() > HISTORY_FRAMES) {
            inner.history.pop_front();
        }
        self.send(LiveMessage::Binary(bytes));
    }

    #[cfg(test)]
    fn generation(&self) -> u64 {
        self.lock().generation
    }

    #[cfg(test)]
    fn clients(&self) -> usize {
        self.lock().clients
    }

    #[cfg(test)]
    fn history_len(&self) -> usize {
        self.lock().history.len()
    }
}

fn status_text(state: &str, reason: &str) -> String {
    serde_json::json!({ "type": "status", "state": state, "reason": reason }).to_string()
}

/// The `lagged` notice sent to a client that missed frames.
pub fn lagged_text(missed: u64) -> String {
    serde_json::json!({ "type": "lagged", "missed": missed }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use orion::control_plane::{SampleChunkBuilder, SeriesKind, StatusSubject, StreamCursor, StreamData, StreamRef};

    fn stream_ref() -> StreamRef {
        StreamRef { owner: "node/raze-1".parse::<StatusSubject>().expect("a node subject"), id: DEFAULT_STREAM_ID.into() }
    }

    fn live(backfill_ms: u64) -> Arc<LiveStreams> {
        // Nothing listens at this path: the follower reports `unavailable` and waits.
        let settings = LiveStreamSettings { backfill_ms, ..LiveStreamSettings::default() };
        LiveStreams::new(settings, Orion::new("/nonexistent/helios-test/control.sock".into(), "/nonexistent/helios-test/control-stream.sock".into()))
    }

    /// A batch of `n` samples of series 0 (F32) starting at `t0_us`, one a millisecond.
    fn batch(seq: u64, t0_us: u64, n: u64) -> codec::SamplesFrame {
        let mut builder = SampleChunkBuilder::new(0, SeriesKind::F32);
        for i in 0..n {
            builder.push(t0_us + i * 1_000, i as f64);
        }
        let data = StreamData { stream: stream_ref(), cursor: StreamCursor { epoch: 1, seq }, prev_seq: seq.saturating_sub(1), backfill: false, chunks: vec![builder.take()] };
        codec::samples_frame(&data, &[SeriesKind::F32]).expect("samples")
    }

    fn catalog_text() -> String {
        r#"{"type":"catalog","stream":"board.hardware","owner":"node/raze-1","epoch":1,"live":true,"series":[]}"#.into()
    }

    /// The binary messages a receiver has queued, without the text ones (status and catalog).
    fn binaries(receiver: &mut broadcast::Receiver<LiveMessage>) -> Vec<Arc<[u8]>> {
        let mut out = Vec::new();
        while let Ok(message) = receiver.try_recv() {
            if let LiveMessage::Binary(bytes) = message {
                out.push(bytes);
            }
        }
        out
    }

    #[tokio::test]
    async fn one_subscription_serves_every_client_the_same_frames() {
        let live = live(5_000);
        let mut first = live.subscribe();
        let mut second = live.subscribe();
        assert_eq!(live.clients(), 2);
        assert_eq!(live.generation(), 1, "the second client shares the first one's subscription");
        let generation = live.generation();
        live.publish_catalog(generation, catalog_text(), true);
        let frame = batch(1, 1_000_000, 4);
        live.publish_samples(generation, frame.clone());
        for receiver in [&mut first.messages, &mut second.messages] {
            assert!(matches!(receiver.try_recv(), Ok(LiveMessage::Text(_))), "catalog");
            assert!(matches!(receiver.try_recv(), Ok(LiveMessage::Text(_))), "live status");
            assert_eq!(binaries(receiver), vec![Arc::<[u8]>::from(frame.bytes.clone())]);
        }
    }

    #[tokio::test]
    async fn a_late_client_starts_with_the_catalog_and_the_recent_history() {
        let live = live(5_000);
        let _early = live.subscribe();
        let generation = live.generation();
        live.publish_catalog(generation, catalog_text(), true);
        // Ten seconds of batches, one a second: only the last five seconds are kept.
        for second in 0..10u64 {
            live.publish_samples(generation, batch(second + 1, second * 1_000_000, 2));
        }
        let late = live.subscribe();
        assert!(matches!(&late.initial[0], LiveMessage::Text(text) if text.contains("\"state\":\"live\"")), "the status comes first");
        let history: Vec<_> = late.initial.iter().filter(|m| matches!(m, LiveMessage::Binary(_))).collect();
        assert!(history.len() <= 6 && history.len() >= 5, "about five seconds of one-a-second batches, got {}", history.len());
        let newest = codec::test_support::decode(match history.last() {
            Some(LiveMessage::Binary(bytes)) => bytes,
            _ => panic!("a history frame"),
        });
        let (_, blocks) = newest.expect("a samples frame");
        assert_eq!(blocks[0].samples.last().map(|s| s.0), Some(9_001_000.0), "the newest sample is there");
    }

    #[tokio::test]
    async fn a_slow_client_drops_its_oldest_frames_and_the_publisher_never_waits() {
        let live = live(5_000);
        let mut slow = live.subscribe();
        let generation = live.generation();
        let total = QUEUE_FRAMES as u64 + 40;
        for seq in 1..=total {
            live.publish_samples(generation, batch(seq, seq * 1_000, 1));
        }
        match slow.messages.try_recv() {
            Err(broadcast::error::TryRecvError::Lagged(missed)) => assert!(missed >= 40, "missed {missed}"),
            other => panic!("expected a lag, got {other:?}"),
        }
        // What is still queued is the newest: the last frame is the last one sent.
        let rest = binaries(&mut slow.messages);
        assert_eq!(rest.len(), QUEUE_FRAMES);
        assert_eq!(rest.last().map(|bytes| codec::test_support::decode(bytes).expect("frame").1[0].samples[0].0), Some(total as f64 * 1_000.0));
    }

    #[tokio::test]
    async fn history_keeps_only_its_window() {
        let live = live(2_000);
        let _client = live.subscribe();
        let generation = live.generation();
        for seq in 0..50u64 {
            live.publish_samples(generation, batch(seq + 1, seq * 500_000, 1));
        }
        // 50 batches half a second apart span 25 s; 2 s of them are kept (and the newest).
        assert!(live.history_len() <= 6, "kept {}", live.history_len());
        assert!(live.history_len() >= 4);
    }

    #[tokio::test]
    async fn the_last_client_out_ends_the_subscription_and_forgets_the_stream() {
        let live = live(5_000);
        let first = live.subscribe();
        let second = live.subscribe();
        let generation = live.generation();
        live.publish_catalog(generation, catalog_text(), true);
        live.publish_samples(generation, batch(1, 1_000, 2));
        drop(first);
        assert_eq!(live.generation(), generation, "one client is still subscribed");
        drop(second);
        assert_eq!(live.clients(), 0);
        assert_eq!(live.generation(), generation + 1, "the follower is stopped");
        assert_eq!(live.history_len(), 0);
        // A frame from the stopped follower is dropped, not published into the next one.
        live.publish_samples(generation, batch(2, 2_000, 2));
        assert_eq!(live.history_len(), 0);
    }

    #[tokio::test]
    async fn a_client_that_joins_after_a_restart_gets_the_new_generation_only() {
        let live = live(5_000);
        let old = live.subscribe();
        let old_generation = live.generation();
        drop(old);
        let _new = live.subscribe();
        live.publish_samples(old_generation, batch(1, 1_000, 2));
        assert_eq!(live.history_len(), 0, "the stopped follower's frame is ignored");
    }

    #[test]
    fn the_lagged_notice_says_how_many() {
        assert_eq!(lagged_text(12), r#"{"missed":12,"type":"lagged"}"#);
    }
}
